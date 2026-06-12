//! One harness identity registry.
//!
//! Before this module, the same harness was spelled four different ways across
//! the codebase — `"claude-code"` for coordination capability and import,
//! `"claude"` for the dream CLI registry key and MCP `HarnessTarget`,
//! `HarnessTarget::Claude` as an enum. Each site reasoned about harness identity
//! independently, so a capability or wiring decision could silently differ by
//! spelling.
//!
//! [`HarnessRegistry`] is the single source of truth. Each [`HarnessDescriptor`]
//! carries a canonical `id`, a set of `aliases`, and the data each site needs:
//! the coordination capability ([`Coordination`]), an optional [`CliSpec`], an
//! optional [`McpConfig`] placement, and an optional [`ImporterId`]. Resolution
//! is alias-aware and case-insensitive, so `"claude"` and `"claude-code"`
//! resolve to the *same* descriptor — proven by the
//! `claude_and_claude_code_resolve_to_one_descriptor` test below.
//!
//! ## Layering
//!
//! This lives in `memorum-coordination` (not `memoryd`) deliberately: `memoryd`
//! already depends on `memorum-coordination`, and the registry needs nothing
//! from `memoryd`. The `CliSpec` / `ImporterId` are *data* (program name, args,
//! opaque importer token) — the actual `HarnessCli` trait and import `Harness`
//! enum stay as code in `memoryd` and key off the descriptor's `id`. So this
//! module introduces no upward edge into `memoryd`.
//!
//! ## What is data vs. code
//!
//! Coordination capability and CLI specs are safe to express as data, so a new
//! Tier-3 harness with full coordination — or a new CLI to drive dreaming — can
//! ship from `config.yaml` alone (see [`HarnessRegistry::with_config_overrides`]).
//! Importers stay code: an [`ImporterId`] only *names* a parser that must exist
//! in `memoryd`; config cannot conjure a new parser.

use serde::{Deserialize, Serialize};

/// Whether a harness participates in full Stream I coordination (peer-update
/// insertion and claim locks) or is restricted to observe-only.
///
/// Unknown harnesses always resolve to [`Coordination::ObserveOnly`] to prevent
/// silent privilege escalation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Coordination {
    /// Full coordination: peer-update insertion and claim locks.
    Full,
    /// Observe-only: surfaces peer activity but does not insert or lock.
    #[default]
    ObserveOnly,
}

impl Coordination {
    /// True when this harness supports full coordination.
    pub fn is_full(self) -> bool {
        matches!(self, Self::Full)
    }
}

/// Where a harness expects its MCP server configuration to live.
///
/// Carries the path *shape* (JSON vs. TOML) rather than a concrete path; the
/// wiring site resolves the real location. `None` means the harness has no MCP
/// config surface to wire.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpConfig {
    /// MCP servers live in a JSON document (Claude-style `mcpServers`).
    JsonAtPath,
    /// MCP servers live in a TOML document (Codex-style `[mcp_servers.*]`).
    TomlAtPath,
}

/// CLI invocation spec for a harness, used by the dream orchestrator to drive an
/// external agent. `program` is the executable name resolved on `PATH`.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct CliSpec {
    /// Executable name (resolved on `PATH`), e.g. `"claude"` or `"codex"`.
    pub program: String,
}

impl CliSpec {
    /// Construct a CLI spec from a program name.
    pub fn new(program: impl Into<String>) -> Self {
        Self { program: program.into() }
    }
}

/// Opaque identifier naming an importer parser that must exist as code in
/// `memoryd`. Config can reference an importer by id but cannot define one.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Deserialize, Serialize)]
pub struct ImporterId(pub String);

impl ImporterId {
    /// Construct an importer id.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the importer id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One harness's identity and capabilities, resolvable by `id` or any alias.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct HarnessDescriptor {
    /// Canonical identifier. The registry stores descriptors keyed by `id`.
    pub id: String,
    /// Alternate spellings that resolve to this descriptor (case-insensitive).
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Support tier (1 = first-class, higher = more peripheral). Informational.
    #[serde(default)]
    pub tier: u8,
    /// Coordination capability. Defaults to observe-only.
    #[serde(default)]
    pub coordination: Coordination,
    /// CLI spec for dream-driving, if this harness has one.
    #[serde(default)]
    pub cli: Option<CliSpec>,
    /// MCP config placement, if this harness has an MCP surface to wire.
    #[serde(default)]
    pub mcp_config: Option<McpConfig>,
    /// Importer parser id, if this harness can be imported from.
    #[serde(default)]
    pub importer: Option<ImporterId>,
}

