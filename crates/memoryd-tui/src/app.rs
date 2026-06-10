use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use memorum_theme::{BorderStyle, Charset, ColorCapability, Glyphs, HotReload, Loader, Theme};
use memoryd::protocol::MemoryId;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::{Frame, Terminal};

use crate::client::DaemonClient;
use crate::config::UiConfig;
use crate::focus::correct_editor::CorrectEditorState;
use crate::inbox::{self, FilterCounts, InboxFilter, InboxItem};
use crate::palette::commands::PaletteAction;
use crate::palette::{PaletteKeyResult, PaletteState};
use crate::state::{FocusKind, RealityCheckState};
use crate::theme_glue::ThemeStyles;
use crate::widgets::trust_artifact::{TrustArtifact, TrustArtifactModalState, TrustArtifactWidget};

pub const MIN_TERMINAL_WIDTH: u16 = 80;
pub const MIN_TERMINAL_HEIGHT: u16 = 24;
pub const REVIEW_UNDO_WINDOW: Duration = Duration::from_secs(1);

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
    CommandPrompt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewAction {
    Approve,
    Reject,
    Forget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RealityCheckAction {
    Confirm,
    Correct { new_body: String },
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

pub struct App {
    socket_state: SocketState,
    snapshot: DaemonSnapshot,
    config: UiConfig,
    theme: Theme,
    charset: Charset,
    color_capability: ColorCapability,
    filter: InboxFilter,
    inbox_items: Vec<InboxItem>,
    selected: usize,
    modal: Option<Modal>,
    palette: PaletteState,
    focus: FocusKind,
    reality_check: RealityCheckState,
    correct_editor: CorrectEditorState,
    pending_action: Option<PendingAction>,
    pending_trust_artifact_id: Option<String>,
    queued_daemon_calls: Vec<DaemonCall>,
    should_quit: bool,
    memory_detail_state: TrustArtifactModalState,
    tick_counter: u64,
    _hot_reload: Option<HotReload>,
}

pub struct AppParts {
    pub config: UiConfig,
    pub theme: Theme,
    pub charset: Charset,
    pub color_capability: ColorCapability,
    pub hot_reload: Option<HotReload>,
    pub snapshot: DaemonSnapshot,
}

impl App {
    pub fn new(config: UiConfig) -> Self {
        let theme = Loader::resolve(Some(&config.theme), config.theme_config.as_deref())
            .unwrap_or_else(|_| Theme::default_warm_dark());
        let capability = config.color_capability.unwrap_or_else(|| memorum_theme::Resolver::detect().capability());
        let charset = config.charset;
        let snapshot = DaemonSnapshot::loading(&config.socket_path);
        Self::from_parts(AppParts { config, theme, charset, color_capability: capability, hot_reload: None, snapshot })
    }

    pub fn with_snapshot(snapshot: DaemonSnapshot) -> Self {
        let config = UiConfig::default();
        Self::from_parts(AppParts {
            config,
            theme: Theme::default_warm_dark(),
            charset: Charset::detect(),
            color_capability: ColorCapability::TrueColor,
            hot_reload: None,
            snapshot,
        })
    }

    pub fn with_theme(snapshot: DaemonSnapshot, theme: Theme) -> Self {
        Self::from_parts(AppParts {
            config: UiConfig::default(),
            theme,
            charset: Charset::Full,
            color_capability: ColorCapability::TrueColor,
            hot_reload: None,
            snapshot,
        })
    }

    pub fn from_parts(parts: AppParts) -> Self {
        let AppParts { config, mut theme, charset, color_capability, hot_reload, snapshot } = parts;
        if config.no_motion {
            theme.motion = memorum_theme::MotionConfig::reduced();
        }
        if charset == Charset::Minimal {
            theme.glyphs = Glyphs::ascii_fallback();
            theme.borders = BorderStyle::Plain;
        }
        let inbox_items = snapshot.inbox_items();
        Self {
            socket_state: SocketState::Connected,
            snapshot,
            config,
            theme,
            charset,
            color_capability,
            filter: InboxFilter::All,
            inbox_items,
            selected: 0,
            modal: None,
            palette: PaletteState::default(),
            focus: FocusKind::None,
            reality_check: RealityCheckState::default(),
            correct_editor: CorrectEditorState::default(),
            pending_action: None,
            pending_trust_artifact_id: None,
            queued_daemon_calls: Vec::new(),
            should_quit: false,
            memory_detail_state: TrustArtifactModalState::default(),
            tick_counter: 0,
            _hot_reload: hot_reload,
        }
    }

    pub fn snapshot(&self) -> &DaemonSnapshot {
        &self.snapshot
    }

    pub fn filter(&self) -> InboxFilter {
        self.filter
    }

    pub fn socket_state(&self) -> &SocketState {
        &self.socket_state
    }

    pub fn modal(&self) -> Option<&Modal> {
        self.modal.as_ref()
    }

    pub fn palette(&self) -> &PaletteState {
        &self.palette
    }

    pub fn theme_name(&self) -> &str {
        &self.theme.name
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn tick_counter(&self) -> u64 {
        self.tick_counter
    }

    pub fn pending_action(&self) -> Option<&PendingAction> {
        self.pending_action.as_ref()
    }

    pub fn queued_daemon_calls(&self) -> &[DaemonCall] {
        &self.queued_daemon_calls
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn inbox_items(&self) -> &[InboxItem] {
        &self.inbox_items
    }

    pub fn selected_item(&self) -> Option<&InboxItem> {
        let selected = selected_index(self.selected, self.visible_items_len())?;
        self.inbox_items.iter().filter(|item| self.filter.matches(item)).nth(selected)
    }

    pub fn reality_check_state(&self) -> &RealityCheckState {
        &self.reality_check
    }

    pub fn correct_editor_state(&self) -> &CorrectEditorState {
        &self.correct_editor
    }

    pub fn focus(&self) -> &FocusKind {
        &self.focus
    }

    pub fn enter_reality_check_focus(&mut self, session: impl Into<String>, reviewed: usize, total: usize) {
        let session = session.into();
        self.reality_check.active_session_id = Some(session.clone());
        self.reality_check.items_reviewed = reviewed;
        self.reality_check.items_total = total;
        let selected = self.selected_item();
        let selected_title = selected.map(|item| item.title().to_string());
        let selected_id = selected.map(|item| item.id().to_string());
        self.reality_check.current_title = selected_title;
        self.reality_check.current_memory_id = selected_id.clone();
        let due = selected_id.as_deref().and_then(|id| self.snapshot.due.iter().find(|row| row.id == id));
        self.reality_check.current_breakdown = due.map(|row| row.breakdown);
        self.reality_check.current_score = due.and_then(|row| row.score.parse::<f64>().ok());
        self.reality_check.current_encrypted = due.is_some_and(|row| row.namespace.starts_with("encrypted/"));
        self.reality_check.transition_start_tick = Some(self.tick_counter);
        self.focus = FocusKind::RealityCheck { session };
        self.modal = None;
    }

    pub fn focus_transition_percent(&self) -> u16 {
        if !self.theme.motion.enabled || self.theme.motion.slide_in_ms == 0 {
            return 100;
        }
        let Some(start) = self.reality_check.transition_start_tick else {
            return 100;
        };
        let elapsed_ms = self.tick_counter.saturating_sub(start).saturating_mul(u64::from(self.theme.motion.tick_ms));
        ((elapsed_ms.saturating_mul(100) / u64::from(self.theme.motion.slide_in_ms)).min(100)) as u16
    }

    pub fn set_filter(&mut self, filter: InboxFilter) {
        self.filter = filter;
        self.selected = 0;
    }

    pub fn set_selected(&mut self, selected: usize) {
        self.selected = selected;
    }

    pub fn mark_socket_unreachable(&mut self, path: impl Into<PathBuf>, error: impl Into<String>) {
        self.socket_state = SocketState::Unreachable { path: path.into(), error: error.into() };
    }

    pub fn mark_socket_connected(&mut self, snapshot: DaemonSnapshot) {
        self.socket_state = SocketState::Connected;
        self.snapshot = snapshot;
        self.rebuild_inbox();
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
        self.tick_counter = self.tick_counter.saturating_add(1);
        let Some(pending) = self.pending_action.as_ref() else {
            return;
        };
        if elapsed_since(pending.staged_at, now) >= REVIEW_UNDO_WINDOW {
            let pending = self.pending_action.take().expect("pending action should exist after as_ref check");
            self.queued_daemon_calls.push(DaemonCall::Review { action: pending.action, memory_id: pending.memory_id });
        }
    }

    pub async fn poll_daemon(&mut self, client: &DaemonClient) {
        match client.fetch_snapshot().await {
            Ok(snapshot) => self.mark_socket_connected(snapshot),
            Err(error) => self.mark_socket_unreachable(client.socket_path(), error.to_string()),
        }
    }

    pub async fn dispatch_queued_daemon_calls(&mut self, client: &DaemonClient) {
        // Drain queued calls one at a time, bailing on the first failure.
        // Successful calls may push new items back into the queue via
        // `after_successful_daemon_call`, so we take+rebuild rather than
        // iterating in place. The explicit `into_iter()` lets us hand the
        // unconsumed tail to `remaining.extend(...)` on the failure path.
        let mut remaining: Vec<DaemonCall> = Vec::new();
        let mut calls = std::mem::take(&mut self.queued_daemon_calls).into_iter();
        while let Some(call) = calls.next() {
            match client.dispatch_daemon_call(&call).await {
                Ok(()) => {
                    self.socket_state = SocketState::Connected;
                    self.after_successful_daemon_call(&call);
                }
                Err(error) => {
                    // Preserve this and all subsequent (not-yet-consumed)
                    // calls so they can retry next tick once the socket
                    // recovers.
                    remaining.push(call);
                    remaining.extend(calls);
                    self.mark_socket_unreachable(client.socket_path(), error.to_string());
                    break;
                }
            }
        }
        self.queued_daemon_calls = remaining;
    }

    fn after_successful_daemon_call(&mut self, call: &DaemonCall) {
        if let DaemonCall::RealityCheck { action, session_id, .. } = call {
            // `SkipWeek` defers rather than reviews — count it as deferred so the
            // remaining-items math stays honest; every other response advances
            // the reviewed counter.
            match action {
                RealityCheckAction::SkipWeek => {
                    self.reality_check.deferred = self.reality_check.deferred.saturating_add(1);
                }
                _ => {
                    self.reality_check.items_reviewed = self.reality_check.items_reviewed.saturating_add(1);
                }
            }
            self.focus = FocusKind::RealityCheck { session: session_id.clone() };
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let styles = ThemeStyles::from_theme(&self.theme, self.color_capability);
        let [header, content, footer] = shell_areas(area);
        render_header(frame, header, self, &styles);
        crate::status::render(frame, footer, self, &styles);
        if area.width < MIN_TERMINAL_WIDTH || area.height < MIN_TERMINAL_HEIGHT {
            render_too_small(frame, content, area, &styles);
            return;
        }
        if let SocketState::Unreachable { .. } = &self.socket_state {
            render_socket_unreachable(frame, content, &self.socket_state, &styles);
            return;
        }
        if self.focus != FocusKind::None {
            crate::focus::render(
                frame,
                content,
                crate::focus::FocusRenderContext { kind: &self.focus, app: self, styles: &styles },
            );
        } else {
            render_inbox_shell(frame, content, self, &styles);
        }
        if self.modal.is_some() {
            self.render_modal(frame, area, &styles);
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
        if self.handle_focus_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Char('?') => self.modal = Some(Modal::HelpOverlay),
            KeyCode::Char('q') => self.handle_quit_key(),
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.queued_daemon_calls.push(DaemonCall::ForceRefresh)
            }
            KeyCode::Char(':') => self.open_palette(),
            KeyCode::Char('u') => self.pending_action = None,
            KeyCode::Char('a') => self.stage_review_action(ReviewAction::Approve, now),
            KeyCode::Char('r') => self.stage_review_action(ReviewAction::Reject, now),
            KeyCode::Char('f') => self.stage_review_action(ReviewAction::Forget, now),
            KeyCode::Char('t') | KeyCode::Enter => self.open_memory_detail(),
            KeyCode::Tab => self.next_filter(),
            KeyCode::BackTab => self.prev_filter(),
            KeyCode::Char('/') => {
                self.snapshot.footer_hint = "search is handled by Task 11B palette/search".to_string()
            }
            KeyCode::Esc => self.focus = FocusKind::None,
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            _ => {}
        }
    }

    fn handle_focus_key(&mut self, key: &KeyEvent) -> bool {
        match &self.focus {
            FocusKind::RealityCheck { .. } => self.handle_reality_check_key(key),
            FocusKind::CorrectEditor { item_id } => self.handle_correct_editor_key(key, item_id.clone()),
            FocusKind::None => false,
        }
    }

    fn handle_reality_check_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.focus = FocusKind::None;
                true
            }
            KeyCode::Char('k') => {
                self.open_correct_editor();
                true
            }
            KeyCode::Char('y') => {
                self.stage_reality_check_action(RealityCheckAction::Confirm);
                true
            }
            KeyCode::Char('f') => {
                self.stage_reality_check_action(RealityCheckAction::Forget);
                true
            }
            KeyCode::Char('n') => {
                self.stage_reality_check_action(RealityCheckAction::NotRelevant);
                true
            }
            KeyCode::Char('s') => {
                self.stage_reality_check_action(RealityCheckAction::SkipWeek);
                true
            }
            _ => false,
        }
    }

    /// Queue a Reality Check response (other than `Correct`, which routes through
    /// the editor) for the currently-focused item. Advances `items_reviewed`
    /// optimistically once the daemon acknowledges the call.
    fn stage_reality_check_action(&mut self, action: RealityCheckAction) {
        let Some(session_id) = self.reality_check.active_session_id.clone() else {
            self.snapshot.footer_hint = "Reality Check session missing; cannot respond".to_string();
            return;
        };
        let Some(memory_id) = self.reality_check.current_memory_id.clone() else {
            self.snapshot.footer_hint = "no Reality Check item selected".to_string();
            return;
        };
        self.queued_daemon_calls.push(DaemonCall::RealityCheck { action, session_id, memory_id });
    }

    fn handle_correct_editor_key(&mut self, key: &KeyEvent, item_id: MemoryId) -> bool {
        if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.submit_correct_editor(item_id);
            return true;
        }
        match key.code {
            KeyCode::Esc => {
                let session =
                    self.reality_check.active_session_id.clone().unwrap_or_else(|| "reality-check".to_string());
                self.focus = FocusKind::RealityCheck { session };
            }
            KeyCode::Enter => self.correct_editor.insert_newline(),
            KeyCode::Backspace => self.correct_editor.backspace(),
            KeyCode::Char(ch) => self.correct_editor.push_char(ch),
            _ => {}
        }
        true
    }

    fn open_correct_editor(&mut self) {
        let Some(item_id) = self.selected_item().and_then(|item| MemoryId::try_new(item.id().to_string()).ok()) else {
            self.snapshot.footer_hint = "selected item cannot be corrected".to_string();
            return;
        };
        self.correct_editor.reset();
        self.focus = FocusKind::CorrectEditor { item_id };
    }

    fn submit_correct_editor(&mut self, item_id: MemoryId) {
        let body = self.correct_editor.body().trim().to_string();
        if body.is_empty() {
            self.correct_editor.show_body_required();
            return;
        }
        let Some(session_id) = self.reality_check.active_session_id.clone() else {
            self.snapshot.footer_hint = "Reality Check session missing; cannot submit correction".to_string();
            return;
        };
        self.queued_daemon_calls.push(DaemonCall::RealityCheck {
            action: RealityCheckAction::Correct { new_body: body },
            session_id: session_id.clone(),
            memory_id: item_id.to_string(),
        });
        self.focus = FocusKind::RealityCheck { session: session_id };
    }

    fn handle_modal_key(&mut self, key: &KeyEvent) -> bool {
        let Some(modal) = self.modal.as_ref() else {
            return false;
        };
        if modal == &Modal::CommandPrompt {
            return self.handle_palette_key(key);
        }
        match (modal, key.code) {
            (_, KeyCode::Esc) => {
                self.modal = None;
                true
            }
            (Modal::HelpOverlay, KeyCode::Char('?')) => {
                self.modal = None;
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

    fn open_palette(&mut self) {
        self.palette.open();
        self.modal = Some(Modal::CommandPrompt);
    }

    fn handle_palette_key(&mut self, key: &KeyEvent) -> bool {
        match self.palette.handle_key(key) {
            PaletteKeyResult::Handled => true,
            PaletteKeyResult::Close => {
                self.modal = None;
                true
            }
            PaletteKeyResult::Submit => {
                self.dispatch_palette_selection();
                true
            }
        }
    }

    fn dispatch_palette_selection(&mut self) {
        let Some(command) = self.palette.selected_command() else {
            self.palette.show_no_match();
            return;
        };
        match command.action {
            PaletteAction::SetFilter(filter) => {
                self.set_filter(filter);
                self.modal = None;
            }
            PaletteAction::SwitchTheme(name) => match Loader::resolve(Some(name), None) {
                Ok(theme) => {
                    self.theme = theme;
                    self.modal = None;
                }
                Err(error) => self.palette.show_error(format!("theme load failed: {error}")),
            },
            PaletteAction::OpenSearch => {
                self.snapshot.footer_hint = "search opens in Task 11B palette/search follow-up".to_string();
                self.modal = None;
            }
            PaletteAction::ShowHelp => self.modal = Some(Modal::HelpOverlay),
            PaletteAction::EnterRealityCheck => {
                self.enter_reality_check_focus("palette-session", 0, self.inbox_items.len());
            }
            PaletteAction::ReloadTheme | PaletteAction::ReadOnly => {
                self.snapshot.footer_hint = format!("{} is queued for a later TUI task", command.id);
                self.modal = None;
            }
        }
    }

    fn handle_quit_key(&mut self) {
        if self.pending_action.is_some() {
            self.modal = Some(Modal::ConfirmQuit);
        } else {
            self.should_quit = true;
        }
    }

    fn stage_review_action(&mut self, action: ReviewAction, now: Instant) {
        let memory_id = self.selected_item().map(|item| item.id().to_string()).unwrap_or_else(|| "unknown".to_string());
        self.pending_action = Some(PendingAction { staged_at: now, action, memory_id });
    }

    fn open_memory_detail(&mut self) {
        self.memory_detail_state.reset();
        self.pending_trust_artifact_id = self.selected_item().map(|item| item.id().to_string());
        self.modal = Some(Modal::MemoryDetail);
    }

    fn next_filter(&mut self) {
        self.rotate_filter(1);
    }

    fn prev_filter(&mut self) {
        self.rotate_filter(-1);
    }

    fn rotate_filter(&mut self, delta: isize) {
        let filters = InboxFilter::all();
        let index = filters.iter().position(|filter| *filter == self.filter).unwrap_or(0) as isize;
        let next = (index + delta).rem_euclid(filters.len() as isize) as usize;
        self.set_filter(filters[next]);
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.visible_items_len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as isize + delta).clamp(0, len.saturating_sub(1) as isize) as usize;
    }

    fn visible_items(&self) -> Vec<InboxItem> {
        self.inbox_items.iter().filter(|item| self.filter.matches(item)).cloned().collect()
    }

    fn visible_items_len(&self) -> usize {
        self.inbox_items.iter().filter(|item| self.filter.matches(item)).count()
    }

    fn rebuild_inbox(&mut self) {
        self.inbox_items = self.snapshot.inbox_items();
        self.selected = self.selected.min(self.visible_items_len().saturating_sub(1));
    }

    fn render_modal(&self, frame: &mut Frame<'_>, area: Rect, styles: &ThemeStyles) {
        match self.modal.as_ref().expect("modal is checked before render_modal") {
            Modal::MemoryDetail => self.render_memory_detail_modal(frame, memory_detail_rect(area), styles),
            Modal::CommandPrompt => crate::palette::render(frame, centered_rect(area, 74, 12), &self.palette, styles),
            modal => render_text_modal(frame, centered_rect(area, 74, 12), modal, styles),
        }
    }

    fn render_memory_detail_modal(&self, frame: &mut Frame<'_>, area: Rect, styles: &ThemeStyles) {
        let body = self
            .snapshot
            .trust_artifact
            .as_ref()
            .map(|artifact| TrustArtifactWidget::new(artifact).render_lines(styles))
            .unwrap_or_else(|| vec![Line::from("No trust artifact loaded."), Line::from("Esc: close")]);
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(body).scroll((self.memory_detail_state.scroll_offset(), 0)).style(styles.base).block(
                Block::new()
                    .title("Memory Detail")
                    .borders(Borders::ALL)
                    .border_set(styles.border)
                    .border_style(styles.block),
            ),
            area,
        );
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
            _ = daemon_poll.tick() => app.poll_daemon(&client).await,
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
        .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)])
        .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

