use crate::{AppState, updater::UpdateEvent};
use async_std::channel::Receiver;
use gtk::gio;
use gtk::{Align, Box, Button, Image, ListBox, Orientation, ProgressBar};
use libadwaita::{
    ActionRow, ApplicationWindow, Banner, SwitchRow, ToastOverlay, glib, gtk, prelude::*,
};
use std::collections::HashMap;
use tracing::error;

#[derive(Debug, Clone)]
pub enum BannerType {
    Success,
    Warning,
    Error,
    Info,
}

#[derive(Debug)]
pub struct MainWindow {
    pub window: ApplicationWindow,
    pub state: AppState,
    pub start_button: Button,
    pub stop_button: Button,
    pub sources_list: ListBox,
    pub dry_run_switch: SwitchRow,
    pub source_rows: HashMap<String, (ActionRow, Box, ProgressBar)>,
    pub toast_overlay: ToastOverlay,
    pub main_box: Box,
    pub current_banner: Option<Banner>,
}

impl MainWindow {
    pub fn new(app: &libadwaita::Application, state: AppState) -> Self {
        let builder = gtk::Builder::from_string(include_str!("window.ui"));

        // Get widgets from the builder
        let (
            window,
            start_button,
            stop_button,
            sources_list,
            dry_run_switch,
            toast_overlay,
            main_box,
        ) = (
            builder.object::<ApplicationWindow>("MainWindow").unwrap(),
            builder.object::<Button>("start_button").unwrap(),
            builder.object::<Button>("stop_button").unwrap(),
            builder.object::<ListBox>("sources_list").unwrap(),
            builder.object::<SwitchRow>("dry_run_switch").unwrap(),
            builder.object::<ToastOverlay>("toast_overlay").unwrap(),
            builder.object::<Box>("main_box").unwrap(),
        );

        window.set_application(Some(app));

        let mut window_self = Self {
            window,
            state,
            start_button,
            stop_button,
            sources_list,
            dry_run_switch,
            source_rows: HashMap::new(),
            toast_overlay,
            main_box,
            current_banner: None,
        };

        window_self.setup_actions();
        window_self.setup_keyboard_shortcuts();
        window_self.load_sources();
        window_self
    }

    fn setup_actions(&self) {
        let state = self.state.clone();
        let start_button = self.start_button.clone();
        let stop_button = self.stop_button.clone();
        let dry_run_switch = self.dry_run_switch.clone();
        let sources_list = self.sources_list.clone();

        self.start_button.connect_clicked(move |_| {
            let state = state.clone();
            let start_button = start_button.clone();
            let stop_button = stop_button.clone();
            let dry_run = dry_run_switch.is_active();
            let sources_list = sources_list.clone();

            glib::spawn_future_local(async move {
                // Get enabled sources
                let enabled_sources = Self::collect_enabled_sources(&sources_list);

                if enabled_sources.is_empty() {
                    return;
                } else {
                    {}
                };

                start_button.set_sensitive(false);
                stop_button.set_sensitive(true);

                state
                    .updater
                    .run_updates(&enabled_sources, dry_run)
                    .await
                    .map_or_else(
                        |e| {
                            error!("Failed to start updates: {e}");
                            start_button.set_sensitive(true);
                            stop_button.set_sensitive(false);
                        },
                        |receiver| {
                            let sources_list = sources_list.clone();
                            let start_button = start_button.clone();
                            let stop_button = stop_button.clone();
                            glib::spawn_future_local(async move {
                                Self::handle_updates(
                                    receiver,
                                    sources_list,
                                    start_button,
                                    stop_button,
                                )
                                .await;
                            });
                        },
                    );
            });
        });

        let state_stop = self.state.clone();
        let start_button_stop = self.start_button.clone();
        let stop_button_stop = self.stop_button.clone();

        self.stop_button.connect_clicked(move |_| {
            let state = state_stop.clone();
            let start_button = start_button_stop.clone();
            let stop_button = stop_button_stop.clone();

            glib::spawn_future_local(async move {
                state
                    .updater
                    .stop()
                    .await
                    .is_err()
                    .then(|| error!("Failed to stop updates"));

                start_button.set_sensitive(true);
                stop_button.set_sensitive(false);
            });
        });
    }

