<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import type {
    DeviceProfile,
    ProfileMeta,
    ProfileControl,
    ProfileControlType,
    MidiControlType,
    ButtonRole,
  } from "./types";

  type View = "list" | "edit";

  let view: View = $state("list");
  let profiles: DeviceProfile[] = $state([]);
  let meta: ProfileMeta[] = $state([]);
  let saving = $state(false);
  let saveError = $state("");
  let pendingDeleteName: string | null = $state(null);
  let pendingDeleteControl: number | null = $state(null);

  // Duplicate modal
  let showDuplicateModal = $state(false);
  let duplicateSourceName = $state("");
  let duplicateNewName = $state("");
  let duplicateError = $state("");

  // Edit form state
  let editName = $state("");
  let editVendor = $state("");
  let editModel = $state("");
  let editPatterns: string[] = $state([]);
  let editControls: ProfileControl[] = $state([]);
  let patternInput = $state("");
  let isNewProfile = $state(false);
  let originalName = $state("");
  let dirty = $state(false);

  function metaFor(name: string): ProfileMeta | undefined {
    return meta.find(m => m.name === name);
  }

  async function refresh() {
    [profiles, meta] = await Promise.all([
      invoke<DeviceProfile[]>("list_profiles").catch(() => [] as DeviceProfile[]),
      invoke<ProfileMeta[]>("list_profile_meta").catch(() => [] as ProfileMeta[]),
    ]);
  }

  let unlistens: UnlistenFn[] = [];

  onMount(async () => {
    await refresh();
    unlistens.push(
      await listen("profiles-changed", refresh),
    );
  });

  onDestroy(() => unlistens.forEach(u => u()));

  function openNewProfile() {
    isNewProfile = true;
    originalName = "";
    editName = "";
    editVendor = "";
    editModel = "";
    editPatterns = [];
    editControls = [];
    patternInput = "";
    saveError = "";
    dirty = false;
    view = "edit";
  }

  async function openEditProfile(name: string) {
    try {
      const p = await invoke<DeviceProfile>("get_profile", { name });
      isNewProfile = false;
      originalName = p.name;
      editName = p.name;
      editVendor = p.vendor ?? "";
      editModel = p.model ?? "";
      editPatterns = [...p.match_patterns];
      editControls = p.controls.map(c => ({ ...c }));
      patternInput = "";
      saveError = "";
      dirty = false;
      view = "edit";
    } catch (e) {
      console.error("Failed to load profile:", e);
    }
  }

  function cancelEdit() {
    if (dirty && !confirm("Discard unsaved changes?")) return;
    view = "list";
    saveError = "";
  }

  function markDirty() {
    dirty = true;
  }

  function addPattern() {
    const p = patternInput.trim();
    if (p && !editPatterns.includes(p)) {
      editPatterns = [...editPatterns, p];
      markDirty();
    }
    patternInput = "";
  }

  function removePattern(i: number) {
    editPatterns = editPatterns.filter((_, idx) => idx !== i);
    markDirty();
  }

  function addControl() {
    editControls = [
      ...editControls,
      {
        label: "",
        control_type: "slider" as ProfileControlType,
        midi_type: "cc" as MidiControlType,
        channel: 0,
        number: 0,
        min_value: 0,
        max_value: 127,
      },
    ];
    markDirty();
  }

  function removeControl(i: number) {
    editControls = editControls.filter((_, idx) => idx !== i);
    pendingDeleteControl = null;
    markDirty();
  }

  function updateControl(i: number, field: string, value: string | number | undefined) {
    const c = { ...editControls[i] } as Record<string, unknown>;
    c[field] = value;
    editControls[i] = c as unknown as ProfileControl;
    editControls = [...editControls];
    markDirty();
  }

  async function saveProfile() {
    saving = true;
    saveError = "";
    const profile: DeviceProfile = {
      name: editName.trim(),
      vendor: editVendor.trim() || undefined,
      model: editModel.trim() || undefined,
      match_patterns: editPatterns,
      controls: editControls.map(c => ({
        ...c,
        group: c.group ?? undefined,
        section: c.section ?? undefined,
        button_role: c.button_role ?? undefined,
      })),
    };
    try {
      await invoke("save_profile", { profile });
      dirty = false;
      view = "list";
      await refresh();
    } catch (e) {
      saveError = String(e);
    } finally {
      saving = false;
    }
  }

  async function deleteProfile(name: string) {
    try {
      await invoke("delete_profile", { name });
      pendingDeleteName = null;
      await refresh();
    } catch (e) {
      console.error("Failed to delete profile:", e);
    }
  }

  function openDuplicate(name: string) {
    duplicateSourceName = name;
    duplicateNewName = name + " (copy)";
    duplicateError = "";
    showDuplicateModal = true;
  }

  async function doDuplicate() {
    if (!duplicateNewName.trim()) {
      duplicateError = "Name cannot be empty";
      return;
    }
    try {
      await invoke("duplicate_profile", {
        name: duplicateSourceName,
        newName: duplicateNewName.trim(),
      });
      showDuplicateModal = false;
      await refresh();
    } catch (e) {
      duplicateError = String(e);
    }
  }
