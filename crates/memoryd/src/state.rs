use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const STATE_SCHEMA_VERSION: u32 = 1;
const STATE_DIR: &str = "state";
const DAEMON_STATE_FILE: &str = "state.json";
const PENDING_FILE: &str = "reality-check-pending.json";
const SESSION_FILE: &str = "reality-check-session.json";
const HISTORY_FILE: &str = "reality-check-history.json";
const PENDING_TTL: Duration = Duration::minutes(30);
const SESSION_TTL: Duration = Duration::days(7);
const HISTORY_TTL: Duration = Duration::days(90);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DaemonState {
    #[serde(default = "state_schema_version")]
    pub version: u32,
    #[serde(default)]
    pub reality_check: RealityCheckState,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self { version: STATE_SCHEMA_VERSION, reality_check: RealityCheckState::default() }
    }
}

impl DaemonState {
    pub fn load(runtime_root: impl AsRef<Path>) -> Self {
        let report = Self::load_with_report(runtime_root);
        if let Some(failure) = &report.failure {
            eprintln!("warning: failed to load daemon state: {failure}");
        }
        report.state
    }

    pub fn load_with_report(runtime_root: impl AsRef<Path>) -> DaemonStateLoadReport {
        match load_versioned_json_result::<Self>(&state_file(runtime_root.as_ref(), DAEMON_STATE_FILE)) {
            Ok(Some(state)) => DaemonStateLoadReport { state, failure: None },
            Ok(None) => DaemonStateLoadReport { state: Self::default(), failure: None },
            Err(failure) => DaemonStateLoadReport { state: Self::default(), failure: Some(failure) },
        }
    }