fn render_header(frame: &mut Frame<'_>, area: Rect, app: &App, styles: &ThemeStyles) {
    let counts = FilterCounts::from_items(&app.inbox_items);
    let mut spans = vec![Span::styled(format!("{} Memorum  ", styles.glyphs.dream), styles.accent)];
    for filter in InboxFilter::all() {
        let label = format!("{}{}{}", filter.label(), styles.glyphs.pill_separator, counts.get(filter));
        let style = if filter == app.filter { styles.selected } else { styles.muted };
        spans.push(Span::styled(format!(" {label} "), style));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        format!("/:search  ::palette  ?:help  theme:{}  charset:{:?}", app.theme.name, app.charset),
        styles.dim,
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)).style(styles.base), area);
}

fn render_inbox_shell(frame: &mut Frame<'_>, area: Rect, app: &App, styles: &ThemeStyles) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(36), Constraint::Min(20)])
        .split(area);
    let visible = app.visible_items();
    inbox::render(frame, panes[0], inbox::InboxRenderContext { items: &visible, selected: app.selected, styles });
    crate::inspector::render(frame, panes[1], app.selected_item(), styles);
}

fn render_text_modal(frame: &mut Frame<'_>, area: Rect, modal: &Modal, styles: &ThemeStyles) {
    let (title, body) = match modal {
        Modal::MemoryDetail => ("Memory Detail", "No trust artifact loaded.\n\nEsc: close"),
        Modal::HelpOverlay => (
            "Help",
            "j/k move · tab filters · enter detail · a/r/f review · Ctrl-r refresh · q quit\n\
             Reality Check: y confirm · k correct · f forget · n not-relevant · s skip · Esc inbox",
        ),
        Modal::ConfirmQuit => ("Confirm quit", "A review action is still undoable. Quit anyway? [y/N]"),
        Modal::CommandPrompt => ("Command", ":q quit\n:reload force refresh\nTask 11B adds fuzzy dispatch."),
    };
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(body).style(styles.base).block(
            Block::new().title(title).borders(Borders::ALL).border_set(styles.border).border_style(styles.block),
        ),
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

fn render_too_small(frame: &mut Frame<'_>, area: Rect, terminal_area: Rect, styles: &ThemeStyles) {
    let text =
        format!("Terminal too small (current: {}x{}, minimum: 80x24).", terminal_area.width, terminal_area.height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text).style(styles.warn).block(
            Block::new()
                .title("Resize required")
                .borders(Borders::ALL)
                .border_set(styles.border)
                .border_style(styles.block),
        ),
        area,
    );
}

