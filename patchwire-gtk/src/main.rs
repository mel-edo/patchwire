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
        let proxy = match rt.block_on(async {
            ensure_daemon_running().await?;
            dbus_client::connect().await
        }) {
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

async fn ensure_daemon_running() -> anyhow::Result<()> {
    eprintln!("ensure_daemon_running called");

    let conn = zbus::Connection::session().await?;
    let proxy = dbus_client::PatchwireDaemonProxy::builder(&conn)
        .build()
        .await?;

    if dbus_client::ping(&proxy).await {
        eprintln!("daemon already running");
        return Ok(());
    }
    eprintln!("systemctl failed or daemon not available, trying direct spawn...");
    let mut daemon_path = std::env::current_exe()?
        .parent()
        .unwrap()
        .to_path_buf();
    daemon_path.push("patchwire");

    eprintln!("looking for daemon at: {}", daemon_path.display());
    eprintln!("exists: {}", daemon_path.exists());

    if !daemon_path.exists() {
        anyhow::bail!(
            "could not find patchwire daemon binary at {}",
            daemon_path.display()
        );
    }

    tokio::process::Command::new(&daemon_path)
        .arg("daemon")
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn daemon: {e}"))?;
    
    eprintln!("daemon spawned, waiting for D-Bus registration...");

    // retry for up to 3 seconds
    for i in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if dbus_client::ping(&proxy).await {
            eprintln!("connected after {}ms", (i+1) * 500);
            return Ok(());
        }
        eprintln!("waiting... attemp {}", i+1);
    }
        
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    if dbus_client::ping(&proxy).await {
        eprintln!("connected successfully");
        return Ok(());
    }
    eprintln!("still could not connect after spawn");
    anyhow::bail!("daemon started but could not connect via D-Bus")
}