<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import type { DeviceProfile, MidiEvent, ProfileControl, ProfileControlType } from "./types";

  /** List of connected MIDI port names — passed in from +page.svelte. */
  export let connectedDevices: string[] = [];

  const dispatch = createEventDispatcher<{
    /** Fired when the user clicks a control; parent navigates to Mappings tab
     *  and pre-fills the form with these values. */
    "open-mapping": {
      device: string;
      channel: number;
      controlTypeName: "CC" | "Note" | "PitchBend";
      controlNumber: number;
      profileControlType?: ProfileControlType;
    };
  }>();

  let profiles: DeviceProfile[] = [];
  let selectedDevice: string | null = null;
  let lastValues: Record<string, number> = {};

  const unlistens: UnlistenFn[] = [];

  function controlKey(channel: number, midiType: string, number: number): string {
    return `${channel}:${midiType}:${number}`;
  }

  function lastValueForControl(ctrl: ProfileControl): number | undefined {
    return lastValues[controlKey(ctrl.channel, ctrl.midi_type ?? "cc", ctrl.number)];
  }

  onMount(async () => {
    profiles = await invoke<DeviceProfile[]>("list_profiles").catch(() => []);
    if (connectedDevices.length > 0) selectedDevice = connectedDevices[0];

    const unlisten = await listen<MidiEvent>("midi-event", (e) => {
      const { channel, message } = e.payload;
      if (message.ControlChange) {
        const { control, value } = message.ControlChange;
        lastValues[controlKey(channel, "cc", control)] = value;
        lastValues = lastValues;
      } else if (message.NoteOn) {
        const { note, velocity } = message.NoteOn;
        lastValues[controlKey(channel, "note", note)] = velocity;
        lastValues = lastValues;
      } else if (message.NoteOff) {
        const { note } = message.NoteOff;
        lastValues[controlKey(channel, "note", note)] = 0;
        lastValues = lastValues;
      }
    });
    unlistens.push(unlisten);
  });

  onDestroy(() => unlistens.forEach(u => u()));

  // Re-select when devices change.
  $: if (connectedDevices.length > 0 && !selectedDevice) {
    selectedDevice = connectedDevices[0];
  } else if (selectedDevice && !connectedDevices.includes(selectedDevice)) {
    selectedDevice = connectedDevices[0] ?? null;
  }

  $: matchedProfile = selectedDevice
    ? profiles.find(p =>
        p.match_patterns.some(pat =>
          selectedDevice!.toLowerCase().includes(pat.toLowerCase())
        )
      ) ?? null
    : null;

  // Group controls by section for the schematic view.
  $: sections = matchedProfile
    ? groupBySectionOrType(matchedProfile.controls)
    : [];

  function groupBySectionOrType(
    controls: ProfileControl[]
  ): { name: string; controls: ProfileControl[] }[] {
    const map = new Map<string, ProfileControl[]>();
    for (const c of controls) {
      const key = c.section ?? c.control_type;
      const list = map.get(key) ?? [];
      list.push(c);
      map.set(key, list);
    }
    // Put known sections first in a logical display order.
    const order = ["Faders", "Knobs", "Encoders", "Pads", "Buttons", "Transport", "Master"];
    const sorted = [...map.entries()].sort(([a], [b]) => {
      const ai = order.indexOf(a);
      const bi = order.indexOf(b);
      if (ai === -1 && bi === -1) return a.localeCompare(b);
      if (ai === -1) return 1;
      if (bi === -1) return -1;
      return ai - bi;
    });
    return sorted.map(([name, controls]) => ({ name, controls }));
  }

  function controlIcon(c: ProfileControl): string {
    switch (c.control_type) {
      case "slider":  return "▮";
      case "knob":    return "◎";
      case "encoder": return "⊛";
      case "button":  return "▪";
    }
  }

  function handleControlClick(c: ProfileControl) {
    if (!selectedDevice) return;
    const midiType = c.midi_type ?? "cc";
    dispatch("open-mapping", {
      device: selectedDevice,
      channel: c.channel,
      controlTypeName: midiType === "note" ? "Note" : midiType === "pitch_bend" ? "PitchBend" : "CC",
      controlNumber: c.number,
      profileControlType: c.control_type,
    });
  }
</script>

