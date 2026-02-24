<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import ChannelStrip from "./ChannelStrip.svelte";
  import type { AudioDeviceInfo, AudioSessionInfo, AudioCapabilities } from "./types";

  const NUM_STRIPS = 8;
  const ASSIGNMENTS_KEY = "midium-strip-assignments";

  type StripTarget = null | "master" | { app: string };

  // ---------------------------------------------------------------------------
  // Strip assignments (persisted in localStorage)
  // ---------------------------------------------------------------------------

  let strips: StripTarget[] = loadAssignments();

  function loadAssignments(): StripTarget[] {
    try {
      const saved = localStorage.getItem(ASSIGNMENTS_KEY);
      if (saved) {
        const parsed: unknown = JSON.parse(saved);
        if (Array.isArray(parsed) && parsed.length === NUM_STRIPS) {
          return parsed as StripTarget[];
        }
      }
    } catch {}
    // Default: strip 0 → master, rest unassigned
    const defaults: StripTarget[] = Array(NUM_STRIPS).fill(null);
    defaults[0] = "master";
    return defaults;
  }

  function saveAssignments() {
    try { localStorage.setItem(ASSIGNMENTS_KEY, JSON.stringify(strips)); } catch {}
  }

  function assignStrip(i: number, target: StripTarget) {
    strips[i] = target;
    strips = [...strips];
    saveAssignments();
    pickerOpen = null;
  }

  // ---------------------------------------------------------------------------
  // Audio state
  // ---------------------------------------------------------------------------

  let masterVolume = 0.7;
  let masterMuted = false;
  let sessions: AudioSessionInfo[] = [];
  let devices: AudioDeviceInfo[] = [];
  let caps: AudioCapabilities = {
    per_app_volume: false,
    device_switching: false,
    input_device_switching: false,
  };

  // ---------------------------------------------------------------------------
  // Strip helpers
  // ---------------------------------------------------------------------------

  function getLabel(target: StripTarget): string {
    if (!target) return "";
    if (target === "master") return "Master";
    return target.app;
  }

  function getVolume(target: StripTarget): number {
    if (!target) return 0;
    if (target === "master") return masterVolume;
    return sessions.find(s => s.name === target.app)?.volume ?? 0.8;
  }

  function getMuted(target: StripTarget): boolean {
    if (!target || target === "master") return masterMuted;
    return sessions.find(s => s.name === target.app)?.muted ?? false;
  }

  function isActive(target: StripTarget): boolean {
    if (!target) return false;
    if (target === "master") return true;
    return sessions.some(s => s.name === target.app);
  }

  function isUnavailable(target: StripTarget): boolean {
    if (!target || target === "master") return false;
    return !caps.per_app_volume;
  }

  function isAppAssigned(target: StripTarget, appName: string): boolean {
    return (
      target !== null &&
      typeof target === "object" &&
      "app" in target &&
      (target as { app: string }).app === appName
    );
  }

  // ---------------------------------------------------------------------------
  // Volume / mute handlers
  // ---------------------------------------------------------------------------

  async function handleVolumeChange(i: number, v: number) {
    const t = strips[i];
    if (!t) return;
    if (t === "master") {
      masterVolume = v;
      await invoke("set_volume", { target: "SystemMaster", volume: v }).catch(console.error);
    } else {
      sessions = sessions.map(s =>
        s.name === (t as { app: string }).app ? { ...s, volume: v } : s
      );
      await invoke("set_volume", {
        target: { Application: { name: (t as { app: string }).app } },
        volume: v,
      }).catch(console.error);
    }
  }

  async function handleMuteToggle(i: number) {
    const t = strips[i];
    if (!t) return;
    if (t === "master") {
      masterMuted = !masterMuted;
      await invoke("toggle_mute", { target: "SystemMaster" }).catch(console.error);
    } else {
      const appName = (t as { app: string }).app;
      sessions = sessions.map(s =>
        s.name === appName ? { ...s, muted: !s.muted } : s
      );
      await invoke("toggle_mute", {
        target: { Application: { name: appName } },
      }).catch(console.error);
    }
  }

  async function setDefaultOutput(id: string) {
    await invoke("set_default_output", { deviceId: id }).catch(console.error);
    devices = await invoke<AudioDeviceInfo[]>("list_output_devices").catch(() => devices);
  }

  // ---------------------------------------------------------------------------
  // Picker state
  // ---------------------------------------------------------------------------

  let pickerOpen: number | null = null;

  function togglePicker(i: number) {
    pickerOpen = pickerOpen === i ? null : i;
  }

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  let unlistens: UnlistenFn[] = [];
  let refreshTimer: ReturnType<typeof setInterval>;

  async function loadState() {
    try {
      caps = await invoke("get_capabilities");
      devices = await invoke("list_output_devices");
      masterVolume = await invoke("get_volume", { target: "SystemMaster" });
      if (caps.per_app_volume) {
        sessions = await invoke("list_sessions");
      }
    } catch (e) {
      console.error("Mixer: failed to load state", e);
    }
  }

  onMount(async () => {
    await loadState();

    refreshTimer = setInterval(async () => {
      masterVolume = await invoke<number>("get_volume", { target: "SystemMaster" })
        .catch(() => masterVolume);
      if (caps.per_app_volume) {
        sessions = await invoke<AudioSessionInfo[]>("list_sessions").catch(() => sessions);
      }
    }, 3000);

    unlistens.push(
      await listen<{ target: unknown; volume: number }>("volume-changed", (e) => {
        if (e.payload.target === "SystemMaster") masterVolume = e.payload.volume;
      }),
      await listen<string>("device-connected", () => loadState()),
      await listen<string>("device-disconnected", () => loadState()),
    );
  });

  onDestroy(() => {
    clearInterval(refreshTimer);
    unlistens.forEach(u => u());
  });
