-- ╔══════════════════════════════════════════════════════════════════╗
-- ║                     LSP TYPE ANNOTATIONS                        ║
-- ║  These provide editor autocompletion & type checking.           ║
-- ║  They are Lua comments — editing them has no runtime effect.    ║
-- ╚══════════════════════════════════════════════════════════════════╝

--- A Bluetooth adapter discovered on the system.
---
--- Returned by `find_adapters()`.
---@class BluetoothAdapter
---@field path          string  D-Bus object path (e.g. "/org/bluez/hci0")
---@field address       string  MAC address       (e.g. "00:1A:7D:DA:71:13")
---@field name          string  Alias / display name
---@field powered       boolean Whether the adapter is currently powered on
---@field discoverable  boolean Whether the adapter is discoverable

--- Optional filter passed to `find_adapters()`.
---
--- All keys are optional. Omitted keys are not filtered.
---@class AdapterFilter
---@field name?             string  Exact match on adapter name
---@field name_pattern?     string  Lua pattern match on adapter name
---@field address?          string  Exact MAC address match
---@field address_prefix?   string  MAC address prefix match
---@field powered?          boolean Filter by powered state
---@field discoverable?     boolean Filter by discoverable state

--- Top-level configuration table.
---@class BluetoothTimeoutConfig
---@field version        string                Config schema version (e.g. "2")
---@field timeout        string                Inactivity duration in humantime format
---@field adapters       BluetoothAdapter[]    Adapters to manage
---@field notifications  NotificationsConfig
---@field runtime        RuntimeConfig

---@class NotificationsConfig
---@field enabled boolean Whether desktop notifications are enabled
---@field at     string[] Warning intervals (humantime)

---@class RuntimeConfig
---@field multithreaded boolean Use multi-threaded tokio runtime

--- Discover Bluetooth adapters.
---
--- With no argument, returns *all* adapters on the system.
--- Pass an optional `AdapterFilter` to narrow the results.
---
---@param filter? AdapterFilter
---@return BluetoothAdapter[]
function find_adapters(filter) end

return {}
