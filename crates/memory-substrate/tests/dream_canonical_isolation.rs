use memory_substrate::tree::{bootstrap_repo_tree, validate_tree, TreeValidationMode};
use memory_substrate::{
    ChunkQuery, ClassificationOutcome, Entity, EventContext, Evidence, InitOptions, Memory, MemoryId, MemoryQuery,
    MemoryStatus, MemoryType, ReadError, RecallIndexQuery, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity,
    Source, SourceKind, Substrate, SupersedeRequest, TrustLevel, ValidationError, WriteFailureKind, WriteMode,
    WriteRequest,
};
use std::process::Command;

#[test]
fn validates_stream_f_noncanonical_tree_files_without_canonical_frontmatter() {
    let temp = tempfile::tempdir().expect("tempdir");
    bootstrap_repo_tree(temp.path()).expect("bootstrap tree");

    write_file(temp.path(), "dreams/journal/me/2026-04-30.md", "Masked journal body without frontmatter.\n");
    write_file(
        temp.path(),
        "dreams/questions/project/proj_abc/2026-04-30.jsonl",
        r#"{"entities":["ent_stream_f"],"question":"What assumption should we revisit?"}"#,
    );
    write_file(temp.path(), "dreams/cleanup/dev_local/2026-04-30.json", r#"{"device_id":"dev_local","operations":[]}"#);
    write_file(
        temp.path(),
        "substrate/dev_local/2026-04-30.jsonl",
        r#"{"id":"sub_01","text":"plain substrate observation"}"#,
    );
    write_file(
        temp.path(),
        "encrypted/substrate/dev_local/2026-04-30.jsonl",
        r#"{"id":"sub_02","ciphertext":"age1...","descriptor":"encrypted fragment"}"#,
    );
    write_file(temp.path(), "leases/journal.lease", r#"{"scope":"me","device_id":"dev_local"}"#);

    validate_tree(temp.path(), TreeValidationMode::FullySynced).expect("noncanonical Stream F files validate");
}

#[test]
fn generated_gitattributes_routes_stream_f_merge_families_to_memory_merge_driver() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_checked(temp.path(), &["init"]);
    bootstrap_repo_tree(temp.path()).expect("bootstrap tree");

    for path in [
        "substrate/dev_local/2026-04-30.jsonl",
        "substrate/archive/dev_local/2026-04.jsonl",
        "encrypted/substrate/dev_local/2026-04-30.jsonl",
        "dreams/questions/me/2026-04-30.jsonl",
        "dreams/cleanup/dev_local/2026-04-30.json",
        "dreams/journal/me/2026-04-30.md",
        "leases/journal.lease",
        "agent/patterns/mem_20260430_a1b2c3d4e5f60718_000001.md",
    ] {
        assert_eq!(git_merge_attr(temp.path(), path), "memory-merge-driver", "{path}");
    }
}

#[test]
fn existing_stale_gitattributes_are_reconciled_without_losing_user_rules() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_checked(temp.path(), &["init"]);
    std::fs::write(
        temp.path().join(".gitattributes"),
        "# user-managed rules\n\
* text=auto eol=crlf\n\
* -diff\n\
*.txt merge=union\n\
*.md merge=union\n\
events/*.jsonl merge=union\n\
substrate/**/*.jsonl merge=union\n\
dreams/questions/**/*.jsonl merge=union\n\
leases/journal.lease merge=union\n",
    )
    .expect("seed stale gitattributes");

    bootstrap_repo_tree(temp.path()).expect("bootstrap existing tree");

    let gitattributes = std::fs::read_to_string(temp.path().join(".gitattributes")).expect("read gitattributes");
    assert!(gitattributes.contains("# user-managed rules"));
    assert!(gitattributes.contains("* -diff"));
    assert!(gitattributes.contains("*.txt merge=union"));
    assert!(!gitattributes.contains("* text=auto eol=crlf"));
    assert!(!gitattributes.contains("dreams/questions/**/*.jsonl merge=union"));
    assert!(!gitattributes.contains("leases/journal.lease merge=union"));
    assert!(gitattributes.contains("* text eol=lf"));
    assert!(gitattributes.contains("dreams/questions/**/*.jsonl merge=memory-merge-driver"));
    assert!(gitattributes.contains("leases/journal.lease merge=memory-merge-driver"));
    bootstrap_repo_tree(temp.path()).expect("bootstrap remains idempotent");
    assert_eq!(
        std::fs::read_to_string(temp.path().join(".gitattributes")).expect("read gitattributes again"),
        gitattributes
    );
    for path in [
        "agent/patterns/mem_20260430_a1b2c3d4e5f60718_000001.md",
        "substrate/archive/dev_local/2026-04.jsonl",
        "encrypted/substrate/dev_local/2026-04-30.jsonl",
        "dreams/questions/project/proj_abc/2026-04-30.jsonl",
        "dreams/cleanup/dev_local/2026-04-30.json",
        "dreams/journal/me/2026-04-30.md",
        "leases/journal.lease",
    ] {
        assert_eq!(git_merge_attr(temp.path(), path), "memory-merge-driver", "{path}");
    }
}

