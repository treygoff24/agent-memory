use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsNotification {
    pub title: String,
    pub body: String,
}

pub trait OsNotificationSink: Send + Sync {
    fn send(&self, notification: &OsNotification) -> Result<(), String>;
}

#[derive(Clone)]
pub struct OsNotifier {
    sink: Option<Arc<dyn OsNotificationSink>>,
}

impl OsNotifier {
    pub fn detect() -> Self {
        match CommandOsNotificationSink::detect() {
            Some(sink) => Self { sink: Some(Arc::new(sink)) },
            None => {
                tracing::warn!("OS notifications configured but tool not found; falling back to passive");
                Self::disabled()
            }
        }
    }

    pub fn disabled() -> Self {
        Self { sink: None }
    }

    pub fn with_sink(sink: Arc<dyn OsNotificationSink>) -> Self {
        Self { sink: Some(sink) }
    }

    pub fn notify(&self, notification: &OsNotification) {
        let Some(sink) = &self.sink else {
            return;
        };
        if let Err(error) = sink.send(notification) {
            tracing::debug!("OS notification failed: {error}");
        }
    }
}

#[derive(Clone, Debug)]
enum OsNotificationTool {
    OsaScript(PathBuf),
    NotifySend(PathBuf),
}

#[derive(Clone, Debug)]
struct CommandOsNotificationSink {
    tool: OsNotificationTool,
}

impl CommandOsNotificationSink {
    fn detect() -> Option<Self> {
        if cfg!(target_os = "macos") {
            return which::which("osascript").ok().map(|path| Self { tool: OsNotificationTool::OsaScript(path) });
        }
        if cfg!(target_os = "linux") {
            return which::which("notify-send").ok().map(|path| Self { tool: OsNotificationTool::NotifySend(path) });
        }
        None
    }
}

impl OsNotificationSink for CommandOsNotificationSink {
    fn send(&self, notification: &OsNotification) -> Result<(), String> {
        let status = match &self.tool {
            OsNotificationTool::OsaScript(path) => Command::new(path)
                .arg("-e")
                .arg(format!("display notification {:?} with title {:?}", notification.body, notification.title))
                .status(),
            OsNotificationTool::NotifySend(path) => {
                Command::new(path).arg(&notification.title).arg(&notification.body).status()
            }
        }
        .map_err(|error| error.to_string())?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("command exited with {status}"))
        }
    }
}
