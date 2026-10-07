// -- std imports
use std::time::Duration;

// -- crate imports
use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

// -- module imports
use crate::{
    bluetooth::{observer::BluetoothEvent, service_proxy::BluetoothServiceProxy},
    configuration::NotificationConf,
    timeout::TimeoutTask,
};

/// Represents the state of the Bluetooth service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BluetoothServiceState {
    /// The Bluetooth adapter is powered off.
    Off,
    /// The Bluetooth adapter is on, but no devices are connected.
    Idle,
    /// The Bluetooth adapter is on and at least one device is connected.
    Running,
}

/// Manages the state of a Bluetooth adapter and handles events.
///
/// This service listens for Bluetooth events and manages a timeout to turn off
/// the adapter when it's idle.
#[derive(Debug)]
pub struct BluetoothService {
    /// The Bluetooth interface name (e.g., "hci0").
    pub iface: String,
    /// Proxy to interact with the Bluetooth service via D-Bus.
    service_proxy: BluetoothServiceProxy,
    /// Current state of the Bluetooth service.
    pub state: BluetoothServiceState,
    /// Handle to the active timeout timer task, if any.
    pub active_timer: Option<tokio::task::JoinHandle<()>>,
    /// Duration before the timeout triggers.
    timeout: Duration,
    /// Notification configuration, cloned to each spawned [`TimeoutTask`].
    notification_conf: NotificationConf,
}

/// Retrieves the number of connected Bluetooth devices using the service proxy.
async fn get_connected_devices_count_from_proxy(proxy: &BluetoothServiceProxy) -> usize {
    let devices = proxy.get_devices().await.unwrap_or(vec![]);
    devices.iter().filter(|dev| dev.connected).count()
}

/// State management, event handling, and timeout lifecycle.
impl BluetoothService {
    /// Creates a new `BluetoothService`.
    ///
    /// It initializes the service by determining the current state of the Bluetooth adapter
    /// and starting a timeout timer if the adapter is idle.
    ///
    /// # Arguments
    ///
    /// - `iface` - The name of the Bluetooth interface to manage.
    /// - `timeout` - The duration to wait before turning off an idle adapter.
    /// - `notification_conf` - The notification configuration.
    pub async fn new(
        iface: String,
        timeout: Duration,
        notification_conf: NotificationConf,
    ) -> Result<Self> {
        let service_proxy = BluetoothServiceProxy::new(iface.clone()).await?;
        let num_connected_devices = get_connected_devices_count_from_proxy(&service_proxy).await;
        // Assume adapter is off if we cannot determine its powered state (e.g., Adapter not found)
        let powered = service_proxy.is_powered().await.unwrap_or(false);

        // When the adapter is powered off, any `Connected` device state is stale, so treat
        // `(false, _)` as `Off` rather than erroring out. That transient state can occur right
        // after boot or wake and would otherwise prevent the service from starting at all.
        let state = match (powered, num_connected_devices) {
            (false, _) => BluetoothServiceState::Off,
            (true, 0) => BluetoothServiceState::Idle,
            (true, _) => BluetoothServiceState::Running,
        };
        info!("Initial BluetoothService state: {:#?}", state);

        let active_timer = if state == BluetoothServiceState::Idle {
            info!(
                "Starting timeout timer for idle adapter with timeout of {:?}",
                timeout
            );
            Some(
                TimeoutTask::new(timeout, service_proxy.clone(), notification_conf.clone()).spawn(),
            )
        } else {
            None
        };

        let service = Self {
            iface,
            service_proxy,
            state,
            active_timer,
            timeout,
            notification_conf,
        };
        debug!("Created new BluetoothService for iface {:?}", service.iface);

        Ok(service)
    }

