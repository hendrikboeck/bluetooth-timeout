// -- module definitions
/// D-Bus constants for `BlueZ` interfaces.
pub mod constants;
/// Represents a Bluetooth device and its properties.
pub mod device;
/// Observes D-Bus signals and broadcasts Bluetooth events.
pub mod observer;
/// Manages Bluetooth adapter state and timeout logic.
pub mod service;
/// D-Bus proxy for `BlueZ` adapter operations.
pub mod service_proxy;
