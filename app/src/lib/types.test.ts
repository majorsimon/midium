import { describe, it, expect } from "vitest";
import { controlLabel, actionLabel, targetLabel } from "./types";
import type { ControlId, Action, AudioTarget } from "./types";

describe("controlLabel", () => {
  it("labels CC controls", () => {
    const c: ControlId = { device: "dev", channel: 0, control_type: { CC: 7 } };
    expect(controlLabel(c)).toBe("CC 7");
  });

  it("labels Note controls", () => {
    const c: ControlId = { device: "dev", channel: 1, control_type: { Note: 60 } };
    expect(controlLabel(c)).toBe("Note 60");
  });

  it("labels PitchBend controls", () => {
    const c: ControlId = { device: "dev", channel: 0, control_type: "PitchBend" };
    expect(controlLabel(c)).toBe("Pitch Bend");
  });
});

describe("targetLabel", () => {
  it("labels SystemMaster", () => {
    expect(targetLabel("SystemMaster")).toBe("Master");
  });

  it("labels FocusedApplication", () => {
    expect(targetLabel("FocusedApplication")).toBe("Focused App");
  });

  it("labels Device target", () => {
    const t: AudioTarget = { Device: { id: "built-in-speaker" } };
    expect(targetLabel(t)).toBe("built-in-speaker");
  });

  it("labels Application target", () => {
    const t: AudioTarget = { Application: { name: "Spotify" } };
    expect(targetLabel(t)).toBe("Spotify");
  });
});

describe("actionLabel", () => {
  it("labels string actions with camelCase expansion", () => {
    const a: Action = "MediaPlayPause";
    expect(actionLabel(a)).toBe("Media Play Pause");
  });

  it("labels SetVolume actions", () => {
    const a: Action = { SetVolume: { target: "SystemMaster" } };
    expect(actionLabel(a)).toBe("Volume → Master");
  });

  it("labels ToggleMute actions", () => {
    const a: Action = { ToggleMute: { target: "FocusedApplication" } };
    expect(actionLabel(a)).toBe("Mute → Focused App");
  });

  it("labels SetDefaultOutput actions", () => {
    const a: Action = { SetDefaultOutput: { device_id: "hdmi-4" } };
    expect(actionLabel(a)).toBe("Out → hdmi-4");
  });

  it("labels SetDefaultInput actions", () => {
    const a: Action = { SetDefaultInput: { device_id: "mic-1" } };
    expect(actionLabel(a)).toBe("In → mic-1");
  });

  it("labels SendMidiMessage actions", () => {
    const a: Action = {
      SendMidiMessage: {
        device: "nanoKONTROL",
        channel: 0,
        message_type: "cc",
        number: 32,
        value: 127,
      },
    };
    expect(actionLabel(a)).toBe("MIDI out → nanoKONTROL ch0 cc 32=127");
  });

  it("labels CycleOutputDevices", () => {
    const a: Action = { CycleOutputDevices: { device_ids: ["a", "b"] } };
    expect(actionLabel(a)).toBe("Cycle Out (2 devices)");
  });

  it("labels ActionGroup actions", () => {
    const a: Action = {
      ActionGroup: {
        actions: [
          { SetVolume: { target: "SystemMaster" } },
          { ToggleMute: { target: { Application: { name: "Firefox" } } } },
        ],
      },
    };
    expect(actionLabel(a)).toBe("Volume:Master + Mute:Firefox");
  });
});
