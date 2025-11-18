use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BluetoothDevice {
    /// The D-Bus object path of the Bluetooth device.
    pub object_path: String,
    /// The name of the Bluetooth device.
    pub common_name: Option<String>,
    /// Whether the device is currently connected.
    pub connected: bool,
}
