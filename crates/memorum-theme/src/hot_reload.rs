use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::watch;

use crate::theme::parse_theme;
use crate::Theme;

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const NOTIFY_DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub struct HotReload {
    last_error: Arc<Mutex<Option<String>>>,
    _thread: thread::JoinHandle<()>,
}

impl HotReload {
    pub fn start(path: PathBuf, initial: Theme) -> (Self, watch::Receiver<Theme>) {
        let (theme_tx, theme_rx) = watch::channel(initial.clone());
        let last_error = Arc::new(Mutex::new(None));
        let error_slot = Arc::clone(&last_error);
        let thread = thread::spawn(move || watch_loop(path, initial, theme_tx, error_slot));
        (Self { last_error, _thread: thread }, theme_rx)
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().expect("hot-reload error lock poisoned").clone()
    }
}

fn watch_loop(
    path: PathBuf,
    mut last_theme: Theme,
    theme_tx: watch::Sender<Theme>,
    last_error: Arc<Mutex<Option<String>>>,
) {
    let (event_tx, event_rx) = mpsc::channel::<()>();
    spawn_notify_watcher(path.clone(), event_tx, Arc::clone(&last_error));
    loop {
        match event_rx.recv_timeout(POLL_INTERVAL) {
            Ok(()) => {
                reload_after_debounce(&path, &mut last_theme, &theme_tx, &last_error);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => reload_if_changed(&path, &mut last_theme, &theme_tx, &last_error),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                reload_if_changed(&path, &mut last_theme, &theme_tx, &last_error);
                thread::sleep(POLL_INTERVAL);
            }
        }
    }
}

fn spawn_notify_watcher(path: PathBuf, event_tx: mpsc::Sender<()>, last_error: Arc<Mutex<Option<String>>>) {
    thread::spawn(move || {
        let notify_error = Arc::clone(&last_error);
        let mut watcher = match RecommendedWatcher::new(
            move |event: notify::Result<Event>| match event {
                Ok(_event) => {
                    // Send errors when the watch_loop has dropped its receiver
                    // (HotReload was dropped). The watcher thread terminates
                    // shortly after, so the missed event is irrelevant.
                    let _ = event_tx.send(());
                }
                Err(error) => {
                    *notify_error.lock().expect("hot-reload error lock poisoned") = Some(error.to_string());
                }
            },
            Config::default(),
        ) {
            Ok(watcher) => watcher,
            Err(error) => {
                *last_error.lock().expect("hot-reload error lock poisoned") = Some(error.to_string());
                return;
            }
        };
        let watch_path = path.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        if let Err(error) = watcher.watch(&watch_path, RecursiveMode::NonRecursive) {
            *last_error.lock().expect("hot-reload error lock poisoned") = Some(error.to_string());
            return;
        }
        loop {
            thread::park_timeout(Duration::from_secs(60));
        }
    });
}

fn reload_after_debounce(
    path: &std::path::Path,
    last_theme: &mut Theme,
    theme_tx: &watch::Sender<Theme>,
    last_error: &Arc<Mutex<Option<String>>>,
) {
    std::thread::sleep(NOTIFY_DEBOUNCE);
    reload_if_changed(path, last_theme, theme_tx, last_error);
}

fn reload_if_changed(
    path: &std::path::Path,
    last_theme: &mut Theme,
    theme_tx: &watch::Sender<Theme>,
    last_error: &Arc<Mutex<Option<String>>>,
) {
    match std::fs::read_to_string(path)
        .map_err(|err| err.to_string())
        .and_then(|text| parse_theme(&text).map_err(|err| err.to_string()))
    {
        Ok(theme) => {
            *last_error.lock().expect("hot-reload error lock poisoned") = None;
            if &theme != last_theme {
                *last_theme = theme.clone();
                // Send errors when every watch::Receiver has been dropped;
                // the watcher thread continues running but no one is
                // listening, which is fine.
                let _ = theme_tx.send(theme);
            }
        }
        Err(error) => *last_error.lock().expect("hot-reload error lock poisoned") = Some(error),
    }
}
