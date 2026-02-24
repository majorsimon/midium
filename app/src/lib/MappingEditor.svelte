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
  let learnedControl: ControlId | null = null;
  let showAddForm = false;

  // New mapping form state
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
    learnedControl = null;
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
    learnedControl = buildControlId();
    showAddForm = true;
  }

  function buildControlId(): ControlId {
    const ct =
      form.controlTypeName === "CC" ? { CC: form.controlNumber }
      : form.controlTypeName === "Note" ? { Note: form.controlNumber }
      : "PitchBend" as const;
    return { device: form.device, channel: form.channel, control_type: ct };
  }

  function buildAction() {
    switch (form.actionTypeName) {
      case "SetVolume":
        return { SetVolume: { target: resolveTarget(form.actionTarget) } };
      case "ToggleMute":
        return { ToggleMute: { target: resolveTarget(form.actionTarget) } };
      case "MediaPlayPause":
        return "MediaPlayPause";
      case "MediaNext":
        return "MediaNext";
      case "MediaPrev":
        return "MediaPrev";
      default:
        return { SetVolume: { target: "SystemMaster" as const } };
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
    learnedControl = null;
    await loadMappings();
  }

  async function deleteMapping(control: ControlId) {
    if (!confirm("Delete this mapping?")) return;
    await invoke("delete_mapping", { control }).catch(console.error);
    await loadMappings();
  }

  onMount(async () => {
    await Promise.all([loadMappings(), loadPorts()]);

    await listen<MidiEvent>("midi-learn-result", (e) => {
      applyLearnedControl(e.payload);
    });
  });
</script>

<div class="editor" style="padding: 20px; max-width: 900px;">
  <!-- Toolbar -->
  <div class="toolbar">
    <span class="section-title" style="margin-bottom: 0;">Mappings ({mappings.length})</span>
    <div class="toolbar-right">
      {#if !learnActive}
        <button on:click={startLearn}>⚡ MIDI Learn</button>
      {:else}
        <span class="learn-badge">Waiting for MIDI input…</span>
        <button on:click={cancelLearn}>Cancel</button>
      {/if}
      <button class="primary" on:click={() => { showAddForm = !showAddForm; learnedControl = null; }}>
        {showAddForm ? "Cancel" : "+ Add"}
      </button>
    </div>
  </div>

  <!-- Add mapping form -->
  {#if showAddForm}
    <div class="card add-form">
      <div class="section-title">
        {learnedControl ? "Configure Learned Control" : "New Mapping"}
      </div>
      <div class="form-grid">
        <div>
          <label>Device</label>
          {#if midiPorts.length > 0}
            <select bind:value={form.device}>
              {#each midiPorts as port}
                <option value={port}>{port}</option>
              {/each}
              <option value="">Custom…</option>
            </select>
          {:else}
            <input bind:value={form.device} placeholder="nanoKONTROL2" />
          {/if}
        </div>

        <div>
          <label>Channel</label>
          <input type="number" min="0" max="15" bind:value={form.channel} />
        </div>

        <div>
          <label>Control Type</label>
          <select bind:value={form.controlTypeName}>
            <option>CC</option>
            <option>Note</option>
            <option>PitchBend</option>
          </select>
        </div>

        {#if form.controlTypeName !== "PitchBend"}
          <div>
            <label>Number (0–127)</label>
            <input type="number" min="0" max="127" bind:value={form.controlNumber} />
          </div>
        {/if}

        <div>
          <label>Action</label>
          <select bind:value={form.actionTypeName}>
            <option value="SetVolume">Set Volume</option>
            <option value="ToggleMute">Toggle Mute</option>
            <option value="MediaPlayPause">Media Play/Pause</option>
            <option value="MediaNext">Media Next</option>
            <option value="MediaPrev">Media Prev</option>
          </select>
        </div>

        {#if form.actionTypeName === "SetVolume" || form.actionTypeName === "ToggleMute"}
          <div>
            <label>Target</label>
            <select bind:value={form.actionTarget}>
              <option value="SystemMaster">System Master</option>
              <option value="FocusedApplication">Focused App</option>
            </select>
          </div>
        {/if}

        <div>
          <label>Transform</label>
          <select bind:value={form.transformName}>
            <option>Linear</option>
            <option>Logarithmic</option>
            <option>Toggle</option>
            <option>Momentary</option>
          </select>
        </div>
      </div>

      <div style="margin-top: 12px; display: flex; gap: 8px;">
        <button class="primary" on:click={saveMapping}>Save Mapping</button>
        <button on:click={() => { showAddForm = false; learnedControl = null; }}>Cancel</button>
      </div>
    </div>
  {/if}

  <!-- Mappings table -->
  {#if mappings.length === 0}
    <div class="card empty-state">
      No mappings configured. Click <strong>+ Add</strong> or use <strong>MIDI Learn</strong>.
    </div>
  {:else}
    <div class="card" style="padding: 0; overflow: hidden;">
      <table>
        <thead>
          <tr>
            <th>Device</th>
            <th>Ch</th>
            <th>Control</th>
            <th>Action</th>
            <th>Transform</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each mappings as m}
            <tr>
              <td style="max-width: 160px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;"
                  title={m.control.device}>
                {m.control.device}
              </td>
              <td>{m.control.channel}</td>
              <td><span class="tag">{controlLabel(m.control)}</span></td>
              <td>{actionLabel(m.action)}</td>
              <td>
                <span class="tag">
                  {typeof m.transform === "string" ? m.transform : JSON.stringify(m.transform)}
                </span>
              </td>
              <td>
                <button class="danger" on:click={() => deleteMapping(m.control)}>×</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }
  .toolbar-right { display: flex; gap: 8px; align-items: center; }
  .learn-badge {
    padding: 5px 10px;
    background: color-mix(in srgb, var(--accent) 20%, transparent);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    color: var(--accent);
    font-size: 12px;
    animation: pulse 1.2s ease-in-out infinite;
  }
  @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.5; } }

  .add-form { margin-bottom: 16px; }
  .form-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 12px;
  }
  .form-grid input, .form-grid select { width: 100%; }

  .empty-state {
    text-align: center;
    padding: 32px;
    color: var(--text-muted);
  }
</style>