    fn setup_keyboard_shortcuts(&self) {
        // Create actions for keyboard shortcuts
        self.create_button_action("start-updates", &self.start_button);
        self.create_button_action("stop-updates", &self.stop_button);

        let toggle_dry_run = gio::SimpleAction::new("toggle-dry-run", None);
        toggle_dry_run.connect_activate(glib::clone!(
            #[weak(rename_to = switch)]
            self.dry_run_switch,
            move |_, _| {
                switch.set_active(!switch.is_active());
            }
        ));
        self.window.add_action(&toggle_dry_run);

        // Set up keyboard shortcuts
        if let Some(app) = self.window.application() {
            app.set_accels_for_action("win.start-updates", &["<Primary>Return"]);
            app.set_accels_for_action("win.stop-updates", &["Escape"]);
            app.set_accels_for_action("win.toggle-dry-run", &["<Primary>d"]);
        }
    }

    fn create_button_action(&self, action_name: &str, button: &Button) {
        let action = gio::SimpleAction::new(action_name, None);
        action.connect_activate(glib::clone!(
            #[weak]
            button,
            move |_, _| {
                button.emit_clicked();
            }
        ));
        self.window.add_action(&action);
    }

    fn load_sources(&mut self) {
        let state = self.state.clone();
        let sources_list = self.sources_list.clone();

        glib::spawn_future_local(async move {
            state.updater.detect_sources().await.map_or_else(
                |e| error!("Failed to detect sources: {e}"),
                |sources| {
                    sources.into_iter().for_each(|source| {
                        let sources_list = sources_list.clone();
                        let state = state.clone();
                        glib::spawn_future_local(async move {
                            Self::create_source_row(source, sources_list, state).await;
                        });
                    });
                },
            );
        });
    }

    fn collect_enabled_sources(sources_list: &ListBox) -> Vec<String> {
        let mut enabled_sources = Vec::new();
        let mut child = sources_list.first_child();

        while let Some(row) = child {
            let next = row.next_sibling();

            row.downcast::<gtk::ListBoxRow>()
                .ok()
                .and_then(|list_box_row| list_box_row.child())
                .and_then(|row_container| row_container.downcast::<Box>().ok())
                .and_then(|box_container| box_container.first_child())
                .and_then(|action_row_widget| action_row_widget.downcast::<ActionRow>().ok())
                .and_then(|action_row| {
                    Self::find_switch_recursive(
                        &action_row
                            .last_child()?
                            .downcast::<Box>()
                            .ok()?
                            .upcast::<gtk::Widget>(),
                    )
                    .filter(|switch| switch.is_active())
                    .and_then(|_| action_row.subtitle())
                    .map(|name| enabled_sources.push(name.to_string()))
                });

            child = next;
        }

        enabled_sources
    }

    fn find_switch_recursive(widget: &gtk::Widget) -> Option<gtk::Switch> {
        widget.clone().downcast::<gtk::Switch>().ok().or_else(|| {
            let mut child = widget.first_child();
            while let Some(child_widget) = child {
                if let Some(switch) = Self::find_switch_recursive(&child_widget) {
                    return Some(switch);
                }
                child = child_widget.next_sibling();
            }
            None
        })
    }

