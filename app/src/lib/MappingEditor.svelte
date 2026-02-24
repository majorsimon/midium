<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import {
    type Mapping, type ControlId, type MidiEvent,
    controlLabel, actionLabel,
  } from "./types";

  let mappings: Mapping[] = [];
  let midiPorts: string[] = [];
  let learnActive = false;
  let showAddForm = false;

  let form = {
    device: "",
    channel: 0,
    controlTypeName: "CC" as "CC" | "Note" | "PitchBend",
    controlNumber: 0,
    actionTypeName: "SetVolume" as string,
    actionTarget: "SystemMaster" as string,
    transformName: "Linear" as string,
  };

  async function loadMappings() {
    mappings = await invoke("get_mappings").catch(() => [] as Mapping[]);
  }

  async function loadPorts() {
    midiPorts = await invoke("list_midi_ports").catch(() => [] as string[]);
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

  function buildAction() {
    switch (form.actionTypeName) {
      case "SetVolume":   return { SetVolume: { target: resolveTarget(form.actionTarget) } };
      case "ToggleMute":  return { ToggleMute: { target: resolveTarget(form.actionTarget) } };
      case "MediaPlayPause": return "MediaPlayPause";
      case "MediaNext":      return "MediaNext";
      case "MediaPrev":      return "MediaPrev";
      default:            return { SetVolume: { target: "SystemMaster" as const } };
    }
  }

  function resolveTarget(t: string) {
    if (t === "SystemMaster") return "SystemMaster" as const;
    if (t === "FocusedApplication") return "FocusedApplication" as const;
    if (t.startsWith("app:")) return { Application: { name: t.slice(4) } };
    return "SystemMaster" as const;
  }

  async function saveMapping() {
    const mapping: Mapping = {
      control: buildControlId(),
      action: buildAction() as Mapping["action"],
      transform: form.transformName as Mapping["transform"],
    };
    await invoke("save_mapping", { mapping }).catch(console.error);
    showAddForm = false;
    await loadMappings();
  }

  async function deleteMapping(control: ControlId) {
    if (!confirm("Delete this mapping?")) return;
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
    await Promise.all([loadMappings(), loadPorts()]);
    await listen<MidiEvent>("midi-learn-result", (e) => {
      applyLearnedControl(e.payload);
    });
  });
</script>

