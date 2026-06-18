//! Web dashboard child-process lifecycle: launcher traits, the runtime that owns the
//! spawned `memoryd-web` process, and the enable/disable/status request handlers.

use super::*;
use rand::RngCore;

const WEB_DASHBOARD_READY_TIMEOUT: Duration = Duration::from_millis(750);
const WEB_DASHBOARD_READY_POLL: Duration = Duration::from_millis(25);
const WEB_AUTH_TOKEN_BYTES: usize = 32;
/// Canonical name of the env var carrying the dashboard auth token from the
/// daemon to the spawned `memoryd-web` child. Re-exported as `memoryd::WEB_AUTH_ENV`
/// and consumed verbatim by `memoryd-web` so both sides of the handshake share one
/// literal — a one-sided rename would break the token handoff silently.
pub const WEB_AUTH_ENV: &str = "MEMORUM_WEB_AUTH_TOKEN";

trait WebDashboardLauncher: std::fmt::Debug + Send + Sync {
    fn ensure_port_available(&self, port: u16) -> Result<(), String>;
    fn spawn(&self, config: WebDashboardSpawnConfig<'_>) -> Result<Box<dyn WebDashboardChild>, String>;
    fn wait_until_ready(&self, port: u16, child: &mut dyn WebDashboardChild) -> Result<(), String>;
}

trait WebDashboardChild: std::fmt::Debug + Send {
    fn try_wait(&mut self) -> Result<Option<String>, String>;
    fn kill(&mut self) -> Result<(), String>;
    fn wait(&mut self) -> Result<(), String>;
}

#[derive(Debug)]
struct OsWebDashboardLauncher;

impl WebDashboardLauncher for OsWebDashboardLauncher {
    fn ensure_port_available(&self, port: u16) -> Result<(), String> {
        ensure_web_dashboard_port_available(port)
    }

    fn spawn(&self, config: WebDashboardSpawnConfig<'_>) -> Result<Box<dyn WebDashboardChild>, String> {
        let binary = resolve_memoryd_web_binary()?;
        let child = Command::new(binary)
            .arg("--socket")
            .arg(config.socket_path)
            .arg("--port")
            .arg(config.port.to_string())
            .arg("--repo")
            .arg(config.repo)
            .env(WEB_AUTH_ENV, config.auth_token)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("start memoryd-web: {error}"))?;
        Ok(Box::new(OsWebDashboardChild { child }))
    }

    fn wait_until_ready(&self, port: u16, child: &mut dyn WebDashboardChild) -> Result<(), String> {
        wait_for_web_dashboard_ready(port, child)
    }
}

#[derive(Debug)]
struct OsWebDashboardChild {
    child: Child,
}

impl WebDashboardChild for OsWebDashboardChild {
    fn try_wait(&mut self) -> Result<Option<String>, String> {
        self.child.try_wait().map(|status| status.map(|status| status.to_string())).map_err(|error| error.to_string())
    }

    fn kill(&mut self) -> Result<(), String> {
        self.child.kill().map_err(|error| error.to_string())
    }

    fn wait(&mut self) -> Result<(), String> {
        self.child.wait().map(drop).map_err(|error| error.to_string())
    }
}

#[derive(Debug)]
pub(crate) struct WebDashboardRuntime {
    port: Option<u16>,
    enabled_at: Option<chrono::DateTime<chrono::Utc>>,
    auth_token: Option<String>,
    child: Option<Box<dyn WebDashboardChild>>,
    launcher: Arc<dyn WebDashboardLauncher>,
}

#[derive(Clone, Copy)]
struct WebDashboardLaunchConfig<'a> {
    port: u16,
    socket_path: &'a str,
    repo: &'a Path,
}

#[derive(Clone, Copy)]
struct WebDashboardSpawnConfig<'a> {
    socket_path: &'a str,
    port: u16,
    repo: &'a Path,
    auth_token: &'a str,
}

