// -- crate imports
use anyhow::Result;
use futures_util::future::BoxFuture;
use zbus::{Connection, names::InterfaceName, zvariant::Value};

// -- module imports
use crate::bluetooth::{
    constants::{BLUEZ_ADAPTER_IFACE, BLUEZ_DEVICE_IFACE, BLUEZ_SERVICE},
    device::BluetoothDevice,
};

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

/// D-Bus operations for a Bluetooth adapter.
impl BluetoothServiceProxy {
    /// Creates a new `BluetoothServiceProxy` for the specified interface.
    ///
    /// # Arguments
    ///
    /// - `iface` - A string slice that holds the D-Bus object path of the Bluetooth adapter (e.g., "/org/bluez/hci0").
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
        let proxy = zbus::fdo::PropertiesProxy::builder(&self.conn)
            .destination(BLUEZ_SERVICE)?
            .path(self.iface.as_str())?
            .build()
            .await?;

        let powered = proxy
            .get(
                InterfaceName::from_static_str(BLUEZ_ADAPTER_IFACE)?,
                "Powered",
            )
            .await?
            .downcast_ref::<bool>()?;

        Ok(powered)
    }

    /// Retrieves a list of Bluetooth devices associated with this adapter.
    ///
    /// This method queries the `ObjectManager` for all managed objects and filters them
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
        let proxy = zbus::fdo::ObjectManagerProxy::builder(&self.conn)
            .destination(BLUEZ_SERVICE)?
            .path("/")?
            .build()
            .await?;

        let objects = proxy.get_managed_objects().await?;
        let mut devices = vec![];

        for (path, ifaces) in objects {
            let Some(props) = ifaces.get(BLUEZ_DEVICE_IFACE) else {
                continue;
            };

            let path_str = path.to_string();
            if !path_str.starts_with(&format!("{}/dev_", self.iface)) {
                continue;
            }

            let name = props.get("Name").map(|v| v.to_string());
            let connected = props
                .get("Connected")
                .and_then(|v| v.downcast_ref::<bool>().ok())
                .unwrap_or(false);

            devices.push(BluetoothDevice {
                object_path: path_str,
                common_name: name,
                connected,
            });
        }

        Ok(devices)
    }

    /// Retrieves the number of currently connected devices.
    ///
    /// Returns the count of devices reporting a connected state, or `0` if the device
    /// list cannot be retrieved.
    pub async fn get_connected_devices_count(&self) -> usize {
        let devices = self.get_devices().await.unwrap_or(vec![]);
        devices.iter().filter(|dev| dev.connected).count()
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
        let proxy = zbus::fdo::PropertiesProxy::builder(&self.conn)
            .destination(BLUEZ_SERVICE)?
            .path(self.iface.as_str())?
            .build()
            .await?;

        proxy
            .set(
                InterfaceName::from_static_str(BLUEZ_ADAPTER_IFACE)?,
                "Powered",
                Value::Bool(false),
            )
            .await?;

        Ok(())
    }
}

/// Abstraction over the adapter operations used by the service and timeout logic.
///
/// # ponytail: introduced only for testability; the sole production impl is [`BluetoothServiceProxy`].
pub trait AdapterProxy: Clone + Send + Sync + 'static {
    /// Whether the adapter is powered on.
    fn is_powered(&self) -> BoxFuture<'_, Result<bool>>;
    /// Number of currently connected devices.
    fn get_connected_devices_count(&self) -> BoxFuture<'_, usize>;
    /// Powers off the adapter.
    fn turn_off_adapter(&self) -> BoxFuture<'_, Result<()>>;
}

impl AdapterProxy for BluetoothServiceProxy {
    fn is_powered(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move { Self::is_powered(self).await })
    }

    fn get_connected_devices_count(&self) -> BoxFuture<'_, usize> {
        Box::pin(async move { Self::get_connected_devices_count(self).await })
    }

    fn turn_off_adapter(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { Self::turn_off_adapter(self).await })
    }
}
