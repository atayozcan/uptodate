use libadwaita::{AboutDialog, Application, prelude::*};
use libadwaita::{gio, glib, gtk};
use uptodate::ui::MainWindow;
use uptodate::{APP_ID, AppState, setup_actions};

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt::init();
    libadwaita::init().unwrap();

    // Load CSS styles
    let provider = gtk::CssProvider::new();
    provider.load_from_data(include_str!("ui/style.css"));

    // Add the provider to the default screen
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("Could not get default display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let app = Application::builder().application_id(APP_ID).build();

    let state = async_std::task::block_on(async { AppState::new().await });

    setup_actions(&app);
    setup_app_actions(&app);

    app.connect_activate(move |app| MainWindow::new(app, state.clone()).present());

    app.run()
}

fn setup_app_actions(app: &Application) {
    // Helper function to create actions with callbacks
    fn create_action_with_callback<F>(app: &Application, name: &str, callback: F)
    where
        F: Fn(&Application) + 'static,
    {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(glib::clone!(
            #[weak]
            app,
            move |_, _| callback(&app)
        ));
        app.add_action(&action);
    }

    // Create all actions using the helper function
    create_action_with_callback(app, "quit", |app| app.quit());
    create_action_with_callback(app, "about", show_about_dialog);
    create_action_with_callback(app, "shortcuts", show_shortcuts_window);
    create_action_with_callback(app, "preferences", show_preferences_window);

    // Set up keyboard shortcuts
    app.set_accels_for_action("app.quit", &["<Primary>q"]);
    app.set_accels_for_action("app.shortcuts", &["<Primary>question"]);
    app.set_accels_for_action("app.preferences", &["<Primary>comma"]);
}

fn show_about_dialog(app: &Application) {
    let about = AboutDialog::builder()
        .application_name("UpToDate")
        .application_icon(APP_ID)
        .version("1.0.0-alpha")
        .developer_name("Atay Özcan")
        .website("https://github.com/user/uptodate")
        .issue_url("https://github.com/user/uptodate/issues")
        .copyright("© 2025 Atay Özcan")
        .license_type(gtk::License::MitX11)
        .comments("Keep your system packages up to date")
        .build();

    about.present(app.active_window().as_ref());
}

fn show_shortcuts_window(app: &Application) {
    // Create the preference dialog which works better for this type of content
    let dialog = libadwaita::PreferencesDialog::builder()
        .title("Keyboard Shortcuts")
        .build();

    // Create the preference page
    let page = libadwaita::PreferencesPage::builder()
        .title("Shortcuts")
        .icon_name("input-keyboard-symbolic")
        .build();

    // Application shortcuts section
    let app_group = libadwaita::PreferencesGroup::builder()
        .title("Application")
        .description("General application shortcuts")
        .build();

    let quit_row = create_shortcut_row("Quit application", "Ctrl+Q");
    let shortcuts_row = create_shortcut_row("Show keyboard shortcuts", "Ctrl+?");
    let preferences_row = create_shortcut_row("Preferences", "Ctrl+,");

    app_group.add(&quit_row);
    app_group.add(&shortcuts_row);
    app_group.add(&preferences_row);

    // Update controls section
    let update_group = libadwaita::PreferencesGroup::builder()
        .title("Update Controls")
        .description("Shortcuts for managing updates")
        .build();

    let start_row = create_shortcut_row("Start Updates", "Ctrl+Return");
    let stop_row = create_shortcut_row("Stop Updates", "Escape");
    let dry_run_row = create_shortcut_row("Toggle Dry Run", "Ctrl+D");

    update_group.add(&start_row);
    update_group.add(&stop_row);
    update_group.add(&dry_run_row);

    page.add(&app_group);
    page.add(&update_group);
    dialog.add(&page);

    if let Some(window) = app.active_window() {
        dialog.present(Some(&window));
    }
}

fn create_shortcut_row(title: &str, shortcut: &str) -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder().title(title).build();

    let shortcut_label = gtk::Label::builder().label(shortcut).build();
    shortcut_label.add_css_class("keycap");
    shortcut_label.set_valign(gtk::Align::Center);

    row.add_suffix(&shortcut_label);
    row
}

fn show_preferences_window(app: &Application) {
    let preferences = libadwaita::PreferencesDialog::new();

    // Create a general page
    let general_page = libadwaita::PreferencesPage::new();
    general_page.set_title("General");
    general_page.set_icon_name(Some("preferences-system-symbolic"));

    // Add a group for update settings
    let update_group = libadwaita::PreferencesGroup::new();
    update_group.set_title("Update Settings");
    update_group.set_description(Some("Configure how updates are performed"));

    // Add the auto-refresh switch
    let auto_refresh_row = libadwaita::SwitchRow::new();
    auto_refresh_row.set_title("Auto-refresh sources");
    auto_refresh_row.set_subtitle("Automatically refresh package lists on startup");
    auto_refresh_row.set_active(true);

    // Add the notification switch
    let notification_row = libadwaita::SwitchRow::new();
    notification_row.set_title("Show notifications");
    notification_row.set_subtitle("Show system notifications when updates complete");
    notification_row.set_active(true);

    update_group.add(&auto_refresh_row);
    update_group.add(&notification_row);

    general_page.add(&update_group);
    preferences.add(&general_page);

    preferences.present(app.active_window().as_ref());
}
