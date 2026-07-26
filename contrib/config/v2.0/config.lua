---@type BluetoothTimeoutConfig
local M = {}

----------------------------------------------------------------------
--  Timeout
----------------------------------------------------------------------

-- Duration of inactivity before the Bluetooth adapter is turned off.
-- Format: humantime (e.g. "5m", "30s", "1m30s", "2h").
M.timeout = "1m"

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

----------------------------------------------------------------------
--  Runtime
----------------------------------------------------------------------

M.runtime = {
  -- Whether to use a multi-threaded tokio runtime.
  --
  -- false (default): Single-threaded. Everything runs on one OS thread
  --   using async concurrency. Lower overhead, ideal for 1-2 adapters.
  --   This is the right choice for virtually all users.
  --
  -- true: Multi-threaded. Work is distributed across a thread pool,
  --   enabling actual parallel execution. Only useful if you manage
  --   many adapters simultaneously (3+).
  multithreaded = false,
}

return M
