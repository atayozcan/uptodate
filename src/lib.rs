pub mod config;
pub mod ui;
pub mod updater;

use async_std::sync::RwLock;
use config::Config;
use libadwaita::Application;
use std::sync::Arc;
use updater::Updater;

pub const APP_ID: &str = "org.gnome.UpToDate";

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub updater: Arc<Updater>,
}

impl AppState {
    pub async fn new() -> Self {
        let config = Arc::new(RwLock::new(Config::load().await.unwrap_or_default()));
        let updater = Arc::new(Updater::new());

        AppState { config, updater }
    }
}

pub fn setup_actions(_app: &Application) {
    // Legacy function - actions are now handled in main.rs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[async_std::test]
    async fn test_app_state_creation() {
        let state = AppState::new().await;

        // Verify state structure
        assert!(!state.config.read().await.dry_run); // Default should be false
        assert!(state.config.read().await.save_logs); // Default should be true

        // Verify updater is initialized
        assert!(!state.updater.is_running());
    }

    #[async_std::test]
    async fn test_app_state_clone() {
        let state1 = AppState::new().await;
        let state2 = state1.clone();

        // Both should point to the same config
        state1.config.write().await.dry_run = true;
        assert!(state2.config.read().await.dry_run);
    }

    #[test]
    fn test_app_id_constant() {
        assert_eq!(APP_ID, "org.gnome.UpToDate");
        assert!(APP_ID.starts_with("org.gnome."));
    }
}
