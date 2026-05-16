use chrono::NaiveDate;
use memory_substrate::config::{DreamsConfig, PromptVersion};
use memoryd::dream::{
    prompts::{render_prompt, DreamPromptInput},
    scope::DreamScope,
    types::{ActiveMemory, DreamPass, EvidenceCatalogEntry, HarnessSelection, MaskingContext, SubstrateFragment},
};

fn prompt_input() -> DreamPromptInput {
    DreamPromptInput {
        scope: DreamScope::parse("project:proj_prompt").expect("scope"),
        run_date: NaiveDate::from_ymd_opt(2026, 5, 7).expect("date"),
        harness: HarnessSelection { name: "codex".to_string(), prompt_transport: "stdin".to_string() },
        masking: MaskingContext {
            session_id: "dream:project:proj_prompt:run_prompt".to_string(),
            seed_surrogate: "mask_seed_prompt".to_string(),
        },
        substrate_fragments: vec![SubstrateFragment {
            id: "sub_prompt_01".to_string(),
            kind: "pattern".to_string(),
            ts: "2026-05-07T12:00:00Z".to_string(),
            entities: vec!["ent_auth_flow".to_string()],
            text: "<PERSON_A> saw auth retry state drift.".to_string(),
        }],
        active_memories: vec![ActiveMemory {
            id: "mem_prompt_01".to_string(),
            namespace: "project:proj_prompt".to_string(),
            kind: "decision".to_string(),
            entities: vec!["ent_auth_flow".to_string()],
            summary: "Auth retries belong behind one seam.".to_string(),
        }],
        pass_1_markdown: Some("# Dream reflection\nAuth retry state is drifting.".to_string()),
        previous_questions: vec!["Which auth retry owner is authoritative?".to_string()],
        evidence_catalog: vec![EvidenceCatalogEntry {
            kind: "substrate_fragment".to_string(),
            reference: "sub_prompt_01".to_string(),
            entities: vec!["ent_auth_flow".to_string()],
            excerpt: "<PERSON_A> saw auth retry state drift.".to_string(),
        }],
    }
}

#[test]
fn dreams_config_prompt_version_defaults_to_v2_and_accepts_explicit_versions() {
    let missing: DreamsConfig = yaml_serde::from_str("{}").expect("missing field defaults");
    let v1: DreamsConfig = yaml_serde::from_str("prompt_version: V1\n").expect("explicit v1");
    let v2: DreamsConfig = yaml_serde::from_str("prompt_version: V2\n").expect("explicit v2");

    assert_eq!(missing.prompt_version, PromptVersion::V2);
    assert_eq!(v1.prompt_version, PromptVersion::V1);
    assert_eq!(v2.prompt_version, PromptVersion::V2);
}

#[test]
fn render_prompt_selects_v1_or_v2_template_for_each_pass() {
    let input = prompt_input();

    for pass in [DreamPass::Pass1, DreamPass::Pass2, DreamPass::Pass3] {
        let v1 = render_prompt(pass, &input, PromptVersion::V1).expect("render v1");
        let v2 = render_prompt(pass, &input, PromptVersion::V2).expect("render v2");

        assert!(v1.contains("v1"), "v1 template marker missing for {pass:?}: {v1}");
        assert!(v2.contains("v2"), "v2 template marker missing for {pass:?}: {v2}");
        assert_ne!(v1, v2, "prompt versions should produce distinct prompts for {pass:?}");
    }
}
