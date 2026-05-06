use std::collections::HashSet;
use std::path::{Path, PathBuf};

use memory_governance::{
    FileSourceResolver, GovernanceDecision, GovernanceRefusalReason, GroundingContext, GroundingVerifier, NextAction,
    SessionSpawnResolver, Source, SourceKind, SourceResolution, WebCaptureResolver,
};

#[test]
fn user_source_passes_with_explicit_user_context() {
    let context = context_with_user_source(Source::new(SourceKind::User, None::<String>));
    let verifier = verifier_with_known_subagents([]);

    let decision = verifier.verify(&context);

    assert!(matches!(decision, GovernanceDecision::Promoted { .. }));
}

#[test]
fn user_source_fails_without_explicit_user_context() {
    let context =
        GroundingContext::new("memory-1", "project/agent-memory", vec![Source::new(SourceKind::User, None::<String>)]);
    let verifier = verifier_with_known_subagents([]);

    let decision = verifier.verify(&context);

    assert_grounding_refusal(decision);
}

#[test]
fn non_user_candidate_with_no_sources_fails_grounding() {
    let context = GroundingContext::new("memory-1", "project/agent-memory", Vec::new());
    let verifier = verifier_with_known_subagents([]);

    let decision = verifier.verify(&context);

    assert_grounding_refusal(decision);
}

#[test]
fn agent_primary_file_ref_passes_when_absolute_file_exists() {
    let source_path = fixture_path("live-source.md");
    let context = GroundingContext::new(
        "memory-1",
        "project/agent-memory",
        vec![Source::new(SourceKind::AgentPrimary, Some(file_ref(&source_path, "#L1-L3")))],
    );
    let verifier = verifier_with_known_subagents([]);

    let decision = verifier.verify(&context);

    assert!(matches!(decision, GovernanceDecision::Promoted { .. }));
}

#[test]
fn missing_file_refs_fail_with_grounding_refusal() {
    let missing_path = fixture_path("missing-source.md");
    let context = GroundingContext::new(
        "memory-1",
        "project/agent-memory",
        vec![Source::new(SourceKind::AgentPrimary, Some(file_ref(&missing_path, "#L1-L3")))],
    );
    let verifier = verifier_with_known_subagents([]);

    let decision = verifier.verify(&context);

    assert_grounding_refusal(decision);
}

#[test]
fn dreams_journal_file_refs_fail_even_when_the_file_exists() {
    let dream_path = std::env::temp_dir()
        .join(format!("memory-governance-grounding-{}", std::process::id()))
        .join("dreams")
        .join("journal")
        .join("live-source.md");
    let dream_parent = dream_path.parent().expect("dream fixture path has a parent");
    std::fs::create_dir_all(dream_parent).expect("dream fixture directory can be created");
    std::fs::write(&dream_path, "dream prose\n").expect("dream fixture can be written");
    let context = GroundingContext::new(
        "memory-1",
        "project/agent-memory",
        vec![Source::new(SourceKind::AgentPrimary, Some(file_ref(&dream_path, "#L1-L1")))],
    );
    let verifier = verifier_with_known_subagents([]);

    let decision = verifier.verify(&context);

    assert_grounding_refusal(decision);
}

#[test]
fn subagent_refs_require_a_session_spawn_registry_entry() {
    let context = GroundingContext::new(
        "memory-1",
        "project/agent-memory",
        vec![Source::new(SourceKind::Subagent, Some("session-spawn:spawn-1".to_owned()))],
    );
    let missing_registry_entry = verifier_with_known_subagents([]);
    let known_registry_entry = verifier_with_known_subagents(["spawn-1"]);

    assert_grounding_refusal(missing_registry_entry.verify(&context));
    assert!(matches!(known_registry_entry.verify(&context), GovernanceDecision::Promoted { .. }));
}

#[test]
fn web_capture_refs_require_verified_artifact_and_excerpt() {
    let context = GroundingContext::new(
        "memory-1",
        "project/agent-memory",
        vec![Source::new(SourceKind::WebCapture, Some("webcap:src_01J0Z7Y8Q9R0ABCDE123456789#quote_0001"))],
    );
    let verifier = GroundingVerifier::new_with_web_capture_resolver(
        FileSourceResolver,
        FakeSessionSpawnResolver::new([]),
        FakeWebCaptureResolver(SourceResolution::Resolved),
    );
    assert!(matches!(verifier.verify(&context), GovernanceDecision::Promoted { .. }));

    let naked_url = GroundingContext::new(
        "memory-1",
        "project/agent-memory",
        vec![Source::new(SourceKind::WebCapture, Some("https://example.com"))],
    );
    assert_grounding_refusal(verifier.verify(&naked_url));

    for resolution in [SourceResolution::Missing, SourceResolution::IntegrityFailed, SourceResolution::Unsupported] {
        let verifier = GroundingVerifier::new_with_web_capture_resolver(
            FileSourceResolver,
            FakeSessionSpawnResolver::new([]),
            FakeWebCaptureResolver(resolution),
        );
        assert_grounding_refusal(verifier.verify(&context));
    }
}

fn context_with_user_source(source: Source) -> GroundingContext<'static> {
    GroundingContext::new("memory-1", "project/agent-memory", vec![source]).with_explicit_user_context()
}

fn verifier_with_known_subagents<const N: usize>(
    known_spawn_ids: [&'static str; N],
) -> GroundingVerifier<FakeSessionSpawnResolver> {
    GroundingVerifier::new(FileSourceResolver, FakeSessionSpawnResolver::new(known_spawn_ids))
}

fn fixture_path(file_name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("grounding").join(file_name)
}

fn file_ref(path: &Path, line_fragment: &str) -> String {
    format!("file:{}{}", path.display(), line_fragment)
}

fn assert_grounding_refusal(decision: GovernanceDecision) {
    assert_eq!(
        decision,
        GovernanceDecision::Refused {
            reason: GovernanceRefusalReason::Grounding,
            message: "source references could not be grounded".to_owned(),
            next_action: NextAction::NoWrite,
        }
    );
}

struct FakeSessionSpawnResolver {
    known_spawn_ids: HashSet<&'static str>,
}

impl FakeSessionSpawnResolver {
    fn new<const N: usize>(known_spawn_ids: [&'static str; N]) -> Self {
        Self { known_spawn_ids: known_spawn_ids.into_iter().collect() }
    }
}

impl SessionSpawnResolver for FakeSessionSpawnResolver {
    fn spawned_in_session(&self, spawn_id: &str) -> bool {
        self.known_spawn_ids.contains(spawn_id)
    }
}

#[derive(Clone, Copy, Debug)]
struct FakeWebCaptureResolver(SourceResolution);

impl WebCaptureResolver for FakeWebCaptureResolver {
    fn resolve_web_capture(&self, source_ref: &str) -> SourceResolution {
        if !source_ref.starts_with("webcap:") || !source_ref.contains('#') {
            return SourceResolution::Unsupported;
        }
        self.0
    }
}
