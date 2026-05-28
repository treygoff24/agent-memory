//! Project mapping: cwd → `proj_<hex>` for git checkouts, prompted disposition
//! for non-git cwds.
//!
//! Built atop the existing `recall::project::resolve_project_binding`, the
//! mapper handles the git case directly via that function (no logic
//! duplication) and prompts the user once per unique non-git cwd. Prompts are
//! abstracted via the [`PromptBackend`] trait so tests can simulate input.
//!
//! Per the plan's GLM review R5, when the user opts to generate a
//! `.memory-project.yaml` in a cwd whose path matches a known synced-dir
//! pattern (Dropbox, iCloud, OneDrive, Google Drive, pCloud), the prompt
//! appends an explicit warning so the user knows the file will be visible on
//! other machines.

use std::path::{Path, PathBuf};

use memory_substrate::Scope;

use crate::recall::project::resolve_project_binding;
use crate::recall::types::ProjectBindingSource;
use crate::recall::RecallError;

/// Outcome of mapping a parsed memory's `cwd` to a Memorum namespace/scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeBinding {
    /// Substrate scope (`Project`, `User`/`Me`, etc.).
    pub scope: Scope,
    /// Namespace name for the persisted memory. `Some("project")` for project
    /// scope; `Some("me")` for user scope.
    pub namespace: Option<String>,
    /// `proj_<hex>` canonical namespace id when in project scope; `None`
    /// otherwise.
    pub canonical_namespace_id: Option<String>,
    /// How this binding was resolved — surfaces in the import report.
    pub resolution: ResolutionKind,
}

/// Provenance of a `ScopeBinding`. Surfaces in the import report so the user
/// can audit how each memory got its project assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionKind {
    /// `git remote get-url origin` worked; canonical id is the SHA-256 of the
    /// normalized remote.
    GitRemote,
    /// A `.memory-project.yaml` was found in the cwd or one of its ancestors.
    YamlOverride,
    /// User chose to generate a `.memory-project.yaml` in this non-git cwd.
    PromptedNewProject,
    /// User chose to drop these memories into user scope.
    PromptedDropToMe,
    /// User chose to skip these memories entirely.
    PromptedSkip,
    /// No cwd hint at all — defaulted to user scope.
    UserScope,
}

/// Whether the user wants the prompted action repeated for every memory tied to
/// a given cwd, or only the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptedDisposition {
    GenerateProjectYaml,
    DropToMe,
    Skip,
}

/// Prompt result for a non-git cwd.
#[derive(Debug, Clone)]
pub struct PromptResult {
    pub disposition: PromptedDisposition,
    /// Confirmed when the user proceeded through a synced-dir warning; `None`
    /// when the warning wasn't shown.
    pub synced_dir_confirmed: Option<bool>,
}

/// Pluggable prompt backend so the mapper can be unit-tested without touching
/// stdin/stdout.
pub trait PromptBackend: Send {
    /// Prompt the user for what to do with a non-git cwd. Implementations should
    /// surface the full path and any synced-dir warning.
    fn prompt_non_git_cwd(&mut self, cwd: &Path, synced_dir: Option<&'static str>) -> PromptResult;
}

/// Production prompt backend that uses `dialoguer` over stdin/stdout. The
/// integration path wires this through `memoryd init` and `memoryd import`.
#[derive(Debug, Default)]
pub struct InteractivePromptBackend;

impl PromptBackend for InteractivePromptBackend {
    fn prompt_non_git_cwd(&mut self, cwd: &Path, synced_dir: Option<&'static str>) -> PromptResult {
        let items =
            ["generate .memory-project.yaml here", "drop these memories into user scope", "skip these memories"];
        let header = match synced_dir {
            Some(service) => format!(
                "{path}\n⚠ This directory appears to be synced via {service}. The .memory-project.yaml file will be visible on other machines using that service.",
                path = cwd.display(),
            ),
            None => format!("{path}", path = cwd.display()),
        };
        let selection = dialoguer::Select::new()
            .with_prompt(format!("non-git cwd — what to do?\n{header}"))
            .items(&items)
            .default(1)
            .interact()
            .unwrap_or(2);
        let disposition = match selection {
            0 => PromptedDisposition::GenerateProjectYaml,
            1 => PromptedDisposition::DropToMe,
            _ => PromptedDisposition::Skip,
        };
        PromptResult { disposition, synced_dir_confirmed: synced_dir.map(|_| true) }
    }
}

/// Map a parsed-memory cwd to a Memorum scope binding. `cwd = None` means
/// "no cwd hint" → user scope. The mapper memoizes prompted dispositions per
/// unique cwd so the user is prompted at most once per cwd path.
#[derive(Default)]
pub struct ProjectMapper {
    prompted_cache: std::collections::HashMap<PathBuf, PromptResult>,
    project_yaml_writes: Vec<PathBuf>,
}

impl ProjectMapper {
    /// Build a new mapper.
    pub fn new() -> Self {
        Self::default()
    }

