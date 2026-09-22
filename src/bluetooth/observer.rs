// -- std imports
use core::panic;

// -- crate imports
use anyhow::Result;
use futures_util::stream::StreamExt;
use tokio::{sync::broadcast, task::JoinHandle};
use tracing::{debug, error, info, instrument};
use zbus::{
    Connection, MatchRule, MessageStream,
    fdo::{ObjectManagerProxy, PropertiesChanged},
    message::Type,
    zvariant::Value,
};

// -- module imports
use crate::bluetooth::constants::{BLUEZ_ADAPTER_IFACE, BLUEZ_DEVICE_IFACE, BLUEZ_SERVICE};

/// Defines the Bluetooth events that can be observed.
///
/// These events are emitted by the `BluetoothObserver` when changes are
/// detected on the D-Bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BluetoothEvent {
    /// Emitted when a Bluetooth adapter is turned on.
    AdapterOn,
    /// Emitted when a Bluetooth adapter is turned off.
    AdapterOff,
    /// Emitted when a Bluetooth interface connects to a device.
    InterfaceAdded,
    /// Emitted when a Bluetooth interface disconnects from a device.
    InterfaceRemoved,
}

/// Observes Bluetooth status changes from D-Bus and broadcasts them.
#[derive(Debug, Clone)]
pub struct BluetoothEventObserver {
    /// The current connection to the D-Bus.
    conn: Connection,
    /// The sender for broadcasting events to subscribers.
    pub tx: broadcast::Sender<BluetoothEvent>,
}

/// D-Bus signal observation and event broadcasting.
impl BluetoothEventObserver {
    /// Creates a new Bluetooth event observer.
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing the new [`BluetoothEventObserver`] instance or an error if the
    /// D-Bus connection fails.
    ///
    /// # Errors
    ///
    /// - [`anyhow::Error`] if the connection to the system D-Bus cannot be established.
    pub async fn new() -> Result<Self> {
        let conn = Connection::system().await?;
        let (tx, _rx) = broadcast::channel(10);

        Ok(Self { conn, tx })
    }

    /// Subscribes to Bluetooth events.
    pub fn subscribe(&self) -> broadcast::Receiver<BluetoothEvent> {
        self.tx.subscribe()
    }

    /// Spawns the observer to run in a background task.
    #[instrument(skip(self))]
    pub fn listen(&self) -> JoinHandle<()> {
        info!("Spawning Bluetooth event observer task.");
        let this = self.clone();
        tokio::spawn(async move {
            if let Err(e) = this.run().await {
                error!("Bluetooth observer failed: {}", e);
                panic!("Bluetooth observer encountered a fatal error.");
            }
        })
    }

    /// The private event loop. Listens for D-Bus signals and processes them.
    ///
    /// # Errors
    ///
    /// - [`anyhow::Error`] if setting up the observers fails or if there are issues
    ///     receiving D-Bus signals.
    #[instrument(skip_all)]
    async fn run(&self) -> Result<()> {
        self.dispatch_iface_observer().await?;
        self.dispatch_props_observer().await?;

        Ok(())
    }

    /// Sets up the observer for Bluetooth interface added signals.
    ///
    /// New device objects are announced via `InterfacesAdded`; device connect/disconnect
    /// transitions are handled by the `Connected` property in [`Self::dispatch_props_observer`].
    ///
    /// # Errors
    ///
    /// - [`anyhow::Error`] if setting up the observer fails.
    #[instrument(skip_all)]
    async fn dispatch_iface_observer(&self) -> Result<()> {
        let proxy = ObjectManagerProxy::builder(&self.conn)
            .destination(BLUEZ_SERVICE)?
            .path("/")? // always root path for ObjectManager
            .build()
            .await?;
        debug!("Bluetooth interface proxy created.");

        let mut iface_add_stream = proxy.receive_interfaces_added().await?;

        tokio::spawn({
            let tx = self.tx.clone();
            async move {
                info!("Listening for InterfacesAdded signals.");
                while let Some(signal) = iface_add_stream.next().await {
                    debug!("Received InterfacesAdded signal: {:#?}", signal.args());
                    if let Err(e) = tx.send(BluetoothEvent::InterfaceAdded) {
                        error!("Failed to send InterfaceAdded event: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Sets up the observer for Bluetooth property changes.
    ///
    /// Listens for `PropertiesChanged` signals from BlueZ: the adapter's `Powered` property
    /// maps to `AdapterOn`/`AdapterOff`, and a device's `Connected` property maps to
    /// `InterfaceAdded`/`InterfaceRemoved`.
    ///
    /// # Errors
    ///
    /// - [`anyhow::Error`] if setting up the observer fails.
    #[instrument(skip_all)]
    async fn dispatch_props_observer(&self) -> Result<()> {
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(BLUEZ_SERVICE)?
            .interface("org.freedesktop.DBus.Properties")?
            .member("PropertiesChanged")?
            .build();
        let mut stream = MessageStream::for_match_rule(rule, &self.conn, Some(1)).await?;

        tokio::spawn({
            let tx = self.tx.clone();
            async move {
                info!("Listening for PropertiesChanged signals.");

                while let Some(item) = stream.next().await {
                    let Ok(msg) = item else { continue };
                    let Some(signal) = PropertiesChanged::from_message(msg) else {
                        continue;
                    };
                    let Ok(args) = signal.args() else { continue };

                    match args.interface_name.as_str() {
                        BLUEZ_ADAPTER_IFACE => {
                            if let Some(Value::Bool(powered)) =
                                args.changed_properties.get("Powered")
                            {
                                let event = if *powered {
                                    BluetoothEvent::AdapterOn
                                } else {
                                    BluetoothEvent::AdapterOff
                                };
                                if let Err(e) = tx.send(event) {
                                    error!("Failed to send Bluetooth event: {}", e);
                                }
                            }
                        }
                        BLUEZ_DEVICE_IFACE => {
                            if let Some(Value::Bool(connected)) =
                                args.changed_properties.get("Connected")
                            {
                                let event = if *connected {
                                    BluetoothEvent::InterfaceAdded
                                } else {
                                    BluetoothEvent::InterfaceRemoved
                                };
                                if let Err(e) = tx.send(event) {
                                    error!("Failed to send Bluetooth event: {}", e);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        });

        Ok(())
    }
}
