<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  let presets: string[] = $state([]);
  let activePreset: string | null = $state(null);
  let showSaveModal = $state(false);
  let saveName = $state("");
  let saveError = $state("");
  let pendingDelete: string | null = $state(null);
  let loading = $state(false);

  async function refresh() {
    [presets, activePreset] = await Promise.all([
      invoke<string[]>("list_presets").catch(() => [] as string[]),
      invoke<string | null>("get_active_preset").catch(() => null),
    ]);
  }

  onMount(refresh);

  async function loadPreset(name: string) {
    loading = true;
    try {
      await invoke("load_preset", { name });
      activePreset = name;
    } catch (e) {
      console.error("Failed to load preset:", e);
    } finally {
      loading = false;
    }
  }

  async function openSave() {
    saveName = activePreset ?? "";
    saveError = "";
    showSaveModal = true;
  }

  async function doSave() {
    if (!saveName.trim()) {
      saveError = "Name cannot be empty";
      return;
    }
    try {
      await invoke("save_preset", { name: saveName.trim() });
      showSaveModal = false;
      await refresh();
    } catch (e) {
      saveError = String(e);
    }
  }

  async function confirmDelete(name: string) {
    try {
      await invoke("delete_preset", { name });
      pendingDelete = null;
      await refresh();
    } catch (e) {
      console.error("Failed to delete preset:", e);
    }
  }
</script>

<div class="presets">
  <div class="presets-header">
    <span class="presets-title">Presets</span>
    <button class="primary" onclick={openSave}>Save as…</button>
  </div>

  {#if presets.length === 0}
    <div class="empty">No saved presets. Click "Save as…" to save your current mappings as a preset.</div>
  {:else}
    <div class="preset-list">
      {#each presets as name}
        <div class="preset-row" class:active={name === activePreset}>
          <span class="preset-name" title={name}>
            {name}
            {#if name === activePreset}
              <span class="active-badge">active</span>
            {/if}
          </span>
          <div class="preset-actions">
            {#if pendingDelete === name}
              <button
                class="del-btn del-yes"
                title="Confirm delete"
                onclick={() => confirmDelete(name)}
              >✓</button>
              <button
                class="del-btn del-no"
                title="Cancel"
                onclick={() => pendingDelete = null}
              >✗</button>
            {:else}
              <button
                class="load-btn"
                onclick={() => loadPreset(name)}
                disabled={loading}
              >Load</button>
              <button
                class="del-btn"
                onclick={() => pendingDelete = name}
                title="Delete"
              >×</button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if showSaveModal}
  <div class="modal-overlay" onclick={(e) => { if (e.target === e.currentTarget) showSaveModal = false; }}>
    <div class="modal card">
      <div class="modal-header">
        <span class="modal-title">Save Preset</span>
        <button class="modal-close" onclick={() => showSaveModal = false}>✕</button>
      </div>
      <div class="field">
        <label>Preset name</label>
        <input
          type="text"
          bind:value={saveName}
          placeholder="e.g. My Setup"
          onkeydown={(e) => { if (e.key === "Enter") doSave(); }}
        />
      </div>
      {#if saveError}
        <div class="save-error">{saveError}</div>
      {/if}
      <div class="modal-footer">
        <button class="primary" onclick={doSave}>Save</button>
        <button onclick={() => showSaveModal = false}>Cancel</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .presets {
    padding: 0 20px 16px;
  }
  .presets-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 2px 10px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 8px;
  }
  .presets-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
  }

  .empty {
    text-align: center;
    padding: 16px;
    color: var(--text-muted);
    font-size: 12px;
  }

  .preset-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .preset-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 10px;
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px solid var(--border);
    transition: background 0.1s;
  }
  .preset-row:hover {
    background: var(--surface2);
  }
  .preset-row.active {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
    background: color-mix(in srgb, var(--accent) 6%, var(--surface));
  }
  .preset-name {
    font-size: 12px;
    font-weight: 500;
    color: var(--text);
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .active-badge {
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    padding: 1px 5px;
    border-radius: 4px;
  }
  .preset-actions {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }
  .load-btn {
    font-size: 11px;
    padding: 2px 10px;
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
  .save-error {
    font-size: 11px;
    color: var(--danger);
  }
</style>