impl Default for WebDashboardRuntime {
    fn default() -> Self {
        Self { port: None, enabled_at: None, auth_token: None, child: None, launcher: Arc::new(OsWebDashboardLauncher) }
    }
}

impl WebDashboardRuntime {
    #[cfg(test)]
    fn with_launcher(launcher: Arc<dyn WebDashboardLauncher>) -> Self {
        Self { port: None, enabled_at: None, auth_token: None, child: None, launcher }
    }

    fn enable(
        &mut self,
        launch: WebDashboardLaunchConfig<'_>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<WebDashboardStatus, HandlerError> {
        if self.child.as_mut().is_some_and(|child| child.try_wait().ok().flatten().is_none())
            && self.port == Some(launch.port)
        {
            return Ok(self.enable_status(now));
        }
        self.stop_child();
        self.launcher.ensure_port_available(launch.port).map_err(HandlerError::port_in_use)?;
        let auth_token = generate_web_auth_token();
        let mut child = self
            .launcher
            .spawn(WebDashboardSpawnConfig {
                socket_path: launch.socket_path,
                port: launch.port,
                repo: launch.repo,
                auth_token: &auth_token,
            })
            .map_err(HandlerError::web_unavailable)?;
        if let Err(error) = self.launcher.wait_until_ready(launch.port, child.as_mut()) {
            terminate_web_dashboard_child(child);
            return Err(HandlerError::web_unavailable(error));
        }
        self.port = Some(launch.port);
        self.enabled_at = Some(now);
        self.auth_token = Some(auth_token);
        self.child = Some(child);
        Ok(self.enable_status(now))
    }

    fn disable(&mut self) -> WebDashboardStatus {
        self.stop_child();
        self.port = None;
        self.enabled_at = None;
        self.auth_token = None;
        WebDashboardStatus::stopped()
    }

    fn status(&self, now: chrono::DateTime<chrono::Utc>) -> WebDashboardStatus {
        let Some(port) = self.port else {
            return WebDashboardStatus::stopped();
        };
        let uptime_seconds = self
            .enabled_at
            .map(|started_at| now.signed_duration_since(started_at).num_seconds().max(0) as u64)
            .unwrap_or(0);
        WebDashboardStatus::running(port, uptime_seconds)
    }

    fn enable_status(&self, now: chrono::DateTime<chrono::Utc>) -> WebDashboardStatus {
        let Some(port) = self.port else {
            return WebDashboardStatus::stopped();
        };
        let uptime_seconds = self
            .enabled_at
            .map(|started_at| now.signed_duration_since(started_at).num_seconds().max(0) as u64)
            .unwrap_or(0);
        match self.auth_token.as_deref() {
            Some(auth_token) => WebDashboardStatus::running_with_launch_url(port, uptime_seconds, auth_token),
            None => WebDashboardStatus::running(port, uptime_seconds),
        }
    }

    fn refresh_status(&mut self, now: chrono::DateTime<chrono::Utc>) -> WebDashboardStatus {
        if self.child.as_mut().and_then(|child| child.try_wait().ok().flatten()).is_some() {
            self.child = None;
            self.port = None;
            self.enabled_at = None;
            self.auth_token = None;
            return WebDashboardStatus::stopped();
        }
        self.status(now)
    }

    fn stop_child(&mut self) {
        let Some(child) = self.child.take() else {
            return;
        };
        terminate_web_dashboard_child(child);
    }
}

