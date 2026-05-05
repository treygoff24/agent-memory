use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::{Frame, Terminal};

use crate::client::DaemonClient;
use crate::config::UiConfig;
use crate::panels;
use crate::widgets::trust_artifact::{TrustArtifact, TrustArtifactModalState, TrustArtifactWidget};

pub const MIN_TERMINAL_WIDTH: u16 = 80;
pub const MIN_TERMINAL_HEIGHT: u16 = 24;
pub const REVIEW_UNDO_WINDOW: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelId {
    Overview,
    ReviewQueue,
    Conflicts,
    Entities,
    Timeline,
    Namespace,
    Policy,
    RealityCheck,
    Recall,
}

impl PanelId {
    pub const fn number(self) -> u8 {
        match self {
            Self::Overview => 1,
            Self::ReviewQueue => 2,
            Self::Conflicts => 3,
            Self::Entities => 4,
            Self::Timeline => 5,
            Self::Namespace => 6,
            Self::Policy => 7,
            Self::RealityCheck => 8,
            Self::Recall => 9,
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::ReviewQueue => "Review",
            Self::Conflicts => "Conflicts",
            Self::Entities => "Entities",
            Self::Timeline => "Timeline",
            Self::Namespace => "Namespaces",
            Self::Policy => "Policy",
            Self::RealityCheck => "Reality Check",
            Self::Recall => "Recall",
        }
    }

    pub const fn all() -> [Self; 9] {
        [
            Self::Overview,
            Self::ReviewQueue,
            Self::Conflicts,
            Self::Entities,
            Self::Timeline,
            Self::Namespace,
            Self::Policy,
            Self::RealityCheck,
            Self::Recall,
        ]
    }