fn render_socket_unreachable(frame: &mut Frame<'_>, area: Rect, socket_state: &SocketState, styles: &ThemeStyles) {
    let SocketState::Unreachable { path, error } = socket_state else {
        return;
    };
    let body = vec![
        Line::from(format!("Socket: {}", path.display())),
        Line::from(format!("Error:  {error}")),
        Line::from(""),
        Line::from("Run `memoryd serve --init` to initialize and start the daemon."),
        Line::from("Or run the Memorum installer if memoryd is not installed yet."),
        Line::from("Ctrl-r to retry. q to quit."),
    ];
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(body).style(styles.bad).block(
            Block::new()
                .title("Daemon unreachable")
                .borders(Borders::ALL)
                .border_set(styles.border)
                .border_style(styles.block),
        ),
        area,
    );
}

#[derive(Clone, Debug, PartialEq)]
pub struct DaemonSnapshot {
    pub version: String,
    pub footer_hint: String,
    pub daemon_state: String,
    pub review_queue: Vec<ReviewQueueRow>,
    pub conflicts: Vec<ConflictRow>,
    pub recall: Vec<RecallHitRow>,
    pub dreams: Vec<DreamRow>,
    pub due: Vec<RealityCheckRow>,
    pub memories: Vec<MemoryRow>,
    pub trust_artifact: Option<TrustArtifact>,
}

