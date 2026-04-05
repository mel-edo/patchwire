use anyhow::Result;
use zbus::{Connection, proxy};

#[derive(Debug, Clone, zbus::zvariant::Type, serde::Serialize, serde::Deserialize)]
pub struct SinkInfo {
    pub name: String,
    pub description: String,
    pub is_default: bool,
    pub is_linked: bool,
    pub is_enabled: bool,
}

#[proxy(
    interface = "com.patchwire.Daemon",
    default_service = "com.patchwire.Daemon",
    default_path = "/com/patchwire/Daemon"
)]
pub trait PatchwireDaemon {
    fn list_sinks(&self) -> zbus::Result<Vec<SinkInfo>>;
    fn set_sink_enabled(&self, name: &str, enabled: bool) -> zbus::Result<()>;
    fn get_profiles(&self) -> zbus::Result<Vec<String>>;
    fn set_active_profile(&self, name: &str) -> zbus::Result<()>;
    fn save_profile(&self, name: &str) -> zbus::Result<()>;
    fn delete_profile(&self, name: &str) -> zbus::Result<()>;
    fn set_default_sink(&self, name: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn sinks_changed(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn link_state_changed(&self, name: &str, linked: bool) -> zbus::Result<()>;
    #[zbus(signal)]
    fn default_changed(&self, new_default: &str) -> zbus::Result<()>;
}

pub async fn connect() -> Result<PatchwireDaemonProxy<'static>> {
    let conn = Connection::session().await?;
    let proxy = PatchwireDaemonProxy::new(&conn).await?;
    Ok(proxy)
}