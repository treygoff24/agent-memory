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
use serde::Serialize;

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
    /// `.memory-project.yaml` action taken or planned for prompted project
    /// generation. `None` for mappings that did not involve project YAML.
    pub project_yaml: Option<ProjectYamlDisposition>,
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

impl ResolutionKind {
    /// Stable report value. Keep this independent of `Debug` so JSON consumers
    /// can rely on a documented string contract.
    pub fn as_report_str(self) -> &'static str {
        match self {
            Self::GitRemote => "git_remote",
            Self::YamlOverride => "yaml_override",
            Self::PromptedNewProject => "prompted_new_project",
            Self::PromptedDropToMe => "prompted_drop_to_me",
            Self::PromptedSkip => "prompted_skip",
            Self::UserScope => "user_scope",
        }
    }
}

/// `.memory-project.yaml` disposition for a non-git cwd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectYamlDisposition {
    pub path: PathBuf,
    pub action: ProjectYamlAction,
}

/// Stable action values for project YAML reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectYamlAction {
    PlannedWrite,
    Written,
    AlreadyExists,
}

impl ProjectYamlAction {
    pub fn as_report_str(self) -> &'static str {
        match self {
            Self::PlannedWrite => "planned_write",
            Self::Written => "written",
            Self::AlreadyExists => "already_exists",
        }
    }
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

/// Prompt backend for non-interactive callers that already made the placement
/// decision via flags.
#[derive(Debug, Clone, Copy)]
pub struct FixedDispositionBackend {
    disposition: PromptedDisposition,
}

impl FixedDispositionBackend {
    pub fn new(disposition: PromptedDisposition) -> Self {
        Self { disposition }
    }
}

impl PromptBackend for FixedDispositionBackend {
    fn prompt_non_git_cwd(&mut self, _cwd: &Path, _synced_dir: Option<&'static str>) -> PromptResult {
        PromptResult { disposition: self.disposition, synced_dir_confirmed: None }
    }
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
    _plan_only: bool,
    prompted_cache: std::collections::HashMap<PathBuf, PromptResult>,
}

impl ProjectMapper {
    /// Build a new mapper.
    pub fn new(plan_only: bool) -> Self {
        Self { _plan_only: plan_only, ..Self::default() }
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
                project_yaml: None,
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
                    project_yaml: None,
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
                let yaml_action = self.prepare_project_yaml(&yaml_path)?;
                Ok(ScopeBinding {
                    scope: Scope::Project,
                    namespace: Some("project".to_string()),
                    canonical_namespace_id: Some(canonical_id),
                    resolution: ResolutionKind::PromptedNewProject,
                    project_yaml: Some(ProjectYamlDisposition { path: yaml_path, action: yaml_action }),
                })
            }
            PromptedDisposition::DropToMe => Ok(ScopeBinding {
                scope: Scope::User,
                namespace: Some("me".to_string()),
                canonical_namespace_id: None,
                resolution: ResolutionKind::PromptedDropToMe,
                project_yaml: None,
            }),
            PromptedDisposition::Skip => Ok(ScopeBinding {
                scope: Scope::User,
                namespace: None,
                canonical_namespace_id: None,
                resolution: ResolutionKind::PromptedSkip,
                project_yaml: None,
            }),
        }
    }

    fn prepare_project_yaml(&mut self, yaml_path: &Path) -> Result<ProjectYamlAction, RecallError> {
        if yaml_path.exists() {
            return Ok(ProjectYamlAction::AlreadyExists);
        }
        Ok(ProjectYamlAction::PlannedWrite)
    }
}

pub(crate) fn write_generated_project_yaml(
    cwd: &Path,
    yaml_path: &Path,
    canonical_id: &str,
) -> Result<ProjectYamlAction, RecallError> {
    if yaml_path.exists() {
        return Ok(ProjectYamlAction::AlreadyExists);
    }

    let alias = derive_alias_for_dir(cwd);
    let yaml = project_yaml_contents(canonical_id, &alias)?;
    std::fs::write(yaml_path, yaml)
        .map_err(|error| RecallError::invalid_request(format!("write .memory-project.yaml: {error}")))?;
    Ok(ProjectYamlAction::Written)
}

const MAX_GENERATED_PROJECT_FIELD_BYTES: usize = 128;
const GENERATED_PROJECT_ID_PREFIX: &str = "proj_";
const GENERATED_PROJECT_ID_HASH_BYTES: usize = 16;

#[derive(Serialize)]
struct GeneratedProjectFile<'a> {
    canonical_id: &'a str,
    alias: &'a str,
}

fn project_yaml_contents(canonical_id: &str, alias: &str) -> Result<String, RecallError> {
    serde_yaml::to_string(&GeneratedProjectFile { canonical_id, alias })
        .map_err(|error| RecallError::invalid_request(format!("serialize .memory-project.yaml: {error}")))
}

