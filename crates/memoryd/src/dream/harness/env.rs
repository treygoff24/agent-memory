use std::{collections::BTreeMap, ffi::OsString, path::PathBuf, process::Command};

pub const DOCUMENTED_ENV_ALLOWLIST: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "HOME",
    "OPENAI_API_KEY",
    "PATH",
    "TERM",
    "USER",
];
// `USER` is required: Claude's claude.ai auth token lives in the macOS login
// keychain, and the keychain lookup keys off `USER`. Without it `claude auth
// status` reports `loggedIn:false` even with a valid `CLAUDE_CONFIG_DIR`, so the
// hardened dream subprocess could never authenticate. `USER` is public identity,
// not a credential, so forwarding it does not weaken the no-secret-leakage intent.
pub const CLAUDE_ENV_ALLOWLIST: &[&str] = &["ANTHROPIC_API_KEY", "CLAUDE_CONFIG_DIR", "HOME", "PATH", "TERM", "USER"];
pub const CODEX_ENV_ALLOWLIST: &[&str] = &["CODEX_HOME", "HOME", "OPENAI_API_KEY", "PATH", "TERM"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinimalEnvironment {
    values: BTreeMap<String, OsString>,
}

impl MinimalEnvironment {
    pub fn from_current(path_env: Option<OsString>) -> Self {
        let pairs = DOCUMENTED_ENV_ALLOWLIST
            .iter()
            .filter_map(|key| std::env::var_os(key).map(|value| ((*key).to_owned(), value)));
        let mut environment = Self { values: pairs.collect() };

        if let Some(path_env) = path_env {
            environment.values.insert("PATH".to_owned(), path_env);
        }
        environment.values.insert("TERM".to_owned(), OsString::from("dumb"));
        environment.retain_documented_keys_only();
        environment
    }

    pub fn from_pairs<K, V, I>(pairs: I) -> Self
    where
        K: Into<String>,
        V: Into<OsString>,
        I: IntoIterator<Item = (K, V)>,
    {
        let mut environment =
            Self { values: pairs.into_iter().map(|(key, value)| (key.into(), value.into())).collect() };
        environment.values.insert("TERM".to_owned(), OsString::from("dumb"));
        environment.retain_documented_keys_only();
        environment
    }

    pub fn retain_documented_keys_only(&mut self) {
        self.retain_keys(DOCUMENTED_ENV_ALLOWLIST);
    }

    pub fn retain_keys(&mut self, allowlist: &[&str]) {
        self.values.retain(|key, _| allowlist.contains(&key.as_str()));
        self.values.insert("TERM".to_owned(), OsString::from("dumb"));
    }

    pub fn for_adapter(path_env: Option<OsString>, allowlist: &[&str]) -> Self {
        let mut environment = Self::from_current(path_env);
        environment.retain_keys(allowlist);
        environment
    }

    /// Like [`Self::for_adapter`], but injects explicit key/value overrides after
    /// allowlist filtering. Overrides whose key is not in `allowlist` are
    /// dropped, so this can never widen the hardened subprocess environment
    /// beyond the adapter's allowlist (e.g. only `CLAUDE_CONFIG_DIR` is injected
    /// for the Claude adapter).
    pub fn for_adapter_with_overrides(
        path_env: Option<OsString>,
        allowlist: &[&str],
        overrides: &[(&str, OsString)],
    ) -> Self {
        let mut environment = Self::for_adapter(path_env, allowlist);
        for (key, value) in overrides {
            if allowlist.contains(key) {
                environment.values.insert((*key).to_owned(), value.clone());
            }
        }
        environment
    }

    /// Build the hardened adapter environment, optionally pinning the Claude
    /// profile via `CLAUDE_CONFIG_DIR`. When `config_dir` is `Some`, the directory
    /// is injected as an (allowlist-filtered) override so the auth probe and
    /// completion run against the same resolved profile; `None` forwards the
    /// allowlisted ambient environment unchanged.
    pub fn for_adapter_with_optional_config_dir(
        path_env: Option<OsString>,
        allowlist: &[&str],
        config_dir: Option<PathBuf>,
    ) -> Self {
        match config_dir {
            Some(dir) => {
                Self::for_adapter_with_overrides(path_env, allowlist, &[("CLAUDE_CONFIG_DIR", dir.into_os_string())])
            }
            None => Self::for_adapter(path_env, allowlist),
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }

    pub(super) fn apply_to(&self, command: &mut Command) {
        command.env_clear();
        for (key, value) in &self.values {
            command.env(key, value);
        }
    }
}

/// External-adapter execution context shared by the real CLI harnesses: whether
/// the binary is present, the PATH override used to find and run it, and the
/// environment-variable allowlist scoping its hardened subprocess.
pub(super) struct AdapterEnv {
    pub(super) installed: bool,
    pub(super) path_env: Option<OsString>,
    pub(super) allowlist: &'static [&'static str],
    /// When set, inject `CLAUDE_CONFIG_DIR=<dir>` (allowlist-filtered) into the
    /// hardened subprocess so the auth probe and completion run against the same
    /// resolved Claude profile. `None` forwards the ambient environment.
    pub(super) config_dir_override: Option<PathBuf>,
}

impl AdapterEnv {
    pub(super) fn min_env(&self) -> MinimalEnvironment {
        MinimalEnvironment::for_adapter_with_optional_config_dir(
            self.path_env.clone(),
            self.allowlist,
            self.config_dir_override.clone(),
        )
    }
}