    /// Starts the main event loop for the service.
    ///
    /// This method will run indefinitely, waiting for and processing `BluetoothEvent`s.
    pub async fn start(&mut self, rx: broadcast::Receiver<BluetoothEvent>) -> Result<()> {
        let mut rx = rx;
        loop {
            let event = match rx.recv().await {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    // The receiver fell behind (e.g. a burst of connect/disconnect signals on
                    // wake). Re-sync from D-Bus instead of aborting, otherwise the dropped
                    // receiver would close the broadcast channel for good.
                    warn!("Missed {skipped} Bluetooth events; re-synchronizing adapter state.");
                    self.sync_state().await;
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("Bluetooth event channel closed; stopping service.");
                    return Ok(());
                }
            };

            tracing::info!("BluetoothService received event: {:#?}", event);

            match event {
                BluetoothEvent::AdapterOn => {
                    let _ = self
                        .on_adapter_on()
                        .await
                        .inspect_err(|e| error!("Error on AdapterOn event: {:#?}", e.backtrace()));
                }
                BluetoothEvent::AdapterOff => {
                    self.on_adapter_off();
                }
                BluetoothEvent::InterfaceAdded => {
                    let _ = self.on_interface_added().await.inspect_err(|e| {
                        error!("Error on InterfaceAdded event: {:#?}", e.backtrace());
                    });
                }
                BluetoothEvent::InterfaceRemoved => {
                    let _ = self.on_interface_removed().await.inspect_err(|e| {
                        error!("Error on InterfaceRemoved event: {:#?}", e.backtrace());
                    });
                }
            }
        }
    }

    /// Handles the `AdapterOn` event.
    ///
    /// This method updates the service state and manages the timeout timer based on
    /// whether any devices are connected.
    pub async fn on_adapter_on(&mut self) -> Result<()> {
        debug!("Handling AdapterOn event...");

        match self.state {
            BluetoothServiceState::Off | BluetoothServiceState::Idle => {
                let need_timer = self
                    .active_timer
                    .as_ref()
                    .is_none_or(tokio::task::JoinHandle::is_finished);
                if need_timer {
                    self.active_timer = Some(
                        TimeoutTask::new(
                            self.timeout,
                            self.service_proxy.clone(),
                            self.notification_conf.clone(),
                        )
                        .spawn(),
                    );
                }
            }
            BluetoothServiceState::Running => {
                if let Some(timer) = self.active_timer.take()
                    && !timer.is_finished()
                {
                    timer.abort();
                    info!("Cancelled active timeout timer.");
                }
            }
        }

        if self.get_connected_devices_count().await > 0 {
            self.state = BluetoothServiceState::Running;
        } else {
            self.state = BluetoothServiceState::Idle;
        }

        Ok(())
    }

    /// Handles the `AdapterOff` event.
    ///
    /// This method cancels any active timeout timer and sets the state to `Off`.
    pub fn on_adapter_off(&mut self) {
        debug!("Handling AdapterOff event...");

        if let Some(timer) = self.active_timer.take() {
            timer.abort();
            info!("Cancelled active timeout timer.");
        }

        self.state = BluetoothServiceState::Off;
    }

    /// Handles the `InterfaceAdded` event, which typically signifies a device connection.
    pub async fn on_interface_added(&mut self) -> Result<()> {
        debug!("Handling InterfaceAdded event...");

        self.on_interface_changed().await
    }

    /// Handles the `InterfaceRemoved` event, which typically signifies a device disconnection.
    pub async fn on_interface_removed(&mut self) -> Result<()> {
        debug!("Handling InterfaceRemoved event...");

        self.on_interface_changed().await
    }

    /// Handles changes in device connections.
    ///
    /// This method checks the number of connected devices and updates the service state
    /// and timeout timer accordingly.
    async fn on_interface_changed(&mut self) -> Result<()> {
        let connected_devices = self.get_connected_devices_count().await;
        debug!("Connected devices count: {}", connected_devices);

        if connected_devices > 0 {
            if let Some(timer) = self.active_timer.take()
                && !timer.is_finished()
            {
                timer.abort();
                info!("Cancelled active timeout timer.");
            }
            self.state = BluetoothServiceState::Running;
        } else {
            if self.active_timer.is_none() {
                debug!("No connected devices and no active timer. Starting timeout timer...");
                self.active_timer = Some(
                    TimeoutTask::new(
                        self.timeout,
                        self.service_proxy.clone(),
                        self.notification_conf.clone(),
                    )
                    .spawn(),
                );
            }
            self.state = BluetoothServiceState::Idle;
        }

        Ok(())
    }

    /// Gets the current number of connected devices.
    async fn get_connected_devices_count(&self) -> usize {
        get_connected_devices_count_from_proxy(&self.service_proxy).await
    }

    /// Re-synchronizes the service's internal state with the adapter's actual state.
    ///
    /// Used to recover after the event receiver lagged behind (missed events), so the timeout
    /// timer and state reflect reality even when some events were dropped.
    async fn sync_state(&mut self) {
        let powered = self.service_proxy.is_powered().await.unwrap_or(false);
        let connected_devices = self.get_connected_devices_count().await;

        self.state = match (powered, connected_devices) {
            (false, _) => BluetoothServiceState::Off,
            (true, 0) => BluetoothServiceState::Idle,
            (true, _) => BluetoothServiceState::Running,
        };

        self.reconcile_timer();
        debug!(
            "Re-synchronized BluetoothService state to {:#?}",
            self.state
        );
    }

    /// Ensures the timeout timer matches the current state: a timer runs only while idle.
    fn reconcile_timer(&mut self) {
        if self.state == BluetoothServiceState::Idle {
            let need_timer = self
                .active_timer
                .as_ref()
                .is_none_or(tokio::task::JoinHandle::is_finished);
            if need_timer {
                self.active_timer = Some(
                    TimeoutTask::new(
                        self.timeout,
                        self.service_proxy.clone(),
                        self.notification_conf.clone(),
                    )
                    .spawn(),
                );
            }
        } else if let Some(timer) = self.active_timer.take()
            && !timer.is_finished()
        {
            timer.abort();
            info!("Cancelled active timeout timer.");
        }
    }
}
