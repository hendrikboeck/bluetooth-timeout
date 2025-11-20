use std::time::Duration;

use anyhow::Result;
use tokio::{sync::broadcast, time::sleep};
use tracing::{debug, info};

use crate::bluetooth::{observer::BluetoothEvent, service_proxy::BluetoothServiceProxy};

pub const BLUEZ_SERVICE: &str = "org.bluez";
pub const BLUEZ_ADAPTER_INTERFACE: &str = "org.bluez.Adapter1";
pub const BLUEZ_DEVICE_INTERFACE: &str = "org.bluez.Device1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BluetoothServiceState {
    Off,
    Idle,
    Running,
}

#[derive(Debug)]
pub struct BluetoothService {
    pub iface: String,
    rx: Option<broadcast::Receiver<BluetoothEvent>>,
    service_proxy: BluetoothServiceProxy,
    pub state: BluetoothServiceState,
    pub active_timer: Option<tokio::task::JoinHandle<()>>,
    timeout: Duration,
}

async fn get_connected_devices_count_from_proxy(proxy: &BluetoothServiceProxy) -> Result<usize> {
    let devices = proxy.get_devices().await?;
    let connected_count = devices.iter().filter(|dev| dev.connected).count();
    Ok(connected_count)
}

async fn timeout_task(timeout: Duration, service_proxy: BluetoothServiceProxy) {
    sleep(timeout).await;
    service_proxy
        .turn_off_adapter()
        .await
        .expect("Could not turn off adapter");
    info!("Adapter turned off.");
}

impl BluetoothService {
    pub async fn new(iface: String, timeout: Duration) -> Result<Self> {
        let service_proxy = BluetoothServiceProxy::new(iface.clone()).await?;
        let num_connected_devices = get_connected_devices_count_from_proxy(&service_proxy).await?;
        let powered = service_proxy.is_powered().await?;
        let state = match (powered, num_connected_devices) {
            (false, _) => BluetoothServiceState::Off,
            (true, 0) => BluetoothServiceState::Idle,
            (true, devs) if devs > 0 => BluetoothServiceState::Running,
            _ => {
                return Err(anyhow::anyhow!(
                    "Could not determine BluetoothService state"
                ));
            }
        };
        info!("Initial BluetoothService state: {:#?}", state);

        let service = Self {
            iface,
            rx: None,
            service_proxy,
            state,
            active_timer: None,
            timeout,
        };
        debug!("Created new BluetoothService: {:#?}", service);

        Ok(service)
    }

    pub fn subscribe_to(&mut self, rx: broadcast::Receiver<BluetoothEvent>) -> &mut Self {
        self.rx = Some(rx);
        return self;
    }

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
                BluetoothEvent::AdapterOn => self.on_adapter_on().await?,
                BluetoothEvent::AdapterOff => self.on_adapter_off().await?,
                BluetoothEvent::InterfaceAdded => self.on_interface_added().await?,
                BluetoothEvent::InterfaceRemoved => self.on_interface_removed().await?,
            }
        }
    }

    pub async fn on_adapter_on(&mut self) -> Result<()> {
        debug!("Handling AdapterOn event...");

        match self.state {
            BluetoothServiceState::Off | BluetoothServiceState::Idle
                if self.active_timer.is_none() =>
            {
                self.active_timer = Some({
                    let timeout = self.timeout;
                    let service_proxy = self.service_proxy.clone();

                    tokio::spawn(async move {
                        timeout_task(timeout, service_proxy).await;
                        info!("Adapter turned off.");
                    })
                });
                info!("Adapter turned ON from OFF state. Started timeout timer.");
            }
            BluetoothServiceState::Running if self.active_timer.is_some() => {
                self.active_timer.take().unwrap().abort();
                info!("Adapter is already RUNNING. Cancelled timeout timer.");
            }
            _ => {
                info!("Adapter is already ON and no timer found. Nothing to do.");
            }
        }

        if self.get_connected_devices_count().await? > 0 {
            self.state = BluetoothServiceState::Running;
        } else {
            self.state = BluetoothServiceState::Idle;
        }

        Ok(())
    }

    pub async fn on_adapter_off(&mut self) -> Result<()> {
        debug!("Handling AdapterOff event...");

        if let Some(timer) = self.active_timer.take() {
            timer.abort();
            info!("Cancelled active timeout timer.");
        }

        self.state = BluetoothServiceState::Off;
        info!("Adapter turned OFF. Updated state to OFF. Canceled any active timers.");

        Ok(())
    }

    pub async fn on_interface_added(&mut self) -> Result<()> {
        debug!("Handling InterfaceAdded event...");

        self.on_interface_changed().await
    }

    pub async fn on_interface_removed(&mut self) -> Result<()> {
        debug!("Handling InterfaceRemoved event...");

        self.on_interface_changed().await
    }

    async fn on_interface_changed(&mut self) -> Result<()> {
        if self.get_connected_devices_count().await? > 0 {
            if let Some(timer) = self.active_timer.take() {
                timer.abort();
                info!("Cancelled active timeout timer.");
            }
            info!("Device connected. Updated state to RUNNING. Canceled any active timers.");
            self.state = BluetoothServiceState::Running;
        } else {
            self.state = BluetoothServiceState::Idle;
        }

        Ok(())
    }

    async fn get_connected_devices_count(&self) -> Result<usize> {
        get_connected_devices_count_from_proxy(&self.service_proxy).await
    }
}
