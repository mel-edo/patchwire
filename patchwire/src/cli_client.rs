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
    fn set_sink_volume(&self, name: &str, volume: f32) -> zbus::Result<()>;
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
    println!("{:<6} {:<10} {:<10} {}", "LINK", "DEFAULT", "ENABLED", "DEVICE");

    for sink in sinks {
        let linked = if sink.is_linked { "linked" } else { "-" };
        let default = if sink.is_default { "default" } else { "-" };
        let enabled = if sink.is_default {
            "(source)"
        } else if sink.is_enabled {
            "yes"
        } else {
            "no"
        };
        println!("{:<6} {:<10} {:<10} {} [{}]", linked, default, enabled, sink.description, sink.name);
    }

    Ok(())
}

pub async fn cmd_toggle(sink_input: &str) -> anyhow::Result<()> {
    let proxy = connect().await?;

    // Read current state so toggle actually flips it
    let sinks = proxy.list_sinks().await?;
    let target = find_sink(&sinks, sink_input)?;

    if target.is_default {
        anyhow::bail!(
            "Cannot toggle '{}' because it is the current default system source.",
            target.description
        );
    }

    let new_state = !target.is_enabled;
    proxy.set_sink_enabled(&target.name, new_state).await?;

    let state_str = if new_state { "enabled" } else { "disabled" };
    println!("Successfully {} routing to {}", state_str, target.description);
    Ok(())
}

pub async fn cmd_profile(name: &str) -> anyhow::Result<()> {
    let proxy = connect().await?;
    proxy.set_active_profile(name).await?;
    println!("switched to profile: {name}");
    Ok(())
}

pub async fn cmd_volume(sink_input: &str, volume: f32) -> anyhow::Result<()> {
    let proxy = connect().await?;

    // Allow user to use the human readable description
    let sinks = proxy.list_sinks().await?;
    let target = find_sink(&sinks, sink_input)?;

    let normalized_vol = (volume / 100.0).clamp(0.0, 1.0);
    proxy.set_sink_volume(&target.name, normalized_vol).await?;
    println!("{} volume set to {}", target.description, volume.clamp(0.0, 100.0));
    Ok(())
}

// Helper function to find a sink via case-insensitive substring
fn find_sink<'a>(sinks: &'a [SinkInfo], input: &str) -> anyhow::Result<&'a SinkInfo> {
    let input_lower = input.to_lowercase();

    // Find all sinks where either the name or description contains the input text
    let matches: Vec<&SinkInfo> = sinks
        .iter()
        .filter(|s| {
            s.name.to_lowercase().contains(&input_lower)
            || s.description.to_lowercase().contains(&input_lower)
        })
        .collect();

    match matches.len() {
        0 => anyhow::bail!("No audio sink found matching '{}'", input),
        1 => Ok(matches[0]),
        _ => {
            // If the search is too vague, list the conflicts
            let names: Vec<String> = matches.iter().map(|s| s.description.clone()).collect();
            anyhow::bail!(
                "Multiple sinks matched '{}'. Please be more specific:\n - {}", input, names.join("\n - ")
            )
        }
    }
}