    pub const fn from_number(number: u8) -> Option<Self> {
        match number {
            1 => Some(Self::Overview),
            2 => Some(Self::ReviewQueue),
            3 => Some(Self::Conflicts),
            4 => Some(Self::Entities),
            5 => Some(Self::Timeline),
            6 => Some(Self::Namespace),
            7 => Some(Self::Policy),
            8 => Some(Self::RealityCheck),
            9 => Some(Self::Recall),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SocketState {
    Connected,
    Unreachable { path: PathBuf, error: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Modal {
    MemoryDetail,
    HelpOverlay,
    ConfirmQuit,
    ConfirmForget,
    CommandPrompt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewAction {
    Approve,
    Reject,
    Forget,
    Quarantine,
    Edit,
    AcceptLocal,
    AcceptRemote,
    Merge,
}

impl ReviewAction {
    const fn past_tense(&self) -> &'static str {
        match self {
            Self::Approve => "Approved",
            Self::Reject => "Rejected",
            Self::Forget => "Forgot",
            Self::Quarantine => "Quarantined",
            Self::Edit => "Edited",
            Self::AcceptLocal => "Accepted local",
            Self::AcceptRemote => "Accepted remote",
            Self::Merge => "Merged",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealityCheckAction {
    Confirm,
    Correct,
    Forget,
    NotRelevant,
    SkipWeek,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DaemonCall {
    Review { action: ReviewAction, memory_id: String },
    RealityCheck { action: RealityCheckAction, session_id: String, memory_id: String },
    ForceRefresh,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingAction {
    staged_at: Instant,
    action: ReviewAction,
    memory_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelCommand {
    OpenModal(Modal),
    StageReview(ReviewAction),
    StartRealityCheck,
    RealityCheck(RealityCheckAction),
}

#[derive(Clone, Debug, PartialEq)]
pub struct App {
    active_panel: PanelId,
    socket_state: SocketState,
    snapshot: DaemonSnapshot,
    config: UiConfig,
    modal: Option<Modal>,
    pending_action: Option<PendingAction>,
    pending_trust_artifact_id: Option<String>,
    queued_daemon_calls: Vec<DaemonCall>,
    should_quit: bool,
    review_queue_state: panels::review_queue::ReviewQueueState,
    conflicts_state: panels::conflicts::ConflictsState,
    entities_state: panels::entities::EntitiesState,
    timeline_state: panels::timeline::TimelineState,
    namespace_state: panels::namespace::NamespaceState,
    policy_state: panels::policy::PolicyState,
    reality_check_state: panels::reality_check::RealityCheckState,
    recall_state: panels::recall::RecallState,
    memory_detail_state: TrustArtifactModalState,
}

impl App {
    pub fn new(config: UiConfig) -> Self {
        Self {
            active_panel: PanelId::Overview,
            socket_state: SocketState::Connected,
            snapshot: DaemonSnapshot::loading(&config.socket_path),
            config,
            modal: None,
            pending_action: None,
            pending_trust_artifact_id: None,
            queued_daemon_calls: Vec::new(),
            should_quit: false,
            review_queue_state: panels::review_queue::ReviewQueueState::default(),
            conflicts_state: panels::conflicts::ConflictsState::default(),
            entities_state: panels::entities::EntitiesState::default(),
            timeline_state: panels::timeline::TimelineState::default(),
            namespace_state: panels::namespace::NamespaceState::default(),
            policy_state: panels::policy::PolicyState::default(),
            reality_check_state: panels::reality_check::RealityCheckState::default(),
            recall_state: panels::recall::RecallState::default(),
            memory_detail_state: TrustArtifactModalState::default(),
        }
    }

    pub fn with_snapshot(snapshot: DaemonSnapshot) -> Self {
        Self {
            active_panel: PanelId::Overview,
            socket_state: SocketState::Connected,
            snapshot,
            config: UiConfig::default(),
            modal: None,
            pending_action: None,
            pending_trust_artifact_id: None,
            queued_daemon_calls: Vec::new(),
            should_quit: false,
            review_queue_state: panels::review_queue::ReviewQueueState::default(),
            conflicts_state: panels::conflicts::ConflictsState::default(),
            entities_state: panels::entities::EntitiesState::default(),
            timeline_state: panels::timeline::TimelineState::default(),
            namespace_state: panels::namespace::NamespaceState::default(),
            policy_state: panels::policy::PolicyState::default(),
            reality_check_state: panels::reality_check::RealityCheckState::default(),
            recall_state: panels::recall::RecallState::default(),
            memory_detail_state: TrustArtifactModalState::default(),
        }
    }

    pub fn active_panel(&self) -> PanelId {
        self.active_panel
    }

    pub fn snapshot(&self) -> &DaemonSnapshot {
        &self.snapshot
    }

    pub fn config(&self) -> &UiConfig {
        &self.config
    }

    pub fn socket_state(&self) -> &SocketState {
        &self.socket_state
    }

    pub fn modal(&self) -> Option<&Modal> {
        self.modal.as_ref()
    }

    pub fn pending_action(&self) -> Option<&PendingAction> {
        self.pending_action.as_ref()
    }

    pub fn pending_trust_artifact_id(&self) -> Option<&str> {
        self.pending_trust_artifact_id.as_deref()
    }

    pub fn queued_daemon_calls(&self) -> &[DaemonCall] {
        &self.queued_daemon_calls
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn reality_check_state(&self) -> &panels::reality_check::RealityCheckState {
        &self.reality_check_state
    }

    pub fn recall_state(&self) -> &panels::recall::RecallState {
        &self.recall_state
    }

    pub fn set_active_panel(&mut self, panel: PanelId) {
        self.active_panel = panel;
    }

    pub fn mark_socket_unreachable(&mut self, path: impl Into<PathBuf>, error: impl Into<String>) {
        self.socket_state = SocketState::Unreachable { path: path.into(), error: error.into() };
    }

    pub fn mark_socket_connected(&mut self, snapshot: DaemonSnapshot) {
        self.socket_state = SocketState::Connected;
        self.snapshot = snapshot;
    }

    pub fn set_trust_artifact(&mut self, artifact: TrustArtifact) {
        self.snapshot.trust_artifact = Some(artifact);
    }

    pub fn handle_event(&mut self, event: Event, now: Instant) {
        self.on_tick(now);

        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat => {
                self.handle_key(key, now);
            }
            Event::Resize(_, _) => self.modal = None,
            _ => {}
        }
    }

    pub fn on_tick(&mut self, now: Instant) {
        let Some(pending) = self.pending_action.as_ref() else {
            return;
        };

        if elapsed_since(pending.staged_at, now) >= REVIEW_UNDO_WINDOW {
            let pending = self.pending_action.take().expect("pending action should exist after as_ref check");
            self.queued_daemon_calls.push(DaemonCall::Review { action: pending.action, memory_id: pending.memory_id });
        }
    }

    pub async fn poll_daemon(&mut self, client: &DaemonClient) {
        match client.status().await {
            Ok(status) => {
                self.snapshot.overview.daemon_state = status.state;
                self.snapshot.overview.recall_startup = status.recall.startup_invoked_total;
                self.snapshot.overview.recall_delta = status.recall.delta_invoked_total;
                self.socket_state = SocketState::Connected;
                self.load_pending_trust_artifact(client).await;
                if self.active_panel == PanelId::Recall {
                    self.load_recall_hits(client).await;
                }
            }
            Err(error) => self.mark_socket_unreachable(client.socket_path(), error.to_string()),
        }
    }

    pub async fn dispatch_queued_daemon_calls(&mut self, client: &DaemonClient) {
        let mut remaining = Vec::new();
        let mut calls = std::mem::take(&mut self.queued_daemon_calls).into_iter();

        while let Some(call) = calls.next() {
            match client.dispatch_daemon_call(&call).await {
                Ok(()) => self.socket_state = SocketState::Connected,
                Err(error) => {
                    remaining.push(call);
                    remaining.extend(calls);
                    self.mark_socket_unreachable(client.socket_path(), error.to_string());
                    break;
                }
            }
        }

        self.queued_daemon_calls = remaining;
    }

    async fn load_pending_trust_artifact(&mut self, client: &DaemonClient) {
        let Some(memory_id) = self.pending_trust_artifact_id.take() else {
            return;
        };

        match client.trust_artifact(&memory_id).await {
            Ok(artifact) => self.snapshot.trust_artifact = Some(artifact),
            Err(error) => {
                self.pending_trust_artifact_id = Some(memory_id);
                self.mark_socket_unreachable(client.socket_path(), error.to_string());
            }
        }
    }

    async fn load_recall_hits(&mut self, client: &DaemonClient) {
        match client.recall_hits(100).await {
            Ok(response) => {
                self.snapshot.recall = RecallPanelData {
                    limit: response.limit,
                    hits: response.hits.into_iter().map(RecallHitRow::from).collect(),
                };
                self.socket_state = SocketState::Connected;
            }
            Err(error) => self.mark_socket_unreachable(client.socket_path(), error.to_string()),
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let [header, content, footer] = shell_areas(area);
        render_header(frame, header, self.active_panel);
        render_footer(frame, footer, self);

        if area.width < MIN_TERMINAL_WIDTH || area.height < MIN_TERMINAL_HEIGHT {
            render_too_small(frame, content, area);
            return;
        }

        if let SocketState::Unreachable { path, error } = &self.socket_state {
            render_socket_unreachable(frame, content, path, error);
            return;
        }

        panels::render_panel(frame, content, self);
        if let Some(modal) = &self.modal {
            self.render_modal(frame, area, modal);
        }
    }

    fn handle_key(&mut self, key: KeyEvent, now: Instant) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            self.modal = None;
            return;
        }

        if self.handle_modal_key(&key) {
            return;
        }

        if let KeyCode::Char(ch) = key.code {
            if let Some(panel) = ch.to_digit(10).and_then(|digit| PanelId::from_number(digit as u8)) {
                self.active_panel = panel;
                return;
            }
        }

        match key.code {
            KeyCode::Char('?') => self.open_modal(Modal::HelpOverlay),
            KeyCode::Char('q') => self.handle_quit_key(),
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.queued_daemon_calls.push(DaemonCall::ForceRefresh);
            }
            KeyCode::Char(':') => self.open_modal(Modal::CommandPrompt),
            KeyCode::Esc => self.modal = None,
            KeyCode::Char('u') => {
                self.pending_action = None;
            }
            _ => self.handle_active_panel_key(&key, now),
        }
    }

    fn handle_modal_key(&mut self, key: &KeyEvent) -> bool {
        let Some(modal) = self.modal.as_ref() else {
            return false;
        };

        match (modal, key.code) {
            (_, KeyCode::Esc) => {
                self.modal = None;
                true
            }
            (Modal::HelpOverlay, KeyCode::Char('?')) => {
                self.modal = None;
                true
            }
            (Modal::MemoryDetail, KeyCode::Char('j') | KeyCode::Down) => {
                self.memory_detail_state.scroll_down();
                true
            }
            (Modal::MemoryDetail, KeyCode::Char('k') | KeyCode::Up) => {
                self.memory_detail_state.scroll_up();
                true
            }
            (Modal::ConfirmQuit, KeyCode::Char('y') | KeyCode::Char('Y')) => {
                self.should_quit = true;
                self.modal = None;
                true
            }
            (Modal::ConfirmQuit, KeyCode::Char('n') | KeyCode::Char('N')) => {
                self.modal = None;
                true
            }
            _ => false,
        }
    }

    fn handle_quit_key(&mut self) {
        if self.pending_action.is_some() {
            self.open_modal(Modal::ConfirmQuit);
        } else {
            self.should_quit = true;
        }
    }

    fn handle_active_panel_key(&mut self, key: &KeyEvent, now: Instant) {
        let command = match self.active_panel {
            PanelId::Overview => None,
            PanelId::ReviewQueue => {
                panels::review_queue::handle_key(key, &mut self.review_queue_state, self.snapshot.review_queue.len())
            }
            PanelId::Conflicts => {
                panels::conflicts::handle_key(key, &mut self.conflicts_state, self.snapshot.conflicts.len())
            }
            PanelId::Entities => {
                panels::entities::handle_key(key, &mut self.entities_state, self.snapshot.entities.top_memories.len())
            }
            PanelId::Timeline => {
                panels::timeline::handle_key(key, &mut self.timeline_state, self.snapshot.timeline.len())
            }
            PanelId::Namespace => {
                panels::namespace::handle_key(key, &mut self.namespace_state, self.snapshot.namespace.tree_lines.len())
            }
            PanelId::Policy => {
                panels::policy::handle_key(key, &mut self.policy_state, self.snapshot.policy.recent_decisions.len())
            }
            PanelId::RealityCheck => panels::reality_check::handle_key(key, &mut self.reality_check_state),
            PanelId::Recall => panels::recall::handle_key(key, &mut self.recall_state, self.snapshot.recall.hits.len()),
        };

        if let Some(command) = command {
            self.apply_panel_command(command, now);
        }
    }

    fn apply_panel_command(&mut self, command: PanelCommand, now: Instant) {
        match command {
            PanelCommand::OpenModal(modal) => self.open_modal(modal),
            PanelCommand::StageReview(action) => self.stage_review_action(action, now),
            PanelCommand::StartRealityCheck => self.reality_check_state.start_run(),
            PanelCommand::RealityCheck(action) => self.queue_reality_check_action(action),
        }
    }

    fn open_modal(&mut self, modal: Modal) {
        if modal == Modal::MemoryDetail {
            self.memory_detail_state.reset();
            self.pending_trust_artifact_id = self.selected_memory_id();
            self.snapshot.trust_artifact = None;
        }
        self.modal = Some(modal);
    }

    fn stage_review_action(&mut self, action: ReviewAction, now: Instant) {
        let memory_id = self
            .selected_review_memory_id()
            .or_else(|| self.selected_conflict_memory_id())
            .unwrap_or_else(|| "unknown".to_owned());

        self.pending_action = Some(PendingAction { staged_at: now, action, memory_id });
    }

    fn queue_reality_check_action(&mut self, action: RealityCheckAction) {
        self.reality_check_state.record_action(action);
        let Some(session_id) = self.snapshot.reality_check.session_id.clone() else {
            self.snapshot.footer_hint = "Reality Check action failed: no active daemon session".to_owned();
            return;
        };
        let Some(memory_id) = self.selected_reality_check_memory_id() else {
            self.snapshot.footer_hint = "Reality Check action failed: no selected memory".to_owned();
            return;
        };

        self.queued_daemon_calls.push(DaemonCall::RealityCheck { action, session_id, memory_id });
    }

    fn selected_review_memory_id(&self) -> Option<String> {
        selected_index(self.review_queue_state.cursor(), self.snapshot.review_queue.len())
            .and_then(|index| self.snapshot.review_queue.get(index))
            .map(|row| row.id.clone())
    }

    fn selected_conflict_memory_id(&self) -> Option<String> {
        selected_index(self.conflicts_state.cursor(), self.snapshot.conflicts.len())
            .and_then(|index| self.snapshot.conflicts.get(index))
            .map(|row| row.id.clone())
    }

    fn selected_reality_check_memory_id(&self) -> Option<String> {
        selected_index(self.reality_check_state.cursor(), self.snapshot.reality_check.items.len())
            .and_then(|index| self.snapshot.reality_check.items.get(index))
            .map(|row| row.memory_id.clone())
    }

    fn selected_memory_id(&self) -> Option<String> {
        match self.active_panel {
            PanelId::ReviewQueue => self.selected_review_memory_id().filter(|id| is_valid_memory_id(id)),
            PanelId::Conflicts => self.selected_conflict_memory_id().filter(|id| is_valid_memory_id(id)),
            PanelId::Entities => first_memory_id(self.snapshot.entities.top_memories.iter().map(String::as_str)),
            PanelId::Timeline => first_memory_id(self.snapshot.timeline.iter().map(|row| row.detail.as_str())),
            PanelId::Policy => first_memory_id(self.snapshot.policy.recent_decisions.iter().map(String::as_str)),
            PanelId::RealityCheck => self.selected_reality_check_memory_id().filter(|id| is_valid_memory_id(id)),
            PanelId::Recall => selected_index(self.recall_state.cursor(), self.snapshot.recall.hits.len())
                .and_then(|index| self.snapshot.recall.hits.get(index))
                .map(|hit| hit.memory_id.clone())
                .filter(|id| is_valid_memory_id(id)),
            PanelId::Overview | PanelId::Namespace => None,
        }
    }

    fn render_modal(&self, frame: &mut Frame<'_>, area: Rect, modal: &Modal) {
        match modal {
            Modal::MemoryDetail => {
                render_memory_detail_modal(
                    frame,
                    memory_detail_rect(area),
                    self.snapshot.trust_artifact.as_ref(),
                    self.memory_detail_state.scroll_offset(),
                );
            }
            _ => render_text_modal(frame, centered_rect(area, 74, 12), modal),
        }
    }
}

pub async fn run(config: UiConfig) -> Result<()> {
    run_inner(config, false).await
}

#[cfg(debug_assertions)]
pub async fn run_with_mid_render_panic(config: UiConfig) -> Result<()> {
    run_inner(config, true).await
}

async fn run_inner(config: UiConfig, panic_after_first_render: bool) -> Result<()> {
    let _terminal_guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let client = DaemonClient::new(config.socket_path.clone());
    let mut app = App::new(config);
    let mut tick = tokio::time::interval(app.config.tick_interval);
    let mut daemon_poll = tokio::time::interval(app.config.daemon_poll_interval);

    app.poll_daemon(&client).await;

    loop {
        terminal.draw(|frame| app.render(frame))?;
        if panic_after_first_render {
            panic!("injected memoryd-tui mid-render panic");
        }

        tokio::select! {
            _ = tick.tick() => {
                while event::poll(Duration::ZERO)? {
                    app.handle_event(event::read()?, Instant::now());
                }
                if app.should_quit() {
                    return Ok(());
                }
                app.dispatch_queued_daemon_calls(&client).await;
            }
            _ = daemon_poll.tick() => {
                app.poll_daemon(&client).await;
            }
        }
    }
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal_blocking();
    }
}

pub fn restore_terminal_blocking() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}

fn shell_areas(area: Rect) -> [Rect; 3] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0), Constraint::Length(1)])
        .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

fn render_header(frame: &mut Frame<'_>, area: Rect, active_panel: PanelId) {
    let mut first_line = vec![Span::styled("Memorum  ", Style::default().add_modifier(Modifier::BOLD))];
    for panel in PanelId::all().into_iter().take(5) {
        first_line.push(panel_tab(panel, active_panel));
        first_line.push(Span::raw(" "));
    }

    let mut second_line = vec![Span::raw("         ")];
    for panel in PanelId::all().into_iter().skip(5) {
        second_line.push(panel_tab(panel, active_panel));
        second_line.push(Span::raw(" "));
    }
    second_line.push(Span::raw("   ?:help  q:quit"));

    frame.render_widget(Paragraph::new(vec![Line::from(first_line), Line::from(second_line)]), area);
}

fn panel_tab(panel: PanelId, active_panel: PanelId) -> Span<'static> {
    let label = format!("[{}]{}", panel.number(), panel.title());
    if panel == active_panel {
        Span::styled(label, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else {
        Span::raw(label)
    }
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let socket = match app.socket_state() {
        SocketState::Connected => Span::styled("socket:ok", Style::default().fg(Color::Green)),
        SocketState::Unreachable { .. } => {
            Span::styled("socket:UNREACHABLE", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        }
    };

    let status = app
        .pending_action
        .as_ref()
        .map(|pending| format!("  {} {} -- press u to undo", pending.action.past_tense(), pending.memory_id))
        .unwrap_or_else(|| format!("  panel:{}  {}", app.active_panel().number(), app.snapshot().footer_hint));

    let line = Line::from(vec![Span::raw(format!("memoryd {}  ", app.snapshot().version)), socket, Span::raw(status)]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_text_modal(frame: &mut Frame<'_>, area: Rect, modal: &Modal) {
    let (title, body) = match modal {
        Modal::MemoryDetail => (
            "Memory detail",
            "No trust artifact loaded.\n\nEsc: close",
        ),
        Modal::HelpOverlay => (
            "Help",
            "Global: 1-9 switch panels  ?:help  q:quit  Ctrl-c:quit  Ctrl-r:refresh  ::command\nPanel: j/k move  h/l pane  Enter detail  / filter  tab focus  u undo\n\nEsc or ?: close",
        ),
        Modal::ConfirmQuit => (
            "Confirm quit",
            "A review action is still in its undo window.\nQuit anyway? [y/N]\n\nEsc: cancel",
        ),
        Modal::ConfirmForget => (
            "Confirm forget",
            "Forgetting tombstones the selected memory.\nReason capture lands with daemon integration.\n\nEsc: close",
        ),
        Modal::CommandPrompt => (
            "Command",
            ":q quit\n:reload force refresh\n:help <topic>\n\nEsc: close",
        ),
    };

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(body)
            .block(Block::default().title(title).borders(Borders::ALL))
            .style(Style::default().fg(Color::White)),
        area,
    );
}

fn render_memory_detail_modal(frame: &mut Frame<'_>, area: Rect, artifact: Option<&TrustArtifact>, scroll_offset: u16) {
    let body = artifact
        .map(|artifact| TrustArtifactWidget::new(artifact).render_lines())
        .unwrap_or_else(|| vec![Line::from("No trust artifact loaded."), Line::from("Esc: close")]);

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(body)
            .scroll((scroll_offset, 0))
            .block(Block::default().title("Memory Detail").borders(Borders::ALL))
            .style(Style::default().fg(Color::White)),
        area,
    );
}

fn memory_detail_rect(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4).max(1),
        height: area.height.saturating_sub(2).max(1),
    }
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let modal_width = width.min(area.width.saturating_sub(2)).max(1);
    let modal_height = height.min(area.height.saturating_sub(2)).max(1);
    Rect {
        x: area.x + area.width.saturating_sub(modal_width) / 2,
        y: area.y + area.height.saturating_sub(modal_height) / 2,
        width: modal_width,
        height: modal_height,
    }
}

fn elapsed_since(start: Instant, now: Instant) -> Duration {
    now.checked_duration_since(start).unwrap_or_default()
}

fn selected_index(cursor: usize, len: usize) -> Option<usize> {
    if len == 0 {
        None
    } else {
        Some(cursor.min(len - 1))
    }
}

fn first_memory_id<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<String> {
    values
        .into_iter()
        .flat_map(|value| value.split_whitespace())
        .map(|token| token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_'))
        .find(|token| is_valid_memory_id(token))
        .map(str::to_owned)
}

fn is_valid_memory_id(value: &str) -> bool {
    memoryd::protocol::MemoryId::try_new(value.to_owned()).is_ok()
}

fn sample_trust_artifact() -> TrustArtifact {
    serde_json::from_value(serde_json::json!({
        "id": "mem_20260501_0123456789abcdef_000009",
        "namespace": "project:atlasos",
        "status": "active",
        "sensitivity": "internal",
        "source": "substrate:projects/atlasos/deploy-target.md",
        "title": {
            "kind": "plaintext",
            "value": "Deploy target is production ECS"
        },
        "body": {
            "kind": "plaintext",
            "value": "The ECS cluster in us-east-1 is the production deployment target. All deploy scripts should target this cluster."
        },
        "current_confidence": "0.95",
        "original_confidence": "0.90",
        "confidence_reason": "promoted from candidate; user confirmed; corroborated by codex-cli and claude-code",
        "trust_summary": "high trust; policy-promoted",
        "recall": {
            "total": 28,
            "last_30_days": 12,
            "last_recalled_at": "2026-05-01T11:02:00Z"
        },
        "provenance_chain": [
            {
                "timestamp": "2026-04-30T14:22:00Z",
                "kind": "write_committed",
                "summary": "written by codex-cli",
                "evidence": "sess_abc123",
                "device": "macbook"
            },
            {
                "timestamp": "2026-04-30T14:22:01Z",
                "kind": "governance_decision",
                "summary": "governance: promoted",
                "evidence": "policy:project-standard@v2",
                "device": "macbook"
            },
            {
                "timestamp": "2026-05-01T11:02:00Z",
                "kind": "recall_hit",
                "summary": "recalled in delta-block",
                "evidence": "session:claude-code",
                "device": "desktop"
            }
        ],
        "policy_decisions": [
            {
                "policy_applied": "project-standard@v2",
                "policy_source": "disk",
                "confidence_floor_pass": "pass (0.90 >= 0.80)",
                "grounding_satisfied": "2 source refs resolved",
                "contradiction_result": "none detected",
                "tombstone_enforced": "no matching tombstone",
                "sensitivity_gate_result": "pass (internal)"
            }
        ],
        "privacy_scan": {
            "labels_detected": ["none"],
            "storage_action": "plaintext"
        },
        "supersedes": [
            {
                "id": "mem_20260428_0123456789abcdef_000004",
                "timestamp": "2026-04-28T00:00:00Z",
                "title": {
                    "kind": "plaintext",
                    "value": "Deploy target ECS (initial)"
                }
            }
        ],
        "superseded_by": [],
        "sync_state": {
            "devices": ["macbook (written here)", "desktop (synced 2026-05-01 06:00)"],
            "merge_status": "clean",
            "claim_lock_status": "Stream I not active"
        }
    }))
    .expect("sample trust artifact fixture must match daemon DTO")
}

fn render_too_small(frame: &mut Frame<'_>, area: Rect, terminal_area: Rect) {
    let text =
        format!("Terminal too small (current: {}x{}, minimum: 80x24).", terminal_area.width, terminal_area.height);
    let widget = Paragraph::new(text)
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .block(Block::default().title("Resize required").borders(Borders::ALL));
    frame.render_widget(Clear, area);
    frame.render_widget(widget, area);
}

fn render_socket_unreachable(frame: &mut Frame<'_>, area: Rect, path: &Path, error: &str) {
    let body = vec![
        Line::from(format!("Socket: {}", path.display())),
        Line::from(format!("Error:  {error}")),
        Line::from(""),
        Line::from("Run `memoryd start` to start the daemon."),
        Line::from("Ctrl-r to retry.  q to quit."),
    ];
    let widget = Paragraph::new(body)
        .block(Block::default().title("Daemon unreachable").borders(Borders::ALL))
        .style(Style::default().fg(Color::Red));
    frame.render_widget(Clear, area);
    frame.render_widget(widget, area);
}

#[derive(Clone, Debug, PartialEq)]
pub struct DaemonSnapshot {
    pub version: String,
    pub footer_hint: String,
    pub overview: OverviewData,
    pub review_queue: Vec<ReviewQueueRow>,
    pub conflicts: Vec<ConflictRow>,
    pub entities: EntityPanelData,
    pub timeline: Vec<TimelineRow>,
    pub namespace: NamespacePanelData,
    pub policy: PolicyPanelData,
    pub reality_check: RealityCheckPanelData,
    pub recall: RecallPanelData,
    pub trust_artifact: Option<TrustArtifact>,
}

impl DaemonSnapshot {
    pub fn loading(socket_path: &Path) -> Self {
        let mut snapshot = Self::empty();
        snapshot.overview.daemon_state = "loading".to_owned();
        snapshot.overview.socket_path = socket_path.display().to_string();
        snapshot
    }

    pub fn empty() -> Self {
        Self {
            version: "v1.0.0".to_owned(),
            footer_hint: "?:help  q:quit".to_owned(),
            overview: OverviewData::empty(),
            review_queue: Vec::new(),
            conflicts: Vec::new(),
            entities: EntityPanelData::empty(),
            timeline: Vec::new(),
            namespace: NamespacePanelData::empty(),
            policy: PolicyPanelData::empty(),
            reality_check: RealityCheckPanelData::empty(),
            recall: RecallPanelData::empty(),
            trust_artifact: None,
        }
    }

    pub fn sample() -> Self {
        Self {
            version: "v1.0.0".to_owned(),
            footer_hint: "?:help  q:quit".to_owned(),
            overview: OverviewData::sample(),
            review_queue: ReviewQueueRow::sample_rows(),
            conflicts: ConflictRow::sample_rows(),
            entities: EntityPanelData::sample(),
            timeline: TimelineRow::sample_rows(),
            namespace: NamespacePanelData::sample(),
            policy: PolicyPanelData::sample(),
            reality_check: RealityCheckPanelData::sample(),
            recall: RecallPanelData::sample(),
            trust_artifact: Some(sample_trust_artifact()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverviewData {
    pub daemon_state: String,
    pub pid: u32,
    pub uptime: String,
    pub socket_path: String,
    pub active_memories: u64,
    pub last_reindex: String,
    pub sync_ahead: u32,
    pub sync_behind: u32,
    pub remote: String,
    pub pending_review: u32,
    pub candidate: u32,
    pub quarantined: u32,
    pub dream_low_confidence: u32,
    pub conflicts: u32,
    pub active_sessions: String,
    pub dream_next: String,
    pub dream_last: String,
    pub dream_promoted: u32,
    pub dream_queued: u32,
    pub dream_dropped: u32,
    pub recall_startup: u64,
    pub recall_delta: u64,
    pub peer_updates: u64,
}

impl OverviewData {
    fn empty() -> Self {
        Self {
            daemon_state: "loading".to_owned(),
            pid: 0,
            uptime: "not loaded".to_owned(),
            socket_path: "not connected".to_owned(),
            active_memories: 0,
            last_reindex: "not loaded".to_owned(),
            sync_ahead: 0,
            sync_behind: 0,
            remote: "not loaded".to_owned(),
            pending_review: 0,
            candidate: 0,
            quarantined: 0,
            dream_low_confidence: 0,
            conflicts: 0,
            active_sessions: "not loaded".to_owned(),
            dream_next: "not loaded".to_owned(),
            dream_last: "not loaded".to_owned(),
            dream_promoted: 0,
            dream_queued: 0,
            dream_dropped: 0,
            recall_startup: 0,
            recall_delta: 0,
            peer_updates: 0,
        }
    }

    fn sample() -> Self {
        Self {
            daemon_state: "running".to_owned(),
            pid: 12_345,
            uptime: "3d 14h".to_owned(),
            socket_path: "/run/user/1000/memoryd.sock".to_owned(),
            active_memories: 1_204,
            last_reindex: "2026-05-01 08:12".to_owned(),
            sync_ahead: 2,
            sync_behind: 0,
            remote: "git@github.com:trey/memory.git".to_owned(),
            pending_review: 7,
            candidate: 3,
            quarantined: 2,
            dream_low_confidence: 2,
            conflicts: 1,
            active_sessions: "claude-code, codex-cli".to_owned(),
            dream_next: "2026-05-02 03:00".to_owned(),
            dream_last: "2026-05-01 03:04".to_owned(),
            dream_promoted: 3,
            dream_queued: 1,
            dream_dropped: 0,
            recall_startup: 42,
            recall_delta: 119,
            peer_updates: 8,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewQueueRow {
    pub status: String,
    pub id: String,
    pub title: String,
    pub namespace: String,
    pub confidence: String,
    pub added: String,
    pub policy: String,
    pub next: String,
    pub reason: Option<String>,
}

impl ReviewQueueRow {
    fn sample_rows() -> Vec<Self> {
        vec![
            Self {
                status: "candidate".to_owned(),
                id: "mem_20260501_0123456789abcdef_000001".to_owned(),
                title: "Prefer CITEXT for email columns".to_owned(),
                namespace: "project:atlasos".to_owned(),
                confidence: "0.72".to_owned(),
                added: "3h ago".to_owned(),
                policy: "project-standard@v2".to_owned(),
                next: "requires_user_confirmation".to_owned(),
                reason: None,
            },
            Self {
                status: "quarantined".to_owned(),
                id: "mem_20260430_0123456789abcdef_000004".to_owned(),
                title: "SSH key rotation every 90d".to_owned(),
                namespace: "me".to_owned(),
                confidence: "0.50".to_owned(),
                added: "1d ago".to_owned(),
                policy: "me-strict@v1".to_owned(),
                next: "review_required".to_owned(),
                reason: Some("grounding_rehydration_failed".to_owned()),
            },
            Self {
                status: "dream_low_confidence".to_owned(),
                id: "mem_20260501_0123456789abcdef_000007".to_owned(),
                title: "Dream candidate needs confirmation".to_owned(),
                namespace: "project:agent-memory".to_owned(),
                confidence: "0.68".to_owned(),
                added: "20m ago".to_owned(),
                policy: "dreaming-strict@v1".to_owned(),
                next: "dream_low_confidence".to_owned(),
                reason: Some("dream_low_confidence".to_owned()),
            },
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictRow {
    pub id: String,
    pub title: String,
    pub namespace: String,
    pub local: String,
    pub remote: String,
    pub ancestor: String,
}

impl ConflictRow {
    fn sample_rows() -> Vec<Self> {
        vec![Self {
            id: "mem_20260501_0123456789abcdef_000002".to_owned(),
            title: "Database connection pool size".to_owned(),
            namespace: "project:atlasos".to_owned(),
            local: "Pool size: 20".to_owned(),
            remote: "Pool size: 30".to_owned(),
            ancestor: "Pool size: 10".to_owned(),
        }]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityPanelData {
    pub query: String,
    pub entity: String,
    pub project_id: String,
    pub memory_count: u32,
    pub recall_count_30d: u32,
    pub top_memories: Vec<String>,
}

impl EntityPanelData {
    fn empty() -> Self {
        Self {
            query: "not loaded".to_owned(),
            entity: "not loaded".to_owned(),
            project_id: "not loaded".to_owned(),
            memory_count: 0,
            recall_count_30d: 0,
            top_memories: vec!["Entity graph endpoint not loaded.".to_owned()],
        }
    }

    fn sample() -> Self {
        Self {
            query: "atlasos".to_owned(),
            entity: "atlasos".to_owned(),
            project_id: "project:proj_a3f2".to_owned(),
            memory_count: 42,
            recall_count_30d: 28,
            top_memories: vec![
                "mem_000009  Deploy target is production ECS  conf:0.95".to_owned(),
                "mem_000006  DB pool size 30  conf:0.88".to_owned(),
                "mem_000014  Prefer CITEXT for emails  conf:0.72  [candidate]".to_owned(),
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineRow {
    pub timestamp: String,
    pub kind: String,
    pub detail: String,
}

impl TimelineRow {
    fn sample_rows() -> Vec<Self> {
        vec![
            Self {
                timestamp: "2026-05-01 11:32:04".to_owned(),
                kind: "write".to_owned(),
                detail: "mem_022 promoted namespace:project".to_owned(),
            },
            Self {
                timestamp: "2026-05-01 11:31:58".to_owned(),
                kind: "dream_pass".to_owned(),
                detail: "scope:project pass:2 promoted:1 queued:0".to_owned(),
            },
            Self {
                timestamp: "2026-05-01 11:28:44".to_owned(),
                kind: "recall".to_owned(),
                detail: "session:claude-code startup 42 items".to_owned(),
            },
            Self {
                timestamp: "2026-05-01 11:15:17".to_owned(),
                kind: "privacy".to_owned(),
                detail: "mem_019 encrypted_at_rest label:email".to_owned(),
            },
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamespacePanelData {
    pub tree_lines: Vec<String>,
    pub detail_lines: Vec<String>,
}

impl NamespacePanelData {
    fn empty() -> Self {
        Self {
            tree_lines: vec!["Namespace endpoint not loaded.".to_owned()],
            detail_lines: vec!["Select a memory after daemon data loads.".to_owned()],
        }
    }

    fn sample() -> Self {
        Self {
            tree_lines: vec![
                "▼ me/".to_owned(),
                "  ▼ identity/".to_owned(),
                "      role.md [active] conf:0.95".to_owned(),
                "▶ projects/".to_owned(),
                "  ▶ atlasos/".to_owned(),
                "▶ agent/".to_owned(),
            ],
            detail_lines: vec![
                "Title: Senior engineer, Rust+TS stack".to_owned(),
                "Namespace: me/identity".to_owned(),
                "Recall (30d): 41".to_owned(),
                "Sensitivity: internal".to_owned(),
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyPanelData {
    pub active_policies: Vec<String>,
    pub recent_decisions: Vec<String>,
    pub refusal_reasons: Vec<String>,
}

impl PolicyPanelData {
    fn empty() -> Self {
        Self {
            active_policies: vec!["Policy endpoint not loaded.".to_owned()],
            recent_decisions: Vec::new(),
            refusal_reasons: Vec::new(),
        }
    }

    fn sample() -> Self {
        Self {
            active_policies: vec![
                "me-strict@v1          source: disk".to_owned(),
                "project-standard@v2   source: disk".to_owned(),
                "agent-strict@v3       source: built_in_fallback".to_owned(),
                "dreaming-strict@v1    source: disk".to_owned(),
            ],
            recent_decisions: vec![
                "2026-05-01 11:32  PROMOTED   mem_022  policy:project-standard@v2".to_owned(),
                "2026-05-01 10:45  CANDIDATE  mem_018  grounding:fail".to_owned(),
            ],
            refusal_reasons: vec!["tombstone 12".to_owned(), "grounding 7".to_owned()],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RealityCheckPanelData {
    pub session_id: Option<String>,
    pub status: String,
    pub last_completed: String,
    pub schedule: String,
    pub items: Vec<RealityCheckRow>,
}

impl RealityCheckPanelData {
    fn empty() -> Self {
        Self {
            session_id: None,
            status: "not loaded".to_owned(),
            last_completed: "not loaded".to_owned(),
            schedule: "not loaded".to_owned(),
            items: Vec::new(),
        }
    }

    fn sample() -> Self {
        Self {
            session_id: Some("rc_sample_session".to_owned()),
            status: "DUE".to_owned(),
            last_completed: "2026-04-20, 11 days ago".to_owned(),
            schedule: "Sunday 09:00 | Next: 2026-05-04 09:00".to_owned(),
            items: vec![RealityCheckRow {
                memory_id: "mem_20260501_0123456789abcdef_000008".to_owned(),
                rank: 1,
                score: 0.82,
                title: "My preferred stack is TypeScript + Rust".to_owned(),
                namespace: "me/identity".to_owned(),
                confidence: 0.88,
                last_observed: "62 days ago".to_owned(),
                recall_count_30d: 0,
                breakdown: "staleness:0.35 recall:0.16 corroboration:0.20 decay:0.08 sensitivity:0.03".to_owned(),
            }],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RealityCheckRow {
    pub memory_id: String,
    pub rank: u8,
    pub score: f64,
    pub title: String,
    pub namespace: String,
    pub confidence: f64,
    pub last_observed: String,
    pub recall_count_30d: u32,
    pub breakdown: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallPanelData {
    pub limit: usize,
    pub hits: Vec<RecallHitRow>,
}

impl RecallPanelData {
    fn empty() -> Self {
        Self { limit: 100, hits: Vec::new() }
    }

    fn sample() -> Self {
        Self {
            limit: 100,
            hits: vec![
                RecallHitRow {
                    event_id: "evt_20260504_001".to_owned(),
                    device: "macbook".to_owned(),
                    seq: 41,
                    memory_id: "mem_20260501_0123456789abcdef_000009".to_owned(),
                    recalled_at: "2026-05-04T09:10:00Z".to_owned(),
                    summary: Some("Deploy target is production ECS".to_owned()),
                },
                RecallHitRow {
                    event_id: "evt_20260504_002".to_owned(),
                    device: "desktop".to_owned(),
                    seq: 42,
                    memory_id: "mem_20260501_0123456789abcdef_000008".to_owned(),
                    recalled_at: "2026-05-04T09:12:00Z".to_owned(),
                    summary: Some("Preferred stack is TypeScript + Rust".to_owned()),
                },
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallHitRow {
    pub event_id: String,
    pub device: String,
    pub seq: u64,
    pub memory_id: String,
    pub recalled_at: String,
    pub summary: Option<String>,
}

impl From<memoryd::protocol::RecallHitSummary> for RecallHitRow {
    fn from(hit: memoryd::protocol::RecallHitSummary) -> Self {
        Self {
            event_id: hit.event_id,
            device: hit.device,
            seq: hit.seq,
            memory_id: hit.memory_id.as_str().to_owned(),
            recalled_at: hit.recalled_at.to_rfc3339(),
            summary: hit.summary,
        }
    }
}