    async fn create_source_row(source: String, sources_list: ListBox, state: AppState) {
        let config = state.config.read().await;
        let is_enabled = config.is_source_enabled(&source);
        drop(config);

        let action_row = ActionRow::new();

        // Set title and subtitle using
        state.updater.get_manager_info(&source).map_or_else(
            || action_row.set_title(&source),
            |manager| {
                action_row.set_title(&manager.description);
                action_row.set_subtitle(&manager.name);
            },
        );

        // Create and configure components functionally
        let switch = gtk::Switch::new();
        switch.set_active(is_enabled);
        switch.set_valign(Align::Center);

        let status_box = Box::new(Orientation::Horizontal, 6);
        status_box.set_halign(Align::End);
        status_box.set_valign(Align::Center);

        let status_icon = Image::from_icon_name("emblem-default-symbolic");
        status_icon.add_css_class("status-icon");

        let progress_bar = ProgressBar::new();
        progress_bar.set_visible(false);
        progress_bar.set_margin_top(6);
        progress_bar.set_margin_bottom(6);
        progress_bar.set_margin_start(12);
        progress_bar.set_margin_end(12);
        progress_bar.add_css_class("osd");

        let row_container = Box::new(Orientation::Vertical, 0);

        // Chain operations functionally
        status_box.append(&status_icon);
        status_box.append(&switch);

        action_row.add_suffix(&status_box);
        action_row.set_activatable_widget(Some(&switch));

        row_container.append(&action_row);
        row_container.append(&progress_bar);

        sources_list.append(&row_container);
    }

    async fn handle_updates(
        receiver: Receiver<UpdateEvent>,
        sources_list: ListBox,
        start_button: Button,
        stop_button: Button,
    ) {
        let mut completed_count = 0;
        let mut failed_count = 0;
        while let Ok(event) = receiver.recv().await {
            match event {
                UpdateEvent::Started => {}
                UpdateEvent::SourceStarted(name) => {
                    Self::update_source_status(
                        sources_list.clone(),
                        name,
                        "Running".to_string(),
                        true,
                    );
                }
                UpdateEvent::SourceProgress(name, _msg) => {
                    Self::update_source_status(
                        sources_list.clone(),
                        name,
                        "Running".to_string(),
                        true,
                    );
                }
                UpdateEvent::SourceCompleted(name, success) => {
                    let status = if success { "Success" } else { "Failed" };
                    Self::update_source_status(
                        sources_list.clone(),
                        name,
                        status.to_string(),
                        false,
                    );

                    if success {
                        completed_count += 1;
                    } else {
                        failed_count += 1;
                    }
                }
                UpdateEvent::SourceError(name, _msg) => {
                    Self::update_source_status(
                        sources_list.clone(),
                        name,
                        "Error".to_string(),
                        false,
                    );
                    failed_count += 1;
                }
                UpdateEvent::Completed(_success) => {
                    start_button.set_sensitive(true);
                    stop_button.set_sensitive(false);

                    // Show completion notification
                    Self::show_completion_notification(completed_count, failed_count);

                    // TODO: Show banner - need to pass window reference for this
                    break;
                }
                _ => {}
            }
        }
    }

    fn update_source_status(
        sources_list: ListBox,
        source_name: String,
        status: String,
        is_running: bool,
    ) {
        glib::spawn_future_local(async move {
            if let Some((action_row, progress_bar)) =
                Self::find_source_row(&sources_list, &source_name)
            {
                // Update the progress bar
                if is_running {
                    progress_bar.set_visible(true);
                    progress_bar.pulse();
                    Self::setup_progress_pulse(progress_bar.clone());
                } else {
                    progress_bar.set_visible(false);
                }

                // Update the status icon
                Self::update_status_icon(&action_row, &status, is_running);
            }
        });
    }

    fn find_source_row(
        sources_list: &ListBox,
        source_name: &str,
    ) -> Option<(ActionRow, ProgressBar)> {
        let mut child = sources_list.first_child();
        while let Some(row) = child {
            let next = row.next_sibling();

            let result = row
                .downcast::<gtk::ListBoxRow>()
                .ok()
                .and_then(|list_box_row| list_box_row.child())
                .and_then(|row_container| row_container.downcast::<Box>().ok())
                .and_then(|box_container| box_container.first_child())
                .and_then(|action_row_widget| {
                    let action_row = action_row_widget.clone().downcast::<ActionRow>().ok()?;
                    let progress_bar = action_row_widget
                        .next_sibling()?
                        .downcast::<ProgressBar>()
                        .ok()?;

                    action_row
                        .subtitle()
                        .filter(|subtitle| subtitle.as_str() == source_name)
                        .map(|_| (action_row, progress_bar))
                });

            if let Some(found) = result {
                return Some(found);
            }

            child = next;
        }
        None
    }

