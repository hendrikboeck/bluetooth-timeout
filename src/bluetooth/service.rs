// -- std imports
use std::time::Duration;

// -- crate imports
use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

// -- module imports
use crate::{
    bluetooth::{
        observer::BluetoothEvent,
        service_proxy::{AdapterProxy, BluetoothServiceProxy},
    },
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
pub struct BluetoothService<P: AdapterProxy = BluetoothServiceProxy> {
    /// The Bluetooth interface name (e.g., "hci0").
    pub iface: String,
    /// Proxy to interact with the Bluetooth service via D-Bus.
    service_proxy: P,
    /// Current state of the Bluetooth service.
    pub state: BluetoothServiceState,
    /// Handle to the active timeout timer task, if any.
    pub active_timer: Option<tokio::task::JoinHandle<()>>,
    /// Duration before the timeout triggers.
    timeout: Duration,
    /// Notification configuration, cloned to each spawned [`TimeoutTask`].
    notification_conf: NotificationConf,
}

/// State management, event handling, and timeout lifecycle.
impl BluetoothService<BluetoothServiceProxy> {
    /// Creates a new `BluetoothService` backed by a real D-Bus proxy.
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
        Self::with_proxy(iface, timeout, notification_conf, service_proxy).await
    }
}

impl<P: AdapterProxy> BluetoothService<P> {
    /// Creates a `BluetoothService` from an existing proxy.
    ///
    /// Determines the initial state and starts a timeout timer if the adapter is idle.
    pub async fn with_proxy(
        iface: String,
        timeout: Duration,
        notification_conf: NotificationConf,
        service_proxy: P,
    ) -> Result<Self> {
        let num_connected_devices = service_proxy.get_connected_devices_count().await;
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

        if self.get_connected_devices_count().await > 0 {
            if let Some(timer) = self.active_timer.take()
                && !timer.is_finished()
            {
                timer.abort();
                info!("Cancelled active timeout timer.");
            }
            self.state = BluetoothServiceState::Running;
        } else {
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
        self.service_proxy.get_connected_devices_count().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::future::BoxFuture;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    /// Mock adapter proxy backed by atomics, so tests can flip powered/connected state on the fly.
    #[derive(Clone, Default)]
    struct MockProxy {
        powered: Arc<AtomicBool>,
        connected: Arc<AtomicUsize>,
        turned_off: Arc<AtomicBool>,
    }

    impl MockProxy {
        fn new(powered: bool, connected: usize) -> Self {
            Self {
                powered: Arc::new(AtomicBool::new(powered)),
                connected: Arc::new(AtomicUsize::new(connected)),
                turned_off: Arc::new(AtomicBool::new(false)),
            }
        }

        fn set_connected(&self, n: usize) {
            self.connected.store(n, Ordering::SeqCst);
        }

        fn is_turned_off(&self) -> bool {
            self.turned_off.load(Ordering::SeqCst)
        }
    }

    impl AdapterProxy for MockProxy {
        fn is_powered(&self) -> BoxFuture<'_, Result<bool>> {
            Box::pin(async move { Ok(self.powered.load(Ordering::SeqCst)) })
        }

        fn get_connected_devices_count(&self) -> BoxFuture<'_, usize> {
            Box::pin(async move { self.connected.load(Ordering::SeqCst) })
        }

        fn turn_off_adapter(&self) -> BoxFuture<'_, Result<()>> {
            Box::pin(async move {
                self.turned_off.store(true, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    fn notif_disabled() -> NotificationConf {
        NotificationConf {
            enabled: false,
            at: vec![],
        }
    }

    async fn service(mock: MockProxy) -> BluetoothService<MockProxy> {
        BluetoothService::with_proxy(
            "/org/bluez/hci0".to_string(),
            Duration::from_mins(5),
            notif_disabled(),
            mock,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn off_when_not_powered() {
        let s = service(MockProxy::new(false, 0)).await;
        assert_eq!(s.state, BluetoothServiceState::Off);
        assert!(s.active_timer.is_none());
    }

    #[tokio::test]
    async fn idle_starts_timer() {
        let s = service(MockProxy::new(true, 0)).await;
        assert_eq!(s.state, BluetoothServiceState::Idle);
        assert!(s.active_timer.is_some());
    }

    #[tokio::test]
    async fn running_when_devices_connected() {
        let s = service(MockProxy::new(true, 2)).await;
        assert_eq!(s.state, BluetoothServiceState::Running);
        assert!(s.active_timer.is_none());
    }

    #[tokio::test]
    async fn errors_when_powered_off_but_connected() {
        let err = BluetoothService::with_proxy(
            "/org/bluez/hci0".to_string(),
            Duration::from_mins(5),
            notif_disabled(),
            MockProxy::new(false, 1),
        )
        .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn adapter_on_from_off_starts_timer_when_idle() {
        let mock = MockProxy::new(false, 0);
        let mut s = service(mock.clone()).await;
        s.on_adapter_on().await.unwrap();
        assert_eq!(s.state, BluetoothServiceState::Idle);
        assert!(s.active_timer.is_some());
    }

    #[tokio::test]
    async fn adapter_on_from_off_with_devices_goes_running() {
        let mock = MockProxy::new(false, 0);
        let mut s = service(mock.clone()).await;
        mock.set_connected(1);
        s.on_adapter_on().await.unwrap();
        assert_eq!(s.state, BluetoothServiceState::Running);
        assert!(s.active_timer.is_none());
    }

    #[tokio::test]
    async fn adapter_off_cancels_timer() {
        let mut s = service(MockProxy::new(true, 0)).await;
        assert!(s.active_timer.is_some());
        s.on_adapter_off();
        assert_eq!(s.state, BluetoothServiceState::Off);
        assert!(s.active_timer.is_none());
    }

    #[tokio::test]
    async fn interface_added_cancels_timer_and_goes_running() {
        let mock = MockProxy::new(true, 0);
        let mut s = service(mock.clone()).await;
        assert!(s.active_timer.is_some());
        mock.set_connected(1);
        s.on_interface_added().await.unwrap();
        assert_eq!(s.state, BluetoothServiceState::Running);
        assert!(s.active_timer.is_none());
    }

    #[tokio::test]
    async fn interface_removed_restarts_timer_when_no_devices() {
        let mock = MockProxy::new(true, 1);
        let mut s = service(mock.clone()).await;
        assert!(s.active_timer.is_none());
        mock.set_connected(0);
        s.on_interface_removed().await.unwrap();
        assert_eq!(s.state, BluetoothServiceState::Idle);
        assert!(s.active_timer.is_some());
    }

    #[tokio::test]
    async fn interface_removed_does_not_duplicate_timer() {
        let mut s = service(MockProxy::new(true, 0)).await;
        assert!(s.active_timer.is_some());
        s.on_interface_removed().await.unwrap();
        assert_eq!(s.state, BluetoothServiceState::Idle);
        assert!(s.active_timer.is_some());
    }

    #[tokio::test]
    async fn timeout_task_turns_off_adapter() {
        let mock = MockProxy::new(true, 0);
        let mut s = BluetoothService::with_proxy(
            "/org/bluez/hci0".to_string(),
            Duration::from_millis(20),
            notif_disabled(),
            mock.clone(),
        )
        .await
        .unwrap();
        assert_eq!(s.state, BluetoothServiceState::Idle);
        let timer = s.active_timer.take().unwrap();
        timer.await.unwrap();
        assert!(mock.is_turned_off());
    }
}
