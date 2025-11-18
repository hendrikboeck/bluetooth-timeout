use anyhow::Result;
use core::panic;
use futures_util::stream::StreamExt;
use std::cmp::max;
use tokio::{sync::broadcast, task::JoinHandle};
use tracing::{debug, error, info, instrument, warn};
use zbus::{
    Connection,
    fdo::{ObjectManagerProxy, PropertiesChangedArgs},
    proxy::Proxy,
};
use zvariant::OwnedValue;

const BLUEZ_SERVICE: &str = "org.bluez";
const BLUEZ_ADAPTER_INTERFACE: &str = "org.bluez.Adapter1";
const BLUEZ_DEVICE_INTERFACE: &str = "org.bluez.Device1";
const OBJECT_MANAGER_INTERFACE: &str = "org.freedesktop.DBus.ObjectManager";

/// Defines the Bluetooth events that can be observed.
///
/// These events are emitted by the `BluetoothObserver` when changes are
/// detected on the D-Bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BluetoothEvent {
    /// Emitted when a Bluetooth adapter is turned on.
    AdapterOn(String), // Adapter path
    /// Emitted when a Bluetooth adapter is turned off.
    AdapterOff(String), // Adapter path
    /// Emitted when a new Bluetooth device is connected.
    DeviceConnected(u32), // number of connected devices
    /// Emitted when a Bluetooth device is disconnected.
    DeviceDisconnected(u32), // number of connected devices
}

/// Observes Bluetooth status changes from D-Bus and broadcasts them.
#[derive(Debug, Clone)]
pub struct BluetoothObserver {
    /// The current connection to the D-Bus.
    conn: Connection,
    /// The sender for broadcasting events to subscribers.
    sender: broadcast::Sender<BluetoothEvent>,
    /// The last known count of connected devices. (used for fallback)
    device_count_fallback: usize,
}

impl BluetoothObserver {
    /// Creates a new `BluetoothObserver`.
    ///
    /// Initializes a connection to the system D-Bus, gets the initial count
    /// of Bluetooth devices, and sets up a broadcast channel for sending events.
    ///
    /// # Errors
    /// - [`anyhow::Error`] if there is a failure connecting to D-Bus or retrieving
    ///   the initial device count.
    ///
    /// # Returns
    /// A `Result` containing the new `BluetoothObserver` or a D-Bus connection error.
    pub async fn new(conn: Connection) -> Result<Self> {
        let (sender, _) = broadcast::channel(10);

        let initial_count = Self::get_authoritative_device_count(&conn).await?;
        info!(
            initial_device_count = initial_count,
            "Initial device state retrieved."
        );

        Ok(Self {
            conn,
            sender,
            device_count_fallback: initial_count,
        })
    }

    /// Retrieves the authoritative count of all Bluetooth devices from BlueZ.
    ///
    /// # Errors
    /// - [`anyhow::Error`] if there is a failure querying D-Bus.
    ///
    /// # Returns
    /// A `Result` containing the count of connected Bluetooth devices.
    async fn get_authoritative_device_count(conn: &Connection) -> Result<usize> {
        Ok(ObjectManagerProxy::new(conn, BLUEZ_SERVICE, "/")
            .await?
            .get_managed_objects()
            .await?
            .values()
            .filter_map(|interfaces| interfaces.get(BLUEZ_DEVICE_INTERFACE))
            .filter_map(|device_props| device_props.get("Connected"))
            .filter_map(|v: &OwnedValue| bool::try_from(v).ok())
            .filter(|connected| *connected)
            .count())
    }

    async fn get_device_count(&mut self, update: i32) -> usize {
        self.device_count_fallback = max(0, self.device_count_fallback as i32 + update) as usize;

        Self::get_authoritative_device_count(&self.conn)
            .await
            .unwrap_or(self.device_count_fallback)
    }

    /// Subscribes to Bluetooth events.
    pub fn subscribe(&self) -> broadcast::Receiver<BluetoothEvent> {
        self.sender.subscribe()
    }

    /// Spawns the observer to run in a background task.
    #[instrument(skip(self))]
    pub fn listen(mut self) -> JoinHandle<()> {
        info!("Spawning Bluetooth observer task.");
        tokio::spawn(async move {
            if let Err(e) = self.run().await {
                error!("Bluetooth observer failed: {}", e);
                panic!("Bluetooth observer encountered a fatal error.");
            }
        })
    }

    /// The private event loop. Listens for D-Bus signals and processes them.
    #[instrument(skip_all)]
    async fn run(&mut self) -> Result<()> {
        let proxy = Proxy::new(
            &self.conn,
            BLUEZ_SERVICE,
            "/org/bluez",
            OBJECT_MANAGER_INTERFACE,
        )
        .await?;

        let status = proxy.introspect().await?;
        debug!("Introspected BlueZ D-Bus interface:\n{}", status);

        let mut props_changed = proxy.receive_signal("PropertiesChanged").await?;
        info!("Listening for Bluetooth property changes...");

        while let Some(signal) = props_changed.next().await {
            debug!("Received PropertiesChanged signal: {:#?}", signal);

            let path = signal
                .header()
                .path()
                .map(|p| p.as_str().to_string())
                .unwrap_or("unknown path".into());

            let body = signal.body();
            let args = match PropertiesChangedArgs::try_from(&body) {
                Ok(args) => args,
                Err(e) => {
                    warn!(
                        error = %e,
                        "Failed to parse PropertiesChanged signal. skipping..."
                    );
                    continue;
                }
            };

            let iface = args.interface_name.as_str();
            let changed_props = args.changed_properties();

            let event = match iface {
                BLUEZ_ADAPTER_INTERFACE => changed_props
                    .get("Powered")
                    .map(|v| bool::try_from(v).ok())
                    .flatten()
                    .map(|powered| {
                        if powered {
                            BluetoothEvent::AdapterOn(path)
                        } else {
                            BluetoothEvent::AdapterOff(path)
                        }
                    }),

                BLUEZ_DEVICE_INTERFACE => match changed_props
                    .get("Connected")
                    .map(|v| bool::try_from(v).ok())
                    .flatten()
                {
                    Some(true) => {
                        let count = self.get_device_count(1).await;
                        Some(BluetoothEvent::DeviceConnected(count as u32))
                    }
                    Some(false) => {
                        let count = self.get_device_count(-1).await;
                        Some(BluetoothEvent::DeviceDisconnected(count as u32))
                    }
                    None => None,
                },

                _ => None,
            };

            if let Some(event) = event {
                if self.sender.send(event).is_err() {
                    debug!("No subscribers to notify about the event.");
                }
            }
        }

        Ok(())
    }
}