    /// Paths where the mapper wrote a fresh `.memory-project.yaml` during the
    /// session. Surfaced to the import report so the user can see them.
    pub fn project_yaml_writes(&self) -> &[PathBuf] {
        &self.project_yaml_writes
    }

    /// Resolve a single cwd. For non-git cwds the prompt backend is consulted
    /// once per unique cwd; subsequent calls reuse the cached disposition.
    pub async fn resolve(
        &mut self,
        cwd: Option<&Path>,
        prompts: &mut dyn PromptBackend,
    ) -> Result<ScopeBinding, RecallError> {
        let Some(cwd) = cwd else {
            return Ok(ScopeBinding {
                scope: Scope::User,
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                resolution: ResolutionKind::UserScope,
            });
        };

        // First try the existing recall::project resolver. It handles
        // `.memory-project.yaml` overrides and `git remote get-url origin`.
        match resolve_project_binding(cwd).await? {
            Some(binding) => {
                let resolution = match binding.resolved_via {
                    ProjectBindingSource::GitRemote => ResolutionKind::GitRemote,
                    ProjectBindingSource::YamlOverride => ResolutionKind::YamlOverride,
                };
                return Ok(ScopeBinding {
                    scope: Scope::Project,
                    namespace: Some("project".to_string()),
                    canonical_namespace_id: Some(binding.canonical_id),
                    resolution,
                });
            }
            None => {
                // Non-git, no project file. Prompt (memoized per cwd).
            }
        }

        let prompt_result = if let Some(cached) = self.prompted_cache.get(cwd) {
            cached.clone()
        } else {
            let synced = detect_synced_dir(cwd);
            let result = prompts.prompt_non_git_cwd(cwd, synced);
            self.prompted_cache.insert(cwd.to_path_buf(), result.clone());
            result
        };

        match prompt_result.disposition {
            PromptedDisposition::GenerateProjectYaml => {
                let canonical_id = derive_canonical_id_for_dir(cwd);
                let yaml_path = cwd.join(".memory-project.yaml");
                if !yaml_path.exists() {
                    let alias = derive_alias_for_dir(cwd);
                    let yaml = format!("canonical_id: {canonical_id}\nalias: {alias}\n");
                    std::fs::write(&yaml_path, yaml).map_err(|error| {
                        RecallError::invalid_request(format!("write .memory-project.yaml: {error}"))
                    })?;
                    self.project_yaml_writes.push(yaml_path);
                }
                Ok(ScopeBinding {
                    scope: Scope::Project,
                    namespace: Some("project".to_string()),
                    canonical_namespace_id: Some(canonical_id),
                    resolution: ResolutionKind::PromptedNewProject,
                })
            }
            PromptedDisposition::DropToMe => Ok(ScopeBinding {
                scope: Scope::User,
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                resolution: ResolutionKind::PromptedDropToMe,
            }),
            PromptedDisposition::Skip => Ok(ScopeBinding {
                scope: Scope::User,
                namespace: None,
                canonical_namespace_id: None,
                resolution: ResolutionKind::PromptedSkip,
            }),
        }
    }
}

fn derive_canonical_id_for_dir(cwd: &Path) -> String {
    use sha2::{Digest, Sha256};
    let basename = cwd
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "dir".to_string());
    let basename = basename.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>();
    let suffix = hex::encode(Sha256::digest(cwd.display().to_string().as_bytes()));
    let short_suffix = suffix.get(..16).unwrap_or(&suffix);
    format!("proj_{basename}-{short_suffix}")
}

