use std::sync::{Mutex, OnceLock};

use memorum_theme::Charset;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn charset_detects_utf8_and_posix() {
    let _guard = env_lock().lock().expect("env lock");
    std::env::set_var("LANG", "en_US.UTF-8");
    std::env::remove_var("LC_ALL");
    std::env::set_var("TERM", "xterm-256color");
    assert!(matches!(Charset::detect(), Charset::Extended | Charset::Full));
    std::env::set_var("LANG", "POSIX");
    assert_eq!(Charset::detect(), Charset::Minimal);
    std::env::set_var("LANG", "");
    assert_eq!(Charset::detect(), Charset::Minimal);
}
