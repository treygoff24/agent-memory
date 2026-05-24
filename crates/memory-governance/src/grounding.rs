//! Local source grounding verification for governance candidates.

use std::path::{Component, Path, PathBuf};

use crate::decision::{GovernanceDecision, GovernanceRefusalReason, NextAction};

const GROUNDING_REFUSAL_MESSAGE: &str = "source references could not be grounded";
const FILE_REF_PREFIX: &str = "file:";
const SESSION_SPAWN_REF_PREFIX: &str = "session-spawn:";

/// Source category supplied with a candidate memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceKind {
    /// The candidate was explicitly supplied by the user.
    User,
    /// The primary agent generated the candidate from a local source.
    AgentPrimary,
    /// A spawned subagent generated the candidate.
    Subagent,
    /// The candidate cites a verified local web capture artifact.
    WebCapture,
}

/// Typed source descriptor used for grounding decisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source {
    kind: SourceKind,
    source_ref: Option<String>,
}

impl Source {
    /// Create a source descriptor with an optional local reference.
    #[must_use]
    pub fn new(kind: SourceKind, source_ref: Option<impl Into<String>>) -> Self {
        Self { kind, source_ref: source_ref.map(Into::into) }
    }

    #[must_use]
    pub fn kind(&self) -> SourceKind {
        self.kind
    }
}

/// Candidate context needed by grounding verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroundingContext<'a> {
    id: &'a str,
    namespace: &'a str,
    sources: Vec<Source>,
    has_explicit_user_context: bool,
}

impl<'a> GroundingContext<'a> {
    /// Create a grounding context for a governance candidate.
    #[must_use]
    pub fn new(id: &'a str, namespace: &'a str, sources: Vec<Source>) -> Self {
        Self { id, namespace, sources, has_explicit_user_context: false }
    }

    /// Mark that user-authored context is explicitly present in the current turn.
    #[must_use]
    pub fn with_explicit_user_context(mut self) -> Self {
        self.has_explicit_user_context = true;
        self
    }
}

/// Resolves a source reference into a typed local grounding result.
pub trait SourceRefResolver {
    /// Resolve a local source reference.
    fn resolve(&self, source_ref: &str) -> SourceResolution;
}

/// Local file source resolver. It never fetches network URLs.
#[derive(Clone, Copy, Debug, Default)]
pub struct FileSourceResolver;

impl SourceRefResolver for FileSourceResolver {
    fn resolve(&self, source_ref: &str) -> SourceResolution {
        let Some(path) = absolute_file_path(source_ref) else {
            return SourceResolution::Unsupported;
        };

        if is_dream_journal_path(&path) {
            return SourceResolution::ForbiddenDreamJournal;
        }

        if path.is_file() {
            SourceResolution::Resolved
        } else {
            SourceResolution::Missing
        }
    }
}

/// Registry used to prove that a subagent source was spawned in-session.
pub trait SessionSpawnResolver {
    /// Returns true when the spawn id belongs to the active session registry.
    fn spawned_in_session(&self, spawn_id: &str) -> bool;
}

/// Registry used to prove a web capture ref resolves to verified local evidence.
pub trait WebCaptureResolver {
    /// Resolve a `webcap:<artifact>#<excerpt>` source reference.
    fn resolve_web_capture(&self, source_ref: &str) -> SourceResolution;
}

/// Default resolver used by compatibility constructors: never resolves web captures.
#[derive(Clone, Copy, Debug, Default)]
pub struct NeverResolveWebCapture;

impl WebCaptureResolver for NeverResolveWebCapture {
    fn resolve_web_capture(&self, _source_ref: &str) -> SourceResolution {
        SourceResolution::Unsupported
    }
}

impl WebCaptureResolver for memory_source::ArtifactStore {
    fn resolve_web_capture(&self, source_ref: &str) -> SourceResolution {
        match self.resolve_excerpt_ref(source_ref) {
            Ok(_) => SourceResolution::Resolved,
            Err(memory_source::SourceError::InvalidSourceRef(_)) => SourceResolution::Unsupported,
            Err(memory_source::SourceError::InvalidId(_)) => SourceResolution::Unsupported,
            Err(memory_source::SourceError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
                SourceResolution::Missing
            }
            Err(memory_source::SourceError::Integrity(_)) => SourceResolution::IntegrityFailed,
            Err(memory_source::SourceError::ExcerptNotFound(_)) => SourceResolution::Missing,
            Err(_) => SourceResolution::Unsupported,
        }
    }
}

