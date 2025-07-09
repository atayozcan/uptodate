use libadwaita::{prelude::*, Application};
use uptodate::ui::MainWindow;
use uptodate::{setup_actions, AppState, APP_ID};

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt::init();
    libadwaita::init().unwrap();

    let app = Application::builder().application_id(APP_ID).build();

    let state = async_std::task::block_on(async { AppState::new().await });

    setup_actions(&app);

    app.connect_activate(move |app| {
        let window = MainWindow::new(app, state.clone());
        window.present();
    });

    app.run()
}
