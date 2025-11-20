use std::time::Duration;

use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use crate::{
    bluetooth::{observer::BluetoothEvent, service_proxy::BluetoothServiceProxy},
    timeout::TimeoutTask,
};

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
            if !timer.is_finished() {
                timer.abort();
                info!("Cancelled active timeout timer.");
            }
        }

        self.state = BluetoothServiceState::Off;
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
        let connected_devices = self.get_connected_devices_count().await?;
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

    async fn get_connected_devices_count(&self) -> Result<usize> {
        get_connected_devices_count_from_proxy(&self.service_proxy).await
    }
}
