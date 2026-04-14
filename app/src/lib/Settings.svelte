<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { AudioCapabilities } from "./types";

  import type { DeviceProfile } from "./types";

  let config: any = null;
  let caps: AudioCapabilities | null = null;
  let profiles: DeviceProfile[] = [];
  let saved = false;
  let shortcutKey = "";
  let shortcutEnabled = false;

  // Export/import state
  let exportContent = "";
  let exportModalType: "mappings" | "profile" | null = null;
  let importContent = "";
  let importStatus = "";
  let showImportModal = false;
  let importModalType: "mappings" | "profile" = "mappings";

  onMount(async () => {
    [config, caps, profiles] = await Promise.all([
      invoke("get_config").catch(() => null),
      invoke<AudioCapabilities>("get_capabilities").catch(() => null),
      invoke<DeviceProfile[]>("list_profiles").catch(() => [] as DeviceProfile[]),
    ]);
    const currentShortcut = await invoke<string | null>("get_shortcut").catch(() => null);
    shortcutKey = currentShortcut ?? "CmdOrCtrl+Shift+M";
    shortcutEnabled = currentShortcut !== null;
  });

  async function updateShortcut() {
    try {
      const value = shortcutEnabled ? shortcutKey : null;
      await invoke("set_shortcut", { shortcut: value });
      if (config) {
        config.general.shortcut = value;
      }
    } catch (e) {
      console.error("Failed to set shortcut:", e);
    }
  }

  async function save() {
    await invoke("save_config", { config }).catch(console.error);
    saved = true;
    setTimeout(() => (saved = false), 2000);
  }

  async function startExportMappings() {
    exportContent = await invoke<string>("export_mappings").catch(e => `# Error: ${e}`);
    exportModalType = "mappings";
  }

  async function startExportProfile(name: string) {
    exportContent = await invoke<string>("export_profile", { name }).catch(e => `# Error: ${e}`);
    exportModalType = "profile";
  }

  function downloadExport() {
    const filename = exportModalType === "profile" ? "profile.toml" : "mappings.toml";
    const blob = new Blob([exportContent], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
  }

  function openImport(type: "mappings" | "profile") {
    importModalType = type;
    importContent = "";
    importStatus = "";
    showImportModal = true;
  }

  async function doImport() {
    importStatus = "";
    if (!importContent.trim()) { importStatus = "Paste TOML content above."; return; }
    try {
      if (importModalType === "mappings") {
        await invoke("import_mappings", { content: importContent });
        importStatus = "✓ Mappings imported successfully.";
      } else {
        const name = await invoke<string>("import_profile", { content: importContent });
        importStatus = `✓ Profile "${name}" imported. Restart to apply.`;
      }
      importContent = "";
    } catch (e) {
      importStatus = `Error: ${e}`;
    }
  }
</script>

<div style="padding: 20px; max-width: 600px;">
  {#if !config}
    <div class="card" style="color: var(--text-muted);">Loading config…</div>
  {:else}
    <!-- General -->
    <div class="card" style="margin-bottom: 16px;">
      <div class="section-title">General</div>
      <div class="field-row">
        <label>Log Level</label>
        <select bind:value={config.general.log_level}>
          <option>error</option>
          <option>warn</option>
          <option>info</option>
          <option>debug</option>
          <option>trace</option>
        </select>
      </div>
      <div class="field-row">
        <label>Global show/hide shortcut</label>
        <input type="checkbox" bind:checked={shortcutEnabled} on:change={updateShortcut} />
      </div>
      {#if shortcutEnabled}
        <div class="field-row">
          <label>Shortcut key</label>
          <input
            type="text"
            bind:value={shortcutKey}
            on:change={updateShortcut}
            placeholder="CmdOrCtrl+Shift+M"
            style="width: 200px; font-family: monospace; font-size: 11px;"
          />
        </div>
      {/if}
    </div>

    <!-- MIDI -->
    <div class="card" style="margin-bottom: 16px;">
      <div class="section-title">MIDI</div>
      <div class="field-row">
        <label>Device Poll Interval (seconds)</label>
        <input type="number" min="1" max="30" bind:value={config.midi.poll_interval_secs} />
      </div>
      <div class="field-row">
        <label>Auto-connect new devices</label>
        <input type="checkbox" bind:checked={config.midi.auto_connect} />
      </div>
    </div>

    <!-- Audio -->
    <div class="card" style="margin-bottom: 16px;">
      <div class="section-title">Audio</div>
      <div class="field-row">
        <label>Refresh Interval (seconds)</label>
        <input type="number" min="1" max="60" bind:value={config.audio.refresh_interval_secs} />
      </div>
      {#if caps}
        <div style="margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--border);">
          <div class="section-title">Platform Capabilities</div>
          <div class="caps-row">
            <span>Per-App Volume</span>
            <span class="tag" class:active={caps.per_app_volume}>
              {caps.per_app_volume ? "Supported" : "Not Supported"}
            </span>
          </div>
          <div class="caps-row">
            <span>Output Device Switching</span>
            <span class="tag" class:active={caps.device_switching}>
              {caps.device_switching ? "Supported" : "Not Supported"}
            </span>
          </div>
        </div>
      {/if}
    </div>

    <div style="display: flex; gap: 8px; align-items: center; margin-bottom: 24px;">
      <button class="primary" on:click={save}>Save Settings</button>
      {#if saved}
        <span style="color: var(--success); font-size: 12px;">✓ Saved</span>
      {/if}
    </div>
  {/if}

  <!-- Export / Import -->
  <div class="card" style="margin-bottom: 16px;">
    <div class="section-title">Export</div>
    <div class="field-row">
      <label>Mappings</label>
      <button on:click={startExportMappings}>Export mappings.toml</button>
    </div>
    {#if profiles.length > 0}
      <div class="field-row" style="align-items: flex-start; flex-direction: column; gap: 6px;">
        <label>Device Profiles</label>
        <div style="display: flex; flex-wrap: wrap; gap: 6px;">
          {#each profiles.filter(p => p.controls.length > 0) as p}
            <button on:click={() => startExportProfile(p.name)} style="font-size: 11px;">
              {p.name}
            </button>
          {/each}
        </div>
      </div>
    {/if}
  </div>

  <div class="card" style="margin-bottom: 16px;">
    <div class="section-title">Import</div>
    <div class="field-row">
      <label>Mappings (TOML)</label>
      <button on:click={() => openImport("mappings")}>Import mappings…</button>
    </div>
    <div class="field-row">
      <label>Device Profile (TOML)</label>
      <button on:click={() => openImport("profile")}>Import profile…</button>
    </div>
  </div>

  <!-- Export modal -->
  {#if exportModalType}
    <div class="modal-overlay" on:click|self={() => exportModalType = null}>
      <div class="modal card">
        <div class="modal-header">
          <span class="modal-title">
            {exportModalType === "mappings" ? "Export Mappings" : "Export Profile"}
          </span>
          <button class="modal-close" on:click={() => exportModalType = null}>✕</button>
        </div>
        <textarea class="export-area" readonly value={exportContent}></textarea>
        <div class="modal-footer">
          <button class="primary" on:click={downloadExport}>Download .toml</button>
          <button on:click={() => navigator.clipboard.writeText(exportContent)}>Copy to clipboard</button>
          <button on:click={() => exportModalType = null}>Close</button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Import modal -->
  {#if showImportModal}
    <div class="modal-overlay" on:click|self={() => showImportModal = false}>
      <div class="modal card">
        <div class="modal-header">
          <span class="modal-title">
            {importModalType === "mappings" ? "Import Mappings" : "Import Device Profile"}
          </span>
          <button class="modal-close" on:click={() => showImportModal = false}>✕</button>
        </div>
        <p class="modal-hint">Paste the TOML content below, then click Import.</p>
        <textarea
          class="export-area"
          placeholder="Paste TOML here…"
          bind:value={importContent}
        ></textarea>
        {#if importStatus}
          <div
            class="import-status"
            class:ok={importStatus.startsWith("✓")}
          >{importStatus}</div>
        {/if}
        <div class="modal-footer">
          <button class="primary" on:click={doImport}>Import</button>
          <button on:click={() => showImportModal = false}>Cancel</button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .field-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 6px 0;
  }
  .field-row label { margin: 0; color: var(--text); font-size: 12px; }
  .field-row input[type="number"] { width: 80px; }
  .field-row input[type="checkbox"] { width: auto; cursor: pointer; }
  .caps-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 0;
    font-size: 12px;
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
    width: 580px;
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
  .modal-hint { font-size: 11px; color: var(--text-muted); margin: 0; }
  .export-area {
    width: 100%;
    height: 260px;
    font-family: monospace;
    font-size: 11px;
    resize: vertical;
    background: var(--surface2);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px;
    box-sizing: border-box;
  }
  .modal-footer { display: flex; gap: 8px; }
  .import-status {
    font-size: 11px;
    padding: 6px 8px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--danger) 12%, var(--surface2));
    color: var(--danger);
    border: 1px solid color-mix(in srgb, var(--danger) 30%, var(--border));
  }
  .import-status.ok {
    background: color-mix(in srgb, var(--success) 12%, var(--surface2));
    color: var(--success);
    border-color: color-mix(in srgb, var(--success) 30%, var(--border));
  }
</style>
