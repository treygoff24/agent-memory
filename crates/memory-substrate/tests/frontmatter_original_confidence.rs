use chrono::{TimeZone, Utc};
use memory_substrate::{
    frontmatter::{parse_frontmatter_yaml, serialize_frontmatter},
    Author, AuthorKind, Frontmatter, MemoryId, MemoryStatus, MemoryType, RetrievalPolicy, Scope, Sensitivity, Source,
    SourceKind, TrustLevel, WritePolicy,
};

#[test]
fn frontmatter_round_trips_with_original_confidence() {
    let mut frontmatter = sample_frontmatter();
    frontmatter.original_confidence = Some(0.92);

    let encoded = serde_json::to_string(&frontmatter).expect("serialize frontmatter");
    assert!(encoded.contains("original_confidence"));
    let decoded: Frontmatter = serde_json::from_str(&encoded).expect("deserialize frontmatter");

    assert_eq!(decoded.original_confidence, Some(0.92));
}

#[test]
fn frontmatter_defaults_original_confidence_to_none_when_omitted() {
    let frontmatter = sample_frontmatter();
    let mut value = serde_json::to_value(&frontmatter).expect("frontmatter value");
    value.as_object_mut().expect("frontmatter object").remove("original_confidence");

    let decoded: Frontmatter = serde_json::from_value(value).expect("deserialize without original_confidence");

    assert_eq!(decoded.original_confidence, None);
}

#[test]
fn frontmatter_skips_original_confidence_when_none() {
    let frontmatter = sample_frontmatter();

    let encoded = serde_json::to_string(&frontmatter).expect("serialize frontmatter");

    assert!(!encoded.contains("original_confidence"));
}

#[test]
fn frontmatter_promotes_observed_at_to_typed_round_trip_field() {
    let observed_at = Utc.with_ymd_and_hms(2026, 5, 2, 14, 30, 0).single().expect("fixture time");
    let mut frontmatter = sample_frontmatter();
    frontmatter.observed_at = Some(observed_at);

    let yaml = serialize_frontmatter(&frontmatter).expect("serialize frontmatter");
    assert!(yaml.contains("observed_at: 2026-05-02T14:30:00"));
    let (decoded, warnings) = parse_frontmatter_yaml(&yaml).expect("parse frontmatter");

    assert_eq!(decoded.observed_at, Some(observed_at));
    assert!(!decoded.extras.contains_key("observed_at"));
    assert!(!warnings.iter().any(|warning| {
        matches!(
            warning,
            memory_substrate::ValidationWarning::UnknownFieldPreserved { field } if field == "observed_at"
        )
    }));
}

fn sample_frontmatter() -> Frontmatter {
    let now = Utc::now();
    Frontmatter {
        schema_version: 1,
        id: MemoryId::new("mem_20260501_a1b2c3d4e5f60718_000201"),
        memory_type: MemoryType::Pattern,
        scope: Scope::Agent,
        summary: "confidence fixture".to_string(),
        confidence: 0.75,
        original_confidence: None,
        trust_level: TrustLevel::Trusted,
        sensitivity: Sensitivity::Internal,
        status: MemoryStatus::Active,
        created_at: now,
        updated_at: now,
        observed_at: None,
        author: Author {
            kind: AuthorKind::System,
            user_handle: None,
            harness: None,
            harness_version: None,
            session_id: None,
            subagent_id: None,
            phase: None,
            component: Some("test".to_string()),
        },
        namespace: None,
        canonical_namespace_id: None,
        tags: Vec::new(),
        entities: Vec::new(),
        aliases: Vec::new(),
        source: Source {
            kind: SourceKind::Import,
            reference: None,
            harness: None,
            harness_version: None,
            session_id: None,
            subagent_id: None,
            device: None,
        },
        evidence: Vec::new(),
        requires_user_confirmation: false,
        review_state: None,
        supersedes: Vec::new(),
        superseded_by: Vec::new(),
        related: Vec::new(),
        tombstone_events: Vec::new(),
        retrieval_policy: RetrievalPolicy {
            passive_recall: true,
            max_scope: Scope::Agent,
            mask_personal_for_synthesis: false,
            index_body: true,
            index_embeddings: false,
        },
        write_policy: WritePolicy {
            human_review_required: false,
            policy_applied: "default-v1".to_string(),
            expected_base_hash: None,
        },
        merge_diagnostics: None,
        abstraction: None,
        cues: Vec::new(),
        extras: Default::default(),
    }
}