impl HarnessDescriptor {
    /// True when any of this descriptor's identifiers (its `id` or an alias)
    /// matches `candidate` after case-folding and trimming.
    pub fn matches(&self, candidate: &str) -> bool {
        let needle = normalize(candidate);
        normalize(&self.id) == needle || self.aliases.iter().any(|alias| normalize(alias) == needle)
    }
}

/// Built-in `claude-code` descriptor. Aliases the bare `"claude"` spelling used
/// by the dream registry and MCP `HarnessTarget` so all four sites agree.
fn builtin_claude_code() -> HarnessDescriptor {
    HarnessDescriptor {
        id: "claude-code".to_string(),
        aliases: vec!["claude".to_string()],
        tier: 1,
        coordination: Coordination::Full,
        cli: Some(CliSpec::new("claude")),
        mcp_config: Some(McpConfig::JsonAtPath),
        importer: Some(ImporterId::new("claude-code")),
    }
}

/// Built-in `codex` descriptor. Aliases `"codex-cli"` so the coordination
/// allowlist spelling resolves identically.
fn builtin_codex() -> HarnessDescriptor {
    HarnessDescriptor {
        id: "codex".to_string(),
        aliases: vec!["codex-cli".to_string()],
        tier: 1,
        coordination: Coordination::Full,
        cli: Some(CliSpec::new("codex")),
        mcp_config: Some(McpConfig::TomlAtPath),
        importer: Some(ImporterId::new("codex")),
    }
}

/// The harness identity registry. Holds built-in descriptors (compiled in) plus
/// any config-supplied descriptors. Resolution is alias-aware and
/// case-insensitive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessRegistry {
    descriptors: Vec<HarnessDescriptor>,
}

impl Default for HarnessRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl HarnessRegistry {
    /// Registry with only the compiled-in built-ins (`claude-code`, `codex`).
    pub fn builtin() -> Self {
        Self { descriptors: vec![builtin_claude_code(), builtin_codex()] }
    }

    /// Built-ins plus config-supplied capability overrides.
    ///
    /// `full_coordination_harnesses`, when `Some`, *replaces* the set of
    /// harnesses granted full coordination: every descriptor whose `id` or an
    /// alias appears in the list becomes [`Coordination::Full`]; every other
    /// known descriptor becomes [`Coordination::ObserveOnly`]. An entry naming
    /// no known descriptor produces a synthetic observe-only-capable descriptor
    /// so the spelling resolves and gains full coordination.
    ///
    /// `additional_descriptors` are appended verbatim (config-defined harnesses
    /// with their own coordination/cli/mcp data).
    pub fn with_config_overrides(
        full_coordination_harnesses: Option<&[String]>,
        additional_descriptors: &[HarnessDescriptor],
    ) -> Self {
        let mut descriptors = vec![builtin_claude_code(), builtin_codex()];

        for extra in additional_descriptors {
            // A config descriptor sharing an id with a built-in replaces it.
            if let Some(slot) = descriptors.iter_mut().find(|d| normalize(&d.id) == normalize(&extra.id)) {
                *slot = extra.clone();
            } else {
                descriptors.push(extra.clone());
            }
        }

        if let Some(allowlist) = full_coordination_harnesses {
            let mut registry = Self { descriptors };
            registry.apply_coordination_allowlist(allowlist);
            return registry;
        }

        Self { descriptors }
    }

    /// Rewrite every descriptor's coordination capability from `allowlist`.
    fn apply_coordination_allowlist(&mut self, allowlist: &[String]) {
        for descriptor in &mut self.descriptors {
            let granted = allowlist.iter().any(|name| descriptor.matches(name));
            descriptor.coordination = if granted { Coordination::Full } else { Coordination::ObserveOnly };
        }

        // Any allowlist entry that matched no known descriptor becomes a
        // synthetic full-coordination descriptor so the spelling resolves.
        for name in allowlist {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            if self.resolve(trimmed).is_none() {
                self.descriptors.push(HarnessDescriptor {
                    id: normalize(trimmed),
                    aliases: Vec::new(),
                    tier: 3,
                    coordination: Coordination::Full,
                    cli: None,
                    mcp_config: None,
                    importer: None,
                });
            }
        }
    }

    /// Resolve a harness identifier (id or alias, case-insensitive) to its
    /// descriptor. Returns `None` for unknown harnesses.
    pub fn resolve(&self, identifier: &str) -> Option<&HarnessDescriptor> {
        self.descriptors.iter().find(|descriptor| descriptor.matches(identifier))
    }