#[test]
fn combined_global_gitattributes_preserve_unmanaged_attributes() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_checked(temp.path(), &["init"]);
    std::fs::write(
        temp.path().join(".gitattributes"),
        "# user-managed global attributes\n\
* text=auto eol=crlf -diff filter=lfs linguist-generated\n",
    )
    .expect("seed combined global gitattributes");

    bootstrap_repo_tree(temp.path()).expect("bootstrap existing tree");

    let gitattributes = std::fs::read_to_string(temp.path().join(".gitattributes")).expect("read gitattributes");
    assert!(gitattributes.contains("* text eol=lf"));
    assert!(gitattributes.contains("* -diff filter=lfs linguist-generated"));
    assert!(!gitattributes.contains("text=auto"));
    assert!(!gitattributes.contains("eol=crlf"));
    bootstrap_repo_tree(temp.path()).expect("bootstrap remains idempotent");
    assert_eq!(
        std::fs::read_to_string(temp.path().join(".gitattributes")).expect("read gitattributes again"),
        gitattributes
    );
}

#[test]
fn rejects_malformed_stream_f_dream_files_with_typed_validation_errors() {
    let temp = tempfile::tempdir().expect("tempdir");
    bootstrap_repo_tree(temp.path()).expect("bootstrap tree");
    write_file(temp.path(), "dreams/questions/project/proj_abc/2026-04-30.jsonl", r#"{"entities":[]}"#);

    let err = validate_tree(temp.path(), TreeValidationMode::FullySynced).expect_err("invalid question record");

    assert!(matches!(
        err,
        ValidationError::NonCanonicalStreamFFile { path, .. }
            if path == std::path::PathBuf::from("dreams/questions/project/proj_abc/2026-04-30.jsonl")
    ));
}

#[test]
fn rejects_dream_cleanup_reports_that_are_not_json_objects() {
    let temp = tempfile::tempdir().expect("tempdir");
    bootstrap_repo_tree(temp.path()).expect("bootstrap tree");
    write_file(temp.path(), "dreams/cleanup/dev_local/2026-04-30.json", "[]");

    let err = validate_tree(temp.path(), TreeValidationMode::FullySynced).expect_err("invalid cleanup report");

    assert!(matches!(
        err,
        ValidationError::NonCanonicalStreamFFile { path, .. }
            if path == std::path::PathBuf::from("dreams/cleanup/dev_local/2026-04-30.json")
    ));
}

#[tokio::test]
async fn read_path_envelope_refuses_stream_f_noncanonical_files_before_frontmatter_parsing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init substrate");

    for (path, contents) in noncanonical_files() {
        write_file(&roots.repo, path, contents);
        let repo_path = RepoPath::new(path);

        let err = substrate.read_path_envelope(&repo_path).await.expect_err("noncanonical read refused");

        assert!(matches!(err, ReadError::NotACanonicalMemory { path: refused } if refused == repo_path));
    }
}

