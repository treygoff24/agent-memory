use std::path::Path;

use memory_privacy::{
    DeterministicPrivacyClassifier, FileKeyProvider, KeyProvider, KeyRotation, PrivacyClassifier, PrivacyNamespace,
};
use memory_substrate::events::EventKind;
use memory_substrate::{Roots, Substrate};

use crate::cli::{DeviceArgs, DeviceCommand, PrivacyArgs, PrivacyCommand, PrivacyFilterArgs, PrivacyFilterCommand};

pub async fn run_privacy(args: PrivacyArgs) -> anyhow::Result<()> {
    match args.command {
        PrivacyCommand::Status(args) => {
            let key_provider = FileKeyProvider::runtime_default(&args.runtime);
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "stream": "D",
                    "layer1": "enabled",
                    "privacy_filter": "disabled",
                    "encrypted_key_available": key_provider.load_key().is_ok(),
                    "guidance": "Layer 1 regex/entropy scanning is always on; optional Privacy Filter is disabled unless configured."
                }))?
            );
        }
        PrivacyCommand::Scan(scan) => {
            let text = match (scan.text, scan.file) {
                (Some(text), None) => text,
                (None, Some(path)) => std::fs::read_to_string(path)?,
                _ => anyhow::bail!("provide exactly one of --text or --file"),
            };
            let classifier = DeterministicPrivacyClassifier::new();
            let decision = classifier.classify(&text, PrivacyNamespace::Project, None)?;
            println!("{}", serde_json::to_string_pretty(&decision)?);
        }
        PrivacyCommand::ScanDelta(args) => {
            let output = std::process::Command::new("git")
                .args(["-C", args.repo.to_string_lossy().as_ref(), "diff", "--cached", "--no-ext-diff", "-U0"])
                .output()?;
            if !output.status.success() {
                anyhow::bail!("git diff --cached failed");
            }
            let text = String::from_utf8(output.stdout)?;
            let classifier = DeterministicPrivacyClassifier::new();
            let decision = classifier.classify(&text, PrivacyNamespace::Project, None)?;
            println!("{}", serde_json::to_string_pretty(&decision)?);
            if decision.tier == memory_privacy::PrivacyTier::Secret {
                anyhow::bail!("staged delta contains secret-like material");
            }
        }
    }
    Ok(())
}

pub async fn run_privacy_filter(args: PrivacyFilterArgs) -> anyhow::Result<()> {
    match args.command {
        PrivacyFilterCommand::Install => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "status": "not_installed",
                    "guidance": "No model weights are downloaded by normal tests. Install the optional OpenAI Privacy Filter out of band, then enable the provider."
                }))?
            );
        }
        PrivacyFilterCommand::Enable => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"status": "disabled", "reason": "provider runtime not configured"})
                )?
            );
        }
        PrivacyFilterCommand::Disable => {
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({"status": "disabled"}))?);
        }
        PrivacyFilterCommand::Status => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({"status": "disabled", "layer1": "enabled"}))?
            );
        }
    }
    Ok(())
}

pub async fn run_device(args: DeviceArgs) -> anyhow::Result<()> {
    match args.command {
        DeviceCommand::Onboard(args) => {
            let provider = FileKeyProvider::runtime_default(&args.runtime);
            let key = provider.onboard_local_file()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "status": "ok",
                    "recipient": key.recipient,
                    "key_path": provider.path(),
                    "guidance": "Local Stream D key material created for encrypted-tier writes."
                }))?
            );
        }
        DeviceCommand::RotateKeys(args) => {
            let provider = FileKeyProvider::runtime_default(&args.runtime);
            let rotation = provider.rotate_local_file()?;
            record_device_keys_rotated_event(&args.runtime, &rotation).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "status": "ok",
                    "recipient": rotation.active_recipient,
                    "previous_recipient": rotation.previous_recipient,
                    "key_path": rotation.active_key_path,
                    "active_manifest": rotation.active_manifest_path,
                    "archived_key_path": rotation.archived_key_path,
                    "guidance": "Local Stream D key material rotated; old local identities remain available for reveal continuity."
                }))?
            );
        }
        DeviceCommand::Revoke(args) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "status": "operator_required",
                    "device_id": args.device_id,
                    "runtime": args.runtime,
                    "guidance": "Remove the device recipient from trusted devices and rotate keys."
                }))?
            );
        }
    }
    Ok(())
}

async fn record_device_keys_rotated_event(runtime: &Path, rotation: &KeyRotation) -> anyhow::Result<()> {
    let Some(local_config) = memory_substrate::config::load_local_device_config(runtime).map_err(anyhow::Error::msg)?
    else {
        return Ok(());
    };
    let Some(repo) = local_config.paths.memory_root else {
        return Ok(());
    };

    let substrate = Substrate::open(Roots::new(repo, runtime)).await?;
    substrate.record_event_best_effort(EventKind::DeviceKeysRotated {
        previous_recipient: rotation.previous_recipient.clone(),
        active_recipient: rotation.active_recipient.clone(),
    })?;
    Ok(())
}
