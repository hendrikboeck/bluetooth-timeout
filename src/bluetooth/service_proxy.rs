use anyhow::Result;
use zbus::{
    Connection,
    fdo::{ObjectManagerProxy, PropertiesProxy},
    names::InterfaceName,
    zvariant::Value,
};

use crate::{bluetooth::device::BluetoothDevice, configuration::Conf};

/// A proxy for interacting with the Bluetooth service via D-Bus.
///
/// This struct manages the connection to the system D-Bus and provides methods
/// to query and manipulate the state of a specific Bluetooth adapter interface.
#[derive(Debug, Clone)]
pub struct BluetoothServiceProxy {
    /// Interface path for the Bluetooth adapter.
    pub iface: String,
    /// The current connection to the D-Bus.
    conn: Connection,
}

impl BluetoothServiceProxy {
    /// Creates a new `BluetoothServiceProxy` for the specified interface.
    ///
    /// # Arguments
    ///
    /// - `iface` - A string slice that holds the D-Bus object path of the Bluetooth adapter (e.g.,
    ///     "/org/bluez/hci0").
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing the new `BluetoothServiceProxy` instance or an error if the
    /// D-Bus connection fails.
    ///
    /// # Errors
    ///
    /// - [`anyhow::Error`] if the connection to the system D-Bus cannot be established.
    pub async fn new(iface: String) -> Result<Self> {
        Ok(Self {
            iface,
            conn: Connection::system().await?,
        })
    }

    /// Checks if the Bluetooth adapter is currently powered on.
    ///
    /// This method queries the "Powered" property of the adapter interface via D-Bus.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the adapter is powered on, `Ok(false)` otherwise.
    ///
    /// # Errors
    ///
    /// - [`anyhow::Error`] if the D-Bus call fails or the property cannot be retrieved.
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

    /// Retrieves a list of Bluetooth devices associated with this adapter.
    ///
    /// This method queries the ObjectManager for all managed objects and filters them
    /// to find devices that belong to the current adapter interface.
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing a vector of `BluetoothDevice` structs representing the found
    /// devices.
    ///
    /// # Errors
    ///
    /// - [`anyhow::Error`] if the D-Bus call fails or the objects cannot be retrieved.
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

    /// Turns off the Bluetooth adapter.
    ///
    /// This method sets the "Powered" property of the adapter interface to `false` via D-Bus.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the operation was successful, or an error otherwise.
    ///
    /// # Errors
    ///
    /// - [`anyhow::Error`] if the D-Bus call fails or the property cannot be set.
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
