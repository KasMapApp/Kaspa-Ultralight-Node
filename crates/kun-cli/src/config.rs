//! Configuration persisted at `~/.kun/config.json` (v0.1 uses JSON for
//! hand-readability; v0.2 switches to TOML).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub default_node: String,
    pub network: String,
    #[serde(default)]
    pub hash_era: HashEraSetting,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum HashEraSetting {
    #[default]
    Auto,
    Blake2b,
    Blake3,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_node: "ws://localhost:17110".into(),
            network: "mainnet".into(),
            hash_era: HashEraSetting::default(),
        }
    }
}

pub fn data_dir_override(globals_data_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = globals_data_dir {
        return Ok(p.to_path_buf());
    }
    let dirs = ProjectDirs::from("sh", "kun", "kun").context("could not resolve user directories")?;
    Ok(dirs.data_local_dir().to_path_buf())
}

pub fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("config.json")
}

pub fn load(data_dir: &Path) -> Result<Config> {
    let path = config_path(data_dir);
    if !path.exists() {
        return Ok(Config::default());
    }
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?)
}

pub fn save(data_dir: &Path, cfg: &Config) -> Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let path = config_path(data_dir);
    let bytes = serde_json::to_vec_pretty(cfg)?;
    std::fs::write(&path, bytes)?;
    Ok(())
}
