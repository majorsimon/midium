<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import {
    type Mapping, type ControlId, type MidiEvent,
    type AudioDeviceInfo, type AudioSessionInfo,
    controlLabel, actionLabel,
  } from "./types";

  import type { ProfileControlType } from "./types";

  
  interface Props {
    /** When set by the Devices tab, opens the Add form pre-filled with this control. */
    prefill?: {
    device: string;
    channel: number;
    controlTypeName: "CC" | "Note" | "PitchBend";
    controlNumber: number;
    profileControlType?: ProfileControlType;
  } | null;
  }

  let { prefill = $bindable(null) }: Props = $props();

  let mappings: Mapping[] = $state([]);
  let midiPorts: string[] = $state([]);
  let outputDevices: AudioDeviceInfo[] = $state([]);
  let inputDevices: AudioDeviceInfo[] = $state([]);
  let sessions: AudioSessionInfo[] = $state([]);
  let learnActive = $state(false);
  let showAddForm = $state(false);
  let pendingDeleteKey: string | null = $state(null);
  let unlistens: UnlistenFn[] = [];

  function controlKey(c: ControlId): string {
    return `${c.device}|${c.channel}|${JSON.stringify(c.control_type)}`;
  }

  let form = $state({
    device: "",
    channel: 0,
    controlTypeName: "CC" as "CC" | "Note" | "PitchBend",
    controlNumber: 0,
    actionTypeName: "SetVolume" as string,
    actionTargets: ["SystemMaster"] as string[],
    transformName: "Linear" as string,
    encoderSensitivity: 0.01,
    midiOutDevice: "",
    midiOutChannel: 0,
    midiOutType: "cc" as "cc" | "note",
    midiOutNumber: 0,
    midiOutValue: 127,
  });


  async function loadMappings() {
    mappings = await invoke<Mapping[]>("get_mappings").catch(() => [] as Mapping[]);
    pendingDeleteKey = null;
  }

  async function loadPorts() {
    midiPorts = await invoke<string[]>("list_midi_ports").catch(() => [] as string[]);
  }

  async function loadAudioTargets() {
    [outputDevices, inputDevices, sessions] = await Promise.all([
      invoke<AudioDeviceInfo[]>("list_output_devices").catch(() => [] as AudioDeviceInfo[]),
      invoke<AudioDeviceInfo[]>("list_input_devices").catch(() => [] as AudioDeviceInfo[]),
      invoke<AudioSessionInfo[]>("list_sessions").catch(() => [] as AudioSessionInfo[]),
    ]);
  }

  async function startLearn() {
    learnActive = true;
    await invoke("start_midi_learn").catch(console.error);
  }

  async function cancelLearn() {
    learnActive = false;
    await invoke("cancel_midi_learn").catch(console.error);
  }

  function applyLearnedControl(event: MidiEvent) {
    learnActive = false;
    form.device = event.device;
    form.channel = event.channel;

    const msg = event.message;
    if (msg.ControlChange !== undefined) {
      form.controlTypeName = "CC";
      form.controlNumber = msg.ControlChange.control;
    } else if (msg.NoteOn !== undefined) {
      form.controlTypeName = "Note";
      form.controlNumber = msg.NoteOn.note;
    } else if (msg.NoteOff !== undefined) {
      form.controlTypeName = "Note";
      form.controlNumber = msg.NoteOff.note;
    } else {
      form.controlTypeName = "PitchBend";
      form.controlNumber = 0;
    }
    showAddForm = true;
  }

  function buildControlId(): ControlId {
    const ct =
      form.controlTypeName === "CC"        ? { CC: form.controlNumber }
      : form.controlTypeName === "Note"    ? { Note: form.controlNumber }
      : "PitchBend" as const;
    return { device: form.device, channel: form.channel, control_type: ct };
  }

  function resolveTarget(t: string) {
    if (t === "SystemMaster") return "SystemMaster" as const;
    if (t === "FocusedApplication") return "FocusedApplication" as const;
    if (t.startsWith("app:")) return { Application: { name: t.slice(4) } };
    if (t.startsWith("device:")) return { Device: { id: t.slice(7) } };
    return "SystemMaster" as const;
  }

  /** Human-readable label for an encoded target string (for tag display). */
  function targetDisplayLabel(t: string): string {
    if (t === "SystemMaster") return "System Master";
    if (t === "FocusedApplication") return "Focused App";
    if (t.startsWith("app:")) return t.slice(4);
    if (t.startsWith("device:")) {
      const id = t.slice(7);
      return [...outputDevices, ...inputDevices].find(d => d.id === id)?.name ?? id;
    }
    return t;
  }

  function buildAction() {
    switch (form.actionTypeName) {
      case "SetVolume": {
        const targets = form.actionTargets.length > 0 ? form.actionTargets : ["SystemMaster"];
        if (targets.length === 1) return { SetVolume: { target: resolveTarget(targets[0]) } };
        return { ActionGroup: { actions: targets.map(t => ({ SetVolume: { target: resolveTarget(t) } })) } };
      }
      case "ToggleMute": {
        const targets = form.actionTargets.length > 0 ? form.actionTargets : ["SystemMaster"];
        if (targets.length === 1) return { ToggleMute: { target: resolveTarget(targets[0]) } };
        return { ActionGroup: { actions: targets.map(t => ({ ToggleMute: { target: resolveTarget(t) } })) } };
      }
      case "SetDefaultOutput": {
        const raw = form.actionTargets[0] ?? "";
        const device_id = raw.startsWith("device:") ? raw.slice(7) : raw;
        return { SetDefaultOutput: { device_id } };
      }
      case "SetDefaultInput": {
        const raw = form.actionTargets[0] ?? "";
        const device_id = raw.startsWith("device:") ? raw.slice(7) : raw;
        return { SetDefaultInput: { device_id } };
      }
      case "CycleOutputDevices": {
        const ids = form.actionTargets
          .filter(t => t.startsWith("device:"))
          .map(t => t.slice(7));
        return { CycleOutputDevices: { device_ids: ids.length > 0 ? ids : null } };
      }
      case "CycleInputDevices": {
        const ids = form.actionTargets
          .filter(t => t.startsWith("device:"))
          .map(t => t.slice(7));
        return { CycleInputDevices: { device_ids: ids.length > 0 ? ids : null } };
      }
      case "MediaPlayPause": return "MediaPlayPause";
      case "MediaNext":      return "MediaNext";
      case "MediaPrev":      return "MediaPrev";
      case "SendMidiMessage":
        return {
          SendMidiMessage: {
            device: form.midiOutDevice,
            channel: form.midiOutChannel,
            message_type: form.midiOutType,
            number: form.midiOutNumber,
            value: form.midiOutValue,
          },
        };
      default: return { SetVolume: { target: "SystemMaster" as const } };
    }
  }

  /** Add a target from the dropdown; reset the select back to placeholder. */
  function onAddTarget(e: Event) {
    const sel = e.target as HTMLSelectElement;
    const val = sel.value;
    if (val && !form.actionTargets.includes(val)) {
      form.actionTargets = [...form.actionTargets, val];
    }
    sel.value = "";
  }

  function removeTarget(i: number) {
    form.actionTargets = form.actionTargets.filter((_, j) => j !== i);
  }

  function buildTransform(): Mapping["transform"] {
    if (form.transformName === "RelativeEncoder") {
      return { RelativeEncoder: { sensitivity: form.encoderSensitivity } };
    }
    return form.transformName as "Linear" | "Logarithmic" | "Toggle" | "Momentary";
  }

  async function saveMapping() {
    const mapping: Mapping = {
      control: buildControlId(),
      action: buildAction() as Mapping["action"],
      transform: buildTransform(),
    };
    await invoke("save_mapping", { mapping }).catch(console.error);
    showAddForm = false;
    await loadMappings();
  }

  async function deleteMapping(control: ControlId) {
    pendingDeleteKey = null;
    await invoke("delete_mapping", { control }).catch(console.error);
    await loadMappings();
  }

  // Transform human-readable display
  function transformLabel(t: Mapping["transform"]): string {
    if (typeof t === "string") return t;
    if ("RelativeEncoder" in t) return `Encoder ×${t.RelativeEncoder.sensitivity}`;
    return JSON.stringify(t);
  }

  onMount(async () => {
    await Promise.all([loadMappings(), loadPorts(), loadAudioTargets()]);
    unlistens.push(
      await listen<MidiEvent>("midi-learn-result", (e) => {
        applyLearnedControl(e.payload);
      }),
      await listen("preset-loaded", async () => {
        await loadMappings();
      }),
    );
  });

  onDestroy(() => unlistens.forEach((u) => u()));
  $effect(() => {
    if (prefill) {
      form.device = prefill.device;
      form.channel = prefill.channel;
      form.controlTypeName = prefill.controlTypeName;
      form.controlNumber = prefill.controlNumber;
      // Auto-select RelativeEncoder transform for encoder controls
      if (prefill.profileControlType === "encoder") {
        form.transformName = "RelativeEncoder";
        form.encoderSensitivity = 0.01;
      }
      showAddForm = true;
      loadAudioTargets();
      prefill = null; // consume it
    }
  });