impl DaemonSnapshot {
    pub fn loading(socket_path: &Path) -> Self {
        let mut snapshot = Self::empty();
        snapshot.daemon_state = format!("loading {}", socket_path.display());
        snapshot
    }

    pub fn empty() -> Self {
        Self {
            version: "v1.0.0".to_string(),
            footer_hint: "?:help  q:quit".to_string(),
            daemon_state: "loading".to_string(),
            review_queue: Vec::new(),
            conflicts: Vec::new(),
            recall: Vec::new(),
            dreams: Vec::new(),
            due: Vec::new(),
            memories: Vec::new(),
            trust_artifact: None,
        }
    }

    pub fn sample() -> Self {
        Self {
            version: "v1.0.0".to_string(),
            footer_hint: "?:help  q:quit".to_string(),
            daemon_state: "running".to_string(),
            review_queue: vec![
                ReviewQueueRow {
                    id: "mem_20260501_0123456789abcdef_000001".to_string(),
                    title: "Prefer CITEXT for email columns".to_string(),
                    namespace: "project:atlasos".to_string(),
                    status: "candidate".to_string(),
                    reason: Some("requires_user_confirmation".to_string()),
                },
                ReviewQueueRow {
                    id: "mem_20260501_0123456789abcdef_000007".to_string(),
                    title: "Dream candidate needs confirmation".to_string(),
                    namespace: "project:agent-memory".to_string(),
                    status: "dream_low_confidence".to_string(),
                    reason: Some("dream_low_confidence".to_string()),
                },
            ],
            conflicts: vec![ConflictRow {
                id: "mem_20260501_0123456789abcdef_000002".to_string(),
                title: "Database connection pool size".to_string(),
                namespace: "project:atlasos".to_string(),
                reason: Some("Pool size: 20 vs Pool size: 30".to_string()),
            }],
            recall: vec![RecallHitRow {
                id: "mem_20260501_0123456789abcdef_000009".to_string(),
                title: "Deploy target is production ECS".to_string(),
                namespace: "project:atlasos".to_string(),
                age: "11:02".to_string(),
            }],
            dreams: vec![DreamRow {
                id: "dream_project_20260501".to_string(),
                title: "Daily synthesis summary ready".to_string(),
                namespace: "project:agent-memory".to_string(),
            }],
            due: vec![RealityCheckRow {
                id: "mem_20260501_0123456789abcdef_000004".to_string(),
                title: "SSH key rotation every 90d".to_string(),
                namespace: "me".to_string(),
                score: "0.82".to_string(),
                breakdown: crate::state::ScoreBreakdown {
                    recency: 0.91,
                    recall_frequency: 0.20,
                    corroboration: 0.0,
                    confidence_decay: 0.65,
                    sensitivity: 1.0,
                },
            }],
            memories: vec![MemoryRow {
                id: "mem_20260501_0123456789abcdef_000010".to_string(),
                title: "Agent memory uses private daemon socket".to_string(),
                namespace: "agent".to_string(),
            }],
            trust_artifact: Some(sample_trust_artifact()),
        }
    }