<div class="devices-view">
  <!-- Header -->
  <div class="header">
    <div class="header-left">
      <span class="header-title">Devices</span>
      {#if connectedDevices.length > 0}
        <span class="count-badge">{connectedDevices.length}</span>
      {/if}
    </div>
  </div>

  {#if connectedDevices.length === 0}
    <div class="card empty-state">
      No MIDI devices connected.<br>
      Plug in a controller and it will appear here automatically.
    </div>
  {:else}
    <!-- Device selector tabs -->
    <div class="device-tabs">
      {#each connectedDevices as dev}
        <button
          class="device-tab"
          class:active={selectedDevice === dev}
          on:click={() => selectedDevice = dev}
          title={dev}
        >
          <span class="tab-dot" class:matched={profiles.some(p =>
            p.match_patterns.some(pat => dev.toLowerCase().includes(pat.toLowerCase()))
          )}></span>
          <span class="tab-name">{dev}</span>
        </button>
      {/each}
    </div>

    {#if selectedDevice}
      <div class="device-panel">
        {#if matchedProfile}
          <!-- Profile header -->
          <div class="profile-header card">
            <div class="profile-name">
              {matchedProfile.vendor ? `${matchedProfile.vendor} ` : ""}{matchedProfile.model ?? matchedProfile.name}
            </div>
            <div class="profile-meta">
              {matchedProfile.controls.length} controls · profile matched
              {#if matchedProfile.vendor || matchedProfile.model}
                <span class="profile-badge">✓</span>
              {/if}
            </div>
          </div>

          <!-- Schematic: sections of controls -->
          {#if sections.length === 0}
            <div class="card empty-state" style="padding: 16px">
              This profile has no controls defined.
            </div>
          {:else}
            <div class="sections">
              {#each sections as section}
                <div class="section-block card">
                  <div class="section-title">{section.name}</div>
                  <div
                    class="controls-grid"
                    class:grid-wide={section.controls.length > 8}
                  >
                    {#each section.controls as ctrl}
                      <button
                        class="ctrl-tile"
                        class:ctrl-slider={ctrl.control_type === "slider"}
                        class:ctrl-knob={ctrl.control_type === "knob"}
                        class:ctrl-encoder={ctrl.control_type === "encoder"}
                        class:ctrl-button={ctrl.control_type === "button"}
                        on:click={() => handleControlClick(ctrl)}
                        title={`${ctrl.label} — click to add mapping`}
                      >
                        <span class="ctrl-icon">{controlIcon(ctrl)}</span>
                        <span class="ctrl-label">{ctrl.label}</span>
                        <span class="ctrl-sub">
                          {ctrl.midi_type === "note" ? `Note ${ctrl.number}` : `CC ${ctrl.number}`}
                          {ctrl.channel > 0 ? ` ch${ctrl.channel + 1}` : ""}
                        </span>
                        {#if lastValueForControl(ctrl) !== undefined}
                          <span class="ctrl-val">= {lastValueForControl(ctrl)}</span>
                        {/if}
                      </button>
                    {/each}
                  </div>
                </div>
              {/each}
            </div>
          {/if}

        {:else}
          <!-- Unknown device -->
          <div class="card unknown-card">
            <div class="unknown-title">Unknown device</div>
            <div class="unknown-body">
              No profile matched <strong>{selectedDevice}</strong>.<br>
              Use <strong>MIDI Learn</strong> in the Mappings tab to map controls manually,
              or import a profile below.
            </div>
          </div>
        {/if}
      </div>
    {/if}
  {/if}
</div>

<style>
  .devices-view {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: 860px;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 2px 10px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 2px;
  }
  .header-left { display: flex; align-items: center; gap: 8px; }
  .header-title { font-size: 13px; font-weight: 600; color: var(--text); }

  .count-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--accent) 18%, var(--surface2));
    color: var(--accent);
    border-radius: 10px;
    font-size: 10px;
    font-weight: 700;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
  }

  .empty-state {
    text-align: center;
    padding: 32px;
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.7;
    max-width: 420px;
  }

  /* Device tabs */
  .device-tabs {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .device-tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text-muted);
    font-size: 12px;
    cursor: pointer;
    transition: background 0.1s, color 0.1s, border-color 0.1s;
    max-width: 200px;
  }
  .device-tab:hover { background: var(--surface2); color: var(--text); }
  .device-tab.active {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface));
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
    color: var(--accent);
    font-weight: 500;
  }
  .tab-name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tab-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-muted);
    flex-shrink: 0;
    opacity: 0.4;
  }
  .tab-dot.matched {
    background: var(--success);
    opacity: 1;
  }

  /* Device panel */
  .device-panel { display: flex; flex-direction: column; gap: 8px; }

  /* Profile header */
  .profile-header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    padding: 10px 14px;
    gap: 12px;
  }
  .profile-name { font-size: 13px; font-weight: 600; color: var(--text); }
  .profile-meta { font-size: 11px; color: var(--text-muted); }
  .profile-badge {
    display: inline-flex;
    margin-left: 6px;
    color: var(--success);
    font-weight: 700;
  }

  /* Sections */
  .sections { display: flex; flex-direction: column; gap: 8px; }

  .section-block { padding: 10px 14px; }

  .section-title {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
    margin-bottom: 8px;
  }

  .controls-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  /* Control tiles */
  .ctrl-tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-start;
    width: 68px;
    padding: 6px 4px 5px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface2);
    cursor: pointer;
    transition: background 0.1s, border-color 0.1s, transform 0.08s;
    gap: 2px;
  }
  .ctrl-tile:hover {
    background: color-mix(in srgb, var(--accent) 10%, var(--surface2));
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
    transform: translateY(-1px);
  }
  .ctrl-tile:active { transform: translateY(0); }

  .ctrl-icon {
    font-size: 16px;
    line-height: 1.2;
    color: var(--text-muted);
  }
  .ctrl-slider .ctrl-icon { color: var(--accent); }
  .ctrl-knob   .ctrl-icon { color: color-mix(in srgb, var(--accent) 70%, var(--success)); }
  .ctrl-encoder .ctrl-icon { color: var(--success); }
  .ctrl-button  .ctrl-icon { color: var(--text-muted); }

  .ctrl-label {
    font-size: 9px;
    font-weight: 600;
    color: var(--text);
    text-align: center;
    line-height: 1.2;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    width: 100%;
  }
  .ctrl-sub {
    font-size: 8px;
    color: var(--text-muted);
    text-align: center;
    white-space: nowrap;
  }
  .ctrl-val { color: var(--accent); font-weight: 700; }

  /* Unknown device */
  .unknown-card { padding: 20px 24px; }
  .unknown-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
    margin-bottom: 8px;
  }
  .unknown-body {
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.7;
  }
</style>