</script>

<div class="editor">
  <!-- Header bar (matches mixer/settings header pattern) -->
  <div class="header">
    <div class="header-left">
      <span class="header-title">Mappings</span>
      {#if mappings.length > 0}
        <span class="count-badge">{mappings.length}</span>
      {/if}
    </div>
    <div class="header-right">
      {#if learnActive}
        <span class="learn-badge">
          <span class="dot-pulse"></span>
          Waiting for MIDI…
        </span>
        <button onclick={cancelLearn}>Cancel</button>
      {:else}
        <button onclick={startLearn}>MIDI Learn</button>
      {/if}
      <button class="primary" onclick={() => {
        showAddForm = !showAddForm;
        if (showAddForm) loadAudioTargets();
      }}>
        {showAddForm ? "Cancel" : "+ Add"}
      </button>
    </div>
  </div>

  <!-- Add / edit form -->
  {#if showAddForm}
    <div class="card form-card">
      <div class="form-title">
        {learnActive ? "Configure Learned Control" : "New Mapping"}
      </div>

      <!-- Two-panel layout: MIDI Control | Action -->
      <div class="form-panels">
        <div class="form-panel">
          <div class="panel-label">MIDI Control</div>
          <div class="field">
            <label>Device</label>
            {#if midiPorts.length > 0}
              <select bind:value={form.device}>
                {#each midiPorts as port}
                  <option value={port}>{port}</option>
                {/each}
                <option value="">Custom…</option>
              </select>
              {#if form.device === ""}
                <input bind:value={form.device} placeholder="Device name" style="margin-top:4px;" />
              {/if}
            {:else}
              <input bind:value={form.device} placeholder="nanoKONTROL2" />
            {/if}
          </div>
          <div class="field-row">
            <div class="field">
              <label>Ch</label>
              <input type="number" min="0" max="15" bind:value={form.channel} />
            </div>
            <div class="field">
              <label>Type</label>
              <select bind:value={form.controlTypeName}>
                <option>CC</option>
                <option>Note</option>
                <option>PitchBend</option>
              </select>
            </div>
            {#if form.controlTypeName !== "PitchBend"}
              <div class="field">
                <label>#</label>
                <input type="number" min="0" max="127" bind:value={form.controlNumber} />
              </div>
            {/if}
          </div>
        </div>

        <div class="form-arrow">→</div>

        <div class="form-panel">
          <div class="panel-label">Action</div>
          <div class="field">
            <label>Action</label>
            <select bind:value={form.actionTypeName}>
              <optgroup label="Volume">
                <option value="SetVolume">Set Volume</option>
                <option value="ToggleMute">Toggle Mute</option>
              </optgroup>
              <optgroup label="Devices">
                <option value="SetDefaultOutput">Set Default Output</option>
                <option value="SetDefaultInput">Set Default Input</option>
                <option value="CycleOutputDevices">Cycle Output Devices</option>
                <option value="CycleInputDevices">Cycle Input Devices</option>
              </optgroup>
              <optgroup label="Media">
                <option value="MediaPlayPause">Play / Pause</option>
                <option value="MediaNext">Next Track</option>
                <option value="MediaPrev">Previous Track</option>
              </optgroup>
              <optgroup label="MIDI">
                <option value="SendMidiMessage">Send MIDI Message</option>
              </optgroup>
            </select>
          </div>
          {#if form.actionTypeName === "SetDefaultOutput" || form.actionTypeName === "SetDefaultInput"}
            <div class="field">
              <label>Device</label>
              <select
                value={form.actionTargets[0] ?? ""}
                onchange={(e) => { form.actionTargets = [e.currentTarget.value]; }}
              >
                <option value="" disabled>Select device…</option>
                {#if form.actionTypeName === "SetDefaultOutput" && outputDevices.length > 0}
                  {#each outputDevices as dev}
                    <option value="device:{dev.id}">{dev.name}{dev.is_default ? " ✓" : ""}</option>
                  {/each}
                {/if}
                {#if form.actionTypeName === "SetDefaultInput" && inputDevices.length > 0}
                  {#each inputDevices as dev}
                    <option value="device:{dev.id}">{dev.name}{dev.is_default ? " ✓" : ""}</option>
                  {/each}
                {/if}
              </select>
            </div>
          {/if}

          {#if form.actionTypeName === "SendMidiMessage"}
            <div class="field">
              <label>Output port</label>
              {#if midiPorts.length > 0}
                <select bind:value={form.midiOutDevice}>
                  <option value="" disabled>Select MIDI port…</option>
                  {#each midiPorts as port}
                    <option value={port}>{port}</option>
                  {/each}
                </select>
              {:else}
                <input bind:value={form.midiOutDevice} placeholder="MIDI output port name" />
              {/if}
            </div>
            <div class="field-row">
              <div class="field">
                <label>Ch</label>
                <input type="number" min="0" max="15" bind:value={form.midiOutChannel} />
              </div>
              <div class="field">
                <label>Type</label>
                <select bind:value={form.midiOutType}>
                  <option value="cc">CC</option>
                  <option value="note">Note On</option>
                </select>
              </div>
              <div class="field">
                <label>#</label>
                <input type="number" min="0" max="127" bind:value={form.midiOutNumber} />
              </div>
              <div class="field">
                <label>Value</label>
                <input type="number" min="0" max="127" bind:value={form.midiOutValue} />
              </div>
            </div>
          {/if}

          {#if form.actionTypeName === "SetVolume" || form.actionTypeName === "ToggleMute"}
            <div class="field">
              <label>Targets</label>

              {#if form.actionTargets.length > 0}
                <div class="target-tags">
                  {#each form.actionTargets as t, i}
                    <span class="target-tag">
                      {targetDisplayLabel(t)}
                      <button class="tag-remove" onclick={() => removeTarget(i)} title="Remove">×</button>
                    </span>
                  {/each}
                </div>
              {/if}

              <select onchange={onAddTarget}>
                <option value="" disabled selected>Add target…</option>
                <option
                  value="SystemMaster"
                  disabled={form.actionTargets.includes("SystemMaster")}
                >System Master</option>

                {#if outputDevices.length > 0}
                  <optgroup label="Output Devices">
                    {#each outputDevices as dev}
                      <option
                        value="device:{dev.id}"
                        disabled={form.actionTargets.includes(`device:${dev.id}`)}
                      >{dev.name}{dev.is_default ? " ✓" : ""}</option>
                    {/each}
                  </optgroup>
                {/if}

                {#if inputDevices.length > 0}
                  <optgroup label="Input Devices">
                    {#each inputDevices as dev}
                      <option
                        value="device:{dev.id}"
                        disabled={form.actionTargets.includes(`device:${dev.id}`)}
                      >{dev.name}{dev.is_default ? " ✓" : ""}</option>
                    {/each}
                  </optgroup>
                {/if}

                {#if sessions.length > 0}
                  <optgroup label="Running Apps">
                    {#each sessions as s}
                      <option
                        value="app:{s.name}"
                        disabled={form.actionTargets.includes(`app:${s.name}`)}
                      >{s.name}</option>
                    {/each}
                  </optgroup>
                {/if}

                <optgroup label="Other">
                  <option
                    value="FocusedApplication"
                    disabled={form.actionTargets.includes("FocusedApplication")}
                  >Focused App (active window)</option>
                </optgroup>
              </select>
            </div>
          {/if}
          {#if form.actionTypeName === "CycleOutputDevices" || form.actionTypeName === "CycleInputDevices"}
            <div class="field">
              <label>Devices to cycle (leave empty for all)</label>

              {#if form.actionTargets.length > 0}
                <div class="target-tags">
                  {#each form.actionTargets as t, i}
                    <span class="target-tag">
                      {targetDisplayLabel(t)}
                      <button class="tag-remove" onclick={() => removeTarget(i)} title="Remove">×</button>
                    </span>
                  {/each}
                </div>
              {/if}

              <select onchange={onAddTarget}>
                <option value="" disabled selected>Add device…</option>
                {#if form.actionTypeName === "CycleOutputDevices"}
                  {#each outputDevices as dev}
                    <option
                      value="device:{dev.id}"
                      disabled={form.actionTargets.includes(`device:${dev.id}`)}
                    >{dev.name}{dev.is_default ? " ✓" : ""}</option>
                  {/each}
                {:else}
                  {#each inputDevices as dev}
                    <option
                      value="device:{dev.id}"
                      disabled={form.actionTargets.includes(`device:${dev.id}`)}
                    >{dev.name}{dev.is_default ? " ✓" : ""}</option>
                  {/each}
                {/if}
              </select>
            </div>
          {/if}
          <div class="field">
            <label>Curve</label>
            <select bind:value={form.transformName}>
              <option>Linear</option>
              <option>Logarithmic</option>
              <option value="RelativeEncoder">Relative Encoder</option>
              <option>Toggle</option>
              <option>Momentary</option>
            </select>
          </div>
          {#if form.transformName === "RelativeEncoder"}
            <div class="field">
              <label>Sensitivity</label>
              <input
                type="number"
                min="0.001"
                max="0.1"
                step="0.005"
                bind:value={form.encoderSensitivity}
              />
            </div>
          {/if}
        </div>
      </div>

      <div class="form-footer">
        <button class="primary" onclick={saveMapping}>Save Mapping</button>
        <button onclick={() => showAddForm = false}>Cancel</button>
      </div>
    </div>
  {/if}

  <!-- Mappings list -->
  {#if mappings.length === 0 && !showAddForm}
    <div class="card empty-state">
      No mappings yet. Use <strong>MIDI Learn</strong> — press a control on your
      device to capture it — or click <strong>+ Add</strong> to enter values manually.
    </div>
  {:else if mappings.length > 0}
    <div class="mapping-list">
      <!-- Column header -->
      <div class="list-header">
        <span class="col-control">MIDI Control</span>
        <span class="col-sep"></span>
        <span class="col-action">Action</span>
        <span class="col-curve">Curve</span>
        <span class="col-del"></span>
      </div>

      {#each mappings as m}
        <div class="mapping-row card">
          <!-- MIDI source -->
          <div class="col-control">
            <span class="tag device-tag" title={m.control.device}>
              {m.control.device.length > 18
                ? m.control.device.slice(0, 16) + "…"
                : m.control.device}
            </span>
            <span class="tag dim-tag">ch{m.control.channel}</span>
            <span class="tag accent-tag">{controlLabel(m.control)}</span>
          </div>

          <div class="col-sep">→</div>

          <!-- Action -->
          <div class="col-action">{actionLabel(m.action)}</div>

          <!-- Transform -->
          <div class="col-curve">
            <span class="tag dim-tag">{transformLabel(m.transform)}</span>
          </div>

          <!-- Delete — two-step inline confirm (window.confirm is blocked in Tauri) -->
          <div class="col-del">
            {#if pendingDeleteKey === controlKey(m.control)}
              <div class="del-confirm">
                <button
                  class="del-btn del-yes"
                  title="Confirm delete"
                  onclick={() => deleteMapping(m.control)}
                >✓</button>
                <button
                  class="del-btn del-no"
                  title="Cancel"
                  onclick={() => pendingDeleteKey = null}
                >✗</button>
              </div>
            {:else}
              <button
                class="del-btn"
                onclick={() => pendingDeleteKey = controlKey(m.control)}
                title="Delete"
              >×</button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .editor {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: 860px;
  }

  /* ---- Header (matches the pattern used in mixer top area) ---- */
  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 2px 10px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 2px;
  }
  .header-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .header-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
  }
  .header-right {
    display: flex;
    gap: 8px;
    align-items: center;
  }

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

  .learn-badge {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    color: var(--accent);
    font-size: 11px;
    animation: badge-pulse 1.4s ease-in-out infinite;
  }
  @keyframes badge-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.55; } }

  .dot-pulse {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    flex-shrink: 0;
    animation: dot-blink 0.9s ease-in-out infinite;
  }
  @keyframes dot-blink { 0%, 100% { opacity: 1; } 50% { opacity: 0; } }

  /* ---- Add form ---- */
  .form-card {
    padding: 14px 16px;
  }

  .form-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin-bottom: 12px;
  }

  .form-panels {
    display: flex;
    gap: 0;
    align-items: flex-start;
  }

  .form-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .panel-label {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
    margin-bottom: 2px;
  }

  .form-arrow {
    font-size: 16px;
    color: var(--text-muted);
    padding: 24px 16px 0;
    flex-shrink: 0;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .field label {
    font-size: 10px;
    color: var(--text-muted);
    font-weight: 500;
  }

  .field input,
  .field select {
    width: 100%;
  }

  .field-row {
    display: flex;
    gap: 6px;
  }
  .field-row .field { flex: 1; }

  .form-footer {
    display: flex;
    gap: 8px;
    margin-top: 14px;
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }

  /* ---- Target tag list ---- */
  .target-tags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-bottom: 5px;
  }

  .target-tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 6px 2px 8px;
    background: color-mix(in srgb, var(--accent) 14%, var(--surface2));
    border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--border));
    border-radius: 10px;
    font-size: 11px;
    color: var(--accent);
    font-weight: 500;
  }

  .tag-remove {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    padding: 0;
    background: transparent;
    border: none;
    color: var(--accent);
    font-size: 13px;
    line-height: 1;
    cursor: pointer;
    opacity: 0.6;
    border-radius: 50%;
    transition: opacity 0.1s, background 0.1s;
  }
  .tag-remove:hover {
    opacity: 1;
    background: color-mix(in srgb, var(--danger) 20%, transparent);
    color: var(--danger);
  }

  /* ---- Empty state ---- */
  .empty-state {
    text-align: center;
    padding: 32px;
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.6;
  }

  /* ---- Mapping list ---- */
  .mapping-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  /* Column header row */
  .list-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 14px 4px;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.07em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  /* Column widths shared between header and rows */
  .col-control {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 260px;
    flex-shrink: 0;
  }
  .col-sep {
    width: 18px;
    flex-shrink: 0;
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
  }
  .col-action {
    flex: 1;
    font-size: 12px;
    font-weight: 500;
    color: var(--text);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .col-curve {
    width: 90px;
    flex-shrink: 0;
    text-align: right;
  }
  .col-del {
    width: 52px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
  }
  .del-confirm {
    display: flex;
    gap: 2px;
  }

  /* Mapping row */
  .mapping-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 14px;
    border-radius: var(--radius);
  }
  .mapping-row:hover {
    background: color-mix(in srgb, var(--surface2) 60%, var(--surface));
  }

  /* Tags */
  .tag {
    font-size: 10px;
    border-radius: 4px;
    padding: 2px 6px;
    white-space: nowrap;
  }
  .device-tag {
    font-weight: 500;
    color: var(--text-muted);
    background: var(--surface2);
    border: 1px solid var(--border);
    max-width: 130px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dim-tag {
    color: var(--text-muted);
    background: var(--surface2);
    border: 1px solid var(--border);
  }
  .accent-tag {
    font-weight: 700;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, var(--surface2));
    border: 1px solid color-mix(in srgb, var(--accent) 30%, var(--border));
  }

  /* Delete button */
  .del-btn {
    width: 22px;
    height: 22px;
    border-radius: 4px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text-muted);
    font-size: 13px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    line-height: 1;
    transition: background 0.1s, color 0.1s, border-color 0.1s;
  }
  .del-btn:hover {
    background: color-mix(in srgb, var(--danger) 12%, var(--surface2));
    color: var(--danger);
    border-color: var(--danger);
  }
  .del-yes:hover {
    background: color-mix(in srgb, var(--success) 15%, var(--surface2));
    color: var(--success);
    border-color: var(--success);
  }
  .del-no:hover {
    background: color-mix(in srgb, var(--text-muted) 15%, var(--surface2));
    color: var(--text);
    border-color: var(--border);
  }
</style>
