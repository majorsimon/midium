-- media_controls.lua
-- Registers custom actions for media transport controls.
-- Assign these to MIDI buttons in the Mapping Editor using
-- Action → Plugin Action → media_controls / play_pause (etc.)

local M = {}

function M.on_load()
  midium.log("media_controls loaded")

  midium.register_action("play_pause", "Toggle media play/pause", function(value)
    midium.log("play/pause triggered (value=" .. tostring(value) .. ")")
    -- Actual media key dispatch is handled by midium-shortcuts in Phase 5.
    -- For now this demonstrates the plugin action system.
  end)

  midium.register_action("next_track", "Skip to next track", function(value)
    midium.log("next track triggered")
  end)

  midium.register_action("prev_track", "Skip to previous track", function(value)
    midium.log("prev track triggered")
  end)
end

function M.on_midi_event(event)
  -- Example: print all incoming MIDI CC messages at trace level
  if event.message.cc then
    -- midium.log("CC " .. event.message.cc.control .. " = " .. event.message.cc.value)
  end
end

function M.on_unload()
  midium.log("media_controls unloaded")
end

return M