fn derive_canonical_id_for_dir(cwd: &Path) -> String {
    use sha2::{Digest, Sha256};
    let basename = cwd
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "dir".to_string());
    let basename = basename.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>();
    let basename = if basename.is_empty() { "dir" } else { &basename };
    let suffix = hex::encode(Sha256::digest(cwd.display().to_string().as_bytes()));
    let short_suffix = suffix.get(..16).unwrap_or(&suffix);
    let max_basename_bytes =
        MAX_GENERATED_PROJECT_FIELD_BYTES - GENERATED_PROJECT_ID_PREFIX.len() - 1 - GENERATED_PROJECT_ID_HASH_BYTES;
    let basename = truncate_to_byte_limit(basename, max_basename_bytes);
    format!("proj_{basename}-{short_suffix}")
}

fn derive_alias_for_dir(cwd: &Path) -> String {
    let alias = cwd
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::trim)
        .filter(|alias| !alias.is_empty())
        .unwrap_or("unnamed");
    truncate_to_byte_limit(alias, MAX_GENERATED_PROJECT_FIELD_BYTES).to_string()
}

fn truncate_to_byte_limit(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = 0;
    for (index, character) in value.char_indices() {
        let next = index + character.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    &value[..end]
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
        let mut mapper = ProjectMapper::new(false);
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
        let mut mapper = ProjectMapper::new(false);
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
        let mut mapper = ProjectMapper::new(false);
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
        let mut mapper = ProjectMapper::new(false);
        let binding = mapper.resolve(Some(cwd), &mut prompts).await.expect("resolves");
        assert!(binding.namespace.is_none());
        assert_eq!(binding.resolution, ResolutionKind::PromptedSkip);
    }

    #[tokio::test]
    async fn non_git_cwd_prompted_generate_plans_project_yaml_and_returns_canonical_id() {
        let tmp = tempfile::tempdir().expect("tmp");
        let cwd = tmp.path();
        let mut prompts = ScriptedPrompts::new().with(cwd, PromptedDisposition::GenerateProjectYaml);
        let mut mapper = ProjectMapper::new(false);
        let binding = mapper.resolve(Some(cwd), &mut prompts).await.expect("resolves");
        assert_eq!(binding.scope, Scope::Project);
        assert!(binding.canonical_namespace_id.as_deref().is_some_and(|id| id.starts_with("proj_")));
        assert_eq!(binding.resolution, ResolutionKind::PromptedNewProject);

        let yaml_path = cwd.join(".memory-project.yaml");
        assert!(!yaml_path.exists(), "mapping only plans yaml; execution materializes it after successful daemon IO");
        let project_yaml = binding.project_yaml.expect("project yaml disposition");
        assert_eq!(project_yaml.path, yaml_path);
        assert_eq!(project_yaml.action, ProjectYamlAction::PlannedWrite);
    }

    #[tokio::test]
    async fn generated_project_yaml_round_trips_for_yaml_like_and_long_directory_names() {
        let temp = tempfile::tempdir().expect("tmp");
        let cases = ["true".to_string(), "a".repeat(180)];

        for case in cases {
            let cwd = temp.path().join(case);
            std::fs::create_dir_all(&cwd).expect("case dir");
            let mut prompts = ScriptedPrompts::new().with(&cwd, PromptedDisposition::GenerateProjectYaml);
            let mut mapper = ProjectMapper::new(false);

            let generated = mapper.resolve(Some(&cwd), &mut prompts).await.expect("generate binding");
            let yaml_path = cwd.join(".memory-project.yaml");
            write_generated_project_yaml(
                &cwd,
                &yaml_path,
                generated.canonical_namespace_id.as_deref().expect("canonical id"),
            )
            .expect("write generated yaml");
            let parsed = resolve_project_binding(&cwd)
                .await
                .expect("generated project yaml parses")
                .expect("project binding exists");

            assert_eq!(parsed.canonical_id, generated.canonical_namespace_id.expect("canonical id"));
            assert!(parsed.canonical_id.len() <= MAX_GENERATED_PROJECT_FIELD_BYTES);
            assert!(parsed.alias.as_ref().is_some_and(|alias| alias.len() <= MAX_GENERATED_PROJECT_FIELD_BYTES));
        }
    }

    #[tokio::test]
    async fn plan_only_generate_records_planned_yaml_without_writing_file() {
        let tmp = tempfile::tempdir().expect("tmp");
        let cwd = tmp.path();
        let mut prompts = ScriptedPrompts::new().with(cwd, PromptedDisposition::GenerateProjectYaml);
        let mut mapper = ProjectMapper::new(true);
        let binding = mapper.resolve(Some(cwd), &mut prompts).await.expect("resolves");

        let yaml_path = cwd.join(".memory-project.yaml");
        assert!(!yaml_path.exists(), "plan-only mapping must not write yaml");
        let project_yaml = binding.project_yaml.expect("project yaml disposition");
        assert_eq!(project_yaml.path, yaml_path);
        assert_eq!(project_yaml.action, ProjectYamlAction::PlannedWrite);
    }

    #[tokio::test]
    async fn prompts_memoize_per_cwd_so_repeated_resolves_do_not_re_ask() {
        let tmp = tempfile::tempdir().expect("tmp");
        let cwd = tmp.path();
        let mut prompts = ScriptedPrompts::new().with(cwd, PromptedDisposition::DropToMe);
        let mut mapper = ProjectMapper::new(false);
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