    pub fn inbox_items(&self) -> Vec<InboxItem> {
        let mut sources = vec![
            self.conflicts
                .iter()
                .map(|row| InboxItem::Conflict {
                    id: row.id.clone(),
                    title: row.title.clone(),
                    namespace: row.namespace.clone(),
                    reason: row.reason.clone(),
                    age_label: "now".to_string(),
                })
                .collect(),
            self.due
                .iter()
                .map(|row| InboxItem::RealityCheckDue {
                    id: row.id.clone(),
                    title: row.title.clone(),
                    namespace: row.namespace.clone(),
                    score: row.score.clone(),
                    age_label: "due".to_string(),
                })
                .collect(),
            self.review_queue
                .iter()
                .map(|row| InboxItem::ReviewCandidate {
                    id: row.id.clone(),
                    title: row.title.clone(),
                    namespace: row.namespace.clone(),
                    reason: row.reason.clone(),
                    age_label: row.status.clone(),
                })
                .collect(),
            self.dreams
                .iter()
                .map(|row| InboxItem::DreamOutput {
                    id: row.id.clone(),
                    title: row.title.clone(),
                    namespace: row.namespace.clone(),
                    age_label: "today".to_string(),
                })
                .collect(),
            self.recall
                .iter()
                .map(|row| InboxItem::RecallHit {
                    id: row.id.clone(),
                    title: row.title.clone(),
                    namespace: row.namespace.clone(),
                    age_label: row.age.clone(),
                })
                .collect(),
            self.memories
                .iter()
                .map(|row| InboxItem::Memory {
                    id: row.id.clone(),
                    title: row.title.clone(),
                    namespace: row.namespace.clone(),
                    age_label: "active".to_string(),
                })
                .collect(),
        ];
        crate::inbox::ranking::merge_and_filter(std::mem::take(&mut sources), InboxFilter::All, 50)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewQueueRow {
    pub id: String,
    pub title: String,
    pub namespace: String,
    pub status: String,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictRow {
    pub id: String,
    pub title: String,
    pub namespace: String,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallHitRow {
    pub id: String,
    pub title: String,
    pub namespace: String,
    pub age: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DreamRow {
    pub id: String,
    pub title: String,
    pub namespace: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct RealityCheckRow {
    pub id: String,
    pub title: String,
    pub namespace: String,
    pub score: String,
    pub breakdown: crate::state::ScoreBreakdown,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRow {
    pub id: String,
    pub title: String,
    pub namespace: String,
}

fn sample_trust_artifact() -> TrustArtifact {
    serde_json::from_value(serde_json::json!({
        "id": "mem_20260501_0123456789abcdef_000009",
        "namespace": "project:atlasos",
        "status": "active",
        "sensitivity": "internal",
        "source": "substrate:projects/atlasos/deploy-target.md",
        "title": { "kind": "plaintext", "value": "Deploy target is production ECS" },
        "body": { "kind": "plaintext", "value": "The ECS cluster in us-east-1 is the production deployment target." },
        "current_confidence": "0.95",
        "original_confidence": "0.90",
        "confidence_reason": "user confirmed; policy-promoted",
        "trust_summary": "high trust; policy-promoted",
        "recall": { "total": 28, "last_30_days": 12, "last_recalled_at": "2026-05-01T11:02:00Z", "strength": "0.74" },
        "provenance_chain": [{ "timestamp": "2026-04-30T14:22:00Z", "kind": "write_committed", "summary": "written by codex-cli", "evidence": "sess_abc123", "device": "macbook" }],
        "policy_decisions": [{ "policy_applied": "project-standard@v2", "policy_source": "disk", "confidence_floor_pass": "pass", "grounding_satisfied": "2 source refs resolved", "contradiction_result": "none detected", "tombstone_enforced": "no matching tombstone", "sensitivity_gate_result": "pass" }],
        "privacy_scan": { "labels_detected": ["none"], "storage_action": "plaintext" },
        "supersedes": [],
        "superseded_by": [],
        "sync_state": { "devices": ["macbook"], "merge_status": "clean", "claim_lock_status": null }
    }))
    .expect("sample trust artifact fixture must match daemon DTO")
}
