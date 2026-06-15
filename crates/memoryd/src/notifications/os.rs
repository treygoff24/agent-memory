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

/// Fixed AppleScript run handler. Contains no caller data: the notification
/// `body` and `title` arrive as `argv` items bound by AppleScript at runtime
/// (passed as positional args after this `-e` script), so adversarial `"`/`\`
/// in the text can never escape into the script source.
const OSASCRIPT_NOTIFICATION_SCRIPT: &str =
    "on run argv\ndisplay notification (item 1 of argv) with title (item 2 of argv)\nend run";

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
            // AppleScript injection: `title`/`body` are passed to `osascript`
            // out-of-band as positional `argv` items, NOT spliced into the script
            // source. The script body below is a fixed string literal that contains
            // no caller data; the dynamic text is bound by AppleScript's runtime as
            // `item N of argv`, so there is no string-literal context for a `"` or
            // `\` to break out of and no dependence on any escaping coincidence.
            // This holds even if a future caller routes poisoned/memory-derived
            // free text into `title`/`body`.
            OsNotificationTool::OsaScript(path) => Command::new(path)
                .arg("-e")
                .arg(OSASCRIPT_NOTIFICATION_SCRIPT)
                // argv items follow the script; order matches `item 1`/`item 2` above.
                .arg(&notification.body)
                .arg(&notification.title)
                .status(),
            OsNotificationTool::NotifySend(path) => {
                // `--` ends option parsing so a title/body starting with `-` is positional data,
                // not a notify-send flag (-i/-u/-c/-h).
                Command::new(path).arg("--").arg(&notification.title).arg(&notification.body).status()
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

#[cfg(test)]
mod tests {
    use super::OSASCRIPT_NOTIFICATION_SCRIPT;

    #[test]
    fn osascript_payload_is_a_fixed_script_carrying_no_caller_data() {
        // The injection-safety invariant: the only thing handed to `osascript -e`
        // is this constant. It must bind the dynamic values via `argv` rather than
        // splice them into the source. No notification text appears in the script.
        assert!(OSASCRIPT_NOTIFICATION_SCRIPT.contains("on run argv"), "script must take args via argv");
        assert!(OSASCRIPT_NOTIFICATION_SCRIPT.contains("item 1 of argv"), "body must be bound from argv, not spliced");
        assert!(OSASCRIPT_NOTIFICATION_SCRIPT.contains("item 2 of argv"), "title must be bound from argv, not spliced");
    }

    #[test]
    fn osascript_script_contains_no_string_literal_breakout_surface() {
        // Because caller text never enters the script source, an adversarial
        // payload like `" & (do shell script "...") & "` cannot reach a string
        // literal context here. Guard against regression to the old splicing form:
        // the fixed script must not embed any double-quoted string literal that a
        // future edit might fill with caller data.
        assert!(
            !OSASCRIPT_NOTIFICATION_SCRIPT.contains('"'),
            "fixed script must hold no string literals that could carry caller text"
        );
    }
}
