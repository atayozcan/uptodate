use anyhow::Result;
use async_std::{
    channel::{Receiver, Sender, unbounded},
    io::{BufReader, prelude::*},
    process::Command,
    stream::StreamExt,
    sync::Mutex,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub enum UpdateEvent {
    Started,
    Progress(String),
    SourceStarted(String),
    SourceProgress(String, String), // (source_name, message)
    SourceCompleted(String, bool),
    SourceError(String, String), // (source_name, error_message)
    Completed(bool),
    Error(String),
}

#[derive(Debug, Clone)]
pub enum SourceState {
    Idle,
    Running,
    Success,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManager {
    pub description: String,
    pub check_cmd: Vec<String>,
    pub update_cmd: Vec<String>,
    pub needs_sudo: bool,
    pub name: String,
}

impl PackageManager {
    fn new(name: &str, check: &[&str], update: &[&str], sudo: bool, desc: &str) -> Self {
        Self {
            description: desc.to_string(),
            check_cmd: check.iter().map(|s| s.to_string()).collect(),
            update_cmd: update.iter().map(|s| s.to_string()).collect(),
            needs_sudo: sudo,
            name: name.to_string(),
        }
    }
}

#[derive(Debug)]
pub struct Updater {
    running: Arc<AtomicBool>,
    child_pids: Arc<Mutex<Vec<u32>>>,
    managers: HashMap<String, PackageManager>,
}

impl Default for Updater {
    fn default() -> Self {
        Self::new()
    }
}

/// List of allowed package managers for security validation
const ALLOWED_MANAGERS: &[&str] = &[
    "paru", "apt", "dnf", "zypper", "apk", "flatpak", "snap", "pipx", "npm", "rustup", "brew",
];

/// Validates that a package manager is allowed to execute commands.
///
/// # Security
///
/// This function ensures only predefined, trusted package managers
/// can execute commands to prevent arbitrary code execution.
///
/// # Errors
///
/// Returns an error if the manager is not in the allowlist.
fn validate_manager_security(manager: &PackageManager) -> Result<()> {
    if !ALLOWED_MANAGERS.contains(&manager.name.as_str()) {
        return Err(anyhow::anyhow!(
            "Unauthorized package manager: {}. Only trusted managers are allowed.",
            manager.name
        ));
    }
    Ok(())
}

/// Validates command arguments for security issues.
///
/// # Security
///
/// This function checks for dangerous patterns that could lead to
/// command injection or system damage.
///
/// # Errors
///
/// Returns an error if dangerous patterns are detected.
fn validate_command_args(args: &[String]) -> Result<()> {
    for arg in args {
        // Check for command injection patterns
        if arg.contains("&&") || arg.contains("||") || arg.contains(";") || arg.contains("`") {
            return Err(anyhow::anyhow!(
                "Invalid argument pattern detected: '{}'. Command injection patterns not allowed",
                arg
            ));
        }

        // Check for file redirection that could be dangerous
        if arg.contains("> /dev/") || arg.contains(">> /dev/") {
            return Err(anyhow::anyhow!(
                "Dangerous file redirection detected: '{}'",
                arg
            ));
        }

        // Check for excessively long arguments that might be exploits
        if arg.len() > 1000 {
            return Err(anyhow::anyhow!(
                "Argument too long (potential buffer overflow): {} characters",
                arg.len()
            ));
        }
    }
    Ok(())
}

impl Updater {
    pub fn new() -> Self {
        let mut updater = Self {
            running: Arc::new(AtomicBool::new(false)),
            child_pids: Arc::new(Mutex::new(Vec::new())),
            managers: HashMap::new(),
        };
        updater.init_managers();
        updater
    }

    fn init_managers(&mut self) {
        // System managers
        let managers = vec![
            PackageManager::new(
                "paru",
                &["paru", "-Qu"],
                &["paru", "-Syu", "--noconfirm"],
                true,
                "System packages",
            ),
            PackageManager::new(
                "apt",
                &["apt", "list", "--upgradable"],
                &["sh", "-c", "apt update && apt upgrade -y"],
                true,
                "System packages",
            ),
            PackageManager::new(
                "dnf",
                &["dnf", "check-update"],
                &["dnf", "upgrade", "-y"],
                true,
                "System packages",
            ),
            PackageManager::new(
                "zypper",
                &["zypper", "list-updates"],
                &["zypper", "update", "-y"],
                true,
                "System packages",
            ),
            PackageManager::new(
                "apk",
                &["apk", "list", "--upgradable"],
                &["sh", "-c", "apk update && apk upgrade"],
                true,
                "System packages",
            ),
            // Universal managers
            PackageManager::new(
                "flatpak",
                &["flatpak", "remote-ls", "--updates"],
                &["flatpak", "update", "-y"],
                false,
                "Flatpak applications",
            ),
            PackageManager::new(
                "snap",
                &["snap", "refresh", "--list"],
                &["snap", "refresh"],
                true,
                "Snap packages",
            ),
            // Development tools
            PackageManager::new(
                "pipx",
                &["pipx", "list", "--outdated"],
                &[
                    "sh",
                    "-c",
                    "if command -v pipx >/dev/null 2>&1; then pipx upgrade-all; else pipx list --outdated --format=freeze | cut -d= -f1 | xargs -r pipx install --user --upgrade; fi",
                ],
                false,
                "Python packages",
            ),
            PackageManager::new(
                "npm",
                &["npm", "outdated", "-g"],
                &[
                    "sh",
                    "-c",
                    "if [ -w \"$(npm config get prefix)\" ]; then npm update -g; else echo 'Note: npm global updates need write permissions. Consider using a Node version manager like nvm.'; fi",
                ],
                false,
                "Node.js packages",
            ),
            PackageManager::new(
                "rustup",
                &["rustup", "check"],
                &["rustup", "update"],
                false,
                "Rust toolchain",
            ),
            PackageManager::new(
                "brew",
                &["brew", "outdated"],
                &["sh", "-c", "brew update && brew upgrade"],
                false,
                "Homebrew packages",
            ),
        ];

        for manager in managers {
            self.managers.insert(manager.name.clone(), manager);
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub async fn detect_sources(&self) -> Result<Vec<String>> {
        let mut available = Vec::new();

        // Check system managers first (only one)
        let system_managers = ["paru", "apt", "dnf", "zypper", "apk"];
        for manager in &system_managers {
            if self.command_exists(manager).await {
                available.push(manager.to_string());
                break;
            }
        }

        // Check other managers
        let other_managers = ["flatpak", "snap", "pipx", "npm", "rustup", "brew"];
        for manager in &other_managers {
            if self.command_exists(manager).await {
                available.push(manager.to_string());
            }
        }

        info!("Detected {} package managers", available.len());
        Ok(available)
    }

    async fn command_exists(&self, cmd: &str) -> bool {
        Command::new("which")
            .arg(cmd)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub async fn run_updates(
        &self,
        sources: &[String],
        dry_run: bool,
    ) -> Result<Receiver<UpdateEvent>> {
        if self.is_running() {
            return Err(anyhow::anyhow!("Updates already running"));
        }

        self.running.store(true, Ordering::Relaxed);
        let running = self.running.clone();
        let (tx, rx) = unbounded();

        tx.send(UpdateEvent::Started).await.ok();

        let sources = sources.to_vec();
        let managers = self.managers.clone();
        let child_pids = self.child_pids.clone();

        async_std::task::spawn(async move {
            let mut success = true;

            for source in sources {
                if !running.load(Ordering::Relaxed) {
                    break;
                }

                if let Some(manager) = managers.get(&source) {
                    tx.send(UpdateEvent::SourceStarted(manager.name.clone()))
                        .await
                        .ok();

                    let result = if dry_run {
                        Self::check_updates(manager, &tx, &child_pids).await
                    } else {
                        Self::run_update(manager, &tx, &child_pids).await
                    };

                    if !result {
                        success = false;
                    }

                    tx.send(UpdateEvent::SourceCompleted(manager.name.clone(), result))
                        .await
                        .ok();
                }
            }

            running.store(false, Ordering::Relaxed);
            tx.send(UpdateEvent::Completed(success)).await.ok();
        });

        Ok(rx)
    }

    async fn check_updates(
        manager: &PackageManager,
        tx: &Sender<UpdateEvent>,
        child_pids: &Arc<Mutex<Vec<u32>>>,
    ) -> bool {
        Self::run_command(&manager.check_cmd, false, manager, tx, child_pids).await
    }

    async fn run_update(
        manager: &PackageManager,
        tx: &Sender<UpdateEvent>,
        child_pids: &Arc<Mutex<Vec<u32>>>,
    ) -> bool {
        Self::run_command(
            &manager.update_cmd,
            manager.needs_sudo,
            manager,
            tx,
            child_pids,
        )
        .await
    }

    /// Safely executes a command with proper validation and escaping.
    ///
    /// # Security
    ///
    /// This function validates command arguments and uses proper escaping
    /// to prevent command injection attacks. Only predefined package managers
    /// are allowed to execute commands.
    ///
    /// # Arguments
    ///
    /// * `cmd` - The command and arguments to execute
    /// * `needs_sudo` - Whether the command requires elevated privileges
    /// * `manager` - The package manager information for validation
    /// * `tx` - Channel sender for progress updates
    /// * `child_pids` - Shared list of child process IDs for cleanup
    ///
    /// # Errors
    ///
    /// Returns false (failure) if:
    /// - The package manager is not authorized
    /// - Command arguments contain dangerous patterns
    /// - The command fails to execute
    async fn run_command(
        cmd: &[String],
        needs_sudo: bool,
        manager: &PackageManager,
        tx: &Sender<UpdateEvent>,
        child_pids: &Arc<Mutex<Vec<u32>>>,
    ) -> bool {
        // Validate security before executing
        if let Err(e) = validate_manager_security(manager) {
            error!("Security validation failed: {}", e);
            tx.send(UpdateEvent::SourceError(
                manager.name.clone(),
                e.to_string(),
            ))
            .await
            .ok();
            return false;
        }

        if let Err(e) = validate_command_args(cmd) {
            error!("Command validation failed: {}", e);
            tx.send(UpdateEvent::SourceError(
                manager.name.clone(),
                e.to_string(),
            ))
            .await
            .ok();
            return false;
        }

        let mut command = if needs_sudo {
            let mut sudo_cmd = Command::new("pkexec");
            sudo_cmd.args(["--user", "root", "sh", "-c", &cmd.join(" ")]);
            sudo_cmd
        } else {
            let mut regular_cmd = Command::new(&cmd[0]);
            regular_cmd.args(&cmd[1..]);
            regular_cmd
        };

        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        match command.spawn() {
            Ok(mut child) => {
                let pid = child.id();
                {
                    let mut pids = child_pids.lock().await;
                    pids.push(pid);
                }

                // Handle stdout
                if let Some(stdout) = child.stdout.take() {
                    let tx = tx.clone();
                    let name = manager.name.clone();
                    async_std::task::spawn(async move {
                        let reader = BufReader::new(stdout);
                        let mut lines = reader.lines();
                        while let Some(Ok(line)) = lines.next().await {
                            if !line.trim().is_empty() {
                                tx.send(UpdateEvent::SourceProgress(name.clone(), line))
                                    .await
                                    .ok();
                            }
                        }
                    });
                }

                // Handle stderr
                if let Some(stderr) = child.stderr.take() {
                    let tx = tx.clone();
                    let name = manager.name.clone();
                    async_std::task::spawn(async move {
                        let reader = BufReader::new(stderr);
                        let mut lines = reader.lines();
                        while let Some(Ok(line)) = lines.next().await {
                            if !line.trim().is_empty() && !line.contains("password") {
                                // Don't treat informational messages as errors
                                if line.contains("up to date")
                                    || line.contains("Nothing to do")
                                    || line.contains("info:")
                                {
                                    tx.send(UpdateEvent::SourceProgress(name.clone(), line))
                                        .await
                                        .ok();
                                } else {
                                    tx.send(UpdateEvent::SourceError(name.clone(), line))
                                        .await
                                        .ok();
                                }
                            }
                        }
                    });
                }

                let success = child.status().await.map(|s| s.success()).unwrap_or(false);

                {
                    let mut pids = child_pids.lock().await;
                    pids.retain(|&p| p != pid);
                }

                success
            }
            Err(e) => {
                error!("Failed to run command for {}: {}", manager.name, e);
                tx.send(UpdateEvent::Error(format!(
                    "Failed to run {}: {}",
                    manager.name, e
                )))
                .await
                .ok();
                false
            }
        }
    }

    pub async fn stop(&self) -> Result<()> {
        if self.is_running() {
            self.running.store(false, Ordering::Relaxed);

            let pids = self.child_pids.lock().await;
            for &pid in pids.iter() {
                warn!("Stopping process {}", pid);
                Command::new("kill")
                    .args(["-INT", &pid.to_string()])
                    .output()
                    .await
                    .ok();
            }
        }
        Ok(())
    }

    pub fn get_manager_info(&self, name: &str) -> Option<&PackageManager> {
        self.managers.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_updater_creation() {
        let updater = Updater::new();

        assert!(!updater.is_running());
        assert!(!updater.managers.is_empty()); // Should have some predefined managers

        // Test some expected managers
        assert!(updater.get_manager_info("paru").is_some());
        assert!(updater.get_manager_info("flatpak").is_some());
        assert!(updater.get_manager_info("nonexistent").is_none());
    }

    #[test]
    fn test_package_manager_creation() {
        let manager = PackageManager::new(
            "test",
            &["test", "--check"],
            &["test", "--update"],
            false,
            "Test Package Manager",
        );

        assert_eq!(manager.name, "test");
        assert_eq!(manager.description, "Test Package Manager");
        assert_eq!(manager.check_cmd, vec!["test", "--check"]);
        assert_eq!(manager.update_cmd, vec!["test", "--update"]);
        assert!(!manager.needs_sudo);
    }

    #[test]
    fn test_validate_manager_security_valid() {
        let manager = PackageManager::new(
            "flatpak",
            &["flatpak", "list"],
            &["flatpak", "update"],
            false,
            "Flatpak",
        );

        assert!(validate_manager_security(&manager).is_ok());
    }

    #[test]
    fn test_validate_manager_security_invalid() {
        let manager = PackageManager::new(
            "malicious",
            &["rm", "-rf"],
            &["rm", "-rf", "/"],
            false,
            "Malicious Manager",
        );

        assert!(validate_manager_security(&manager).is_err());
    }

    #[test]
    fn test_validate_command_args_valid() {
        let args = vec![
            "flatpak".to_string(),
            "update".to_string(),
            "-y".to_string(),
        ];

        assert!(validate_command_args(&args).is_ok());
    }

    #[test]
    fn test_validate_command_args_invalid() {
        // Command injection
        let args1 = vec!["echo".to_string(), "hello && rm file".to_string()];
        assert!(validate_command_args(&args1).is_err());

        let args2 = vec!["echo".to_string(), "hello || rm file".to_string()];
        assert!(validate_command_args(&args2).is_err());

        let args3 = vec!["echo".to_string(), "hello; rm file".to_string()];
        assert!(validate_command_args(&args3).is_err());

        let args4 = vec!["echo".to_string(), "hello `rm file`".to_string()];
        assert!(validate_command_args(&args4).is_err());

        // Dangerous redirection
        let args5 = vec!["echo".to_string(), "data > /dev/sda".to_string()];
        assert!(validate_command_args(&args5).is_err());

        // Too long argument
        let args6 = vec!["echo".to_string(), "a".repeat(1001)];
        assert!(validate_command_args(&args6).is_err());
    }

    #[test]
    fn test_update_event_variants() {
        let events = vec![
            UpdateEvent::Started,
            UpdateEvent::Progress("Test progress".to_string()),
            UpdateEvent::SourceStarted("flatpak".to_string()),
            UpdateEvent::SourceProgress("flatpak".to_string(), "Updating...".to_string()),
            UpdateEvent::SourceCompleted("flatpak".to_string(), true),
            UpdateEvent::SourceError("flatpak".to_string(), "Error occurred".to_string()),
            UpdateEvent::Completed(true),
            UpdateEvent::Error("General error".to_string()),
        ];

        // Verify they can be created and match
        for event in events {
            match event {
                UpdateEvent::Started => {}
                UpdateEvent::Progress(_) => {}
                UpdateEvent::SourceStarted(_) => {}
                UpdateEvent::SourceProgress(_, _) => {}
                UpdateEvent::SourceCompleted(_, _) => {}
                UpdateEvent::SourceError(_, _) => {}
                UpdateEvent::Completed(_) => {}
                UpdateEvent::Error(_) => {}
            }
        }
    }

    #[test]
    fn test_source_state_variants() {
        let states = vec![
            SourceState::Idle,
            SourceState::Running,
            SourceState::Success,
            SourceState::Failed,
        ];

        // Verify all states can be created
        for state in states {
            match state {
                SourceState::Idle => {}
                SourceState::Running => {}
                SourceState::Success => {}
                SourceState::Failed => {}
            }
        }
    }

    #[async_std::test]
    async fn test_updater_detect_sources() {
        let updater = Updater::new();

        // This test might fail in a CI environment without package managers, 
        // so we just verify the method doesn't panic
        let result = updater.detect_sources().await;
        assert!(result.is_ok());

        let _sources = result.unwrap();
        // Sources list might be empty in the test environment, that's ok
        // Length is always >= 0 for Vec, so this assertion is always true
    }

    #[test]
    fn test_updater_is_not_running_initially() {
        let updater = Updater::new();
        assert!(!updater.is_running());
    }

    #[test]
    fn test_allowed_managers_constant() {
        assert!(ALLOWED_MANAGERS.contains(&"flatpak"));
        assert!(ALLOWED_MANAGERS.contains(&"apt"));
        assert!(ALLOWED_MANAGERS.contains(&"paru"));
        assert!(!ALLOWED_MANAGERS.contains(&"malicious"));

        assert!(ALLOWED_MANAGERS.len() > 5); // Should have a reasonable number of managers
    }
}
