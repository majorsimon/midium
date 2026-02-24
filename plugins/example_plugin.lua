-- example_plugin.lua
-- Demonstrates the full midium plugin API.
-- Two patterns are supported:
--   1. Return a module table (shown here)
--   2. Define global on_load / on_midi_event / on_unload functions

local M = {}

function M.on_load()
  midium.log("example_plugin loaded")

  -- Read current master volume and store it
  local vol = midium.audio.get_volume("master")
  midium.state.set("startup_volume", tostring(vol))
  midium.log("Startup master volume: " .. tostring(vol))

  -- Register a custom action: boost volume to 80%
  midium.register_action("boost_volume", "Set master volume to 80%", function(value)
    midium.audio.set_volume("master", 0.8)
    midium.log("Boosted master volume to 80%")
  end)

  -- Register a custom action: restore startup volume
  midium.register_action("restore_volume", "Restore startup volume", function(value)
    local saved = midium.state.get("startup_volume")
    if saved then
      midium.audio.set_volume("master", tonumber(saved))
      midium.log("Restored volume to " .. saved)
    end
  end)
end

function M.on_midi_event(event)
  -- Example: print note-on events
  if event.message.note and event.message.note.on then
    midium.log(
      "Note ON: " .. tostring(event.message.note.note) ..
      " velocity=" .. tostring(event.message.note.velocity) ..
      " device=" .. event.device
    )
  end
end

function M.on_unload()
  midium.log("example_plugin unloaded")
end

return M
