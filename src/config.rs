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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.dry_run, false);
        assert_eq!(config.enabled_sources.len(), 0);
        assert_eq!(config.custom_commands.len(), 0);
        assert_eq!(config.save_logs, true);
        assert!(config.logs_dir.to_string_lossy().contains("uptodate"));
    }

    #[test]
    fn test_custom_command_creation() {
        let cmd = CustomCommand {
            name: "test".to_string(),
            command: "echo test".to_string(),
            enabled: true,
        };
        assert_eq!(cmd.name, "test");
        assert_eq!(cmd.command, "echo test");
        assert_eq!(cmd.enabled, true);
    }

    #[test]
    fn test_set_source_enabled() {
        let mut config = Config::default();
        config.set_source_enabled("pacman", true);
        assert!(config.is_source_enabled("pacman"));

        config.set_source_enabled("pacman", false);
        assert!(!config.is_source_enabled("pacman"));
    }

    #[test]
    fn test_is_source_enabled_default() {
        let config = Config::default();
        assert!(config.is_source_enabled("unknown_source"));
    }

    #[test]
    fn test_get_enabled_sources() {
        let mut config = Config::default();
        config.set_source_enabled("pacman", true);
        config.set_source_enabled("flatpak", true);
        config.set_source_enabled("snap", false);

        let enabled = config.get_enabled_sources();
        assert_eq!(enabled.len(), 2);
        assert!(enabled.contains(&"pacman".to_string()));
        assert!(enabled.contains(&"flatpak".to_string()));
        assert!(!enabled.contains(&"snap".to_string()));
    }

    #[test]
    fn test_add_custom_command() {
        let mut config = Config::default();
        config.add_custom_command("test".to_string(), "echo test".to_string());

        assert_eq!(config.custom_commands.len(), 1);
        assert_eq!(config.custom_commands[0].name, "test");
        assert_eq!(config.custom_commands[0].command, "echo test");
        assert_eq!(config.custom_commands[0].enabled, true);
    }

    #[test]
    fn test_get_enabled_custom_commands() {
        let mut config = Config::default();
        config.custom_commands.push(CustomCommand {
            name: "enabled".to_string(),
            command: "echo enabled".to_string(),
            enabled: true,
        });
        config.custom_commands.push(CustomCommand {
            name: "disabled".to_string(),
            command: "echo disabled".to_string(),
            enabled: false,
        });

        let enabled = config.get_enabled_custom_commands();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].name, "enabled");
    }

    #[async_std::test]
    async fn test_config_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let _config_dir = temp_dir.path().join("uptodate");

        // Mock the config directory
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        }

        let mut config = Config::default();
        config.dry_run = true;
        config.set_source_enabled("pacman", true);
        config.add_custom_command("test".to_string(), "echo test".to_string());

        // Save config
        config.save().await.unwrap();

        // Load config
        let loaded_config = Config::load().await.unwrap();

        assert_eq!(loaded_config.dry_run, true);
        assert!(loaded_config.is_source_enabled("pacman"));
        assert_eq!(loaded_config.custom_commands.len(), 1);
        assert_eq!(loaded_config.custom_commands[0].name, "test");

        // Cleanup
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn test_config_serialization() {
        let mut config = Config::default();
        config.dry_run = true;
        config.set_source_enabled("pacman", true);
        config.add_custom_command("test".to_string(), "echo test".to_string());

        let serialized = toml::to_string(&config).unwrap();
        assert!(serialized.contains("dry_run = true"));
        assert!(serialized.contains("pacman"));
        assert!(serialized.contains("test"));

        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.dry_run, true);
        assert!(deserialized.is_source_enabled("pacman"));
        assert_eq!(deserialized.custom_commands.len(), 1);
    }
}
