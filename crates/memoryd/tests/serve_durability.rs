use clap::Parser as _;
use memory_substrate::markdown::probe_durability;
use memory_substrate::{DurabilityTier, OpenError, Roots};
use memoryd::cli::{Cli, Command as CliCommand};
use memoryd::serve_runtime::open_substrate_for_serve;
use std::ffi::OsString;

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
    let expected_tier = probe_durability(&roots.repo, false);
    let args = parse_serve_init_args(&roots, false);

    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(async { open_substrate_for_serve(&args).await });

    match expected_tier {
        DurabilityTier::Full | DurabilityTier::BestEffort => {
            assert_eq!(result.expect("supported safe init").durability_tier(), expected_tier);
        }
        DurabilityTier::Refused => {
            assert!(matches!(result, Err(OpenError::DurabilityUnsupported { tier: DurabilityTier::Refused })));
        }
    }
}

#[test]
fn serve_init_force_unsafe_options_open_best_effort() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let args = parse_serve_init_args(&roots, true);

    let substrate = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(async { open_substrate_for_serve(&args).await.expect("unsafe init") });

    assert_eq!(substrate.durability_tier(), DurabilityTier::BestEffort);
}

fn parse_serve_init_args(roots: &Roots, force_unsafe_durability: bool) -> memoryd::cli::ServeArgs {
    let mut argv = vec![
        OsString::from("memoryd"),
        OsString::from("serve"),
        OsString::from("--repo"),
        roots.repo.as_os_str().to_owned(),
        OsString::from("--runtime"),
        roots.runtime.as_os_str().to_owned(),
        OsString::from("--init"),
    ];
    if force_unsafe_durability {
        argv.push(OsString::from("--force-unsafe-durability"));
    }

    let cli = Cli::try_parse_from(argv).expect("serve parses");
    let CliCommand::Serve(args) = cli.command else { panic!("expected serve command") };
    args
}
