---@type BluetoothTimeoutConfig
local M = {}

----------------------------------------------------------------------
--  Timeout
----------------------------------------------------------------------

-- Duration of inactivity before the Bluetooth adapter is turned off.
-- Format: humantime (e.g. "5m", "30s", "1m30s", "2h").
M.timeout = "5m"

----------------------------------------------------------------------
--  Adapters
----------------------------------------------------------------------

-- Which Bluetooth adapters to manage.
-- find_adapters() discovers all adapters automatically.
-- Pass an optional filter table to narrow the results:
--
--   find_adapters { powered = true }
--   find_adapters { name_pattern = "Dongle" }
--   find_adapters { address_prefix = "00:1A" }
--   find_adapters { powered = true, name = "My Adapter" }
--
-- To hardcode adapters, replace with a table of paths:
--   M.adapters = { { path = "/org/bluez/hci0" } }
M.adapters = find_adapters()

----------------------------------------------------------------------
--  Notifications
----------------------------------------------------------------------

M.notifications = {
  -- Set to false to disable all desktop notifications.
  enabled = true,

  -- Warning notifications are sent at these remaining times before the
  -- adapter is turned off. Add or remove entries as needed.
  at = { "5m", "1m", "30s", "10s" },
}

return M
