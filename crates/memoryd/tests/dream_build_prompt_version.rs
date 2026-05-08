use memory_substrate::{config::PromptVersion, InitOptions, Roots, Substrate};
use memoryd::dream::{
    orchestration::{build_dream_run, DreamRunBuildRequest},
    scope::DreamScope,
};

async fn init_substrate(roots: Roots) -> Substrate {
    Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_dreampromptver".to_string()) },
    )
    .await
    .expect("init substrate")
}

#[tokio::test]
async fn build_dream_run_carries_prompt_version_into_runtime_options() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(Roots::new(temp.path().join("repo"), temp.path().join("runtime"))).await;

    for prompt_version in [PromptVersion::V1, PromptVersion::V2] {
        let build = build_dream_run(
            &substrate,
            DreamRunBuildRequest {
                scope: DreamScope::Agent,
                run_id: format!("run_{prompt_version:?}"),
                run_date: chrono::Utc::now().date_naive(),
                prompt_version,
                notifications: None,
                pass_timeout: std::time::Duration::from_secs(1),
                pass_2_max_candidates: 8,
                pass_1_window_days: 7,
            },
        )
        .await
        .expect("dream run builds");

        assert_eq!(build.options.prompt_version, prompt_version);
    }
}
