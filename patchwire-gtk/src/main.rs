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
        let (_conn, proxy): (&'static zbus::Connection, dbus_client::PatchwireDaemonProxy<'static>) = match rt.block_on( async {
            let conn = zbus::Connection::session().await?;
            let conn: &'static zbus::Connection = Box::leak(Box::new(conn));
            let proxy = dbus_client::PatchwireDaemonProxy::builder(conn).build().await?;
            anyhow::Ok((conn, proxy))
        }) {
            Ok(p) => p,
            Err(e) => {
                show_error(app, &e.to_string());
                return;
            }
        };

        if let Err(e) = rt.block_on(ensure_daemon_running(&proxy)) {
            show_error(app, &e.to_string());
            return;
        }

        let win = window::PatchwireWindow::new(app, proxy, rt.handle().clone());
        win.present();
    });
    app.run()
}

async fn ensure_daemon_running(proxy: &dbus_client::PatchwireDaemonProxy<'static>) -> anyhow::Result<()> {
    // already running
    if dbus_client::ping(proxy).await {
        return Ok(());
    }

    let systemctl_ok = tokio::process::Command::new("systemctl")
        .args(["--user", "start", "patchwire"])
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);

    if systemctl_ok {
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if dbus_client::ping(proxy).await {
                return Ok(());
            }
        }
        anyhow::bail!("systemctl started daemon but sinks never appeared");
    }

    // fallback: direct spawn (dev)
    let mut daemon_path = std::env::current_exe()?
        .parent()
        .unwrap()
        .to_path_buf();
    daemon_path.push("patchwire");

    if !daemon_path.exists() {
        anyhow::bail!(
            "could not find patchwire daemon binary at {}",
            daemon_path.display()
        );
    }

    let log_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~"))
        .join(".config")
        .join("patchwire")
        .join("daemon.log");

    std::fs::create_dir_all(log_path.parent().unwrap()).ok();

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let log_file2 = log_file.try_clone()?;

    tokio::process::Command::new(&daemon_path)
        .arg("daemon")
        .stdout(log_file)
        .stderr(log_file2)
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn daemon: {e}"))?;
    
    // retry for up to 3 seconds
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if dbus_client::ping(proxy).await {
            return Ok(());
        }
    }

    anyhow::bail!("daemon started but could not connect via D-Bus")
}

fn show_error(app: &adw::Application, msg: &str) {
    let win = adw::ApplicationWindow::builder()
        .application(app)
        .title("Patchwire")
        .default_width(420)
        .default_height(200)
        .build();
    let status = adw::StatusPage::new();
    status.set_icon_name(Some("dialog-error-symbolic"));
    status.set_title("Could not start daemon");
    status.set_description(Some(msg));
    win.set_content(Some(&status));
    win.present();
}