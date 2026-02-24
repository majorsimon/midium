<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import ChannelStrip from "./ChannelStrip.svelte";
  import type {
    AudioDeviceInfo,
    AudioSessionInfo,
    AudioCapabilities,
    Mapping,
    DeviceProfile,
  } from "./types";

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
    const defaults: StripTarget[] = Array(NUM_STRIPS).fill(null);
    defaults[0] = "master";
    return defaults;
  }

  function saveAssignments() {
    try { localStorage.setItem(ASSIGNMENTS_KEY, JSON.stringify(strips)); } catch {}
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
  // Profile + mapping state (for LED feedback)
  // ---------------------------------------------------------------------------

  let profiles: DeviceProfile[] = [];
  let mappings: Mapping[] = [];

  // ---------------------------------------------------------------------------
  // Reactive strip data — explicit dependencies so Svelte tracks them correctly
  // ---------------------------------------------------------------------------

  $: stripVolumes = strips.map(target => {
    if (!target) return 0;
    if (target === "master") return masterVolume;
    return sessions.find(s => s.name === (target as { app: string }).app)?.volume ?? 0.8;
  });

  $: stripMuted = strips.map(target => {
    if (!target || target === "master") return masterMuted;
    return sessions.find(s => s.name === (target as { app: string }).app)?.muted ?? false;
  });

  $: stripActive = strips.map(target => {
    if (!target) return false;
    if (target === "master") return true;
    return sessions.some(s => s.name === (target as { app: string }).app);
  });

  // ---------------------------------------------------------------------------
  // Strip helpers
  // ---------------------------------------------------------------------------

  function getLabel(target: StripTarget): string {
    if (!target) return "";
    if (target === "master") return "Master";
    return (target as { app: string }).app;
  }

  function isUnavailable(target: StripTarget): boolean {
    if (!target || target === "master") return false;
    return !caps.per_app_volume;
  }

  // ---------------------------------------------------------------------------
  // LED feedback helpers
  // ---------------------------------------------------------------------------

  /**
   * Find the MIDI device+CC for a strip's S/M/R button.
   *
   * Algorithm:
   * 1. Find the SetVolume mapping whose target matches the strip target
   * 2. Get the CC number from that mapping (= the physical fader CC)
   * 3. Look up the matching device profile
   * 4. Find the profile control with that CC → get its group
   * 5. Find the button with the desired role in the same group
   */
  function findLedTarget(
    target: StripTarget,
    role: "solo" | "mute" | "record"
  ): { device: string; channel: number; cc: number } | null {
    if (!target) return null;

    // Find the fader mapping for this target
    const faderMapping = mappings.find(m => {
      if (typeof m.action === "string") return false;
      if (!("SetVolume" in m.action)) return false;
      const t = m.action.SetVolume.target;
      if (target === "master") return t === "SystemMaster";
      const appName = (target as { app: string }).app;
      return (
        typeof t === "object" &&
        "Application" in t &&
        t.Application.name === appName
      );
    });
    if (!faderMapping) return null;

    const ctType = faderMapping.control.control_type;
    if (typeof ctType !== "object" || !("CC" in ctType)) return null;
    const faderCC = (ctType as { CC: number }).CC;
    const faderCh = faderMapping.control.channel;

    // Find matching profile
    const profile = profiles.find(p =>
      p.match_patterns.some(pat =>
        faderMapping.control.device.toLowerCase().includes(pat.toLowerCase())
      )
    );
    if (!profile) return null;

    // Find the slider control in this profile that matches fader CC + channel
    const faderControl = profile.controls.find(c =>
      c.number === faderCC &&
      c.channel === faderCh &&
      c.control_type === "slider"
    );
    if (!faderControl?.group) return null;

    // Find the button with the matching role in the same group
    const buttonControl = profile.controls.find(c =>
      c.group === faderControl.group && c.button_role === role
    );
    if (!buttonControl) return null;

    return {
      device: faderMapping.control.device,
      channel: buttonControl.channel,
      cc: buttonControl.number,
    };
  }

  async function sendLed(
    target: StripTarget,
    role: "solo" | "mute" | "record",
    on: boolean
  ) {
    const led = findLedTarget(target, role);
    if (!led) return;
    const data = [0xB0 | led.channel, led.cc, on ? 127 : 0];
    await invoke("send_midi", { device: led.device, data }).catch(() => {});
  }

  async function syncStripLeds(i: number) {
    const t = strips[i];
    const assigned = t !== null;
    const muted = stripMuted[i];
    const active = stripActive[i];

    await Promise.all([
      sendLed(t, "solo",   assigned),
      sendLed(t, "mute",   muted),
      sendLed(t, "record", active),
    ]);
  }

  async function syncAllLeds() {
    for (let i = 0; i < NUM_STRIPS; i++) {
      await syncStripLeds(i);
    }
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
      const appName = (t as { app: string }).app;
      sessions = sessions.map(s => s.name === appName ? { ...s, volume: v } : s);
      await invoke("set_volume", {
        target: { Application: { name: appName } },
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
      sessions = sessions.map(s => s.name === appName ? { ...s, muted: !s.muted } : s);
      await invoke("toggle_mute", {
        target: { Application: { name: appName } },
      }).catch(console.error);
    }
    // Update M LED immediately
    await sendLed(t, "mute", stripMuted[i]);
  }

  // ---------------------------------------------------------------------------
  // Assignment picker
  // ---------------------------------------------------------------------------

  let pickerOpen: number | null = null;

  function togglePicker(i: number) {
    pickerOpen = pickerOpen === i ? null : i;
  }

  async function assignStrip(i: number, target: StripTarget) {
    strips[i] = target;
    strips = [...strips];
    saveAssignments();
    pickerOpen = null;
    // Update LEDs for this strip after re-assignment
    await syncStripLeds(i);
  }

  function isAppAssigned(target: StripTarget, appName: string): boolean {
    return (
      target !== null &&
      typeof target === "object" &&
      "app" in target &&
      (target as { app: string }).app === appName
    );
  }

  async function setDefaultOutput(id: string) {
    await invoke("set_default_output", { deviceId: id }).catch(console.error);
    devices = await invoke<AudioDeviceInfo[]>("list_output_devices").catch(() => devices);
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

    // Load profiles and mappings for LED feedback
    [profiles, mappings] = await Promise.all([
      invoke<DeviceProfile[]>("list_profiles").catch(() => []),
      invoke<Mapping[]>("get_mappings").catch(() => []),
    ]);

    // Push current LED state to controller on startup
    await syncAllLeds();

    refreshTimer = setInterval(async () => {
      masterVolume = await invoke<number>("get_volume", { target: "SystemMaster" })
        .catch(() => masterVolume);
      if (caps.per_app_volume) {
        const prev = sessions;
        sessions = await invoke<AudioSessionInfo[]>("list_sessions").catch(() => sessions);
        // Re-sync R LEDs when sessions change
        if (sessions !== prev) await syncAllLeds();
      }
    }, 3000);

    unlistens.push(
      await listen<{ target: unknown; volume: number }>("volume-changed", (e) => {
        if (e.payload.target === "SystemMaster") masterVolume = e.payload.volume;
      }),
      await listen<string>("device-connected", async () => {
        await loadState();
        // Re-sync all LEDs when a new device connects
        await syncAllLeds();
      }),
      await listen<string>("device-disconnected", () => loadState()),
    );
  });

  onDestroy(() => {
    clearInterval(refreshTimer);
    unlistens.forEach(u => u());
  });
</script>

<div class="mixer">
  <!-- Channel strips -->
  <div class="strips-row">
    {#each strips as target, i}
      <ChannelStrip
        label={getLabel(target)}
        volume={stripVolumes[i]}
        muted={stripMuted[i]}
        active={stripActive[i]}
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
        <button
          class="picker-opt"
          class:current={strips[idx] === "master"}
          on:click={() => assignStrip(idx, "master")}
        >
          Master {strips[idx] === "master" ? "✓" : ""}
        </button>

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

  {#if !caps.per_app_volume}
    <div class="platform-note">
      Per-app volume requires the macOS Audio Tap API (14.2+) — coming soon.
      Master volume control is fully functional. MIDI LED feedback is active.
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

  .strips-row {
    display: flex;
    gap: 8px;
    overflow-x: auto;
    padding-bottom: 6px;
  }

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
