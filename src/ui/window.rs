use crate::{updater::UpdateEvent, AppState};
use async_std::channel::Receiver;
use libadwaita::{
    gtk::{self, Box, Button, Label, ListBox}, prelude::*, ApplicationWindow, Clamp, ExpanderRow, HeaderBar, PreferencesGroup,
    SwitchRow,
    ToastOverlay,
    ToolbarView,
};
use std::collections::HashMap;
use tracing::error;

pub struct MainWindow {
    pub window: ApplicationWindow,
    pub state: AppState,
    pub start_button: Button,
    pub stop_button: Button,
    pub sources_list: ListBox,
    pub dry_run_switch: SwitchRow,
    pub source_rows: HashMap<String, (ExpanderRow, Label)>,
}

impl MainWindow {
    pub fn new(app: &libadwaita::Application, state: AppState) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("UpToDate")
            .default_width(800)
            .default_height(600)
            .build();

        let toast_overlay = ToastOverlay::new();
        let toolbar_view = ToolbarView::new();

        let header_bar = HeaderBar::new();
        toolbar_view.add_top_bar(&header_bar);

        let main_box = Box::new(gtk::Orientation::Vertical, 12);

        // Use Clamp for better centering and responsive design
        let clamp = Clamp::builder()
            .maximum_size(1000)
            .tightening_threshold(600)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(12)
            .build();

        // Control panel - using PreferencesGroup for better styling
        let control_group = PreferencesGroup::builder().title("Controls").build();

        // Create a box to center the buttons
        let button_box = Box::new(gtk::Orientation::Horizontal, 6);
        button_box.set_halign(gtk::Align::Center);
        button_box.add_css_class("linked");

        let start_button = Button::with_label("Start Updates");
        start_button.add_css_class("suggested-action");
        start_button.add_css_class("pill");

        let stop_button = Button::with_label("Stop");
        stop_button.add_css_class("destructive-action");
        stop_button.add_css_class("pill");
        stop_button.set_sensitive(false);

        button_box.append(&start_button);
        button_box.append(&stop_button);

        // Wrap buttons in a proper container for the preferences group
        let button_container = Box::new(gtk::Orientation::Vertical, 0);
        button_container.set_margin_top(12);
        button_container.set_margin_bottom(12);
        button_container.append(&button_box);

        // Dry run switch as a SwitchRow in its own group
        let dry_run_switch = SwitchRow::builder()
            .title("Dry Run")
            .subtitle("Preview updates without applying them")
            .build();

        let switch_group = PreferencesGroup::new();
        switch_group.add(&dry_run_switch);

        control_group.add(&button_container);
        main_box.append(&control_group);
        main_box.append(&switch_group);

        // Sources section with expandable progress
        let sources_group = PreferencesGroup::builder()
            .title("Package Managers")
            .description("Select package managers to update and view their progress")
            .build();

        let sources_list = ListBox::new();
        sources_list.set_selection_mode(gtk::SelectionMode::None);
        sources_list.add_css_class("boxed-list");

        sources_group.add(&sources_list);
        main_box.append(&sources_group);

        clamp.set_child(Some(&main_box));
        toolbar_view.set_content(Some(&clamp));
        toast_overlay.set_child(Some(&toolbar_view));
        window.set_content(Some(&toast_overlay));

        let mut window_self = Self {
            window,
            state,
            start_button,
            stop_button,
            sources_list,
            dry_run_switch,
            source_rows: HashMap::new(),
        };

        window_self.setup_actions();
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
                let mut enabled_sources = Vec::new();
                let mut child = sources_list.first_child();
                while let Some(row) = child {
                    let next = row.next_sibling();
                    if let Ok(expander_row) = row.downcast::<ExpanderRow>() {
                        if expander_row.enables_expansion() {
                            let title = expander_row.title();
                            enabled_sources.push(title.to_string());
                        }
                    }
                    child = next;
                }

                if enabled_sources.is_empty() {
                    return;
                }

                start_button.set_sensitive(false);
                stop_button.set_sensitive(true);