#[tokio::test]
async fn stream_f_noncanonical_files_are_never_indexed_or_returned_by_queries() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init substrate");
    let memory = canonical_memory("mem_20260430_a1b2c3d4e5f60718_000001");
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: Default::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write canonical memory");
    for (path, contents) in noncanonical_files() {
        write_file(&roots.repo, path, contents);
    }

    let indexed = substrate.reindex().await.expect("reindex ignores noncanonical files");
    let memory_hits = substrate.query_memory(MemoryQuery::default()).await.expect("memory query");
    let recall_hits = substrate
        .query_recall_index(RecallIndexQuery {
            namespace_prefix: None,
            statuses: vec![MemoryStatus::Active],
            passive_recall_only: true,
            updated_since: None,
            match_terms: vec!["stream-f-isolation".to_string()],
        })
        .await
        .expect("recall query");
    let chunk_hits = substrate
        .query_chunks(ChunkQuery { text: Some("stream-f-isolation".to_string()), triple: None, vector: None })
        .await
        .expect("chunk query");

    assert_eq!(indexed, 1);
    assert_eq!(memory_hits.len(), 1);
    assert_eq!(memory_hits[0].path, memory.path.clone().expect("canonical path"));
    assert_eq!(recall_hits.len(), 1);
    assert_eq!(recall_hits[0].path, memory.path.clone().expect("canonical path"));
    assert_eq!(chunk_hits.len(), 1);
    assert_eq!(chunk_hits[0].memory_id, memory.frontmatter.id);
}

#[tokio::test]
async fn write_memory_refuses_dream_artifacts_as_grounding_sources() {
    let (_temp, substrate) = initialized_substrate().await;

    let mut source_journal = canonical_memory("mem_20260430_a1b2c3d4e5f60718_000101");
    source_journal.frontmatter.source.reference = Some("dreams/journal/me/2026-04-30.md".to_string());
    let evidence_journal = memory_with_evidence_ref(
        canonical_memory("mem_20260430_a1b2c3d4e5f60718_000102"),
        "dreams/journal/me/2026-04-30.md",
    );
    let evidence_questions = memory_with_evidence_ref(
        canonical_memory("mem_20260430_a1b2c3d4e5f60718_000103"),
        "file:dreams/questions/me/2026-04-30.jsonl#L1",
    );
    let mut source_cleanup = canonical_memory("mem_20260430_a1b2c3d4e5f60718_000104");
    source_cleanup.frontmatter.source.reference = Some("dreams/cleanup/dev_test/2026-04-30.json".to_string());

    for (label, memory) in [
        ("source journal", source_journal),
        ("evidence journal", evidence_journal),
        ("evidence questions", evidence_questions),
        ("source cleanup", source_cleanup),
    ] {
        let err = substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .expect_err("dream artifacts cannot ground canonical memory");

        assert_eq!(err.kind, WriteFailureKind::DreamProseAsSource, "{label}");
        assert!(!err.outcome.committed, "{label}");
    }
}

#[tokio::test]
async fn supersede_memory_refuses_dream_artifacts_as_grounding_sources() {
    let (_temp, substrate) = initialized_substrate().await;
    let old = canonical_memory("mem_20260430_a1b2c3d4e5f60718_000111");
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: old.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("seed old memory");
    let replacement = memory_with_evidence_ref(
        canonical_memory("mem_20260430_a1b2c3d4e5f60718_000112"),
        "dreams/questions/project/proj_abc/2026-04-30.jsonl",
    );

    let err = substrate
        .supersede_memory(SupersedeRequest {
            old_id: old.frontmatter.id,
            replacement,
            reason: "regression guard".to_string(),
            classification: ClassificationOutcome::Trusted,
            allow_best_effort_durability: true,
        })
        .await
        .expect_err("supersede replacement cannot cite dream prose");

    assert_eq!(err.kind, WriteFailureKind::DreamProseAsSource);
    assert!(!err.outcome.committed);
}