    pub fn save(&self, runtime_root: impl AsRef<Path>) -> std::io::Result<()> {
        let mut state = self.clone();
        state.version = STATE_SCHEMA_VERSION;
        atomic_write_json(&state_dir(runtime_root.as_ref()), DAEMON_STATE_FILE, &state)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonStateLoadReport {
    pub state: DaemonState,
    pub failure: Option<StateLoadFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateLoadFailure {
    Read { path: PathBuf, message: String },
    Parse { path: PathBuf, message: String },
    VersionMismatch { expected: u32, actual: u32 },
}

impl std::fmt::Display for StateLoadFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, message } => write!(formatter, "read {}: {message}", path.display()),
            Self::Parse { path, message } => write!(formatter, "parse {}: {message}", path.display()),
            Self::VersionMismatch { expected, actual } => {
                write!(formatter, "state version mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl VersionedStateFile for DaemonState {
    fn version(&self) -> u32 {
        self.version
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RealityCheckState {
    #[serde(default)]
    pub last_completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub snooze_until: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RcPendingCache {
    #[serde(default = "state_schema_version")]
    pub version: u32,
    pub computed_at: DateTime<Utc>,
    #[serde(default)]
    pub items: Vec<Value>,
}

impl Default for RcPendingCache {
    fn default() -> Self {
        Self { version: STATE_SCHEMA_VERSION, computed_at: Utc::now(), items: Vec::new() }
    }
}

impl RcPendingCache {
    pub fn load(runtime_root: impl AsRef<Path>) -> Option<Self> {
        load_versioned_json(&state_file(runtime_root.as_ref(), PENDING_FILE))
    }

    pub fn save(&self, runtime_root: impl AsRef<Path>) -> std::io::Result<()> {
        let mut cache = self.clone();
        cache.version = STATE_SCHEMA_VERSION;
        atomic_write_json(&state_dir(runtime_root.as_ref()), PENDING_FILE, &cache)
    }

    pub fn delete(runtime_root: impl AsRef<Path>) -> std::io::Result<()> {
        delete_if_exists(&state_file(runtime_root.as_ref(), PENDING_FILE))
    }

    pub fn is_fresh(&self, now: DateTime<Utc>) -> bool {
        now.signed_duration_since(self.computed_at) <= PENDING_TTL
    }
}

impl VersionedStateFile for RcPendingCache {
    fn version(&self) -> u32 {
        self.version
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RcSessionState {
    #[serde(default = "state_schema_version")]
    pub version: u32,
    #[serde(default)]
    pub session_id: String,
    pub started_at: DateTime<Utc>,
    #[serde(default)]
    pub items_total: usize,
    #[serde(default)]
    pub items_reviewed: Vec<String>,
    #[serde(default)]
    pub items_confirmed: Vec<String>,
    #[serde(default)]
    pub items_corrected: Vec<String>,
    #[serde(default)]
    pub items_forgotten: Vec<String>,
    #[serde(default)]
    pub items_not_relevant: Vec<String>,
    #[serde(default)]
    pub items_deferred: Vec<String>,
    #[serde(default)]
    pub items_remaining: Vec<String>,
    #[serde(default)]
    pub current_index: usize,
}

impl Default for RcSessionState {
    fn default() -> Self {
        Self {
            version: STATE_SCHEMA_VERSION,
            session_id: String::new(),
            started_at: Utc::now(),
            items_total: 0,
            items_reviewed: Vec::new(),
            items_confirmed: Vec::new(),
            items_corrected: Vec::new(),
            items_forgotten: Vec::new(),
            items_not_relevant: Vec::new(),
            items_deferred: Vec::new(),
            items_remaining: Vec::new(),
            current_index: 0,
        }
    }
}

impl VersionedStateFile for RcSessionState {
    fn version(&self) -> u32 {
        self.version
    }
}

#[derive(Clone, Debug)]
pub struct RcSessionStore {
    runtime_root: PathBuf,
}

impl RcSessionStore {
    pub fn new(runtime_root: impl AsRef<Path>) -> Self {
        Self { runtime_root: runtime_root.as_ref().to_path_buf() }
    }

    pub fn load_if_recent(&self, now: DateTime<Utc>) -> std::io::Result<Option<RcSessionState>> {
        let path = self.session_path();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Ok(None),
        };

        let session = match serde_json::from_str::<RcSessionState>(&text) {
            Ok(session) if session.version == STATE_SCHEMA_VERSION => session,
            Ok(_) | Err(_) => {
                rename_corrupt_session(&path)?;
                return Ok(None);
            }
        };

        if now.signed_duration_since(session.started_at) > SESSION_TTL {
            delete_if_exists(&path)?;
            return Ok(None);
        }

        Ok(Some(session))
    }

    pub fn save(&self, session: &RcSessionState) -> std::io::Result<()> {
        let mut session = session.clone();
        session.version = STATE_SCHEMA_VERSION;
        atomic_write_json(&state_dir(&self.runtime_root), SESSION_FILE, &session)
    }

    pub fn delete(&self) -> std::io::Result<()> {
        delete_if_exists(&self.session_path())
    }

    fn session_path(&self) -> PathBuf {
        state_file(&self.runtime_root, SESSION_FILE)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RcSessionHistory {
    #[serde(default = "state_schema_version")]
    pub version: u32,
    #[serde(default)]
    pub sessions: Vec<RcSessionHistoryEntry>,
}

impl VersionedStateFile for RcSessionHistory {
    fn version(&self) -> u32 {
        self.version
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RcSessionHistoryEntry {
    pub session_id: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub items_total: usize,
    pub reviewed: u32,
    pub confirmed: u32,
    pub corrected: u32,
    pub forgotten: u32,
    pub not_relevant: u32,
    pub deferred: u32,
    pub remaining: u32,
}

#[derive(Clone, Debug)]
pub struct RcHistoryStore {
    runtime_root: PathBuf,
}

impl RcHistoryStore {
    pub fn new(runtime_root: impl AsRef<Path>) -> Self {
        Self { runtime_root: runtime_root.as_ref().to_path_buf() }
    }

    pub fn load(&self, now: DateTime<Utc>, limit: Option<usize>) -> std::io::Result<RcSessionHistory> {
        let mut history = load_versioned_json_result::<RcSessionHistory>(&state_file(&self.runtime_root, HISTORY_FILE))
            .map_err(|failure| std::io::Error::other(failure.to_string()))?
            .unwrap_or_default();
        history.sessions.retain(|entry| now.signed_duration_since(entry.completed_at) <= HISTORY_TTL);
        history.sessions.sort_by(|left, right| right.completed_at.cmp(&left.completed_at));
        if let Some(limit) = limit {
            history.sessions.truncate(limit);
        }
        Ok(history)
    }

    pub fn append_completed(
        &self,
        session: &RcSessionState,
        completed_at: DateTime<Utc>,
    ) -> std::io::Result<RcSessionHistoryEntry> {
        let mut history = self.load(completed_at, None)?;
        let entry = RcSessionHistoryEntry {
            session_id: session.session_id.clone(),
            started_at: session.started_at,
            completed_at,
            items_total: session.items_total,
            reviewed: session.items_reviewed.len() as u32,
            confirmed: session.items_confirmed.len() as u32,
            corrected: session.items_corrected.len() as u32,
            forgotten: session.items_forgotten.len() as u32,
            not_relevant: session.items_not_relevant.len() as u32,
            deferred: session.items_deferred.len() as u32,
            remaining: session.items_remaining.len() as u32,
        };
        history.sessions.retain(|existing| existing.session_id != entry.session_id);
        history.sessions.push(entry.clone());
        history.sessions.sort_by(|left, right| right.completed_at.cmp(&left.completed_at));
        let mut persisted = history;
        persisted.version = STATE_SCHEMA_VERSION;
        atomic_write_json(&state_dir(&self.runtime_root), HISTORY_FILE, &persisted)?;
        Ok(entry)
    }
}

trait VersionedStateFile {
    fn version(&self) -> u32;
}

fn load_versioned_json<T>(path: &Path) -> Option<T>
where
    T: for<'de> Deserialize<'de> + VersionedStateFile,
{
    load_versioned_json_result(path).ok().flatten()
}

fn load_versioned_json_result<T>(path: &Path) -> Result<Option<T>, StateLoadFailure>
where
    T: for<'de> Deserialize<'de> + VersionedStateFile,
{
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(StateLoadFailure::Read { path: path.to_path_buf(), message: error.to_string() });
        }
    };
    let state = serde_json::from_str::<T>(&text)
        .map_err(|error| StateLoadFailure::Parse { path: path.to_path_buf(), message: error.to_string() })?;
    if state.version() != STATE_SCHEMA_VERSION {
        return Err(StateLoadFailure::VersionMismatch { expected: STATE_SCHEMA_VERSION, actual: state.version() });
    }
    Ok(Some(state))
}

fn atomic_write_json<T>(dir: &Path, file_name: &str, value: &T) -> std::io::Result<()>
where
    T: Serialize,
{
    fs::create_dir_all(dir)?;
    let final_path = dir.join(file_name);
    let temp_path = dir.join(format!("{file_name}.tmp"));
    delete_if_exists(&temp_path)?;

    let mut bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    bytes.push(b'\n');

    let mut file = OpenOptions::new().create_new(true).write(true).open(&temp_path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(&temp_path, &final_path)?;
    File::open(dir)?.sync_all()?;
    Ok(())
}

fn rename_corrupt_session(path: &Path) -> std::io::Result<()> {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Ok(());
    };
    let suffix = Utc::now().timestamp_micros();
    let corrupt_path = path.with_file_name(format!("{file_name}.corrupt-{suffix}"));
    fs::rename(path, corrupt_path)
}

fn delete_if_exists(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn state_dir(runtime_root: &Path) -> PathBuf {
    runtime_root.join(STATE_DIR)
}

fn state_file(runtime_root: &Path, file_name: &str) -> PathBuf {
    state_dir(runtime_root).join(file_name)
}

fn state_schema_version() -> u32 {
    STATE_SCHEMA_VERSION
}
