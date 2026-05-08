use clap::Parser as _;
use memory_substrate::{DurabilityTier, InitOptions, Roots, Substrate};
use memoryd::cli::{Cli, Command as CliCommand};

#[test]
fn serve_init_defaults_to_safe_durability_flag_off() {
    let cli = Cli::try_parse_from(["memoryd", "serve", "--init"]).expect("serve parses");

    let CliCommand::Serve(args) = cli.command else { panic!("expected serve command") };
    assert!(args.init);
    assert!(!args.force_unsafe_durability);
}

#[test]
fn serve_init_force_unsafe_durability_is_explicit_opt_in() {
    let cli = Cli::try_parse_from(["memoryd", "serve", "--init", "--force-unsafe-durability"]).expect("serve parses");

    let CliCommand::Serve(args) = cli.command else { panic!("expected serve command") };
    assert!(args.init);
    assert!(args.force_unsafe_durability);
}

#[test]
fn serve_init_safe_options_open_with_full_durability_when_supported() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));

    let substrate =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime").block_on(async {
            Substrate::init(
                roots,
                InitOptions { force_unsafe_durability: false, device_id: Some("dev_servedurability".to_string()) },
            )
            .await
            .expect("safe init")
        });

    assert_eq!(substrate.durability_tier(), DurabilityTier::Full);
}

#[test]
fn serve_init_force_unsafe_options_open_best_effort() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));

    let substrate =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime").block_on(async {
            Substrate::init(
                roots,
                InitOptions { force_unsafe_durability: true, device_id: Some("dev_servedurability".to_string()) },
            )
            .await
            .expect("unsafe init")
        });

    assert_eq!(substrate.durability_tier(), DurabilityTier::BestEffort);
}