fn noncanonical_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("dreams/journal/me/2026-04-30.md", "Masked journal body without frontmatter.\n"),
        (
            "dreams/questions/project/proj_abc/2026-04-30.jsonl",
            r#"{"entities":["ent_stream_f"],"question":"What assumption should we revisit?"}"#,
        ),
        ("dreams/cleanup/dev_local/2026-04-30.json", r#"{"device_id":"dev_local","operations":[]}"#),
        ("substrate/dev_local/2026-04-30.jsonl", r#"{"id":"sub_01","text":"plain substrate observation"}"#),
        (
            "encrypted/substrate/dev_local/2026-04-30.jsonl",
            r#"{"id":"sub_02","ciphertext":"age1...","descriptor":"encrypted fragment"}"#,
        ),
        ("leases/journal.lease", r#"{"scope":"me","device_id":"dev_local"}"#),
    ]
}

fn write_file(root: &std::path::Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    std::fs::write(path, contents).expect("write fixture");
}

fn git_merge_attr(root: &std::path::Path, path: &str) -> String {
    let output = git_checked(root, &["check-attr", "merge", "--", path]);
    let stdout = String::from_utf8(output.stdout).expect("git output is utf8");
    stdout.trim().rsplit_once(": ").map(|(_, value)| value.to_string()).unwrap_or_else(|| stdout)
}

fn git_checked(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    let output = Command::new("git").arg("-C").arg(root).args(args).output().expect("spawn git");
    assert!(output.status.success(), "git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr));
    output
}

fn canonical_memory(id: &str) -> Memory {
    Memory {
        frontmatter: memory_frontmatter(id),
        body: "stream-f-isolation canonical body".to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}

async fn initialized_substrate() -> (tempfile::TempDir, Substrate) {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init substrate");
    (temp, substrate)
}

fn memory_with_evidence_ref(mut memory: Memory, reference: &str) -> Memory {
    memory.frontmatter.evidence.push(Evidence {
        id: "ev_01HZXJK7J7W0X4Q4KJ7A2R8V1A".to_string(),
        quote: "supporting quote".to_string(),
        quote_norm_hash: None,
        reference: reference.to_string(),
        weight: 1.0,
        observed_at: None,
        source: None,
    });
    memory
}

fn memory_frontmatter(id: &str) -> memory_substrate::Frontmatter {
    memory_substrate::Frontmatter {
        schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
        id: MemoryId::new(id),
        memory_type: MemoryType::Pattern,
        scope: Scope::Agent,
        namespace: None,
        canonical_namespace_id: None,
        summary: "stream-f-isolation canonical summary".to_string(),
        confidence: 1.0,
        original_confidence: None,
        trust_level: TrustLevel::Trusted,
        sensitivity: Sensitivity::Internal,
        status: MemoryStatus::Active,
        created_at: "2026-04-30T12:00:00Z".parse().expect("created_at"),
        updated_at: "2026-04-30T12:00:00Z".parse().expect("updated_at"),
        observed_at: None,
        author: memory_substrate::Author {
            kind: memory_substrate::AuthorKind::System,
            user_handle: None,
            harness: None,
            harness_version: None,
            session_id: None,
            subagent_id: None,
            phase: None,
            component: Some("test".to_string()),
        },
        source: Source {
            kind: SourceKind::System,
            reference: None,
            harness: None,
            harness_version: None,
            session_id: None,
            subagent_id: None,
            device: None,
        },
        tags: vec!["stream-f-isolation".to_string()],
        entities: vec![Entity {
            id: "ent_stream_f".to_string(),
            label: "Stream F".to_string(),
            aliases: vec!["stream-f-isolation".to_string()],
        }],
        aliases: Vec::new(),
        supersedes: Vec::new(),
        superseded_by: Vec::new(),
        related: Vec::new(),
        evidence: Vec::new(),
        retrieval_policy: RetrievalPolicy {
            passive_recall: true,
            max_scope: Scope::Agent,
            mask_personal_for_synthesis: false,
            index_body: true,
            index_embeddings: false,
        },
        write_policy: memory_substrate::WritePolicy {
            human_review_required: false,
            policy_applied: "test".to_string(),
            expected_base_hash: None,
        },
        review_state: None,
        requires_user_confirmation: false,
        tombstone_events: Vec::new(),
        merge_diagnostics: None,
        extras: Default::default(),
    }
}
