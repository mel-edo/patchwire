mod window;
mod dbus_client;

use gtk4::glib;
use libadwaita as adw;
use adw::prelude::*;

const APP_ID: &str = "com.patchwire.Gtk";

fn main() -> glib::ExitCode {
    let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(move |app| {
        let proxy = match rt.block_on(dbus_client::connect()) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("could not connect to patchwire daemon: {e}");
                let win = adw::ApplicationWindow::builder()
                    .application(app)
                    .title("Patchwire")
                    .default_width(420)
                    .default_height(200)
                    .build();
                let status = adw::StatusPage::new();
                status.set_icon_name(Some("dialog-error-symbolic"));
                status.set_title("Daemon not running");
                status.set_description(Some("Start patchwire daemon first"));
                win.set_content(Some(&status));
                win.present();
                return;
            }
        };
        let win = window::PatchwireWindow::new(app, proxy, rt.handle().clone());
        win.present();
    });
    app.run()
}