<div class="editor">
  <!-- Toolbar -->
  <div class="toolbar card">
    <span class="section-title" style="margin-bottom: 0;">
      Mappings
      {#if mappings.length > 0}
        <span class="count-badge">{mappings.length}</span>
      {/if}
    </span>

    <div class="toolbar-right">
      {#if learnActive}
        <span class="learn-badge">
          <span class="dot-pulse"></span>
          Waiting for MIDI…
        </span>
        <button on:click={cancelLearn}>Cancel</button>
      {:else}
        <button on:click={startLearn}>MIDI Learn</button>
      {/if}
      <button
        class="primary"
        on:click={() => { showAddForm = !showAddForm; }}
      >
        {showAddForm ? "Cancel" : "+ Add"}
      </button>
    </div>
  </div>

  <!-- Add / edit form -->
  {#if showAddForm}
    <div class="card form-card">
      <div class="section-title" style="margin-bottom: 14px;">
        {learnActive ? "Configure Learned Control" : "New Mapping"}
      </div>

      <div class="form-row">
        <!-- Control section -->
        <div class="form-section">
          <div class="form-section-title">MIDI Control</div>
          <div class="form-fields">
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

            <div class="field-row2">
              <div class="field">
                <label>Channel</label>
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
                  <label>Number</label>
                  <input type="number" min="0" max="127" bind:value={form.controlNumber} />
                </div>
              {/if}
            </div>
          </div>
        </div>

        <div class="form-divider">→</div>

        <!-- Action section -->
        <div class="form-section">
          <div class="form-section-title">Action</div>
          <div class="form-fields">
            <div class="field">
              <label>Action</label>
              <select bind:value={form.actionTypeName}>
                <option value="SetVolume">Set Volume</option>
                <option value="ToggleMute">Toggle Mute</option>
                <option value="MediaPlayPause">Play / Pause</option>
                <option value="MediaNext">Next Track</option>
                <option value="MediaPrev">Previous Track</option>
              </select>
            </div>

            {#if form.actionTypeName === "SetVolume" || form.actionTypeName === "ToggleMute"}
              <div class="field">
                <label>Target</label>
                <select bind:value={form.actionTarget}>
                  <option value="SystemMaster">System Master</option>
                  <option value="FocusedApplication">Focused App</option>
                </select>
              </div>
            {/if}

            <div class="field">
              <label>Transform</label>
              <select bind:value={form.transformName}>
                <option>Linear</option>
                <option>Logarithmic</option>
                <option>Toggle</option>
                <option>Momentary</option>
              </select>
            </div>
          </div>
        </div>
      </div>

      <div class="form-actions">
        <button class="primary" on:click={saveMapping}>Save</button>
        <button on:click={() => showAddForm = false}>Cancel</button>
      </div>
    </div>
  {/if}

  <!-- Mappings list -->
  {#if mappings.length === 0}
    <div class="card empty-state">
      No mappings yet. Use <strong>MIDI Learn</strong> — press a control on your
      device to capture it — or click <strong>+ Add</strong> to enter values manually.
    </div>
  {:else}
    <div class="mapping-list">
      {#each mappings as m}
        <div class="mapping-card card">
          <!-- MIDI source pill -->
          <div class="mapping-control">
            <span class="device-tag" title={m.control.device}>
              {m.control.device.length > 20
                ? m.control.device.slice(0, 18) + "…"
                : m.control.device}
            </span>
            <span class="ch-tag">ch{m.control.channel}</span>
            <span class="cc-tag">{controlLabel(m.control)}</span>
          </div>

          <!-- Arrow -->
          <div class="mapping-arrow">→</div>

          <!-- Action -->
          <div class="mapping-action">
            {actionLabel(m.action)}
          </div>

          <!-- Transform -->
          <div class="mapping-transform">
            <span class="tag">{transformLabel(m.transform)}</span>
          </div>

          <!-- Delete -->
          <button
            class="del-btn"
            on:click={() => deleteMapping(m.control)}
            title="Delete mapping"
          >×</button>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .editor {
    padding: 20px;
    max-width: 900px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  /* ---- Toolbar ---- */
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
  }
  .toolbar-right { display: flex; gap: 8px; align-items: center; }

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
    margin-left: 6px;
    vertical-align: middle;
  }

  .learn-badge {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 10px;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    color: var(--accent);
    font-size: 12px;
    animation: badge-pulse 1.4s ease-in-out infinite;
  }
  @keyframes badge-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.6; } }

  .dot-pulse {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    flex-shrink: 0;
    animation: dot-blink 0.9s ease-in-out infinite;
  }
  @keyframes dot-blink { 0%, 100% { opacity: 1; } 50% { opacity: 0; } }

  /* ---- Add form ---- */
  .form-card { }

  .form-row {
    display: flex;
    gap: 16px;
    align-items: flex-start;
  }

  .form-section {
    flex: 1;
  }

  .form-section-title {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
    margin-bottom: 8px;
  }

  .form-divider {
    font-size: 18px;
    color: var(--text-muted);
    padding-top: 28px;
    flex-shrink: 0;
  }

  .form-fields {
    display: flex;
    flex-direction: column;
    gap: 8px;
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

  .field-row2 {
    display: flex;
    gap: 8px;
  }
  .field-row2 .field { flex: 1; }

  .form-actions {
    display: flex;
    gap: 8px;
    margin-top: 14px;
    padding-top: 12px;
    border-top: 1px solid var(--border);
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
    gap: 6px;
  }

  .mapping-card {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
  }

  .mapping-control {
    display: flex;
    align-items: center;
    gap: 5px;
    flex-shrink: 0;
    min-width: 0;
  }

  .device-tag {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-muted);
    background: var(--surface2);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 2px 7px;
    white-space: nowrap;
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .ch-tag {
    font-size: 10px;
    color: var(--text-muted);
    background: var(--surface2);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 2px 5px;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .cc-tag {
    font-size: 11px;
    font-weight: 600;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, var(--surface2));
    border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--border));
    border-radius: 4px;
    padding: 2px 7px;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .mapping-arrow {
    color: var(--text-muted);
    font-size: 14px;
    flex-shrink: 0;
  }

  .mapping-action {
    flex: 1;
    font-size: 12px;
    font-weight: 500;
    color: var(--text);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mapping-transform {
    flex-shrink: 0;
  }

  .del-btn {
    flex-shrink: 0;
    width: 22px;
    height: 22px;
    border-radius: 4px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text-muted);
    font-size: 14px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    transition: background 0.1s, color 0.1s, border-color 0.1s;
  }
  .del-btn:hover {
    background: color-mix(in srgb, var(--danger) 12%, var(--surface2));
    color: var(--danger);
    border-color: var(--danger);
  }
</style>
