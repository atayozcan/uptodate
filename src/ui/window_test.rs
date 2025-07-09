use crate::{config::Config, updater::Updater, AppState};
use async_std::sync::RwLock;
use libadwaita::{prelude::*, Application};
use std::sync::Arc;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_app() -> Application {
        Application::builder()
            .application_id("org.gnome.UpToDate.Test")
            .build()
    }

    fn create_test_state() -> AppState {
        let config = Arc::new(RwLock::new(Config::default()));
        let updater = Arc::new(Updater::new());
        AppState { config, updater }
    }

    #[test]
    fn test_create_test_app() {
        let app = create_test_app();
        assert!(app.application_id().is_some());
        assert!(app.application_id().unwrap().contains("Test"));
    }

    #[test]
    fn test_create_test_state() {
        let state = create_test_state();
        assert!(!state.updater.is_running());
    }

    #[async_std::test]
    async fn test_state_integration() {
        let state = create_test_state();

        // Modify state before creating window
        {
            let mut config = state.config.write().await;
            config.dry_run = true;
            config.set_source_enabled("test", true);
        }

        // Verify state is accessible from window
        {
            let config = state.config.read().await;
            assert_eq!(config.dry_run, true);
            assert!(config.is_source_enabled("test"));
        }

        assert!(!state.updater.is_running());
    }
}
