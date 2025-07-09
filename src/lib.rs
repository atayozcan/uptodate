pub mod config;
pub mod ui;
pub mod updater;

use async_std::sync::RwLock;
use config::Config;
use libadwaita::{Application, prelude::*};
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
