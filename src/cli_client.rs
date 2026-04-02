use zbus::Connection;
use zbus::proxy;

use crate::dbus_server::SinkInfo;

/// Proxy generated from the D-Bus interface
/// zbus uses this to make type safe method calls manually constructing D-Bus messages
#[proxy(
    interface = "com.patchwire.Daemon",
    default_service = "com.patchwire.Daemon",
    default_path = "/com/patchwire/Daemon"
)]
trait PatchwireDaemon {
    fn list_sinks(&self) -> zbus::Result<Vec<SinkInfo>>;
    fn set_sink_enabled(&self, name: &str, enabled: bool) -> zbus::Result<()>;
    fn get_profiles(&self) -> zbus::Result<Vec<String>>;
    fn set_active_profile(&self, name: &str) -> zbus::Result<()>;
    fn get_default_sink(&self) -> zbus::Result<String>;
}

/// Connect to the running daemon. Gives a clear error if it isn't running
async fn connect() -> anyhow::Result<PatchwireDaemonProxy<'static>> {
    let conn = Connection::session().await?;
    let proxy = PatchwireDaemonProxy::new(&conn).await?;
    Ok(proxy)
}

pub async fn cmd_list() -> anyhow::Result<()> {
    let proxy = connect().await?;
    let sinks = proxy.list_sinks().await?;

    if sinks.is_empty() {
        println!("no sinks found");
        return Ok(());
    }

    // Column header
    println!("{:<6} {:<10} {:<10} {}", "LINK", "DEFAULT", "ENABLED", "NAME");

    for sink in sinks {
        let linked = if sink.is_linked { "linked" } else { "-" };
        let default = if sink.is_default { "default" } else { "-" };
        let enabled = if sink.is_enabled { "yes" } else { "no" };
        println!("{:<6} {:<10} {:<10} {}", linked, default, enabled, sink.name);
    }

    Ok(())
}

pub async fn cmd_toggle(sink: &str) -> anyhow::Result<()> {
    let proxy = connect().await?;

    // Read current state so toggle actually flips it
    let sinks = proxy.list_sinks().await?;
    let current = sinks
        .iter()
        .find(|s| s.name == sink)
        .map(|s| s.is_enabled);

    match current {
        None => {
            anyhow::bail!("sink not found: {sink}");
        }
        Some(was_enabled) => {
            let new_state = !was_enabled;
            proxy.set_sink_enabled(sink, new_state).await?;
            println!(
                "{} {}",
                sink,
                if new_state { "enabled" } else { "disabled" }
            );
        }
    }

    Ok(())
}

pub async fn cmd_profile(name: &str) -> anyhow::Result<()> {
    let proxy = connect().await?;
    proxy.set_active_profile(name).await?;
    println!("switched to profile: {name}");
    Ok(())
}