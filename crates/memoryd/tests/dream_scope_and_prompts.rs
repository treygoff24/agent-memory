use chrono::NaiveDate;
use memory_substrate::config::PromptVersion;
use memoryd::dream::prompts::{render_prompt, DreamPromptInput};
use memoryd::dream::scope::DreamScope;
use memoryd::dream::types::{
    ActiveMemory, DreamPass, EvidenceCatalogEntry, HarnessSelection, MaskingContext, SubstrateFragment,
};

#[test]
fn dream_scope_encodes_journal_and_question_paths() {
    let date = NaiveDate::from_ymd_opt(2026, 4, 30).expect("valid test date");

    let cases = [
        ("me", "me/2026-04-30", "dreams/journal/me/2026-04-30.md", "dreams/questions/me/2026-04-30.jsonl"),
        ("agent", "agent/2026-04-30", "dreams/journal/agent/2026-04-30.md", "dreams/questions/agent/2026-04-30.jsonl"),
        (
            "project:proj_abc",
            "project/proj_abc/2026-04-30",
            "dreams/journal/project/proj_abc/2026-04-30.md",
            "dreams/questions/project/proj_abc/2026-04-30.jsonl",
        ),
        (
            "org:org_abc",
            "org/org_abc/2026-04-30",
            "dreams/journal/org/org_abc/2026-04-30.md",
            "dreams/questions/org/org_abc/2026-04-30.jsonl",
        ),
    ];

    for (raw_scope, expected_scope_path, expected_journal, expected_questions) in cases {
        let scope = DreamScope::parse(raw_scope).expect("valid dream scope");

        assert_eq!(scope.storage_path_for_date(date), expected_scope_path);
        assert_eq!(scope.journal_path(date), expected_journal);
        assert_eq!(scope.questions_path(date), expected_questions);
    }
}

#[test]
fn invalid_scope_rejects_with_stable_invalid_request_code() {
    for raw_scope in ["project:", "project:.", "project:..", "org:.", "org:.."] {
        let error = DreamScope::parse(raw_scope).expect_err("invalid scope id must reject");

        assert_eq!(error.code(), "invalid_request");
        assert!(
            error.to_string().contains("invalid dream scope"),
            "error should stay human-readable without changing the stable code: {error}"
        );
    }
}

#[test]
fn dream_prompts_are_deterministic_embedded_and_pass_2_gets_evidence_catalog() {
    let date = NaiveDate::from_ymd_opt(2026, 4, 30).expect("valid test date");
    let scope = DreamScope::parse("project:proj_abc").expect("valid project scope");
    let input = DreamPromptInput {
        scope,
        run_date: date,
        harness: HarnessSelection { name: "codex".to_string(), prompt_transport: "stdin".to_string() },
        masking: MaskingContext {
            session_id: "dream:project:proj_abc:run_01".to_string(),
            seed_surrogate: "mask_seed_surrogate_01".to_string(),
        },
        substrate_fragments: vec![
            SubstrateFragment {
                id: "sub_01".to_string(),
                kind: "pattern".to_string(),
                ts: "2026-04-30T12:00:00Z".to_string(),
                entities: vec!["ent_auth_flow".to_string(), "ent_jwt".to_string()],
                text: "<PERSON_A> saw JWT rotation failures three times.".to_string(),
            },
            SubstrateFragment {
                id: "sub_02".to_string(),
                kind: "signal".to_string(),
                ts: "2026-04-30T13:00:00Z".to_string(),
                entities: vec!["ent_auth_flow".to_string()],
                text: "The fix keeps landing in route middleware.".to_string(),
            },
        ],
        active_memories: vec![ActiveMemory {
            id: "mem_20260430_auth".to_string(),
            namespace: "project:proj_abc".to_string(),
            kind: "decision".to_string(),
            entities: vec!["ent_auth_flow".to_string()],
            summary: "JWT key rotation belongs behind a single verifier seam.".to_string(),
        }],
        pass_1_markdown: Some("# Why\nThe masked pattern is repeating near <PERSON_A>.".to_string()),
        previous_questions: vec!["What auth assumption did we avoid testing?".to_string()],
        evidence_catalog: vec![
            EvidenceCatalogEntry {
                kind: "substrate_fragment".to_string(),
                reference: "sub_01".to_string(),
                entities: vec!["ent_auth_flow".to_string(), "ent_jwt".to_string()],
                excerpt: "<PERSON_A> saw JWT rotation failures three times.".to_string(),
            },
            EvidenceCatalogEntry {
                kind: "memory".to_string(),
                reference: "mem_20260430_auth".to_string(),
                entities: vec!["ent_auth_flow".to_string()],
                excerpt: "JWT key rotation belongs behind a single verifier seam.".to_string(),
            },
        ],
    };

    let temp_dir = tempfile::tempdir().expect("temp dir");
    let original_cwd = std::env::current_dir().expect("cwd before test");
    std::env::set_current_dir(temp_dir.path()).expect("move cwd away from repo prompts directory");

    let pass_1 = render_prompt(DreamPass::Pass1, &input, PromptVersion::V2).expect("render pass 1");
    let pass_1_again = render_prompt(DreamPass::Pass1, &input, PromptVersion::V2).expect("render pass 1 again");
    let pass_2 = render_prompt(DreamPass::Pass2, &input, PromptVersion::V2).expect("render pass 2");
    let pass_2_again = render_prompt(DreamPass::Pass2, &input, PromptVersion::V2).expect("render pass 2 again");
    let pass_3 = render_prompt(DreamPass::Pass3, &input, PromptVersion::V2).expect("render pass 3");
    let pass_3_again = render_prompt(DreamPass::Pass3, &input, PromptVersion::V2).expect("render pass 3 again");

    std::env::set_current_dir(original_cwd).expect("restore cwd after prompt test");

    assert_eq!(pass_1, pass_1_again);
    assert_eq!(pass_2, pass_2_again);
    assert_eq!(pass_3, pass_3_again);

    assert!(!pass_1.contains("evidence_catalog"));
    assert!(pass_2.contains("evidence_catalog"));
    assert!(pass_2.contains("sub_01"));
    assert!(pass_2.contains("mem_20260430_auth"));
    assert!(!pass_3.contains("evidence_catalog"));
}
