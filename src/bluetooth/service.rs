use std::time::Duration;

use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use crate::{
    bluetooth::{observer::BluetoothEvent, service_proxy::BluetoothServiceProxy},
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
    /// Receiver for Bluetooth events.
    ///
    /// This is an `Option` to allow for late initialization via `subscribe_to`.
    rx: Option<broadcast::Receiver<BluetoothEvent>>,
    /// Proxy to interact with the Bluetooth service via D-Bus.
    service_proxy: BluetoothServiceProxy,
    /// Current state of the Bluetooth service.
    pub state: BluetoothServiceState,
    /// Handle to the active timeout timer task, if any.
    pub active_timer: Option<tokio::task::JoinHandle<()>>,
    /// Duration before the timeout triggers.
    timeout: Duration,
}

/// Retrieves the number of connected Bluetooth devices using the service proxy.
async fn get_connected_devices_count_from_proxy(proxy: &BluetoothServiceProxy) -> usize {
    let devices = proxy.get_devices().await.unwrap_or(vec![]);
    let connected_count = devices.iter().filter(|dev| dev.connected).count();

    connected_count
}

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
    pub async fn new(iface: String, timeout: Duration) -> Result<Self> {
        let service_proxy = BluetoothServiceProxy::new(iface.clone()).await?;
        let num_connected_devices = get_connected_devices_count_from_proxy(&service_proxy).await;
        // Assume adapter is off if we cannot determine its powered state (e.g., Adapter not found)
        let powered = service_proxy.is_powered().await.unwrap_or(false);

        let state = match (powered, num_connected_devices) {
            (false, 0) => BluetoothServiceState::Off,
            (true, 0) => BluetoothServiceState::Idle,
            (true, devs) if devs > 0 => BluetoothServiceState::Running,
            _ => {
                return Err(anyhow::anyhow!(
                    "Could not determine BluetoothService state or encountered unexpected state
                    (like powered: false with connected devices)"
                ));
            }
        };
        info!("Initial BluetoothService state: {:#?}", state);

        let active_timer = if state == BluetoothServiceState::Idle {
            info!(
                "Starting timeout timer for idle adapter with timeout of {:?}",
                timeout
            );
            Some(TimeoutTask::new(timeout, service_proxy.clone()).spawn())
        } else {
            None
        };

        let service = Self {
            iface,
            rx: None,
            service_proxy,
            state,
            active_timer,
            timeout,
        };
        debug!("Created new BluetoothService for iface {:?}", service.iface);

        Ok(service)
    }

    /// Subscribes the service to a broadcast channel for `BluetoothEvent`s.
    pub fn subscribe_to(&mut self, rx: broadcast::Receiver<BluetoothEvent>) -> &mut Self {
        self.rx = Some(rx);
        return self;
    }

    /// Starts the main event loop for the service.
    ///
    /// This method will run indefinitely, waiting for and processing `BluetoothEvent`s.
    /// It requires a receiver to have been subscribed via `subscribe_to`.
    pub async fn start(&mut self) -> Result<()> {
        if self.rx.is_none() {
            return Err(anyhow::anyhow!(
                "Cannot start BluetoothService without a subscribed receiver"
            ));
        }

        let mut rx = self.rx.take().unwrap();
        loop {
            let event = rx.recv().await?;
            tracing::info!("BluetoothService received event: {:#?}", event);

            match event {
                BluetoothEvent::AdapterOn => {
                    let _ = self
                        .on_adapter_on()
                        .await
                        .inspect_err(|e| error!("Error on AdapterOn event: {:#?}", e.backtrace()));
                }
                BluetoothEvent::AdapterOff => {
                    let _ = self
                        .on_adapter_off()
                        .await
                        .inspect_err(|e| error!("Error on AdapterOff event: {:#?}", e.backtrace()));
                }
                BluetoothEvent::InterfaceAdded => {
                    let _ = self.on_interface_added().await.inspect_err(|e| {
                        error!("Error on InterfaceAdded event: {:#?}", e.backtrace())
                    });
                }
                BluetoothEvent::InterfaceRemoved => {
                    let _ = self.on_interface_removed().await.inspect_err(|e| {
                        error!("Error on InterfaceRemoved event: {:#?}", e.backtrace())
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
            BluetoothServiceState::Off | BluetoothServiceState::Idle
                if self.active_timer.is_none()
                    || self.active_timer.as_ref().unwrap().is_finished() =>
            {
                self.active_timer =
                    Some(TimeoutTask::new(self.timeout, self.service_proxy.clone()).spawn());
            }
            BluetoothServiceState::Running
                if self.active_timer.is_some()
                    && !self.active_timer.as_ref().unwrap().is_finished() =>
            {
                self.active_timer.take().unwrap().abort();
                info!("Cancelled active timeout timer.");
            }
            _ => {}
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
    pub async fn on_adapter_off(&mut self) -> Result<()> {
        debug!("Handling AdapterOff event...");

        if self.active_timer.is_some() {
            tokio::spawn({
                let timer = self.active_timer.take().unwrap();

                async move {
                    // Give some time for the timer to abort gracefully
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    if !timer.is_finished() {
                        timer.abort();
                        info!("Cancelled active timeout timer.");
                    }
                }
            });
        }

        self.state = BluetoothServiceState::Off;
        Ok(())
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
            if let Some(timer) = self.active_timer.take() {
                if !timer.is_finished() {
                    timer.abort();
                    info!("Cancelled active timeout timer.");
                }
            }
            self.state = BluetoothServiceState::Running;
        } else {
            if self.active_timer.is_none() {
                debug!("No connected devices and no active timer. Starting timeout timer...");
                self.active_timer =
                    Some(TimeoutTask::new(self.timeout, self.service_proxy.clone()).spawn());
            }
            self.state = BluetoothServiceState::Idle;
        }

        Ok(())
    }

    /// Gets the current number of connected devices.
    async fn get_connected_devices_count(&self) -> usize {
        get_connected_devices_count_from_proxy(&self.service_proxy).await
    }
}
