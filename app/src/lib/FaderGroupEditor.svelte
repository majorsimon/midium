<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import type {
    FaderGroup,
    AudioTarget,
    AudioDeviceInfo,
    AudioSessionInfo,
    AudioCapabilities,
    DeviceProfile,
    MidiEvent,
  } from "./types";

  // ---------------------------------------------------------------------------
  // State
  // ---------------------------------------------------------------------------

  let faderGroups: FaderGroup[] = $state([]);
  let profiles: DeviceProfile[] = $state([]);
  let midiPorts: string[] = $state([]);
  let outputDevices: AudioDeviceInfo[] = $state([]);
  let inputDevices: AudioDeviceInfo[] = $state([]);
  let sessions: AudioSessionInfo[] = $state([]);
  let caps: AudioCapabilities = { per_app_volume: false, device_switching: false, input_device_switching: false };



  let showForm = $state(false);
  let editingIndex: number | null = $state(null); // null = adding new
  let pendingDeleteIndex: number | null = $state(null);

  // Form state
  let formDevice = $state("");
  let formGroup = $state(1);
  let formTargetEncoded = $state("SystemMaster"); // "SystemMaster" | "FocusedApplication" | "app:Name" | "device:id"
  let formTransform: FaderGroup["transform"] = $state("Logarithmic");

  // Profile group detection: listen for a MIDI event to auto-fill device.
  let detectingDevice = $state(false);





  // Label for a profile group's slider/encoder control.
  function profileGroupLabel(g: number): string {
    if (!selectedProfile) return `Group ${g}`;
    const fader = selectedProfile.controls.find(c =>
      c.group === g && (c.control_type === "slider" || c.control_type === "encoder")
    );
    return fader?.label ?? `Group ${g}`;
  }

  /** True if the selected profile group uses an encoder rather than a slider. */
  function groupUsesEncoder(g: number): boolean {
    if (!selectedProfile) return false;
    const fader = selectedProfile.controls.find(c =>
      c.group === g && (c.control_type === "slider" || c.control_type === "encoder")
    );
    return fader?.control_type === "encoder";
  }

  /** Return the appropriate default transform for a group. */
  function defaultTransformForGroup(g: number): FaderGroup["transform"] {
    return groupUsesEncoder(g)
      ? { RelativeEncoder: { sensitivity: 0.01 } }
      : "Logarithmic";
  }

  // ---------------------------------------------------------------------------
  // Target helpers (reused from MappingEditor pattern)
  // ---------------------------------------------------------------------------

  function resolveTarget(encoded: string): AudioTarget {
    if (encoded === "SystemMaster") return "SystemMaster";
    if (encoded === "FocusedApplication") return "FocusedApplication";
    if (encoded.startsWith("app:")) return { Application: { name: encoded.slice(4) } };
    if (encoded.startsWith("device:")) return { Device: { id: encoded.slice(7) } };
    return "SystemMaster";
  }

  function encodeTarget(t: AudioTarget): string {
    if (t === "SystemMaster") return "SystemMaster";
    if (t === "FocusedApplication") return "FocusedApplication";
    if (typeof t === "object" && "Application" in t) return `app:${t.Application.name}`;
    if (typeof t === "object" && "Device" in t) return `device:${t.Device.id}`;
    return "SystemMaster";
  }

  function displayTarget(encoded: string): string {
    if (encoded === "SystemMaster") return "System Master";
    if (encoded === "FocusedApplication") return "Focused App";
    if (encoded.startsWith("app:")) return encoded.slice(4);
    if (encoded.startsWith("device:")) {
      const id = encoded.slice(7);
      return [...outputDevices, ...inputDevices].find(d => d.id === id)?.name ?? id;
    }
    return encoded;
  }

  // ---------------------------------------------------------------------------
  // CRUD operations
  // ---------------------------------------------------------------------------

  function openAddForm() {
    editingIndex = null;
    formDevice = midiPorts[0] ?? "";
    formGroup = profileGroups[0] ?? 1;
    formTargetEncoded = "SystemMaster";
    formTransform = defaultTransformForGroup(formGroup);
    showForm = true;
    loadAudioTargets();
  }

  function openEditForm(i: number) {
    const g = faderGroups[i];
    editingIndex = i;
    formDevice = g.device;
    formGroup = g.group;
    formTargetEncoded = encodeTarget(g.target);
    formTransform = g.transform;
    showForm = true;
    loadAudioTargets();
  }

  function cancelForm() {
    showForm = false;
    editingIndex = null;
    detectingDevice = false;
    invoke("cancel_midi_learn").catch(() => {});
  }

  async function saveGroup() {
    const group: FaderGroup = {
      device: formDevice,
      group: formGroup,
      target: resolveTarget(formTargetEncoded),
      transform: formTransform,
    };
    await invoke("save_fader_group", { group }).catch(console.error);
    showForm = false;
    editingIndex = null;
    await loadGroups();
  }

  async function deleteGroup(i: number) {
    const g = faderGroups[i];
    await invoke("delete_fader_group", { device: g.device, group: g.group }).catch(console.error);
    pendingDeleteIndex = null;
    await loadGroups();
  }

  // ---------------------------------------------------------------------------
  // Device auto-detect
  // ---------------------------------------------------------------------------

  async function startDetect() {
    detectingDevice = true;
    await invoke("start_midi_learn").catch(console.error);
  }

  function applyDetectedDevice(event: MidiEvent) {
    if (!detectingDevice) return;
    detectingDevice = false;
    formDevice = event.device;
  }

  // ---------------------------------------------------------------------------
  // Data loading
  // ---------------------------------------------------------------------------

  async function loadGroups() {
    const raw = await invoke<FaderGroup[]>("get_fader_groups").catch(() => [] as FaderGroup[]);
    faderGroups = [...raw].sort((a, b) => a.group - b.group);
  }

  async function loadAudioTargets() {
    [outputDevices, inputDevices, sessions, caps] = await Promise.all([
      invoke<AudioDeviceInfo[]>("list_output_devices").catch(() => [] as AudioDeviceInfo[]),
      invoke<AudioDeviceInfo[]>("list_input_devices").catch(() => [] as AudioDeviceInfo[]),
      invoke<AudioSessionInfo[]>("list_sessions").catch(() => [] as AudioSessionInfo[]),
      invoke<AudioCapabilities>("get_capabilities").catch(() => caps),
    ]);
  }

  function isDeviceTarget(g: FaderGroup): string | null {
    if (typeof g.target === "object" && "Device" in g.target) return g.target.Device.id;
    return null;
  }


  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  let unlistens: UnlistenFn[] = [];

  onMount(async () => {
    [midiPorts, profiles] = await Promise.all([
      invoke<string[]>("list_midi_ports").catch(() => [] as string[]),
      invoke<DeviceProfile[]>("list_profiles").catch(() => [] as DeviceProfile[]),
    ]);
    await loadAudioTargets();
    await loadGroups();

    unlistens.push(
      await listen<MidiEvent>("midi-learn-result", (e) => {
        applyDetectedDevice(e.payload);
      }),
      await listen<string>("device-connected", async (e) => {
        midiPorts = [...new Set([...midiPorts, e.payload])];
      }),
      await listen<string>("device-disconnected", (e) => {
        midiPorts = midiPorts.filter(p => p !== e.payload);
      }),
      await listen("preset-loaded", async () => {
        await loadGroups();
      }),
    );
  });

  onDestroy(() => {
    unlistens.forEach(u => u());
  });
  // ---------------------------------------------------------------------------
  // Derived: profile groups available for the selected device
  // ---------------------------------------------------------------------------

  let selectedProfile = $derived(profiles.find(p =>
    p.match_patterns.some(pat =>
      formDevice.toLowerCase().includes(pat.toLowerCase()) ||
      pat.toLowerCase().includes(formDevice.toLowerCase())
    )
  ));
  /** Unique group numbers in the selected profile that contain a slider. */
  let profileGroups = $derived((() => {
    if (!selectedProfile) return [] as number[];
    const seen = new Set<number>();
    for (const c of selectedProfile.controls) {
      if ((c.control_type === "slider" || c.control_type === "encoder") && c.group != null) {
        seen.add(c.group);
      }
    }
    return [...seen].sort((a, b) => a - b);
  })());
  $effect(() => {
    if (profileGroups.length > 0 && !profileGroups.includes(formGroup)) {
      formGroup = profileGroups[0];
    }
  });
  $effect(() => {
    if (showForm && editingIndex === null && selectedProfile) {
      formTransform = defaultTransformForGroup(formGroup);
    }
  });
  let resolvedTargetLabels = $derived(faderGroups.map(g => {
    if (g.target === "SystemMaster") return "System Master";
    if (g.target === "FocusedApplication") return "Focused App";
    if (typeof g.target === "object" && "Application" in g.target) return g.target.Application.name;
    if (typeof g.target === "object" && "Device" in g.target) {
      const id = g.target.Device.id;
      return [...outputDevices, ...inputDevices].find(d => d.id === id)?.name ?? id;
    }
    return "?";
  }));