fn generate_web_auth_token() -> String {
    let mut bytes = [0_u8; WEB_AUTH_TOKEN_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn ensure_web_dashboard_port_available(port: u16) -> Result<(), String> {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpListener::bind(address)
        .map(drop)
        .map_err(|error| format!("web dashboard port {address} is unavailable before start: {error}"))
}

fn wait_for_web_dashboard_ready(port: u16, child: &mut dyn WebDashboardChild) -> Result<(), String> {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let started_at = Instant::now();
    while started_at.elapsed() < WEB_DASHBOARD_READY_TIMEOUT {
        if let Some(status) =
            child.try_wait().map_err(|error| format!("check memoryd-web readiness status: {error}"))?
        {
            return Err(format!("memoryd-web exited before binding {address}: {status}"));
        }
        if TcpStream::connect_timeout(&address, WEB_DASHBOARD_READY_POLL).is_ok() {
            return Ok(());
        }
        std::thread::sleep(WEB_DASHBOARD_READY_POLL);
    }
    Err(format!("memoryd-web did not bind {address} before readiness timeout"))
}

fn terminate_web_dashboard_child(mut child: Box<dyn WebDashboardChild>) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
}

impl Drop for WebDashboardRuntime {
    fn drop(&mut self) {
        self.stop_child();
    }
}

pub(crate) fn web_enable_response(
    substrate: &Substrate,
    state: &HandlerState,
    port: u16,
    socket_path: &str,
) -> Result<ResponsePayload, HandlerError> {
    if port < 1024 {
        return Err(HandlerError::invalid_request("web dashboard port must be in 1024..=65535"));
    }
    let mut dashboard = state.web_dashboard.lock().expect("web dashboard lock poisoned");
    Ok(ResponsePayload::WebStatus(dashboard.enable(
        WebDashboardLaunchConfig { port, socket_path, repo: substrate.roots().repo.as_path() },
        chrono::Utc::now(),
    )?))
}

pub(crate) fn web_disable_response(state: &HandlerState) -> Result<ResponsePayload, HandlerError> {
    let mut dashboard = state.web_dashboard.lock().expect("web dashboard lock poisoned");
    Ok(ResponsePayload::WebStatus(dashboard.disable()))
}

pub(crate) fn web_status_response(state: &HandlerState) -> Result<ResponsePayload, HandlerError> {
    let mut dashboard = state.web_dashboard.lock().expect("web dashboard lock poisoned");
    Ok(ResponsePayload::WebStatus(dashboard.refresh_status(chrono::Utc::now())))
}

fn resolve_memoryd_web_binary() -> Result<PathBuf, String> {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(sibling) = current_exe.parent().map(|dir| dir.join("memoryd-web")).filter(|path| path.is_file()) {
            return Ok(sibling);
        }
    }
    let Some(path_env) = std::env::var_os("PATH") else {
        return Err("memoryd-web binary not found on PATH".to_owned());
    };
    std::env::split_paths(&path_env)
        .map(|dir| dir.join("memoryd-web"))
        .find(|path| path.is_file())
        .ok_or_else(|| "memoryd-web binary not found on PATH".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct LaunchRecord {
        program: String,
        args: Vec<String>,
        auth_token: String,
    }

    #[derive(Clone, Debug)]
    struct FakeChildHandle {
        state: Arc<Mutex<FakeChildState>>,
    }

    #[derive(Clone, Debug, Default)]
    struct FakeChildState {
        running: bool,
        killed: bool,
        waited: bool,
    }

    #[derive(Debug)]
    struct FakeWebDashboardLauncher {
        readiness: FakeReadiness,
        launches: Mutex<Vec<LaunchRecord>>,
        children: Mutex<Vec<FakeChildHandle>>,
    }

    #[derive(Debug)]
    enum FakeReadiness {
        Ready,
        ExitedBeforeBinding,
        Timeout,
    }

    impl FakeWebDashboardLauncher {
        fn ready() -> Self {
            Self::new(FakeReadiness::Ready)
        }

        fn exited_before_binding() -> Self {
            Self::new(FakeReadiness::ExitedBeforeBinding)
        }

        fn timeout() -> Self {
            Self::new(FakeReadiness::Timeout)
        }

        fn new(readiness: FakeReadiness) -> Self {
            Self { readiness, launches: Mutex::new(Vec::new()), children: Mutex::new(Vec::new()) }
        }

        fn launches(&self) -> Vec<LaunchRecord> {
            self.launches.lock().expect("launches lock poisoned").clone()
        }

        fn only_child(&self) -> FakeChildState {
            let children = self.children.lock().expect("children lock poisoned");
            let child = children.first().expect("launcher recorded child");
            let state = child.state.lock().expect("child state lock poisoned").clone();
            state
        }
    }

    impl WebDashboardLauncher for FakeWebDashboardLauncher {
        fn ensure_port_available(&self, _port: u16) -> Result<(), String> {
            Ok(())
        }

        fn spawn(&self, config: WebDashboardSpawnConfig<'_>) -> Result<Box<dyn WebDashboardChild>, String> {
            self.launches.lock().expect("launches lock poisoned").push(LaunchRecord {
                program: "memoryd-web".to_owned(),
                args: vec![
                    "--socket".to_owned(),
                    config.socket_path.to_owned(),
                    "--port".to_owned(),
                    config.port.to_string(),
                    "--repo".to_owned(),
                    config.repo.display().to_string(),
                ],
                auth_token: config.auth_token.to_owned(),
            });
            let state = Arc::new(Mutex::new(FakeChildState {
                running: matches!(self.readiness, FakeReadiness::Ready | FakeReadiness::Timeout),
                killed: false,
                waited: false,
            }));
            self.children.lock().expect("children lock poisoned").push(FakeChildHandle { state: Arc::clone(&state) });
            Ok(Box::new(FakeWebDashboardChild { state }))
        }

        fn wait_until_ready(&self, port: u16, child: &mut dyn WebDashboardChild) -> Result<(), String> {
            let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
            match self.readiness {
                FakeReadiness::Ready => Ok(()),
                FakeReadiness::ExitedBeforeBinding => {
                    let status = child.try_wait()?.expect("fake child exited before binding");
                    Err(format!("memoryd-web exited before binding {address}: {status}"))
                }
                FakeReadiness::Timeout => Err(format!("memoryd-web did not bind {address} before readiness timeout")),
            }
        }
    }

    #[derive(Debug)]
    struct FakeWebDashboardChild {
        state: Arc<Mutex<FakeChildState>>,
    }

    impl WebDashboardChild for FakeWebDashboardChild {
        fn try_wait(&mut self) -> Result<Option<String>, String> {
            let state = self.state.lock().expect("child state lock poisoned");
            Ok((!state.running).then(|| "exit status: 1".to_owned()))
        }

        fn kill(&mut self) -> Result<(), String> {
            let mut state = self.state.lock().expect("child state lock poisoned");
            state.running = false;
            state.killed = true;
            Ok(())
        }

        fn wait(&mut self) -> Result<(), String> {
            self.state.lock().expect("child state lock poisoned").waited = true;
            Ok(())
        }
    }

    fn unused_localhost_port() -> u16 {
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).expect("test listener binds");
        listener.local_addr().expect("test listener has local address").port()
    }

    fn web_launch_config<'a>(port: u16, socket_path: &'a str, repo: &'a Path) -> WebDashboardLaunchConfig<'a> {
        WebDashboardLaunchConfig { port, socket_path, repo }
    }

    #[test]
    fn web_dashboard_enable_success_records_running_status_and_spawn_argv() {
        let launcher = Arc::new(FakeWebDashboardLauncher::ready());
        let mut runtime = WebDashboardRuntime::with_launcher(launcher.clone());
        let port = unused_localhost_port();
        let socket_path = "/tmp/memoryd-test.sock";
        let repo = Path::new("/tmp/memoryd-test-repo");

        let status =
            runtime.enable(web_launch_config(port, socket_path, repo), chrono::Utc::now()).expect("dashboard starts");

        assert!(status.running);
        assert_eq!(status.port, Some(port));
        let public_url = format!("http://localhost:{port}");
        assert_eq!(status.url.as_deref(), Some(public_url.as_str()));
        let launch_url = status.launch_url.as_deref().expect("enable status includes launch URL");
        let auth_token = launch_url
            .strip_prefix(&format!("http://localhost:{port}/?auth="))
            .expect("launch URL includes auth query");
        assert_eq!(auth_token.len(), WEB_AUTH_TOKEN_BYTES * 2);
        assert!(auth_token.chars().all(|character| character.is_ascii_hexdigit()));
        assert_eq!(runtime.status(chrono::Utc::now()).launch_url, None, "status must not expose bearer token");
        assert_eq!(
            launcher.launches(),
            vec![LaunchRecord {
                program: "memoryd-web".to_owned(),
                args: vec![
                    "--socket".to_owned(),
                    socket_path.to_owned(),
                    "--port".to_owned(),
                    port.to_string(),
                    "--repo".to_owned(),
                    repo.display().to_string(),
                ],
                auth_token: auth_token.to_owned(),
            }]
        );
    }

    #[test]
    fn web_dashboard_enable_child_exit_before_binding_cleans_up_and_stops_status() {
        let launcher = Arc::new(FakeWebDashboardLauncher::exited_before_binding());
        let mut runtime = WebDashboardRuntime::with_launcher(launcher.clone());

        let error = runtime
            .enable(
                web_launch_config(
                    unused_localhost_port(),
                    "/tmp/memoryd-test.sock",
                    Path::new("/tmp/memoryd-test-repo"),
                ),
                chrono::Utc::now(),
            )
            .expect_err("start fails");

        assert_eq!(error.code, "web_unavailable");
        assert!(error.message.contains("exited before binding"));
        assert!(!runtime.status(chrono::Utc::now()).running);
        let child = launcher.only_child();
        assert!(!child.killed);
        assert!(child.waited);
    }

    #[test]
    fn web_dashboard_enable_readiness_timeout_kills_child_and_stops_status() {
        let launcher = Arc::new(FakeWebDashboardLauncher::timeout());
        let mut runtime = WebDashboardRuntime::with_launcher(launcher.clone());

        let error = runtime
            .enable(
                web_launch_config(
                    unused_localhost_port(),
                    "/tmp/memoryd-test.sock",
                    Path::new("/tmp/memoryd-test-repo"),
                ),
                chrono::Utc::now(),
            )
            .expect_err("start fails");

        assert_eq!(error.code, "web_unavailable");
        assert!(error.message.contains("did not bind"));
        assert!(!runtime.status(chrono::Utc::now()).running);
        let child = launcher.only_child();
        assert!(child.killed);
        assert!(child.waited);
    }

    #[test]
    fn web_dashboard_enable_same_live_port_is_idempotent_without_second_spawn() {
        let launcher = Arc::new(FakeWebDashboardLauncher::ready());
        let mut runtime = WebDashboardRuntime::with_launcher(launcher.clone());
        let port = unused_localhost_port();
        let repo = Path::new("/tmp/memoryd-test-repo");

        let first = runtime
            .enable(web_launch_config(port, "/tmp/memoryd-test.sock", repo), chrono::Utc::now())
            .expect("dashboard starts");
        let second = runtime
            .enable(web_launch_config(port, "/tmp/memoryd-test.sock", repo), chrono::Utc::now())
            .expect("dashboard is reused");

        assert!(first.running);
        assert!(second.running);
        assert_eq!(second.port, Some(port));
        assert_eq!(first.url, second.url, "idempotent enable must preserve the same public URL");
        assert_eq!(first.launch_url, second.launch_url, "idempotent enable must preserve the same launch URL");
        assert_eq!(runtime.status(chrono::Utc::now()).launch_url, None, "status must not expose bearer token");
        assert_eq!(launcher.launches().len(), 1);
    }

    #[test]
    fn web_dashboard_enable_rejects_preoccupied_port_before_spawn() {
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).expect("test listener binds");
        let port = listener.local_addr().expect("test listener has local address").port();
        let mut runtime = WebDashboardRuntime::default();

        let error = runtime
            .enable(
                web_launch_config(port, "/tmp/memoryd-test.sock", Path::new("/tmp/memoryd-test-repo")),
                chrono::Utc::now(),
            )
            .expect_err("port is rejected");

        assert_eq!(error.code, "port_in_use");
        assert!(error.message.contains("is unavailable before start"));
        assert!(!runtime.status(chrono::Utc::now()).running);
    }
}