</script>

<div class="profile-editor">
  {#if view === "list"}
    <!-- ================================================================
         Profile List View
    ================================================================ -->
    <div class="header">
      <div class="header-left">
        <span class="header-title">Device Profiles</span>
        {#if profiles.length > 0}
          <span class="count-badge">{profiles.length}</span>
        {/if}
      </div>
      <div class="header-right">
        <button class="primary" onclick={openNewProfile}>+ New Profile</button>
      </div>
    </div>

    {#if profiles.length === 0}
      <div class="card empty-state">
        <div class="empty-icon">🎹</div>
        <strong>No device profiles</strong>
        <p>
          Profiles describe the physical controls on your MIDI device.
          Create one to get started.
        </p>
        <button class="primary" onclick={openNewProfile}>+ New Profile</button>
      </div>
    {:else}
      <div class="profile-list">
        <div class="list-header">
          <span class="col-name">Name</span>
          <span class="col-vendor">Vendor / Model</span>
          <span class="col-controls">Controls</span>
          <span class="col-source">Source</span>
          <span class="col-actions"></span>
        </div>

        {#each profiles as p (p.name)}
          {@const m = metaFor(p.name)}
          <div class="profile-row card">
            <div class="col-name">
              <span class="profile-name" title={p.name}>{p.name}</span>
            </div>

            <div class="col-vendor">
              {#if p.vendor || p.model}
                <span class="tag dim-tag">
                  {[p.vendor, p.model].filter(Boolean).join(" / ")}
                </span>
              {:else}
                <span class="text-muted">--</span>
              {/if}
            </div>

            <div class="col-controls">
              <span class="tag accent-tag">{p.controls.length}</span>
            </div>

            <div class="col-source">
              {#if m?.is_user}
                <span class="tag user-tag">user</span>
              {:else}
                <span class="tag bundled-tag">bundled</span>
              {/if}
            </div>

            <div class="col-actions">
              {#if pendingDeleteName === p.name}
                <div class="del-confirm">
                  <button class="del-btn del-yes" title="Confirm" onclick={() => deleteProfile(p.name)}>✓</button>
                  <button class="del-btn del-no" title="Cancel" onclick={() => pendingDeleteName = null}>✗</button>
                </div>
              {:else}
                <button class="edit-btn" onclick={() => openEditProfile(p.name)} title={m?.is_user ? "Edit" : "Edit (creates override)"}>✎</button>
                <button class="edit-btn" onclick={() => openDuplicate(p.name)} title="Duplicate">⧉</button>
                {#if m?.is_user}
                  <button class="del-btn" onclick={() => pendingDeleteName = p.name} title="Delete">×</button>
                {/if}
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}

  {:else}
    <!-- ================================================================
         Edit Form View
    ================================================================ -->
    <div class="header">
      <div class="header-left">
        <button class="back-btn" onclick={cancelEdit} title="Back to list">←</button>
        <span class="header-title">
          {isNewProfile ? "New Profile" : `Edit: ${originalName}`}
        </span>
        {#if !isNewProfile && !metaFor(originalName)?.is_user}
          <span class="tag override-tag">will save as user override</span>
        {/if}
      </div>
    </div>

    <!-- Profile metadata -->
    <div class="card form-card">
      <div class="form-title">Profile Info</div>

      <div class="field">
        <label>Name <span class="required">*</span></label>
        <input type="text" bind:value={editName} oninput={markDirty} placeholder="e.g. My Controller" />
      </div>

      <div class="field-row-2">
        <div class="field">
          <label>Vendor</label>
          <input type="text" bind:value={editVendor} oninput={markDirty} placeholder="e.g. Korg" />
        </div>
        <div class="field">
          <label>Model</label>
          <input type="text" bind:value={editModel} oninput={markDirty} placeholder="e.g. nanoKONTROL2" />
        </div>
      </div>

      <div class="field">
        <label>Match Patterns</label>
        <div class="pattern-input-row">
          <input
            type="text"
            bind:value={patternInput}
            placeholder="Add a pattern..."
            onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); addPattern(); } }}
          />
          <button onclick={addPattern} disabled={!patternInput.trim()}>Add</button>
        </div>
        {#if editPatterns.length > 0}
          <div class="pattern-tags">
            {#each editPatterns as pat, i}
              <span class="tag pattern-tag">
                {pat}
                <button class="tag-remove" onclick={() => removePattern(i)}>×</button>
              </span>
            {/each}
          </div>
        {:else}
          <span class="hint">No patterns -- this profile will only match by explicit name.</span>
        {/if}
      </div>
    </div>

    <!-- Controls list -->
    <div class="card form-card">
      <div class="form-title-row">
        <div class="form-title">Controls <span class="count-badge">{editControls.length}</span></div>
        <button onclick={addControl}>+ Add Control</button>
      </div>

      {#if editControls.length === 0}
        <div class="empty-controls">
          No controls defined. Click "+ Add Control" to add a slider, knob, button, or encoder.
        </div>
      {:else}
        <div class="controls-table">
          <div class="controls-header">
            <span class="ctrl-label">Label</span>
            <span class="ctrl-type">Type</span>
            <span class="ctrl-midi">MIDI</span>
            <span class="ctrl-ch">Ch</span>
            <span class="ctrl-num">No.</span>
            <span class="ctrl-range">Range</span>
            <span class="ctrl-group">Group</span>
            <span class="ctrl-section">Section</span>
            <span class="ctrl-role">Role</span>
            <span class="ctrl-del"></span>
          </div>

          {#each editControls as ctrl, i (i)}
            <div class="control-row">
              <div class="ctrl-label">
                <input
                  type="text"
                  value={ctrl.label}
                  oninput={(e) => updateControl(i, "label", e.currentTarget.value)}
                  placeholder="Label"
                />
              </div>

              <div class="ctrl-type">
                <select
                  value={ctrl.control_type}
                  onchange={(e) => updateControl(i, "control_type", e.currentTarget.value)}
                >
                  <option value="slider">Slider</option>
                  <option value="knob">Knob</option>
                  <option value="button">Button</option>
                  <option value="encoder">Encoder</option>
                </select>
              </div>

              <div class="ctrl-midi">
                <select
                  value={ctrl.midi_type ?? "cc"}
                  onchange={(e) => updateControl(i, "midi_type", e.currentTarget.value)}
                >
                  <option value="cc">CC</option>
                  <option value="note">Note</option>
                  <option value="pitch_bend">PB</option>
                </select>
              </div>

              <div class="ctrl-ch">
                <input
                  type="number"
                  min="0"
                  max="15"
                  value={ctrl.channel}
                  oninput={(e) => updateControl(i, "channel", parseInt(e.currentTarget.value) || 0)}
                />
              </div>

              <div class="ctrl-num">
                <input
                  type="number"
                  min="0"
                  max="127"
                  value={ctrl.number}
                  oninput={(e) => updateControl(i, "number", parseInt(e.currentTarget.value) || 0)}
                />
              </div>

              <div class="ctrl-range">
                <input
                  type="number"
                  min="0"
                  max="127"
                  value={ctrl.min_value ?? 0}
                  oninput={(e) => updateControl(i, "min_value", parseInt(e.currentTarget.value) || 0)}
                  title="Min"
                />
                <span class="range-sep">-</span>
                <input
                  type="number"
                  min="0"
                  max="127"
                  value={ctrl.max_value ?? 127}
                  oninput={(e) => updateControl(i, "max_value", parseInt(e.currentTarget.value) || 0)}
                  title="Max"
                />
              </div>

              <div class="ctrl-group">
                <input
                  type="number"
                  min="1"
                  max="128"
                  value={ctrl.group ?? ""}
                  oninput={(e) => {
                    const v = e.currentTarget.value;
                    updateControl(i, "group", v ? parseInt(v) || undefined : undefined);
                  }}
                  placeholder="--"
                />
              </div>

              <div class="ctrl-section">
                <input
                  type="text"
                  value={ctrl.section ?? ""}
                  oninput={(e) => updateControl(i, "section", e.currentTarget.value || undefined)}
                  placeholder="--"
                />
              </div>

              <div class="ctrl-role">
                {#if ctrl.control_type === "button"}
                  <select
                    value={ctrl.button_role ?? ""}
                    onchange={(e) => updateControl(i, "button_role", e.currentTarget.value || undefined)}
                  >
                    <option value="">--</option>
                    <option value="solo">Solo</option>
                    <option value="mute">Mute</option>
                    <option value="record">Record</option>
                  </select>
                {:else}
                  <span class="text-muted">--</span>
                {/if}
              </div>

              <div class="ctrl-del">
                {#if pendingDeleteControl === i}
                  <div class="del-confirm">
                    <button class="del-btn del-yes" title="Confirm" onclick={() => removeControl(i)}>✓</button>
                    <button class="del-btn del-no" title="Cancel" onclick={() => pendingDeleteControl = null}>✗</button>
                  </div>
                {:else}
                  <button class="del-btn" onclick={() => pendingDeleteControl = i} title="Remove">×</button>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    {#if saveError}
      <div class="save-error">{saveError}</div>
    {/if}

    <div class="form-footer">
      <button class="primary" onclick={saveProfile} disabled={saving}>
        {saving ? "Saving..." : "Save Profile"}
      </button>
      <button onclick={cancelEdit}>Cancel</button>
    </div>
  {/if}
</div>

<!-- Duplicate modal -->
{#if showDuplicateModal}
  <div class="modal-overlay" onclick={(e) => { if (e.target === e.currentTarget) showDuplicateModal = false; }}>
    <div class="modal card">
      <div class="modal-header">
        <span class="modal-title">Duplicate Profile</span>
        <button class="modal-close" onclick={() => showDuplicateModal = false}>✕</button>
      </div>
      <div class="field">
        <label>New profile name</label>
        <input
          type="text"
          bind:value={duplicateNewName}
          placeholder="e.g. My Custom Profile"
          onkeydown={(e) => { if (e.key === "Enter") doDuplicate(); }}
        />
      </div>
      {#if duplicateError}
        <div class="save-error">{duplicateError}</div>
      {/if}
      <div class="modal-footer">
        <button class="primary" onclick={doDuplicate}>Duplicate</button>
        <button onclick={() => showDuplicateModal = false}>Cancel</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .profile-editor {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: 960px;
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

  .back-btn {
    width: 28px;
    height: 28px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface2);
    color: var(--text);
    font-size: 14px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    transition: background 0.1s;
  }
  .back-btn:hover {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface2));
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
    color: var(--accent);
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

  /* ---- Profile list ---- */
  .profile-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .list-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 14px 4px;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.07em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .col-name {
    width: 200px;
    flex-shrink: 0;
    min-width: 0;
  }
  .col-vendor {
    flex: 1;
    min-width: 0;
  }
  .col-controls {
    width: 64px;
    flex-shrink: 0;
    text-align: center;
  }
  .col-source {
    width: 64px;
    flex-shrink: 0;
    text-align: center;
  }
  .col-actions {
    width: 80px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 3px;
  }

  .profile-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    border-radius: var(--radius);
    transition: background 0.1s;
  }
  .profile-row:hover {
    background: color-mix(in srgb, var(--surface2) 60%, var(--surface));
  }

  .profile-name {
    font-size: 12px;
    font-weight: 500;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text-muted { color: var(--text-muted); font-size: 11px; }

  /* Tags */
  .tag {
    font-size: 10px;
    border-radius: 4px;
    padding: 2px 6px;
    white-space: nowrap;
  }
  .dim-tag {
    color: var(--text-muted);
    background: var(--surface2);
    border: 1px solid var(--border);
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 180px;
    display: inline-block;
  }
  .accent-tag {
    font-weight: 700;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, var(--surface2));
    border: 1px solid color-mix(in srgb, var(--accent) 30%, var(--border));
  }
  .user-tag {
    font-weight: 600;
    color: var(--success);
    background: color-mix(in srgb, var(--success) 12%, var(--surface2));
    border: 1px solid color-mix(in srgb, var(--success) 30%, var(--border));
  }
  .bundled-tag {
    font-weight: 600;
    color: var(--text-muted);
    background: var(--surface2);
    border: 1px solid var(--border);
  }
  .override-tag {
    font-size: 9px;
    font-weight: 600;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 25%, var(--border));
    padding: 1px 6px;
    border-radius: 4px;
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
  .form-title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
  }
  .form-title-row .form-title { margin-bottom: 0; }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 10px;
  }
  .field:last-child { margin-bottom: 0; }

  .field label {
    font-size: 10px;
    color: var(--text-muted);
    font-weight: 500;
  }
  .required { color: var(--danger); }

  .field-row-2 {
    display: flex;
    gap: 12px;
  }
  .field-row-2 .field { flex: 1; }

  .hint {
    font-size: 10px;
    color: var(--text-muted);
    font-style: italic;
  }

  /* Match patterns */
  .pattern-input-row {
    display: flex;
    gap: 6px;
  }
  .pattern-input-row input { flex: 1; }

  .pattern-tags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 4px;
  }
  .pattern-tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    font-family: monospace;
    color: var(--text);
    background: var(--surface2);
    border: 1px solid var(--border);
    padding: 2px 4px 2px 8px;
  }
  .tag-remove {
    width: 16px;
    height: 16px;
    border-radius: 3px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: 11px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    transition: background 0.1s, color 0.1s;
  }
  .tag-remove:hover {
    background: color-mix(in srgb, var(--danger) 15%, var(--surface2));
    color: var(--danger);
  }

  /* ---- Controls table ---- */
  .empty-controls {
    text-align: center;
    padding: 20px;
    color: var(--text-muted);
    font-size: 12px;
  }

  .controls-table {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .controls-header {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 4px 4px;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.07em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .control-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px 4px;
    border-radius: var(--radius);
    transition: background 0.1s;
  }
  .control-row:hover {
    background: color-mix(in srgb, var(--surface2) 50%, transparent);
  }

  .control-row input,
  .control-row select {
    font-size: 11px;
    padding: 3px 4px;
    width: 100%;
  }
  .control-row input[type="number"] {
    -moz-appearance: textfield;
  }

  .ctrl-label { flex: 1; min-width: 80px; }
  .ctrl-type { width: 76px; flex-shrink: 0; }
  .ctrl-midi { width: 56px; flex-shrink: 0; }
  .ctrl-ch { width: 40px; flex-shrink: 0; }
  .ctrl-num { width: 44px; flex-shrink: 0; }
  .ctrl-range {
    width: 84px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .ctrl-range input { width: 36px; }
  .range-sep { color: var(--text-muted); font-size: 10px; }
  .ctrl-group { width: 44px; flex-shrink: 0; }
  .ctrl-section { width: 72px; flex-shrink: 0; }
  .ctrl-role { width: 68px; flex-shrink: 0; }
  .ctrl-del {
    width: 48px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 2px;
  }

  /* ---- Save footer ---- */
  .form-footer {
    display: flex;
    gap: 8px;
    padding-top: 4px;
  }

  .save-error {
    font-size: 11px;
    padding: 6px 8px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--danger) 12%, var(--surface2));
    color: var(--danger);
    border: 1px solid color-mix(in srgb, var(--danger) 30%, var(--border));
  }

  /* ---- Buttons ---- */
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

  /* ---- Modal ---- */
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .modal {
    width: 380px;
    max-width: 90vw;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 16px;
  }
  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .modal-title { font-size: 13px; font-weight: 600; color: var(--text); }
  .modal-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 14px;
    cursor: pointer;
    padding: 2px 6px;
  }
  .modal-close:hover { color: var(--text); }
  .modal-footer { display: flex; gap: 8px; }
</style>
