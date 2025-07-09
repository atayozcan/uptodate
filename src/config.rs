use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub dry_run: bool,
    pub enabled_sources: HashMap<String, bool>,
    pub custom_commands: Vec<CustomCommand>,
    pub save_logs: bool,
    pub logs_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomCommand {
    pub name: String,
    pub command: String,
    pub enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        let logs_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("uptodate");

        Self {
            dry_run: false,
            enabled_sources: HashMap::new(),
            custom_commands: Vec::new(),
            save_logs: true,
            logs_dir,
        }
    }
}

impl Config {
    pub async fn load() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("No config directory"))?
            .join("uptodate");

        async_std::fs::create_dir_all(&config_dir).await?;

        let config_path = config_dir.join("config.toml");
        if config_path.exists() {
            let content = async_std::fs::read_to_string(&config_path).await?;
            let config: Config = toml::from_str(&content)?;

            if config.save_logs {
                async_std::fs::create_dir_all(&config.logs_dir).await?;
            }

            Ok(config)
        } else {
            let config = Self::default();
            config.save().await?;
            Ok(config)
        }
    }

    pub async fn save(&self) -> Result<()> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("No config directory"))?
            .join("uptodate");

        async_std::fs::create_dir_all(&config_dir).await?;

        if self.save_logs {
            async_std::fs::create_dir_all(&self.logs_dir).await?;
        }

        let config_path = config_dir.join("config.toml");
        let content = toml::to_string_pretty(self)?;
        async_std::fs::write(&config_path, content).await?;

        Ok(())
    }

    pub fn get_enabled_sources(&self) -> Vec<String> {
        self.enabled_sources
            .iter()
            .filter_map(|(name, enabled)| if *enabled { Some(name.clone()) } else { None })
            .collect()
    }

    pub fn set_source_enabled(&mut self, source: &str, enabled: bool) {
        self.enabled_sources.insert(source.to_string(), enabled);
    }

    pub fn is_source_enabled(&self, source: &str) -> bool {
        self.enabled_sources.get(source).copied().unwrap_or(true)
    }

    pub fn add_custom_command(&mut self, name: String, command: String) {
        self.custom_commands.push(CustomCommand {
            name,
            command,
            enabled: true,
        });
    }

    pub fn get_enabled_custom_commands(&self) -> Vec<CustomCommand> {
        self.custom_commands
            .iter()
            .filter(|cmd| cmd.enabled)
            .cloned()
            .collect()
    }
}
