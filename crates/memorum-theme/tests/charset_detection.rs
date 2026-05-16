use std::ffi::OsString;
use std::sync::{Mutex, OnceLock};

use memorum_theme::Charset;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn charset_detects_utf8_and_posix() {
    let _guard = env_lock().lock().expect("env lock");
    assert_eq!(detect_with_env("en_US.UTF-8", None, "ghostty"), Charset::Full);
    assert_eq!(detect_with_env("en_US.UTF-8", None, "xterm-256color"), Charset::Extended);
    assert_eq!(detect_with_env("POSIX", None, "xterm-256color"), Charset::Minimal);
    assert_eq!(detect_with_env("", None, "xterm-256color"), Charset::Minimal);
}

#[test]
fn charset_detection_restores_process_environment() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore_original = CharsetEnvGuard::capture();
    std::env::set_var("LANG", "sentinel-lang");
    std::env::set_var("LC_ALL", "sentinel-lc-all");
    std::env::set_var("TERM", "sentinel-term");

    {
        let _scoped = CharsetEnvGuard::set("en_US.UTF-8", None, "kitty");
        assert_eq!(Charset::detect(), Charset::Full);
    }

    assert_eq!(std::env::var_os("LANG"), Some(OsString::from("sentinel-lang")));
    assert_eq!(std::env::var_os("LC_ALL"), Some(OsString::from("sentinel-lc-all")));
    assert_eq!(std::env::var_os("TERM"), Some(OsString::from("sentinel-term")));
}

fn detect_with_env(lang: &str, lc_all: Option<&str>, term: &str) -> Charset {
    let _env = CharsetEnvGuard::set(lang, lc_all, term);
    Charset::detect()
}

struct CharsetEnvGuard {
    lang: Option<OsString>,
    lc_all: Option<OsString>,
    term: Option<OsString>,
}

impl CharsetEnvGuard {
    fn capture() -> Self {
        Self { lang: std::env::var_os("LANG"), lc_all: std::env::var_os("LC_ALL"), term: std::env::var_os("TERM") }
    }

    fn set(lang: &str, lc_all: Option<&str>, term: &str) -> Self {
        let previous = Self::capture();
        std::env::set_var("LANG", lang);
        match lc_all {
            Some(value) => std::env::set_var("LC_ALL", value),
            None => std::env::remove_var("LC_ALL"),
        }
        std::env::set_var("TERM", term);
        previous
    }

    fn restore_var(name: &str, value: &Option<OsString>) {
        if let Some(value) = value {
            std::env::set_var(name, value);
        } else {
            std::env::remove_var(name);
        }
    }
}

impl Drop for CharsetEnvGuard {
    fn drop(&mut self) {
        Self::restore_var("LANG", &self.lang);
        Self::restore_var("LC_ALL", &self.lc_all);
        Self::restore_var("TERM", &self.term);
    }
}
