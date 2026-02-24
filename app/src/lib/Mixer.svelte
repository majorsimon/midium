<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import type { AudioDeviceInfo, AudioSessionInfo, AudioCapabilities } from "./types";

  let devices: AudioDeviceInfo[] = [];
  let sessions: AudioSessionInfo[] = [];
  let caps: AudioCapabilities = { per_app_volume: false, device_switching: false, input_device_switching: false };
  let masterVolume = 0.5;
  let masterMuted = false;
  let unlistens: UnlistenFn[] = [];
  let refreshTimer: ReturnType<typeof setInterval>;

  async function loadState() {
    try {
      caps = await invoke("get_capabilities");
      devices = await invoke("list_output_devices");
      if (caps.per_app_volume) {
        sessions = await invoke("list_sessions");
      }
      masterVolume = await invoke("get_volume", { target: "SystemMaster" });
    } catch (e) {
      console.error("Failed to load audio state:", e);
    }
  }

  async function setMasterVolume(v: number) {
    masterVolume = v;
    await invoke("set_volume", { target: "SystemMaster", volume: v }).catch(console.error);
  }

  async function toggleMasterMute() {
    await invoke("toggle_mute", { target: "SystemMaster" }).catch(console.error);
    masterMuted = !masterMuted;
  }

  async function setSessionVolume(name: string, volume: number) {
    sessions = sessions.map(s => s.name === name ? { ...s, volume } : s);
    await invoke("set_volume", {
      target: { Application: { name } },
      volume,
    }).catch(console.error);
  }

  async function setDefaultOutput(id: string) {
    await invoke("set_default_output", { deviceId: id }).catch(console.error);
    await loadState();
  }

  onMount(async () => {
    await loadState();

    // Refresh sessions periodically (volume may change outside the app)
    refreshTimer = setInterval(async () => {
      if (caps.per_app_volume) {
        sessions = await invoke("list_sessions").catch(() => sessions);
      }
      masterVolume = await invoke("get_volume", { target: "SystemMaster" })
        .catch(() => masterVolume);
    }, 3000);

    // Real-time updates from EventBus
    unlistens.push(
      await listen<{ target: unknown; volume: number }>("volume-changed", (e) => {
        if (e.payload.target === "SystemMaster") {
          masterVolume = e.payload.volume;
        }
      })
    );
    unlistens.push(
      await listen<string>("device-connected", () => loadState())
    );
    unlistens.push(
      await listen<string>("device-disconnected", () => loadState())
    );
  });

  onDestroy(() => {
    clearInterval(refreshTimer);
    unlistens.forEach(u => u());
  });
</script>

<div class="mixer">
  <!-- Master strip -->
  <div class="card master-strip">
    <div class="section-title">Master Output</div>

    <div class="strip-row">
      <div class="volume-area">
        <div class="vol-label">{Math.round(masterVolume * 100)}%</div>
        <input
          type="range" min="0" max="1" step="0.005"
          value={masterVolume}
          on:input={(e) => setMasterVolume(+e.currentTarget.value)}
          class:muted={masterMuted}
          style="width: 100%;"
        />
      </div>

      <button
        class:danger={masterMuted}
        class:active={masterMuted}
        on:click={toggleMasterMute}
        title={masterMuted ? "Unmute" : "Mute"}
        style="width: 60px;"
      >
        {masterMuted ? "🔇 Muted" : "🔊"}
      </button>
    </div>

    {#if caps.device_switching && devices.length > 0}
      <div class="device-select" style="margin-top: 12px;">
        <label>Output Device</label>
        <select on:change={(e) => setDefaultOutput(e.currentTarget.value)}>
          {#each devices as dev}
            <option value={dev.id} selected={dev.is_default}>{dev.name}</option>
          {/each}
        </select>
      </div>
    {/if}
  </div>

  <!-- Per-app sessions -->
  {#if caps.per_app_volume && sessions.length > 0}
    <div class="card" style="margin-top: 16px;">
      <div class="section-title">Applications</div>
      <div class="sessions-grid">
        {#each sessions as session}
          <div class="session-strip">
            <div class="session-name" title={session.name}>
              {session.name.length > 18 ? session.name.slice(0, 16) + "…" : session.name}
            </div>
            {#if session.muted}
              <span class="tag muted">muted</span>
            {/if}
            <div class="vol-label">{Math.round(session.volume * 100)}%</div>
            <input
              type="range" min="0" max="1" step="0.005"
              value={session.volume}
              on:input={(e) => setSessionVolume(session.name, +e.currentTarget.value)}
              class:muted={session.muted}
            />
          </div>
        {/each}
      </div>
    </div>
  {:else if caps.per_app_volume}
    <div class="card" style="margin-top: 16px; text-align: center; color: var(--text-muted);">
      No applications playing audio
    </div>
  {:else}
    <div class="card" style="margin-top: 16px; color: var(--text-muted);">
      Per-app volume is not supported on this platform (macOS limitation).
    </div>
  {/if}
</div>

<style>
  .mixer { padding: 20px; max-width: 860px; }
  .master-strip { }
  .strip-row { display: flex; align-items: center; gap: 12px; }
  .volume-area { flex: 1; }
  .vol-label { font-size: 12px; color: var(--text-muted); margin-bottom: 4px; }
  .device-select select { width: 100%; }

  .sessions-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 12px;
  }
  .session-strip {
    background: var(--surface2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px;
  }
  .session-name {
    font-size: 12px;
    font-weight: 500;
    margin-bottom: 4px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  input[type="range"].muted::-webkit-slider-runnable-track { background: var(--danger); opacity: 0.4; }
</style>