</script>

<div class="fader-groups">
  <!-- ======================================================================
       Header
  ====================================================================== -->
  <div class="header">
    <div class="header-left">
      <span class="header-title">Fader Groups</span>
      {#if faderGroups.length > 0}
        <span class="count-badge">{faderGroups.length}</span>
      {/if}
    </div>
    <div class="header-right">
      <button class="primary" onclick={showForm ? cancelForm : openAddForm}>
        {showForm ? "Cancel" : "+ Add Group"}
      </button>
    </div>
  </div>

  <!-- ======================================================================
       Add / Edit form
  ====================================================================== -->
  {#if showForm}
    <div class="card form-card">
      <div class="form-title">
        {editingIndex !== null ? "Edit Fader Group" : "New Fader Group"}
      </div>

      <div class="form-panels">
        <!-- Left panel: MIDI Device + Group -->
        <div class="form-panel">
          <div class="panel-label">MIDI Channel Strip</div>

          <div class="field">
            <label>Device</label>
            <div class="device-row">
              {#if midiPorts.length > 0}
                <select bind:value={formDevice} class="device-select">
                  {#each midiPorts as port}
                    <option value={port}>{port}</option>
                  {/each}
                  <option value="">Custom…</option>
                </select>
              {:else}
                <input bind:value={formDevice} placeholder="nanoKONTROL2" class="device-select" />
              {/if}

              {#if detectingDevice}
                <button class="detect-btn detecting" onclick={() => {
                  detectingDevice = false;
                  invoke("cancel_midi_learn").catch(() => {});
                }}>
                  <span class="dot-pulse"></span> Cancel
                </button>
              {:else}
                <button class="detect-btn" title="Move any control on the device to auto-detect it" onclick={startDetect}>
                  Detect
                </button>
              {/if}
            </div>
            {#if formDevice === ""}
              <input bind:value={formDevice} placeholder="Device name pattern" style="margin-top:4px;" />
            {/if}
          </div>

          <!-- Profile group picker -->
          <div class="field">
            <label>Group</label>
            {#if profileGroups.length > 0}
              <div class="group-grid">
                {#each profileGroups as g}
                  <button
                    class="group-tile"
                    class:selected={formGroup === g}
                    onclick={() => formGroup = g}
                    title={profileGroupLabel(g)}
                  >
                    <span class="group-num">{g}</span>
                    <span class="group-lbl">{profileGroupLabel(g)}</span>
                    <div class="group-smr">
                      {#each (selectedProfile?.controls.filter(c => c.group === g && c.control_type === "button") ?? []) as btn}
                        <span
                          class="smr-dot"
                          class:smr-solo={btn.button_role === "solo"}
                          class:smr-mute={btn.button_role === "mute"}
                          class:smr-rec={btn.button_role === "record"}
                          title={btn.label}
                        >{btn.button_role === "solo" ? "S" : btn.button_role === "mute" ? "M" : "R"}</span>
                      {/each}
                    </div>
                  </button>
                {/each}
              </div>
            {:else}
              <input type="number" min="1" max="128" bind:value={formGroup} />
              {#if formDevice && !selectedProfile}
                <span class="hint">No matching profile found — enter group number manually.</span>
              {/if}
            {/if}
          </div>

          <div class="field">
            <label>Fader Curve</label>
            <select value={typeof formTransform === "string" ? formTransform : "RelativeEncoder"}
              onchange={(e) => {
                const v = e.currentTarget.value;
                if (v === "RelativeEncoder") {
                  formTransform = { RelativeEncoder: { sensitivity: 0.01 } };
                } else if (v === "Logarithmic") {
                  formTransform = "Logarithmic";
                } else {
                  formTransform = "Linear";
                }
              }}
            >
              <option value="Logarithmic">Logarithmic (recommended for sliders)</option>
              <option value="Linear">Linear</option>
              <option value="RelativeEncoder">Relative Encoder</option>
            </select>
          </div>
          {#if typeof formTransform === "object" && "RelativeEncoder" in formTransform}
            <div class="field">
              <label>Encoder Sensitivity</label>
              <input
                type="number"
                min="0.001"
                max="0.1"
                step="0.005"
                value={formTransform.RelativeEncoder.sensitivity}
                oninput={(e) => {
                  formTransform = { RelativeEncoder: { sensitivity: parseFloat(e.currentTarget.value) || 0.01 } };
                }}
              />
            </div>
          {/if}
        </div>

        <div class="form-arrow">→</div>

        <!-- Right panel: Audio Target -->
        <div class="form-panel">
          <div class="panel-label">Audio Target</div>

          <div class="field">
            <label>Target</label>
            <select bind:value={formTargetEncoded}>
              <option value="SystemMaster">System Master</option>
              <option value="FocusedApplication">Focused App (active window)</option>

              {#if outputDevices.length > 0}
                <optgroup label="Output Devices">
                  {#each outputDevices as dev}
                    <option value="device:{dev.id}">{dev.name}{dev.is_default ? " ✓" : ""}</option>
                  {/each}
                </optgroup>
              {/if}

              {#if inputDevices.length > 0}
                <optgroup label="Input Devices">
                  {#each inputDevices as dev}
                    <option value="device:{dev.id}">{dev.name}{dev.is_default ? " ✓" : ""}</option>
                  {/each}
                </optgroup>
              {/if}

              {#if sessions.length > 0}
                <optgroup label="Running Apps">
                  {#each sessions as s}
                    <option value="app:{s.name}">{s.name}</option>
                  {/each}
                </optgroup>
              {/if}
            </select>
          </div>

          <div class="target-preview card">
            <div class="preview-label">LED behaviour</div>
            <div class="preview-row">
              <span class="smr-dot smr-solo">S</span>
              <span>Always lit — strip has an assignment</span>
            </div>
            <div class="preview-row">
              <span class="smr-dot smr-mute">M</span>
              <span>Lit while <strong>{displayTarget(formTargetEncoded)}</strong> is muted</span>
            </div>
            <div class="preview-row">
              <span class="smr-dot smr-rec">R</span>
              <span>Lit while <strong>{displayTarget(formTargetEncoded)}</strong> is active</span>
            </div>
          </div>
        </div>
      </div>

      <div class="form-footer">
        <button class="primary" onclick={saveGroup}>
          {editingIndex !== null ? "Update Group" : "Save Group"}
        </button>
        <button onclick={cancelForm}>Cancel</button>
      </div>
    </div>
  {/if}

  <!-- ======================================================================
       Empty state
  ====================================================================== -->
  {#if faderGroups.length === 0 && !showForm}
    <div class="card empty-state">
      <div class="empty-icon">🎚</div>
      <strong>No fader groups configured</strong>
      <p>
        A fader group links a physical channel strip on your MIDI controller
        to an audio target. The fader sets volume, M mutes, and the S/M/R
        button LEDs update automatically.
      </p>
      <button class="primary" onclick={openAddForm}>+ Add Group</button>
    </div>
  {/if}

  <!-- ======================================================================
       Group list
  ====================================================================== -->
  {#if faderGroups.length > 0}
    <div class="group-list">
      <div class="list-header">
        <span class="col-device">Device · Group</span>
        <span class="col-sep"></span>
        <span class="col-target">Target</span>
        <span class="col-curve">Curve</span>
        <span class="col-del"></span>
      </div>

      {#each faderGroups as g, i (g.device + g.group)}
        <div class="group-row card" class:editing={editingIndex === i}>
          <div class="col-device">
            <span class="tag device-tag" title={g.device}>
              {g.device.length > 16 ? g.device.slice(0, 14) + "…" : g.device}
            </span>
            <span class="tag accent-tag">Group {g.group}</span>
          </div>

          <div class="col-sep">→</div>

          <div class="col-target">
            <span class="target-text">{resolvedTargetLabels[i]}</span>
          </div>

          <div class="col-curve">
            <span class="tag dim-tag">
              {typeof g.transform === "string"
                ? g.transform
                : "RelativeEncoder" in g.transform
                  ? `Encoder ×${g.transform.RelativeEncoder.sensitivity}`
                  : JSON.stringify(g.transform)}
            </span>
          </div>

          <div class="col-del">
            {#if pendingDeleteIndex === i}
              <div class="del-confirm">
                <button class="del-btn del-yes" title="Confirm" onclick={() => deleteGroup(i)}>✓</button>
                <button class="del-btn del-no" title="Cancel" onclick={() => pendingDeleteIndex = null}>✗</button>
              </div>
            {:else}
              <button class="edit-btn" onclick={() => openEditForm(i)} title="Edit">✎</button>
              <button class="del-btn" onclick={() => pendingDeleteIndex = i} title="Delete">×</button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <!-- Note about external LED mode -->
  {#if faderGroups.length > 0}
    <div class="tip card">
      <strong>Note:</strong> Button LEDs require your device to be in External LED mode.
      For the nanoKONTROL2, enable this in the Korg Kontrol Editor
      (Scene → LED Mode → External).
    </div>
  {/if}
</div>

<style>
  .fader-groups {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: 860px;
  }

  /* ---- Header ---- */
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
  .header-right { display: flex; gap: 8px; }

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

  /* ---- Form ---- */
  .form-card { padding: 14px 16px; }

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
    gap: 10px;
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
    padding: 28px 16px 0;
    flex-shrink: 0;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .field label {
    font-size: 10px;
    color: var(--text-muted);
    font-weight: 500;
  }

  .field input,
  .field select { width: 100%; }

  .hint {
    font-size: 10px;
    color: var(--text-muted);
    font-style: italic;
  }

  /* Device row: select + detect button side by side */
  .device-row {
    display: flex;
    gap: 6px;
    align-items: stretch;
  }
  .device-select { flex: 1; }

  .detect-btn {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 0 10px;
    font-size: 11px;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .detect-btn.detecting {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border-color: var(--accent);
    color: var(--accent);
  }

  .dot-pulse {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
    flex-shrink: 0;
    animation: dot-blink 0.9s ease-in-out infinite;
  }
  @keyframes dot-blink { 0%, 100% { opacity: 1; } 50% { opacity: 0; } }

  /* ---- Profile group grid ---- */
  .group-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }

  .group-tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 58px;
    padding: 6px 4px;
    background: var(--surface2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    cursor: pointer;
    transition: background 0.1s, border-color 0.1s;
    gap: 2px;
  }
  .group-tile:hover {
    background: color-mix(in srgb, var(--accent) 10%, var(--surface2));
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
  }
  .group-tile.selected {
    background: color-mix(in srgb, var(--accent) 18%, var(--surface2));
    border-color: var(--accent);
  }

  .group-num {
    font-size: 14px;
    font-weight: 700;
    color: var(--text);
    line-height: 1;
  }
  .group-tile.selected .group-num { color: var(--accent); }

  .group-lbl {
    font-size: 8px;
    color: var(--text-muted);
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    width: 100%;
    line-height: 1.2;
  }

  .group-smr {
    display: flex;
    gap: 2px;
    margin-top: 2px;
  }

  /* ---- Target preview card ---- */
  .target-preview {
    padding: 10px 12px;
    background: var(--surface2);
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 4px;
  }

  .preview-label {
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted);
    margin-bottom: 2px;
  }

  .preview-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    color: var(--text-muted);
  }
  .preview-row strong { color: var(--text); }

  /* ---- S / M / R indicator dots ---- */
  .smr-dot {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    font-size: 8px;
    font-weight: 800;
    border-radius: 3px;
    border: 1px solid var(--border);
    background: var(--surface2);
    color: var(--text-muted);
    flex-shrink: 0;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
  }

  .smr-solo {
    background: color-mix(in srgb, var(--accent) 18%, var(--surface2));
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
  }

  .smr-mute {
    background: color-mix(in srgb, var(--danger) 18%, var(--surface2));
    color: var(--danger);
    border-color: var(--danger);
  }

  .smr-rec {
    background: color-mix(in srgb, var(--success) 22%, var(--surface2));
    color: var(--success);
    border-color: var(--success);
    animation: pulse 2s ease-in-out infinite;
  }

  .smr-off {
    opacity: 0.35;
  }

  @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.5; } }

  .form-footer {
    display: flex;
    gap: 8px;
    margin-top: 14px;
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }

  /* ---- Empty state ---- */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 36px 24px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }
  .empty-icon { font-size: 32px; opacity: 0.5; }
  .empty-state p {
    max-width: 380px;
    line-height: 1.6;
    margin: 0;
    font-size: 12px;
  }

  /* ---- Group list ---- */
  .group-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

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

  .col-device {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 210px;
    flex-shrink: 0;
  }
  .col-sep {
    width: 18px;
    flex-shrink: 0;
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
  }
  .col-target {
    flex: 1;
    font-size: 12px;
    font-weight: 500;
    color: var(--text);
    min-width: 0;
  }
  .col-curve {
    width: 90px;
    flex-shrink: 0;
    text-align: right;
  }
  .col-del {
    width: 58px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 3px;
  }

  .group-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 14px;
    border-radius: var(--radius);
    transition: background 0.1s;
  }
  .group-row:hover {
    background: color-mix(in srgb, var(--surface2) 60%, var(--surface));
  }
  .group-row.editing {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 6%, var(--surface));
  }

  .target-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Tags (shared with MappingEditor) */
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
    max-width: 110px;
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

  /* Edit / Delete buttons */
  .edit-btn {
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
    transition: background 0.1s, color 0.1s, border-color 0.1s;
  }
  .edit-btn:hover {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface2));
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
  }

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
  .del-confirm { display: flex; gap: 2px; }
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

  /* ---- Tip ---- */
  .tip {
    font-size: 11px;
    color: var(--text-muted);
    padding: 9px 12px;
    line-height: 1.5;
  }
  .tip strong { color: var(--text); }
</style>