    /// Coordination capability for `identifier`. Unknown harnesses are
    /// observe-only, never full — this is the privilege-escalation guard.
    pub fn coordination(&self, identifier: &str) -> Coordination {
        self.resolve(identifier).map(|descriptor| descriptor.coordination).unwrap_or(Coordination::ObserveOnly)
    }

    /// True when `identifier` resolves to a full-coordination harness.
    pub fn is_full_coordination(&self, identifier: &str) -> bool {
        self.coordination(identifier).is_full()
    }

    /// Iterate descriptors in registration order.
    pub fn descriptors(&self) -> impl Iterator<Item = &HarnessDescriptor> {
        self.descriptors.iter()
    }
}

/// Case-fold and trim a harness identifier for comparison.
fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_and_claude_code_resolve_to_one_descriptor() {
        // Load-bearing: if these resolved to different descriptors, coordination
        // capability would silently differ by spelling across the four sites.
        let registry = HarnessRegistry::builtin();
        let claude = registry.resolve("claude").expect("`claude` resolves");
        let claude_code = registry.resolve("claude-code").expect("`claude-code` resolves");
        assert_eq!(claude, claude_code, "claude and claude-code must be one descriptor");
        assert_eq!(claude.id, "claude-code");
        assert!(claude.coordination.is_full());
    }

    #[test]
    fn codex_aliases_resolve_to_one_descriptor() {
        let registry = HarnessRegistry::builtin();
        let codex = registry.resolve("codex").expect("`codex` resolves");
        let codex_cli = registry.resolve("codex-cli").expect("`codex-cli` resolves");
        assert_eq!(codex, codex_cli);
        assert_eq!(codex.id, "codex");
    }

    #[test]
    fn resolution_is_case_and_whitespace_insensitive() {
        let registry = HarnessRegistry::builtin();
        assert_eq!(registry.resolve(" CLAUDE-CODE ").map(|d| d.id.as_str()), Some("claude-code"));
        assert_eq!(registry.resolve("Codex").map(|d| d.id.as_str()), Some("codex"));
    }

    #[test]
    fn builtin_coordination_matches_legacy_allowlist() {
        // Equivalence with the retired FULL_COORDINATION_HARNESSES =
        // ["codex", "codex-cli", "claude-code"].
        let registry = HarnessRegistry::builtin();
        for full in ["codex", "codex-cli", "claude-code", "claude", " CODEX "] {
            assert!(registry.is_full_coordination(full), "{full} should be full coordination");
        }
        for observe in ["cursor", "claude-code-v2", "opencode", "gemini"] {
            assert!(!registry.is_full_coordination(observe), "{observe} must default observe-only");
        }
    }

    #[test]
    fn unknown_harness_is_observe_only() {
        let registry = HarnessRegistry::builtin();
        assert_eq!(registry.coordination("nope"), Coordination::ObserveOnly);
        assert!(!registry.is_full_coordination("nope"));
    }

    #[test]
    fn config_override_replaces_full_coordination_set() {
        // Override grants full coordination to claude only; codex drops to
        // observe-only.
        let registry = HarnessRegistry::with_config_overrides(Some(&["claude".to_string()]), &[]);
        assert!(registry.is_full_coordination("claude-code"));
        assert!(registry.is_full_coordination("claude"));
        assert!(!registry.is_full_coordination("codex"), "codex dropped from override list");
    }

    #[test]
    fn config_override_adds_tier3_full_harness_without_code() {
        let registry = HarnessRegistry::with_config_overrides(Some(&["codex".to_string(), "cursor".to_string()]), &[]);
        assert!(registry.is_full_coordination("cursor"), "config grants cursor full coordination");
        assert!(registry.is_full_coordination("codex"));
        assert!(!registry.is_full_coordination("claude-code"), "claude-code not in override list");
    }

    #[test]
    fn additional_descriptor_is_resolvable_by_alias() {
        let extra = HarnessDescriptor {
            id: "windsurf".to_string(),
            aliases: vec!["windsurf-cli".to_string()],
            tier: 3,
            coordination: Coordination::ObserveOnly,
            cli: Some(CliSpec::new("windsurf")),
            mcp_config: Some(McpConfig::JsonAtPath),
            importer: None,
        };
        let registry = HarnessRegistry::with_config_overrides(None, std::slice::from_ref(&extra));
        assert_eq!(registry.resolve("windsurf-cli").map(|d| d.id.as_str()), Some("windsurf"));
        assert!(!registry.is_full_coordination("windsurf"));
    }
}
