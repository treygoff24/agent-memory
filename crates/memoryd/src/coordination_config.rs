use std::path::Path;

use memorum_coordination::CoordinationConfig;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CoordinationConfigRoot {
    #[serde(default)]
    coordination: Option<CoordinationConfig>,
}

pub fn load_coordination_config(repo: &Path) -> Result<CoordinationConfig, String> {
    let path = repo.join("config.yaml");
    if !path.exists() {
        return Ok(CoordinationConfig::default());
    }

    let text = std::fs::read_to_string(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let root: CoordinationConfigRoot =
        serde_yaml::from_str(&text).map_err(|error| format!("parse {}: {error}", path.display()))?;
    let config = root.coordination.unwrap_or_default();
    config.validate()?;
    Ok(config)
}
