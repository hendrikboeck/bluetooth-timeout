use anyhow::Result;
use zbus::{
    Connection,
    fdo::{ObjectManagerProxy, PropertiesProxy},
    names::InterfaceName,
    zvariant::Value,
};

use crate::{bluetooth::device::BluetoothDevice, configuration::Conf};

#[derive(Debug, Clone)]
pub struct BluetoothServiceProxy {
    /// Interface path for the Bluetooth adapter.
    pub iface: String,
    /// The current connection to the D-Bus.
    conn: Connection,
}

impl BluetoothServiceProxy {
    pub async fn new(iface: String) -> Result<Self> {
        Ok(Self {
            iface,
            conn: Connection::system().await?,
        })
    }

    pub async fn is_powered(&self) -> Result<bool> {
        let conf = Conf::instance();
        let proxy = PropertiesProxy::builder(&self.conn)
            .destination(conf.dbus.service.as_str())?
            .path(self.iface.as_str())?
            .build()
            .await?;

        let powered = proxy
            .get(
                InterfaceName::from_static_str(conf.dbus.adapter_iface.as_str())?,
                "Powered",
            )
            .await?
            .downcast_ref::<bool>()?;

        Ok(powered)
    }

    pub async fn get_devices(&self) -> Result<Vec<BluetoothDevice>> {
        let conf = Conf::instance();
        let proxy = ObjectManagerProxy::builder(&self.conn)
            .destination(conf.dbus.service.as_str())?
            .path("/")?
            .build()
            .await?;

        let objects = proxy.get_managed_objects().await?;
        let mut devices = vec![];

        for (path, ifaces) in objects {
            let props = match ifaces.get(conf.dbus.device_iface.as_str()) {
                Some(p) => p,
                None => continue,
            };

            let path_str = path.to_string();
            if !path_str.starts_with(&format!("{}/dev_", self.iface)) {
                continue;
            }

            let name = props.get("Name").map(|v| v.to_string());
            let connected = props
                .get("Connected")
                .map(|v| v.downcast_ref::<bool>().ok())
                .flatten()
                .unwrap_or(false);

            devices.push(BluetoothDevice {
                object_path: path_str,
                common_name: name,
                connected,
            });
        }

        Ok(devices)
    }

    pub async fn turn_off_adapter(&self) -> Result<()> {
        let conf = Conf::instance();
        let proxy = PropertiesProxy::builder(&self.conn)
            .destination(conf.dbus.service.as_str())?
            .path(self.iface.as_str())?
            .build()
            .await?;

        proxy
            .set(
                InterfaceName::from_static_str(conf.dbus.adapter_iface.as_str())?,
                "Powered",
                Value::Bool(false),
            )
            .await?;

        Ok(())
    }
}
