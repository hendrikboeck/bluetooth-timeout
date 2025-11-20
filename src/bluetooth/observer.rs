use anyhow::Result;
use core::panic;
use futures_util::stream::StreamExt;
use tokio::{sync::broadcast, task::JoinHandle};
use tracing::{debug, error, info, instrument, warn};
use zbus::{
    Connection,
    fdo::{ObjectManagerProxy, PropertiesProxy},
};
use zvariant::Value;

use crate::configuration::Conf;

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
    /// Interface path for the Bluetooth adapter.
    pub iface: String,
    /// The current connection to the D-Bus.
    conn: Connection,
    /// The sender for broadcasting events to subscribers.
    pub tx: broadcast::Sender<BluetoothEvent>,
}

impl BluetoothEventObserver {
    /// Creates a new Bluetooth event observer for the specified adapter interface.
    pub async fn new(iface: String) -> Result<Self> {
        let conn = Connection::system().await?;
        let (tx, _rx) = broadcast::channel(10);

        Ok(Self { iface, conn, tx })
    }

    /// Subscribes to Bluetooth events.
    pub fn subscribe(&self) -> broadcast::Receiver<BluetoothEvent> {
        self.tx.subscribe()
    }

    /// Spawns the observer to run in a background task.
    #[instrument(skip(self))]
    pub fn listen(self) -> JoinHandle<()> {
        info!("Spawning Bluetooth event observer task.");
        tokio::spawn(async move {
            if let Err(e) = self.run().await {
                error!("Bluetooth observer failed: {}", e);
                panic!("Bluetooth observer encountered a fatal error.");
            }
        })
    }

    /// The private event loop. Listens for D-Bus signals and processes them.
    #[instrument(skip_all)]
    async fn run(&self) -> Result<()> {
        self.dispatch_iface_observer().await?;
        self.dispatch_adapter_props_observer().await?;

        Ok(())
    }

    /// Sets up the observer for Bluetooth interface added/removed signals.
    #[instrument(skip_all)]
    async fn dispatch_iface_observer(&self) -> Result<()> {
        let proxy = ObjectManagerProxy::builder(&self.conn)
            .destination(Conf::instance().dbus.service.as_str())?
            .path("/")? // always root path for ObjectManager
            .build()
            .await?;
        debug!("Bluetooth interface proxy created.");

        let mut iface_add_stream = proxy.receive_interfaces_added().await?;
        let mut iface_rm_stream = proxy.receive_interfaces_removed().await?;

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

        tokio::spawn({
            let tx = self.tx.clone();
            async move {
                info!("Listening for InterfacesRemoved signals.");
                while let Some(signal) = iface_rm_stream.next().await {
                    debug!("Received InterfacesRemoved signal: {:#?}", signal.args());
                    if let Err(e) = tx.send(BluetoothEvent::InterfaceRemoved) {
                        error!("Failed to send InterfaceRemoved event: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Sets up the observer for Bluetooth adapter property changes.
    #[instrument(skip_all)]
    async fn dispatch_adapter_props_observer(&self) -> Result<()> {
        let proxy = PropertiesProxy::builder(&self.conn)
            .destination(Conf::instance().dbus.service.as_str())?
            .path(self.iface.as_str())?
            .build()
            .await?;
        debug!("Bluetooth adapter properties proxy created.");

        let mut props_changed_stream = proxy.receive_properties_changed().await?;

        tokio::spawn({
            let tx = self.tx.clone();
            async move {
                info!("Listening for PropertiesChanged signals.");

                while let Some(signal) = props_changed_stream.next().await {
                    debug!("Received PropertiesChanged signal: {:#?}", signal.args());
                    let args = &signal.args().unwrap();

                    match args.changed_properties.get("Powered") {
                        Some(Value::Bool(true)) => {
                            debug!(
                                "Bluetooth adapter powered ON on interface: {}",
                                args.interface_name
                            );
                            if let Err(e) = tx.send(BluetoothEvent::AdapterOn) {
                                error!("Failed to send AdapterOn event: {}", e);
                            }
                        }

                        Some(Value::Bool(false)) => {
                            debug!(
                                "Bluetooth adapter powered OFF on interface: {}",
                                args.interface_name
                            );
                            if let Err(e) = tx.send(BluetoothEvent::AdapterOff) {
                                error!("Failed to send AdapterOff event: {}", e);
                            }
                        }

                        _ => {
                            debug!("Powered property not changed or not a boolean.");
                        }
                    }
                }
            }
        });

        Ok(())
    }
}