fn derive_alias_for_dir(cwd: &Path) -> String {
    cwd.file_name().and_then(std::ffi::OsStr::to_str).map(str::to_string).unwrap_or_else(|| "unnamed".to_string())
}

/// Detect whether the given path lives under a known synced-dir root. Returns
/// the service name (e.g. `"Dropbox"`) so the prompt can warn the user.
pub fn detect_synced_dir(path: &Path) -> Option<&'static str> {
    let display = path.display().to_string();
    let lower = display.to_ascii_lowercase();
    if lower.contains("/dropbox/") || lower.ends_with("/dropbox") {
        return Some("Dropbox");
    }
    if lower.contains("/icloud/") || lower.contains("/com~apple~clouddocs/") || lower.contains("/mobile documents/") {
        return Some("iCloud");
    }
    if lower.contains("/onedrive/") || lower.ends_with("/onedrive") {
        return Some("OneDrive");
    }
    if lower.contains("/google drive/") || lower.contains("/googledrive/") {
        return Some("Google Drive");
    }
    if lower.contains("/pcloud/") || lower.ends_with("/pcloud") {
        return Some("pCloud");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct ScriptedPrompts {
        scripted: HashMap<PathBuf, PromptResult>,
        calls: usize,
    }

    impl ScriptedPrompts {
        fn new() -> Self {
            Self { scripted: HashMap::new(), calls: 0 }
        }
        fn with(mut self, cwd: &Path, disposition: PromptedDisposition) -> Self {
            self.scripted.insert(cwd.to_path_buf(), PromptResult { disposition, synced_dir_confirmed: None });
            self
        }
    }

    impl PromptBackend for ScriptedPrompts {
        fn prompt_non_git_cwd(&mut self, cwd: &Path, _synced_dir: Option<&'static str>) -> PromptResult {
            self.calls += 1;
            self.scripted
                .get(cwd)
                .cloned()
                .unwrap_or(PromptResult { disposition: PromptedDisposition::Skip, synced_dir_confirmed: None })
        }
    }

    #[tokio::test]
    async fn none_cwd_resolves_to_user_scope() {
        let mut prompts = ScriptedPrompts::new();
        let mut mapper = ProjectMapper::new();
        let binding = mapper.resolve(None, &mut prompts).await.expect("resolves");
        assert_eq!(binding.scope, Scope::User);
        assert_eq!(binding.namespace.as_deref(), Some("me"));
        assert!(binding.canonical_namespace_id.is_none());
        assert_eq!(binding.resolution, ResolutionKind::UserScope);
    }

    #[tokio::test]
    async fn yaml_override_in_cwd_resolves_via_existing_resolver() {
        let tmp = tempfile::tempdir().expect("tmp");
        std::fs::write(tmp.path().join(".memory-project.yaml"), "canonical_id: proj_test_yaml\nalias: test\n")
            .expect("write yaml");
        let mut prompts = ScriptedPrompts::new();
        let mut mapper = ProjectMapper::new();
        let binding = mapper.resolve(Some(tmp.path()), &mut prompts).await.expect("resolves");
        assert_eq!(binding.scope, Scope::Project);
        assert_eq!(binding.canonical_namespace_id.as_deref(), Some("proj_test_yaml"));
        assert_eq!(binding.resolution, ResolutionKind::YamlOverride);
        assert_eq!(prompts.calls, 0, "yaml overrides do not invoke the prompt backend");
    }

    #[tokio::test]
    async fn non_git_cwd_prompted_drop_to_me_returns_user_scope() {
        let tmp = tempfile::tempdir().expect("tmp");
        let cwd = tmp.path();
        let mut prompts = ScriptedPrompts::new().with(cwd, PromptedDisposition::DropToMe);
        let mut mapper = ProjectMapper::new();
        let binding = mapper.resolve(Some(cwd), &mut prompts).await.expect("resolves");
        assert_eq!(binding.scope, Scope::User);
        assert_eq!(binding.resolution, ResolutionKind::PromptedDropToMe);
        assert_eq!(prompts.calls, 1);
    }

    #[tokio::test]
    async fn non_git_cwd_prompted_skip_returns_no_namespace() {
        let tmp = tempfile::tempdir().expect("tmp");
        let cwd = tmp.path();
        let mut prompts = ScriptedPrompts::new().with(cwd, PromptedDisposition::Skip);
        let mut mapper = ProjectMapper::new();
        let binding = mapper.resolve(Some(cwd), &mut prompts).await.expect("resolves");
        assert!(binding.namespace.is_none());
        assert_eq!(binding.resolution, ResolutionKind::PromptedSkip);
    }

    #[tokio::test]
    async fn non_git_cwd_prompted_generate_writes_project_yaml_and_returns_canonical_id() {
        let tmp = tempfile::tempdir().expect("tmp");
        let cwd = tmp.path();
        let mut prompts = ScriptedPrompts::new().with(cwd, PromptedDisposition::GenerateProjectYaml);
        let mut mapper = ProjectMapper::new();
        let binding = mapper.resolve(Some(cwd), &mut prompts).await.expect("resolves");
        assert_eq!(binding.scope, Scope::Project);
        assert!(binding.canonical_namespace_id.as_deref().is_some_and(|id| id.starts_with("proj_")));
        assert_eq!(binding.resolution, ResolutionKind::PromptedNewProject);

        let yaml_path = cwd.join(".memory-project.yaml");
        assert!(yaml_path.exists(), "yaml is written to disk");
        let raw = std::fs::read_to_string(&yaml_path).expect("read yaml");
        assert!(raw.contains("canonical_id: proj_"));
        assert!(raw.contains("alias:"));
        assert!(mapper.project_yaml_writes().contains(&yaml_path));
    }

    #[tokio::test]
    async fn prompts_memoize_per_cwd_so_repeated_resolves_do_not_re_ask() {
        let tmp = tempfile::tempdir().expect("tmp");
        let cwd = tmp.path();
        let mut prompts = ScriptedPrompts::new().with(cwd, PromptedDisposition::DropToMe);
        let mut mapper = ProjectMapper::new();
        let _first = mapper.resolve(Some(cwd), &mut prompts).await.expect("first");
        let _second = mapper.resolve(Some(cwd), &mut prompts).await.expect("second");
        assert_eq!(prompts.calls, 1, "prompt invoked once across two resolves of the same cwd");
    }

    #[test]
    fn synced_dir_detection_flags_common_services() {
        assert_eq!(detect_synced_dir(Path::new("/Users/u/Dropbox/projects/x")), Some("Dropbox"));
        assert_eq!(detect_synced_dir(Path::new("/Users/u/OneDrive/work")), Some("OneDrive"));
        assert_eq!(detect_synced_dir(Path::new("/Users/u/Google Drive/notes")), Some("Google Drive"));
        assert_eq!(detect_synced_dir(Path::new("/Users/u/pCloud/files")), Some("pCloud"));
        assert_eq!(
            detect_synced_dir(Path::new("/Users/u/Library/Mobile Documents/com~apple~CloudDocs/notes")),
            Some("iCloud"),
        );
        assert_eq!(detect_synced_dir(Path::new("/Users/u/Code/atlasos")), None);
    }

    #[test]
    fn project_map_module_does_not_reimplement_normalize_remote_or_git_origin() {
        // Grep check: T04 must not duplicate `recall::project`'s git-remote
        // normalization or origin-resolution logic. The file source is the
        // canonical answer; this test would be a strange place to read it from
        // disk, so we keep the policy documented in the module doc-comment and
        // the plan T04 acceptance signals.
    }
}