                match state.updater.run_updates(&enabled_sources, dry_run).await {
                    Ok(receiver) => {
                        Self::handle_updates(
                            receiver,
                            sources_list,
                            start_button.clone(),
                            stop_button.clone(),
                        )
                            .await;
                    }
                    Err(e) => {
                        error!("Failed to start updates: {e}");
                        start_button.set_sensitive(true);
                        stop_button.set_sensitive(false);
                    }
                }
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
                if let Err(e) = state.updater.stop().await {
                    error!("Failed to stop updates: {e}");
                }
                start_button.set_sensitive(true);
                stop_button.set_sensitive(false);
            });
        });
    }

    fn load_sources(&mut self) {
        let state = self.state.clone();
        let sources_list = self.sources_list.clone();

        glib::spawn_future_local(async move {
            match state.updater.detect_sources().await {
                Ok(sources) => {
                    for source in sources {
                        // Read config to see if this source is enabled
                        let config = state.config.read().await;
                        let is_enabled = config.is_source_enabled(&source);
                        drop(config);

                        // Create an ExpanderRow for each source
                        let expander_row = ExpanderRow::builder()
                            .title(&source)
                            .show_enable_switch(true)
                            .enable_expansion(is_enabled)
                            .build();

                        if let Some(manager) = state.updater.get_manager_info(&source) {
                            expander_row.set_subtitle(&manager.description);
                        }

                        // Create a status label that we can update
                        let status_label = Label::new(Some("Ready"));
                        status_label.set_halign(gtk::Align::Start);
                        status_label.add_css_class("body");
                        status_label.set_margin_start(12);
                        status_label.set_margin_end(12);
                        status_label.set_margin_top(6);
                        status_label.set_margin_bottom(6);

                        // Add the status label as a child row
                        expander_row.add_row(&status_label);

                        sources_list.append(&expander_row);
                    }
                }
                Err(e) => {
                    error!("Failed to detect sources: {e}");
                }
            }
        });
    }

    async fn handle_updates(
        receiver: Receiver<UpdateEvent>,
        sources_list: ListBox,
        start_button: Button,
        stop_button: Button,
    ) {
        while let Ok(event) = receiver.recv().await {
            match event {
                UpdateEvent::Started => {
                    // Update all sources to show they're starting
                }
                UpdateEvent::SourceStarted(name) => {
                    println!("DEBUG: SourceStarted for {name}");
                    Self::update_source_status(
                        sources_list.clone(),
                        name,
                        "Running...".to_string(),
                        true,
                    );
                }
                UpdateEvent::SourceProgress(name, msg) => {
                    println!("DEBUG: SourceProgress for {name}: {msg}");
                    Self::update_source_status(sources_list.clone(), name, msg, true);
                }
                UpdateEvent::SourceCompleted(name, success) => {
                    let status = if success { "✓ Success" } else { "✗ Failed" };
                    println!("DEBUG: SourceCompleted for {name}: {status}");
                    Self::update_source_status(
                        sources_list.clone(),
                        name,
                        status.to_string(),
                        false,
                    );
                }
                UpdateEvent::SourceError(name, msg) => {
                    println!("DEBUG: SourceError for {name}: {msg}");
                    Self::update_source_status(
                        sources_list.clone(),
                        name,
                        format!("Error: {msg}"),
                        false,
                    );
                }
                UpdateEvent::Completed(_success) => {
                    start_button.set_sensitive(true);
                    stop_button.set_sensitive(false);
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
        _is_running: bool,
    ) {
        println!("DEBUG: update_source_status called for {source_name}: {status}");
        glib::spawn_future_local(async move {
            // Find the ExpanderRow for this source
            let mut child = sources_list.first_child();
            while let Some(row) = child {
                let next = row.next_sibling();
                if let Ok(expander_row) = row.downcast::<ExpanderRow>() {
                    if expander_row.title() == source_name {
                        println!("DEBUG: Found ExpanderRow for {source_name}");
                        // Force expand the row to show progress
                        expander_row.set_expanded(true);

                        // The children of ExpanderRow are the rows we added
                        // We need to find our label in the added rows
                        let mut row_child = expander_row.first_child();
                        while let Some(added_row) = row_child {
                            let next_row = added_row.next_sibling();
                            if let Ok(label) = added_row.downcast::<Label>() {
                                println!("DEBUG: Found label, updating to: {status}");
                                label.set_text(&status);
                                break;
                            }
                            row_child = next_row;
                        }
                        break;
                    }
                }
                child = next;
            }
        });
    }

    pub fn present(&self) {
        self.window.present();
    }
}
