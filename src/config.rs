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

/// Validates a package source name.
///
/// # Errors
///
/// Returns an error if:
/// - The name is empty or only whitespace
/// - The name is longer than 50 characters
/// - The name contains invalid characters (only alphanumeric, hyphens, and underscores allowed)
///
/// # Examples
///
/// ```
/// use uptodate::config::validate_source_name;
///
/// assert!(validate_source_name("flatpak").is_ok());
/// assert!(validate_source_name("my-custom_manager").is_ok());
/// assert!(validate_source_name("").is_err()); // Empty name
/// assert!(validate_source_name("invalid name with spaces").is_err()); // Spaces aren't allowed
/// ```
pub fn validate_source_name(name: &str) -> Result<()> {
    let name = name.trim();

    if name.is_empty() {
        return Err(anyhow::anyhow!("Source name cannot be empty"));
    }

    if name.len() > 50 {
        return Err(anyhow::anyhow!(
            "Source name too long (max 50 characters): {}",
            name
        ));
    }

    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(anyhow::anyhow!(
            "Invalid characters in source name: '{}'. Only alphanumeric, hyphens, and underscores allowed",
            name
        ));
    }

    Ok(())
}

/// Validates a custom command.
///
/// # Errors
///
/// Returns an error if:
/// - The name is empty or only whitespace
/// - The name is longer than 100 characters
/// - The command is empty or only whitespace
/// - The command is longer than 1000 characters
/// - The command contains dangerous patterns
///
/// # Examples
///
/// ```
/// use uptodate::config::validate_custom_command;
///
/// assert!(validate_custom_command("Update Rust", "rustup update").is_ok());
/// assert!(validate_custom_command("", "rustup update").is_err()); // Empty name
/// assert!(validate_custom_command("Test", "rm -rf /").is_err()); // Dangerous command
/// ```
pub fn validate_custom_command(name: &str, command: &str) -> Result<()> {
    let name = name.trim();
    let command = command.trim();

    if name.is_empty() {
        return Err(anyhow::anyhow!("Custom command name cannot be empty"));
    }

    if name.len() > 100 {
        return Err(anyhow::anyhow!(
            "Custom command name too long (max 100 characters): {}",
            name
        ));
    }

    if command.is_empty() {
        return Err(anyhow::anyhow!("Custom command cannot be empty"));
    }

    if command.len() > 1000 {
        return Err(anyhow::anyhow!(
            "Custom command too long (max 1000 characters)"
        ));
    }

    // Check for dangerous patterns
    let dangerous_patterns = [
        "rm -rf", "sudo rm", "dd if=", "mkfs", "fdisk", "parted", "> /dev/",
    ];
    for pattern in &dangerous_patterns {
        if command.to_lowercase().contains(pattern) {
            return Err(anyhow::anyhow!(
                "Command contains potentially dangerous pattern: '{}'",
                pattern
            ));
        }
    }

    // Check for command injection patterns
    if command.contains("&&")
        || command.contains("||")
        || command.contains(";")
        || command.contains("|")
    {
        return Err(anyhow::anyhow!("Command contains shell injection patterns"));
    }

    Ok(())
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
    /// Loads configuration from the standard config directory.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - The config directory cannot be determined
    /// - The config file exists but contains invalid TOML
    /// - File system permissions prevent reading/writing
    /// - The logs directory cannot be created when `save_logs` is true
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use uptodate::config::Config;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let config = Config::load().await?;
    /// println!("Dry run mode: {}", config.dry_run);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn load() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Cannot determine config directory. Please set $HOME environment variable."
                )
            })?
            .join("uptodate");

        async_std::fs::create_dir_all(&config_dir)
            .await
            .map_err(|e| {
                anyhow::anyhow!("Failed to create config directory {:?}: {}", config_dir, e)
            })?;

        let config_path = config_dir.join("config.toml");
        if config_path.exists() {
            let content = async_std::fs::read_to_string(&config_path)
                .await
                .map_err(|e| {
                    anyhow::anyhow!("Failed to read config file {:?}: {}", config_path, e)
                })?;

            let config: Config = toml::from_str(&content).map_err(|e| {
                anyhow::anyhow!("Invalid TOML in config file {:?}: {}", config_path, e)
            })?;

            if config.save_logs {
                async_std::fs::create_dir_all(&config.logs_dir)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to create logs directory {:?}: {}",
                            config.logs_dir,
                            e
                        )
                    })?;
            }

            tracing::info!("Loaded configuration from {:?}", config_path);
            Ok(config)
        } else {
            let config = Self::default();
            config
                .save()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to save default config: {}", e))?;
            tracing::info!("Created default configuration at {:?}", config_path);
            Ok(config)
        }
    }

    /// Saves the current configuration to disk.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - The config directory cannot be determined
    /// - File system permissions prevent writing
    /// - The configuration cannot be serialized to TOML
    /// - The logs directory cannot be created when `save_logs` is true
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use uptodate::config::Config;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let mut config = Config::default();
    /// config.dry_run = true;
    /// config.save().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn save(&self) -> Result<()> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Cannot determine config directory. Please set $HOME environment variable."
                )
            })?
            .join("uptodate");

        async_std::fs::create_dir_all(&config_dir)
            .await
            .map_err(|e| {
                anyhow::anyhow!("Failed to create config directory {:?}: {}", config_dir, e)
            })?;

        if self.save_logs {
            async_std::fs::create_dir_all(&self.logs_dir)
                .await
                .map_err(|e| {
                    anyhow::anyhow!("Failed to create logs directory {:?}: {}", self.logs_dir, e)
                })?;
        }

        let config_path = config_dir.join("config.toml");
        let content = toml::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize config to TOML: {}", e))?;

        async_std::fs::write(&config_path, content)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write config file {:?}: {}", config_path, e))?;

        tracing::debug!("Saved configuration to {:?}", config_path);
        Ok(())
    }

    /// Returns a list of all enabled package sources.
    ///
    /// # Examples
    ///
    /// ```
    /// use uptodate::config::Config;
    ///
    /// let mut config = Config::default();
    /// config.set_source_enabled("flatpak", true);
    /// config.set_source_enabled("snap", false);
    ///
    /// let enabled = config.get_enabled_sources();
    /// assert!(enabled.contains(&"flatpak".to_string()));
    /// assert!(!enabled.contains(&"snap".to_string()));
    /// ```
    pub fn get_enabled_sources(&self) -> Vec<String> {
        self.enabled_sources
            .iter()
            .filter_map(|(name, enabled)| if *enabled { Some(name.clone()) } else { None })
            .collect()
    }

    /// Sets whether a package source is enabled for updates.
    ///
    /// # Arguments
    ///
    /// * `source` - The name of the package manager (e.g., "paru", "flatpak")
    /// * `enabled` - Whether to enable this source for updates
    ///
    /// # Errors
    ///
    /// Returns an error if the source name is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use uptodate::config::Config;
    ///
    /// let mut config = Config::default();
    /// config.set_source_enabled("flatpak", true).unwrap();
    /// assert!(config.is_source_enabled("flatpak"));
    /// ```
    pub fn set_source_enabled(&mut self, source: &str, enabled: bool) -> Result<()> {
        validate_source_name(source)?;

        if enabled {
            tracing::info!("Enabled package source: {}", source);
        } else {
            tracing::info!("Disabled package source: {}", source);
        }
        self.enabled_sources.insert(source.to_string(), enabled);
        Ok(())
    }

    /// Checks if a package source is enabled for updates.
    ///
    /// Returns `true` by default for unknown sources to allow new package managers.
    ///
    /// # Arguments
    ///
    /// * `source` - The name of the package manager to check
    ///
    /// # Examples
    ///
    /// ```
    /// use uptodate::config::Config;
    ///
    /// let config = Config::default();
    /// // Unknown sources default to enabled
    /// assert!(config.is_source_enabled("unknown-manager"));
    /// ```
    pub fn is_source_enabled(&self, source: &str) -> bool {
        self.enabled_sources.get(source).copied().unwrap_or(true)
    }

    /// Adds a custom update command to the configuration.
    ///
    /// Custom commands are user-defined shell commands that will be executed
    /// during the update process. They are enabled by default when added.
    ///
    /// # Arguments
    ///
    /// * `name` - A descriptive name for the command
    /// * `command` - The shell command to execute
    ///
    /// # Errors
    ///
    /// Returns an error if the name or command is invalid or contains dangerous patterns.
    ///
    /// # Examples
    ///
    /// ```
    /// use uptodate::config::Config;
    ///
    /// let mut config = Config::default();
    /// config.add_custom_command(
    ///     "Update Rust".to_string(),
    ///     "rustup update".to_string()
    /// ).unwrap();
    ///
    /// let commands = config.get_enabled_custom_commands();
    /// assert_eq!(commands.len(), 1);
    /// assert_eq!(commands[0].name, "Update Rust");
    /// ```
    pub fn add_custom_command(&mut self, name: String, command: String) -> Result<()> {
        validate_custom_command(&name, &command)?;

        tracing::info!("Added custom command: {} -> {}", name, command);
        self.custom_commands.push(CustomCommand {
            name,
            command,
            enabled: true,
        });
        Ok(())
    }

    /// Returns a list of all enabled custom commands.
    ///
    /// # Examples
    ///
    /// ```
    /// use uptodate::config::Config;
    ///
    /// let mut config = Config::default();
    /// config.add_custom_command("Test".to_string(), "echo test".to_string());
    ///
    /// let commands = config.get_enabled_custom_commands();
    /// assert_eq!(commands.len(), 1);
    /// assert!(commands[0].enabled);
    /// ```
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
    use std::env;
    use tempfile::tempdir;

    #[test]
    fn test_config_default() {
        let config = Config::default();

        assert!(!config.dry_run);
        assert!(config.save_logs);
        assert!(config.enabled_sources.is_empty());
        assert!(config.custom_commands.is_empty());
        assert!(config.logs_dir.ends_with("uptodate"));
    }

    #[test]
    fn test_validate_source_name_valid() {
        assert!(validate_source_name("flatpak").is_ok());
        assert!(validate_source_name("my-custom_manager").is_ok());
        assert!(validate_source_name("apt").is_ok());
        assert!(validate_source_name("npm").is_ok());
    }

    #[test]
    fn test_validate_source_name_invalid() {
        assert!(validate_source_name("").is_err());
        assert!(validate_source_name("   ").is_err());
        assert!(validate_source_name("invalid name with spaces").is_err());
        assert!(validate_source_name("name@with$special!chars").is_err());
        assert!(validate_source_name(&"a".repeat(51)).is_err()); // Too long
    }

    #[test]
    fn test_validate_custom_command_valid() {
        assert!(validate_custom_command("Update Rust", "rustup update").is_ok());
        assert!(validate_custom_command("Test", "echo hello").is_ok());
        assert!(validate_custom_command("npm", "npm update -g").is_ok());
    }

    #[test]
    fn test_validate_custom_command_invalid() {
        // Empty name or command
        assert!(validate_custom_command("", "rustup update").is_err());
        assert!(validate_custom_command("Test", "").is_err());

        // Too long
        assert!(validate_custom_command(&"a".repeat(101), "test").is_err());
        assert!(validate_custom_command("Test", &"a".repeat(1001)).is_err());

        // Dangerous patterns
        assert!(validate_custom_command("Test", "rm -rf /").is_err());
        assert!(validate_custom_command("Test", "sudo rm something").is_err());
        assert!(validate_custom_command("Test", "dd if=/dev/zero").is_err());

        // Command injection
        assert!(validate_custom_command("Test", "echo hello && rm file").is_err());
        assert!(validate_custom_command("Test", "echo hello || rm file").is_err());
        assert!(validate_custom_command("Test", "echo hello; rm file").is_err());
        assert!(validate_custom_command("Test", "echo hello | rm file").is_err());
    }

    #[test]
    fn test_config_source_management() {
        let mut config = Config::default();

        // Test setting sources
        config.set_source_enabled("flatpak", true).unwrap();
        config.set_source_enabled("snap", false).unwrap();

        assert!(config.is_source_enabled("flatpak"));
        assert!(!config.is_source_enabled("snap"));
        assert!(config.is_source_enabled("unknown")); // Default to true

        // Test getting enabled sources
        let enabled = config.get_enabled_sources();
        assert!(enabled.contains(&"flatpak".to_string()));
        assert!(!enabled.contains(&"snap".to_string()));
    }

    #[test]
    fn test_config_custom_commands() {
        let mut config = Config::default();

        // Test adding valid command
        config
            .add_custom_command("Update Rust".to_string(), "rustup update".to_string())
            .unwrap();

        let commands = config.get_enabled_custom_commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "Update Rust");
        assert_eq!(commands[0].command, "rustup update");
        assert!(commands[0].enabled);

        // Test adding invalid command should fail
        assert!(
            config
                .add_custom_command("Dangerous".to_string(), "rm -rf /".to_string())
                .is_err()
        );
    }

    #[async_std::test]
    async fn test_config_save_load_cycle() {
        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path().to_path_buf();

        // Override config directory for testing
        unsafe {
            env::set_var("XDG_CONFIG_HOME", &temp_path);
        }

        // Create and save config
        let mut original_config = Config::default();
        original_config.dry_run = true;
        original_config.set_source_enabled("flatpak", true).unwrap();
        original_config
            .add_custom_command("Test".to_string(), "echo test".to_string())
            .unwrap();

        original_config.save().await.unwrap();

        // Load config and verify
        let loaded_config = Config::load().await.unwrap();
        assert_eq!(loaded_config.dry_run, original_config.dry_run);
        assert_eq!(
            loaded_config.enabled_sources,
            original_config.enabled_sources
        );
        assert_eq!(
            loaded_config.custom_commands.len(),
            original_config.custom_commands.len()
        );

        // Clean up
        unsafe {
            env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn test_custom_command_struct() {
        let cmd = CustomCommand {
            name: "Test".to_string(),
            command: "echo test".to_string(),
            enabled: false,
        };

        assert_eq!(cmd.name, "Test");
        assert_eq!(cmd.command, "echo test");
        assert!(!cmd.enabled);
    }
}
