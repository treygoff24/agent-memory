use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SimulatorConfig {
    socket_path: Option<PathBuf>,
    cwd: Option<PathBuf>,
    harness: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulatorAgent {
    config: SimulatorConfig,
    observations: SimulatorObservations,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SimulatorAction {
    Status,
    Startup { since_event_id: Option<String> },
    StartupWithBudget { since_event_id: Option<String>, budget_tokens: usize },
    Search { query: String, namespace: Option<String> },
    Write { body: String, title: Option<String>, meta: GovernanceMeta },
    WriteWithMetaJson { body: String, title: Option<String>, meta_json: String },
    Supersede { old_id: String, new_body: String, reason: String, meta: GovernanceMeta },
    Forget { id: String, reason: String },
    Get { id: String },
    Reveal { id: String, reason: String },
    ReviewQueue { limit: Option<usize> },
    Assert { condition: AssertionSpec },
    NewSession { cwd: Option<PathBuf>, harness: Option<String> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct GovernanceMeta {
    pub confidence: f64,
    pub source_kind: String,
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssertionSpec {
    LastWriteStatusIsNotRefused,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SimulatorObservations {
    pub last_status_json: Option<String>,
    pub last_startup_block: Option<String>,
    pub last_startup_json: Option<String>,
    pub last_search_result_count: Option<usize>,
    pub last_search_json: Option<String>,
    pub last_write_outcome: Option<String>,
    pub last_write_json: Option<String>,
    pub last_supersede_outcome: Option<String>,
    pub last_supersede_json: Option<String>,
    pub last_forget_outcome: Option<String>,
    pub last_forget_json: Option<String>,
    pub last_get_json: Option<String>,
    pub last_reveal_json: Option<String>,
    pub last_review_queue_json: Option<String>,
}

impl SimulatorConfig {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self { socket_path: Some(socket_path.as_ref().to_path_buf()), ..Self::default() }
    }
}

impl SimulatorAgent {
    pub fn new(config: SimulatorConfig) -> Self {
        Self { config, observations: SimulatorObservations::default() }
    }

    pub fn config(&self) -> &SimulatorConfig {
        &self.config
    }

    pub async fn run_script(&mut self, script: impl IntoIterator<Item = SimulatorAction>) -> SimulatorObservations {
        for action in script {
            self.run_action(action);
        }
        self.observations.clone()
    }

    fn run_action(&mut self, action: SimulatorAction) {
        match action {
            SimulatorAction::Status => self.status(),
            SimulatorAction::Startup { since_event_id } => self.startup(since_event_id),
            SimulatorAction::StartupWithBudget { since_event_id, budget_tokens } => {
                self.startup_with_budget(since_event_id, Some(budget_tokens));
            }
            SimulatorAction::Search { query, namespace } => self.search(&query, namespace.as_deref()),
            SimulatorAction::Write { body, title, meta } => self.write_memory(&body, title.as_deref(), &meta),
            SimulatorAction::WriteWithMetaJson { body, title, meta_json } => {
                self.write_memory_with_meta_json(&body, title.as_deref(), &meta_json);
            }
            SimulatorAction::Supersede { old_id, new_body, reason, meta } => {
                self.supersede(SupersedeRequest { old_id: &old_id, new_body: &new_body, reason: &reason, meta: &meta });
            }
            SimulatorAction::Forget { id, reason } => self.forget(&id, &reason),
            SimulatorAction::Get { id } => self.get(&id),
            SimulatorAction::Reveal { id, reason } => self.reveal(&id, &reason),
            SimulatorAction::ReviewQueue { limit } => self.review_queue(limit),
            SimulatorAction::Assert { condition } => self.assert(condition),
            SimulatorAction::NewSession { cwd, harness } => self.new_session(cwd, harness),
        }
    }

    fn status(&mut self) {
        self.observations.last_status_json = Some(self.request(r#""status""#.to_owned()));
    }

    fn startup(&mut self, since_event_id: Option<String>) {
        self.startup_with_budget(since_event_id, None);
    }

    fn startup_with_budget(&mut self, since_event_id: Option<String>, budget_tokens: Option<usize>) {
        let budget_tokens = budget_tokens.map_or_else(|| "null".to_owned(), |budget| budget.to_string());
        let request = format!(
            r#"{{"startup":{{"cwd":"{}","session_id":"{}","harness":"{}","harness_version":null,"include_recent":true,"since_event_id":{},"budget_tokens":{budget_tokens}}}}}"#,
            crate::json_escape(&self.cwd()),
            crate::json_escape(&self.session_id()),
            crate::json_escape(&self.harness()),
            optional_string_json(since_event_id.as_deref())
        );
        let response = self.request(request);
        self.observations.last_startup_block = extract_string_field(&response, "recall_block");
        self.observations.last_startup_json = Some(response);
    }

    fn search(&mut self, query: &str, namespace: Option<&str>) {
        let query = namespace.map_or_else(|| query.to_owned(), |ns| format!("{query} namespace:{ns}"));
        let response = self.request(format!(
            r#"{{"search":{{"query":"{}","limit":null,"include_body":true}}}}"#,
            crate::json_escape(&query)
        ));
        self.observations.last_search_result_count = extract_usize_field(&response, "total");
        self.observations.last_search_json = Some(response);
    }

    fn write_memory(&mut self, body: &str, title: Option<&str>, meta: &GovernanceMeta) {
        self.write_memory_with_meta_json(body, title, &meta.to_json(body));
    }

    fn write_memory_with_meta_json(&mut self, body: &str, title: Option<&str>, meta_json: &str) {
        let response = self.request(format!(
            r#"{{"write_memory":{{"body":"{}","title":{},"tags":[],"meta":{}}}}}"#,
            crate::json_escape(body),
            optional_string_json(title),
            meta_json
        ));
        self.observations.last_write_outcome = extract_string_field(&response, "status");
        self.observations.last_write_json = Some(response);
    }

    fn supersede(&mut self, request: SupersedeRequest<'_>) {
        let response = self.request(format!(
            r#"{{"supersede":{{"old_id":"{}","content":"{}","reason":"{}","meta":{}}}}}"#,
            crate::json_escape(request.old_id),
            crate::json_escape(request.new_body),
            crate::json_escape(request.reason),
            request.meta.to_json(request.new_body)
        ));
        self.observations.last_supersede_outcome = extract_string_field(&response, "status");
        self.observations.last_supersede_json = Some(response);
    }

    fn forget(&mut self, id: &str, reason: &str) {
        let response = self.request(format!(
            r#"{{"forget":{{"id":"{}","reason":"{}"}}}}"#,
            crate::json_escape(id),
            crate::json_escape(reason)
        ));
        self.observations.last_forget_outcome = extract_string_field(&response, "status");
        self.observations.last_forget_json = Some(response);
    }

    fn get(&mut self, id: &str) {
        self.observations.last_get_json =
            Some(self.request(format!(r#"{{"get":{{"id":"{}","include_provenance":true}}}}"#, crate::json_escape(id))));
    }

    fn reveal(&mut self, id: &str, reason: &str) {
        self.observations.last_reveal_json = Some(self.request(format!(
            r#"{{"reveal":{{"id":"{}","reason":"{}"}}}}"#,
            crate::json_escape(id),
            crate::json_escape(reason)
        )));
    }

    fn review_queue(&mut self, limit: Option<usize>) {
        let limit_json = limit.map_or_else(|| "null".to_owned(), |limit| limit.to_string());
        self.observations.last_review_queue_json =
            Some(self.request(format!(r#"{{"review_queue":{{"limit":{limit_json}}}}}"#)));
    }

    fn assert(&self, condition: AssertionSpec) {
        match condition {
            AssertionSpec::LastWriteStatusIsNotRefused => assert_ne!(
                self.observations.last_write_outcome.as_deref(),
                Some("refused"),
                "expected last write not to be refused: {:#?}",
                self.observations
            ),
        }
    }

    fn new_session(&mut self, cwd: Option<PathBuf>, harness: Option<String>) {
        self.config.session_id = Some(format!("memorum-eval-{}", next_id()));
        if let Some(cwd) = cwd {
            self.config.cwd = Some(cwd);
        }
        if let Some(harness) = harness {
            self.config.harness = Some(harness);
        }
    }

    fn request(&self, request: String) -> String {
        let socket_path = self.config.socket_path.as_ref().expect("SimulatorConfig socket_path is required");
        let mut stream = UnixStream::connect(socket_path)
            .unwrap_or_else(|err| panic!("connect to memoryd socket {}: {err}", socket_path.display()));
        let frame = format!(r#"{{"id":"{}","request":{request}}}"#, next_id());
        stream.write_all(frame.as_bytes()).expect("write simulator daemon request");
        stream.write_all(b"\n").expect("terminate simulator daemon request");

        let mut response = String::new();
        BufReader::new(stream).read_line(&mut response).expect("read simulator daemon response");
        response
    }

    fn cwd(&self) -> String {
        self.config
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")))
            .to_string_lossy()
            .into_owned()
    }

    fn session_id(&self) -> String {
        self.config.session_id.clone().unwrap_or_else(|| "memorum-eval-simulator".to_owned())
    }

    fn harness(&self) -> String {
        self.config.harness.clone().unwrap_or_else(|| "memorum-eval".to_owned())
    }
}

impl GovernanceMeta {
    fn to_json(&self, grounding_body: &str) -> String {
        format!(
            r#"{{"namespace":"project","type":"project","confidence":{},"source_kind":"{}","source_ref":{},"explicit_user_context":true}}"#,
            self.confidence,
            crate::json_escape(&self.source_kind),
            optional_string_json(self.daemon_source_ref(grounding_body).as_deref())
        )
    }

    fn daemon_source_ref(&self, grounding_body: &str) -> Option<String> {
        let source_ref = self.source_ref.as_deref()?;
        if self.source_kind != "agent_primary" || source_ref.starts_with("file:") {
            return Some(source_ref.to_owned());
        }

        let path = std::env::temp_dir().join(format!("memorum-eval-grounding-{}-{}", std::process::id(), source_ref));
        std::fs::write(&path, grounding_body)
            .unwrap_or_else(|err| panic!("write simulator grounding fixture {}: {err}", path.display()));
        Some(format!("file:{}#{source_ref}", path.display()))
    }
}

fn extract_string_field(json: &str, field: &str) -> Option<String> {
    let marker = format!(r#""{field}":"#);
    let start = json.find(&marker)? + marker.len();
    let rest = json.get(start..)?.strip_prefix('"')?;
    let mut value = String::new();
    let mut chars = rest.chars();
    while let Some(character) = chars.next() {
        match character {
            '"' => return Some(value),
            '\\' => match chars.next()? {
                '"' => value.push('"'),
                '\\' => value.push('\\'),
                '/' => value.push('/'),
                'b' => value.push('\u{0008}'),
                'f' => value.push('\u{000c}'),
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                'u' => {
                    let codepoint = parse_json_codepoint(&mut chars)?;
                    value.push(char::from_u32(codepoint)?);
                }
                escaped => value.push(escaped),
            },
            other => value.push(other),
        }
    }
    None
}

fn extract_usize_field(json: &str, field: &str) -> Option<usize> {
    let marker = format!(r#""{field}":"#);
    let start = json.find(&marker)? + marker.len();
    let digits: String = json[start..].chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn optional_string_json(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| format!(r#""{}""#, crate::json_escape(value)))
}

struct SupersedeRequest<'a> {
    old_id: &'a str,
    new_body: &'a str,
    reason: &'a str,
    meta: &'a GovernanceMeta,
}

fn parse_json_codepoint(chars: &mut impl Iterator<Item = char>) -> Option<u32> {
    let mut codepoint = 0;
    for _ in 0..4 {
        codepoint = (codepoint << 4) + chars.next()?.to_digit(16)?;
    }
    Some(codepoint)
}

fn next_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("sim-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))
}