    fn setup_progress_pulse(progress_bar: ProgressBar) {
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            if progress_bar.is_visible() {
                {
                    progress_bar.pulse();
                    glib::ControlFlow::Continue
                }
            } else {
                glib::ControlFlow::Break
            }
        });
    }

    fn update_status_icon(action_row: &ActionRow, status: &str, is_running: bool) {
        if let Some(status_icon) = action_row
            .last_child()
            .and_then(|suffix_box| suffix_box.downcast::<Box>().ok())
            .and_then(|status_box| status_box.first_child())
            .and_then(|status_icon_widget| status_icon_widget.downcast::<Image>().ok())
        {
            // Clear existing CSS classes
            ["success", "error", "running", "warning"]
                .iter()
                .for_each(|class| status_icon.remove_css_class(class));

            // Set icon and class based on status
            let (icon_name, css_class) = match (is_running, status) {
                (true, _) => ("process-working-symbolic", "running"),
                (false, s) if s.contains("Success") || s.contains("✓") => {
                    ("emblem-ok-symbolic", "success")
                }
                (false, s) if s.contains("Failed") || s.contains("Error") || s.contains("✗") => {
                    ("dialog-error-symbolic", "error")
                }
                _ => ("emblem-default-symbolic", ""),
            };

            status_icon.set_icon_name(Some(icon_name));
            (!css_class.is_empty()).then(|| status_icon.add_css_class(css_class));
        }
    }

    fn show_completion_notification(completed: i32, failed: i32) {
        let notification = gio::Notification::new("Updates Complete");

        let message = match (completed, failed) {
            (0, 0) => "No updates were performed".to_string(),
            (c, 0) => format!("Successfully updated {c} package manager(s)"),
            (0, f) => format!("Failed to update {f} package manager(s)"),
            (c, f) => format!("Updated {c} package manager(s), {f} failed"),
        };

        notification.set_body(Some(&message));
        notification.set_icon(&gio::ThemedIcon::new("system-software-update"));

        if let Some(app) = gio::Application::default() {
            app.send_notification(Some("update-complete"), &notification);
        }
    }

    /// Shows a banner with the specified message and type.
    ///
    /// If a banner is already visible, it will be replaced with the new one.
    ///
    /// # Arguments
    ///
    /// * `message` - The message to display in the banner
    /// * `banner_type` - The type of banner (Success, Warning, Error, Info)
    ///
    /// # Examples
    ///
    /// ```ignore
    /// window.show_banner("Updates completed successfully!", BannerType::Success);
    /// ```
    pub fn show_banner(&mut self, message: &str, banner_type: BannerType) {
        // Remove the existing banner if present
        if let Some(ref current_banner) = self.current_banner {
            self.main_box.remove(current_banner);
        }

        let banner = Banner::builder().title(message).revealed(true).build();

        match banner_type {
            BannerType::Success => banner.add_css_class("success"),
            BannerType::Warning => banner.add_css_class("warning"),
            BannerType::Error => banner.add_css_class("error"),
            BannerType::Info => banner.add_css_class("info"),
        }

        // Add a banner at the top of the main box
        self.main_box.prepend(&banner);
        self.current_banner = Some(banner);

        tracing::debug!(
            "Showed {} banner: {}",
            match banner_type {
                BannerType::Success => "success",
                BannerType::Warning => "warning",
                BannerType::Error => "error",
                BannerType::Info => "info",
            },
            message
        );
    }

    /// Hides the current banner if one is visible.
    pub fn hide_banner(&mut self) {
        if let Some(ref current_banner) = self.current_banner {
            current_banner.set_revealed(false);

            // Remove banner after animation completes
            let banner_clone = current_banner.clone();
            let main_box_clone = self.main_box.clone();

            glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                main_box_clone.remove(&banner_clone);
            });

            self.current_banner = None;
            tracing::debug!("Hidden banner");
        }
    }

    /// Creates a modern progress bar using libadwaita styling.
    fn create_modern_progress(&self) -> ProgressBar {
        let progress = ProgressBar::builder().show_text(true).build();
        progress.add_css_class("osd");
        progress
    }

    pub fn present(&self) {
        self.window.present();
    }
}
