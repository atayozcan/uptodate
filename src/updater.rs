use anyhow::Result;
use async_std::{
    channel::{unbounded, Receiver, Sender},
    io::{prelude::*, BufReader},
    process::Command,
    stream::StreamExt,
    sync::Mutex,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
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
    pub name: String,
    pub check_cmd: Vec<String>,
    pub update_cmd: Vec<String>,
    pub needs_sudo: bool,
    pub description: String,
}

impl PackageManager {
    fn new(name: &str, check: &[&str], update: &[&str], sudo: bool, desc: &str) -> Self {
        Self {
            name: name.to_string(),
            check_cmd: check.iter().map(|s| s.to_string()).collect(),
            update_cmd: update.iter().map(|s| s.to_string()).collect(),
            needs_sudo: sudo,
            description: desc.to_string(),
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
                "Arch Linux packages",
            ),
            PackageManager::new(
                "apt",
                &["apt", "list", "--upgradable"],
                &["sh", "-c", "apt update && apt upgrade -y"],
                true,
                "Debian/Ubuntu packages",
            ),
            PackageManager::new(
                "dnf",
                &["dnf", "check-update"],
                &["dnf", "upgrade", "-y"],
                true,
                "Fedora packages",
            ),
            PackageManager::new(
                "zypper",
                &["zypper", "list-updates"],
                &["zypper", "update", "-y"],
                true,
                "openSUSE packages",
            ),
            PackageManager::new(
                "apk",
                &["apk", "list", "--upgradable"],
                &["sh", "-c", "apk update && apk upgrade"],
                true,
                "Alpine packages",
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
        Self::run_command(&manager.check_cmd, false, &manager.name, tx, child_pids).await
    }

    async fn run_update(
        manager: &PackageManager,
        tx: &Sender<UpdateEvent>,
        child_pids: &Arc<Mutex<Vec<u32>>>,
    ) -> bool {
        Self::run_command(
            &manager.update_cmd,
            manager.needs_sudo,
            &manager.name,
            tx,
            child_pids,
        )
            .await
    }

    async fn run_command(
        cmd: &[String],
        needs_sudo: bool,
        name: &str,
        tx: &Sender<UpdateEvent>,
        child_pids: &Arc<Mutex<Vec<u32>>>,
    ) -> bool {
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
                    let name = name.to_string();
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
                    let name = name.to_string();
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
                error!("Failed to run command for {name}: {e}");
                tx.send(UpdateEvent::Error(format!("Failed to run {name}: {e}")))
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
