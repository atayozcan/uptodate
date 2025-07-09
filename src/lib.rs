pub mod config;
pub mod ui;
pub mod updater;

use async_std::sync::RwLock;
use config::Config;
use libadwaita::{prelude::*, Application};
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

pub fn setup_actions(app: &Application) {
    use libadwaita::gio::ActionEntry;

    let quit_action = ActionEntry::builder("quit")
        .activate(|app: &Application, _, _| app.quit())
        .build();

    app.add_action_entries([quit_action]);
    app.set_accels_for_action("app.quit", &["<Control>q"]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_std::sync::RwLock;
    use std::sync::Arc;

    #[test]
    fn test_app_state_creation() {
        let config = Arc::new(RwLock::new(Config::default()));
        let updater = Arc::new(Updater::new());

        let state = AppState {
            config: config.clone(),
            updater: updater.clone(),
        };

        assert!(Arc::ptr_eq(&state.config, &config));
        assert!(Arc::ptr_eq(&state.updater, &updater));
    }

    #[test]
    fn test_app_state_clone() {
        let config = Arc::new(RwLock::new(Config::default()));
        let updater = Arc::new(Updater::new());

        let state = AppState {
            config: config.clone(),
            updater: updater.clone(),
        };

        let cloned_state = state.clone();

        assert!(Arc::ptr_eq(&state.config, &cloned_state.config));
        assert!(Arc::ptr_eq(&state.updater, &cloned_state.updater));
    }

    #[test]
    fn test_app_state_debug() {
        let config = Arc::new(RwLock::new(Config::default()));
        let updater = Arc::new(Updater::new());

        let state = AppState { config, updater };

        let debug_string = format!("{:?}", state);
        assert!(debug_string.contains("AppState"));
        assert!(debug_string.contains("config"));
        assert!(debug_string.contains("updater"));
    }

    #[test]
    fn test_app_constants() {
        assert_eq!(APP_ID, "org.gnome.UpToDate");
        assert!(APP_ID.starts_with("org.gnome."));
        assert!(APP_ID.contains("UpToDate"));
    }

    #[async_std::test]
    async fn test_app_state_async_access() {
        let config = Arc::new(RwLock::new(Config::default()));
        let updater = Arc::new(Updater::new());

        let state = AppState {
            config: config.clone(),
            updater: updater.clone(),
        };

        // Test that we can access the config through the state
        {
            let config_guard = state.config.read().await;
            assert_eq!(config_guard.dry_run, false);
        }

        // Test that we can modify the config through the state
        {
            let mut config_guard = state.config.write().await;
            config_guard.dry_run = true;
        }

        // Verify the change
        {
            let config_guard = state.config.read().await;
            assert_eq!(config_guard.dry_run, true);
        }

        // Test updater access
        assert!(!state.updater.is_running());
    }

    #[async_std::test]
    async fn test_app_state_new() {
        let state = AppState::new().await;

        // Test that state is properly initialized
        {
            let config = state.config.read().await;
            assert_eq!(config.dry_run, false);
            assert_eq!(config.save_logs, true);
        }

        assert!(!state.updater.is_running());
    }
}
