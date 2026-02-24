export interface AudioDeviceInfo {
  id: string;
  name: string;
  is_default: boolean;
  is_input: boolean;
}

export interface AudioSessionInfo {
  name: string;
  pid: number | null;
  volume: number;
  muted: boolean;
}

export interface AudioCapabilities {
  per_app_volume: boolean;
  device_switching: boolean;
  input_device_switching: boolean;
}

export interface ControlId {
  device: string;
  channel: number;
  control_type: ControlType;
}

export type ControlType =
  | { CC: number }
  | { Note: number }
  | "PitchBend";

export type AudioTarget =
  | "SystemMaster"
  | { Device: { id: string } }
  | { Application: { name: string } }
  | "FocusedApplication";

export type ValueTransform =
  | "Linear"
  | "Logarithmic"
  | { RelativeEncoder: { sensitivity: number } }
  | "Toggle"
  | "Momentary";

export type Action =
  | { SetVolume: { target: AudioTarget } }
  | { ToggleMute: { target: AudioTarget } }
  | { SetDefaultOutput: { device_id: string } }
  | { SetDefaultInput: { device_id: string } }
  | "CycleOutputDevices"
  | "CycleInputDevices"
  | "MediaPlayPause"
  | "MediaNext"
  | "MediaPrev"
  | { ActionGroup: { actions: Action[] } };

export interface Mapping {
  control: ControlId;
  action: Action;
  transform: ValueTransform;
}

export interface MidiMessage {
  ControlChange?: { control: number; value: number };
  NoteOn?: { note: number; velocity: number };
  NoteOff?: { note: number; velocity: number };
  PitchBend?: { value: number };
}

export interface MidiEvent {
  device: string;
  channel: number;
  message: MidiMessage;
}

// Helper: human-readable label for a ControlId
export function controlLabel(c: ControlId): string {
  const ct = c.control_type;
  if (typeof ct === "object" && "CC" in ct) return `CC ${ct.CC}`;
  if (typeof ct === "object" && "Note" in ct) return `Note ${ct.Note}`;
  return "Pitch Bend";
}

// Helper: human-readable label for an Action
export function actionLabel(a: Action): string {
  if (typeof a === "string") return a.replace(/([A-Z])/g, " $1").trim();
  if ("SetVolume" in a) return `Volume → ${targetLabel(a.SetVolume.target)}`;
  if ("ToggleMute" in a) return `Mute → ${targetLabel(a.ToggleMute.target)}`;
  if ("SetDefaultOutput" in a) return `Out → ${a.SetDefaultOutput.device_id}`;
  if ("ActionGroup" in a) return `Group (${a.ActionGroup.actions.length})`;
  return JSON.stringify(a);
}

export function targetLabel(t: AudioTarget): string {
  if (t === "SystemMaster") return "Master";
  if (t === "FocusedApplication") return "Focused App";
  if (typeof t === "object" && "Device" in t) return t.Device.id;
  if (typeof t === "object" && "Application" in t) return t.Application.name;
  return String(t);
}