/// Grounding verifier composed from typed resolvers.
#[derive(Clone, Debug)]
pub struct GroundingVerifier<S, W = NeverResolveWebCapture> {
    file_resolver: FileSourceResolver,
    session_spawn_resolver: S,
    web_capture_resolver: W,
}

impl<S> GroundingVerifier<S, NeverResolveWebCapture>
where
    S: SessionSpawnResolver,
{
    /// Create a grounding verifier.
    #[must_use]
    pub fn new(file_resolver: FileSourceResolver, session_spawn_resolver: S) -> Self {
        Self { file_resolver, session_spawn_resolver, web_capture_resolver: NeverResolveWebCapture }
    }
}

impl<S, W> GroundingVerifier<S, W>
where
    S: SessionSpawnResolver,
    W: WebCaptureResolver,
{
    /// Create a grounding verifier with an explicit web-capture resolver.
    #[must_use]
    pub fn new_with_web_capture_resolver(
        file_resolver: FileSourceResolver,
        session_spawn_resolver: S,
        web_capture_resolver: W,
    ) -> Self {
        Self { file_resolver, session_spawn_resolver, web_capture_resolver }
    }

    /// Verify that every supplied source has acceptable local grounding.
    #[must_use]
    pub fn verify(&self, context: &GroundingContext<'_>) -> GovernanceDecision {
        if context.sources.is_empty() && !context.has_explicit_user_context {
            return grounding_refusal();
        }

        if context.sources.iter().all(|source| self.source_is_grounded(context, source)) {
            GovernanceDecision::promoted(context.id, context.namespace)
        } else {
            grounding_refusal()
        }
    }

    fn source_is_grounded(&self, context: &GroundingContext<'_>, source: &Source) -> bool {
        match source.kind {
            SourceKind::User => context.has_explicit_user_context,
            SourceKind::AgentPrimary => source
                .source_ref
                .as_deref()
                .is_some_and(|source_ref| self.file_resolver.resolve(source_ref).is_resolved()),
            SourceKind::Subagent => source.source_ref.as_deref().is_some_and(|source_ref| {
                source_ref
                    .strip_prefix(SESSION_SPAWN_REF_PREFIX)
                    .is_some_and(|spawn_id| self.session_spawn_resolver.spawned_in_session(spawn_id))
            }),
            SourceKind::WebCapture => source
                .source_ref
                .as_deref()
                .is_some_and(|source_ref| self.web_capture_resolver.resolve_web_capture(source_ref).is_resolved()),
        }
    }
}

/// Typed source-resolution outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceResolution {
    /// The source reference resolves to allowed local evidence.
    Resolved,
    /// The reference is syntactically valid but its target is absent.
    Missing,
    /// The reference points at dream journal prose, which is not acceptable evidence.
    ForbiddenDreamJournal,
    /// The reference kind is unsupported by this resolver.
    Unsupported,
    /// The artifact exists but failed integrity/excerpt verification.
    IntegrityFailed,
}

impl SourceResolution {
    fn is_resolved(self) -> bool {
        matches!(self, Self::Resolved)
    }
}

fn grounding_refusal() -> GovernanceDecision {
    GovernanceDecision::Refused {
        reason: GovernanceRefusalReason::Grounding,
        message: GROUNDING_REFUSAL_MESSAGE.to_owned(),
        next_action: NextAction::NoWrite,
    }
}

fn absolute_file_path(source_ref: &str) -> Option<PathBuf> {
    let file_ref = source_ref.strip_prefix(FILE_REF_PREFIX)?;
    let path_without_fragment = file_ref.split_once('#').map_or(file_ref, |(path, _fragment)| path);
    let path = PathBuf::from(path_without_fragment);

    path.is_absolute().then_some(path)
}

fn is_dream_journal_path(path: &Path) -> bool {
    path.components()
        .map(Component::as_os_str)
        .collect::<Vec<_>>()
        .windows(2)
        .any(|window| window[0] == "dreams" && window[1] == "journal")
}
