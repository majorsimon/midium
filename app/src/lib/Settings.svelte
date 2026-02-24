<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { AudioCapabilities } from "./types";

  let config: any = null;
  let caps: AudioCapabilities | null = null;
  let saved = false;

  onMount(async () => {
    [config, caps] = await Promise.all([
      invoke("get_config").catch(() => null),
      invoke("get_capabilities").catch(() => null),
    ]);
  });

  async function save() {
    await invoke("save_config", { config }).catch(console.error);
    saved = true;
    setTimeout(() => (saved = false), 2000);
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

    <div style="display: flex; gap: 8px; align-items: center;">
      <button class="primary" on:click={save}>Save Settings</button>
      {#if saved}
        <span style="color: var(--success); font-size: 12px;">✓ Saved</span>
      {/if}
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
</style>
