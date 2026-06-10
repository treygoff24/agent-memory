use std::{collections::BTreeMap, sync::Arc};

use memorum_coordination::HarnessRegistry;

use crate::protocol::{HarnessCliStatus, PromptTransport};

use super::harness::{ClaudeCodeCli, CodexCli, HarnessCli};

pub struct HarnessCliRegistry {
    adapters: BTreeMap<&'static str, Arc<dyn HarnessCli>>,
    disabled_adapters: Vec<HarnessCliStatus>,
    identities: HarnessRegistry,
}

impl HarnessCliRegistry {
    pub fn builtin_v0_2() -> Self {
        let mut adapters: BTreeMap<&'static str, Arc<dyn HarnessCli>> = BTreeMap::new();
        adapters.insert("claude", Arc::new(ClaudeCodeCli::new()));
        adapters.insert("codex", Arc::new(CodexCli::new()));

        Self {
            adapters,
            identities: HarnessRegistry::builtin(),
            disabled_adapters: vec![HarnessCliStatus {
                name: "gemini".to_owned(),
                is_installed: false,
                is_authenticated: None,
                prompt_transport: PromptTransport::Stdin,
                last_probe_at: None,
                last_probe_error: Some("disabled in Stream F v0.2 until stdin support is proven".to_owned()),
            }],
        }
    }

    pub fn adapters(&self) -> impl Iterator<Item = (&'static str, &Arc<dyn HarnessCli>)> {
        self.adapters.iter().map(|(name, adapter)| (*name, adapter))
    }

    pub fn disabled_adapters(&self) -> impl Iterator<Item = &HarnessCliStatus> {
        self.disabled_adapters.iter()
    }

    /// Resolve a harness CLI by any recognized spelling.
    ///
    /// Exact adapter keys (`"claude"`, `"codex"`) resolve directly, preserving
    /// the historical surface. Any other alias of the same descriptor — e.g.
    /// `"claude-code"` for the Claude adapter, `"codex-cli"` for Codex — also
    /// resolves, so a single identity decides the adapter regardless of
    /// spelling.
    pub fn get(&self, name: &str) -> Option<Arc<dyn HarnessCli>> {
        if let Some(adapter) = self.adapters.get(name).cloned() {
            return Some(adapter);
        }

        let descriptor = self.identities.resolve(name)?;
        self.adapters.iter().find_map(|(key, adapter)| descriptor.matches(key).then(|| adapter.clone()))
    }

    pub async fn select_first_available(&self, priority: &[String]) -> Option<Arc<dyn HarnessCli>> {
        for name in priority {
            let Some(adapter) = self.get(name) else {
                continue;
            };
            if adapter.is_installed() && matches!(adapter.is_authenticated().await, Ok(true)) {
                return Some(adapter);
            }
        }

        None
    }
}