</script>

<div class="mixer">
  <!-- Channel strips row -->
  <div class="strips-row">
    {#each strips as target, i}
      <ChannelStrip
        label={getLabel(target)}
        volume={getVolume(target)}
        muted={getMuted(target)}
        active={isActive(target)}
        assigned={target !== null}
        isMaster={target === "master"}
        unavailable={isUnavailable(target)}
        on:volume-change={(e) => handleVolumeChange(i, e.detail)}
        on:mute-toggle={() => handleMuteToggle(i)}
        on:assign-click={() => togglePicker(i)}
      />
    {/each}
  </div>

  <!-- Assignment picker -->
  {#if pickerOpen !== null}
    {@const idx = pickerOpen}
    <div class="card picker-card">
      <div class="section-title" style="margin-bottom: 10px;">
        Assign channel {idx + 1}
      </div>

      <div class="picker-list">
        <!-- Master option -->
        <button
          class="picker-opt"
          class:current={strips[idx] === "master"}
          on:click={() => assignStrip(idx, "master")}
        >
          Master {strips[idx] === "master" ? "✓" : ""}
        </button>

        <!-- App sessions -->
        {#each sessions as s}
          <button
            class="picker-opt"
            class:current={isAppAssigned(strips[idx], s.name)}
            on:click={() => assignStrip(idx, { app: s.name })}
          >
            {s.name} {isAppAssigned(strips[idx], s.name) ? "✓" : ""}
          </button>
        {/each}

        {#if sessions.length === 0}
          <div class="picker-note">
            {#if caps.per_app_volume}
              No apps playing audio right now.
            {:else}
              Per-app volume not yet available on macOS.<br>
              Only <strong>Master</strong> is controllable.
            {/if}
          </div>
        {/if}

        <!-- Unassign -->
        {#if strips[idx] !== null}
          <button
            class="picker-opt danger"
            on:click={() => assignStrip(idx, null)}
          >
            Unassign
          </button>
        {/if}
      </div>

      <button class="picker-close" on:click={() => pickerOpen = null}>Done</button>
    </div>
  {/if}

  <!-- Output device selector -->
  {#if caps.device_switching && devices.length > 0}
    <div class="card device-row">
      <span class="device-label">Output</span>
      <select on:change={(e) => setDefaultOutput(e.currentTarget.value)}>
        {#each devices as dev}
          <option value={dev.id} selected={dev.is_default}>{dev.name}</option>
        {/each}
      </select>
    </div>
  {/if}

  <!-- Platform note when per-app is unavailable -->
  {#if !caps.per_app_volume}
    <div class="platform-note">
      Per-app volume requires the macOS Audio Tap API (14.2+) — coming soon.
      Master volume control is fully functional.
    </div>
  {/if}
</div>

<style>
  .mixer {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  /* ---- Strips row ---- */
  .strips-row {
    display: flex;
    gap: 8px;
    overflow-x: auto;
    padding-bottom: 6px;
  }

  /* ---- Picker ---- */
  .picker-card {
    margin-top: 16px;
    max-width: 480px;
  }

  .picker-list {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 12px;
  }

  .picker-opt {
    font-size: 12px;
    padding: 4px 12px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface2);
    color: var(--text);
    cursor: pointer;
    transition: background 0.1s, border-color 0.1s;
    white-space: nowrap;
  }

  .picker-opt:hover {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface2));
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
  }

  .picker-opt.current {
    background: color-mix(in srgb, var(--accent) 18%, var(--surface2));
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 600;
  }

  .picker-opt.danger {
    color: var(--danger);
    border-color: color-mix(in srgb, var(--danger) 40%, var(--border));
  }
  .picker-opt.danger:hover {
    background: color-mix(in srgb, var(--danger) 10%, var(--surface2));
    border-color: var(--danger);
  }

  .picker-note {
    font-size: 11px;
    color: var(--text-muted);
    padding: 4px 0;
    line-height: 1.5;
  }

  .picker-close {
    font-size: 11px;
    padding: 4px 14px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface2);
    color: var(--text-muted);
    cursor: pointer;
  }
  .picker-close:hover { color: var(--text); }

  /* ---- Device row ---- */
  .device-row {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 16px;
  }

  .device-label {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
  }

  .device-row select { flex: 1; }

  /* ---- Platform note ---- */
  .platform-note {
    margin-top: 16px;
    font-size: 11px;
    color: var(--text-muted);
    padding: 8px 12px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    line-height: 1.5;
    max-width: 480px;
  }
</style>
