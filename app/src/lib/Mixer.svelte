<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import ChannelStrip from "./ChannelStrip.svelte";
  import type {
    Action,
    AudioTarget,
    AudioDeviceInfo,
    AudioSessionInfo,
    AudioCapabilities,
    Mapping,
    DeviceProfile,
  } from "./types";

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
  // Mapping + profile state
  // ---------------------------------------------------------------------------

  let profiles: DeviceProfile[] = [];
  let mappings: Mapping[] = [];

  // ---------------------------------------------------------------------------
  // Derived strips — one strip per SetVolume mapping (order = mapping order)
  //
  // ActionGroup mappings that contain SetVolume sub-actions become a single
  // strip whose fader controls all of the group's targets simultaneously.
  // ---------------------------------------------------------------------------

  /** Extract all SetVolume AudioTargets from any action (handles ActionGroup). */
  function getSetVolumeTargets(action: Action): AudioTarget[] {
    if (typeof action === "string") return [];
    if ("SetVolume" in action) return [action.SetVolume.target];
    if ("ActionGroup" in action) {
      return action.ActionGroup.actions.flatMap(a =>
        typeof a !== "string" && "SetVolume" in a ? [a.SetVolume.target] : []
      );
    }
    return [];
  }

  /** Convert an AudioTarget to the local StripTarget discriminant. */
  function toStripTarget(t: AudioTarget): StripTarget {
    if (t === "SystemMaster") return "master";
    if (t === "FocusedApplication") return "master";
    if (typeof t === "object" && "Application" in t) return { app: t.Application.name };
    if (typeof t === "object" && "Device" in t) return { device: t.Device.id };
    return null;
  }

  /** Human-readable label for a list of AudioTargets (joined with " + "). */
  function targetsLabel(targets: AudioTarget[]): string {
    return targets.map(t => {
      if (t === "SystemMaster") return "Master";
      if (t === "FocusedApplication") return "Focused";
      if (typeof t === "object" && "Application" in t) return t.Application.name;
      if (typeof t === "object" && "Device" in t)
        return devices.find(d => d.id === (t as { Device: { id: string } }).Device.id)?.name
          ?? (t as { Device: { id: string } }).Device.id;
      return "?";
    }).join(" + ");
  }

  // "Strip" type (local discriminant, mirrors what ChannelStrip props expect)
  type StripTarget = null | "master" | { app: string } | { device: string };

  $: derivedStrips = mappings
    .map(m => ({ mapping: m, svTargets: getSetVolumeTargets(m.action) }))
    .filter(s => s.svTargets.length > 0);

  // Primary target — used for volume-reading, mute-state, and LED lookup.
  function primaryStripTarget(svTargets: AudioTarget[]): StripTarget {
    return toStripTarget(svTargets[0] ?? ("SystemMaster" as AudioTarget));
  }

  function isDeviceTarget(t: StripTarget): t is { device: string } {
    return typeof t === "object" && t !== null && "device" in t;
  }

  // ---------------------------------------------------------------------------
  // Reactive strip data
  // ---------------------------------------------------------------------------

  $: stripVolumes = derivedStrips.map(s => {
    const t = primaryStripTarget(s.svTargets);
    if (!t || isDeviceTarget(t)) return 0;
    if (t === "master") return masterVolume;
    return sessions.find(ss => ss.name === (t as { app: string }).app)?.volume ?? 0.8;
  });

  $: stripMuted = derivedStrips.map(s => {
    const t = primaryStripTarget(s.svTargets);
    if (!t || isDeviceTarget(t)) return false;
    if (t === "master") return masterMuted;
    return sessions.find(ss => ss.name === (t as { app: string }).app)?.muted ?? false;
  });

  $: stripActive = derivedStrips.map(s => {
    const t = primaryStripTarget(s.svTargets);
    if (!t) return false;
    if (t === "master") return true;
    if (isDeviceTarget(t))
      return devices.find(d => d.id === (t as { device: string }).device)?.is_default ?? false;
    return sessions.some(ss => ss.name === (t as { app: string }).app);
  });

  // ---------------------------------------------------------------------------
  // LED feedback helpers
  // ---------------------------------------------------------------------------

  function findAllLedTargets(
    target: StripTarget,
    role: "solo" | "mute" | "record"
  ): { device: string; channel: number; cc: number }[] {
    if (!target || isDeviceTarget(target)) return [];

    const faderMappings = mappings.filter(m => {
      if (typeof m.action === "string") return false;
      if (!("SetVolume" in m.action)) return false;
      const t = m.action.SetVolume.target;
      if (target === "master") return t === "SystemMaster";
      const appName = (target as { app: string }).app;
      return typeof t === "object" && "Application" in t && t.Application.name === appName;
    });

    const results: { device: string; channel: number; cc: number }[] = [];

    for (const fm of faderMappings) {
      const ctType = fm.control.control_type;
      if (typeof ctType !== "object" || !("CC" in ctType)) continue;
      const faderCC = (ctType as { CC: number }).CC;
      const faderCh = fm.control.channel;

      const profile = profiles.find(p =>
        p.match_patterns.some(pat =>
          fm.control.device.toLowerCase().includes(pat.toLowerCase())
        )
      );
      if (!profile) continue;

      const faderControl = profile.controls.find(c =>
        c.number === faderCC && c.channel === faderCh && c.control_type === "slider"
      );
      if (!faderControl?.group) continue;

      const buttonControl = profile.controls.find(c =>
        c.group === faderControl.group && c.button_role === role
      );
      if (!buttonControl) continue;

      results.push({ device: fm.control.device, channel: buttonControl.channel, cc: buttonControl.number });
    }

    return results;
  }

  async function sendLed(target: StripTarget, role: "solo" | "mute" | "record", on: boolean) {
    const leds = findAllLedTargets(target, role);
    await Promise.all(leds.map(led => {
      const data = [0xB0 | led.channel, led.cc, on ? 127 : 0];
      return invoke("send_midi", { device: led.device, data }).catch(() => {});
    }));
  }

  async function syncStripLeds(i: number) {
    const s = derivedStrips[i];
    if (!s) return;
    const t = primaryStripTarget(s.svTargets);
    await Promise.all([
      sendLed(t, "solo",   t !== null),
      sendLed(t, "mute",   stripMuted[i]),
      sendLed(t, "record", stripActive[i]),
    ]);
  }

  async function syncAllLeds() {
    for (let i = 0; i < derivedStrips.length; i++) await syncStripLeds(i);
  }

  // ---------------------------------------------------------------------------
  // Volume / mute / device handlers
  // ---------------------------------------------------------------------------

  async function handleVolumeChange(i: number, v: number) {
    const s = derivedStrips[i];
    if (!s) return;
    // Apply to every SetVolume target in the mapping (handles ActionGroup)
    for (const target of s.svTargets) {
      if (target === "SystemMaster") {
        masterVolume = v;
        await invoke("set_volume", { target: "SystemMaster", volume: v }).catch(console.error);
      } else if (target === "FocusedApplication") {
        await invoke("set_volume", { target: "FocusedApplication", volume: v }).catch(console.error);
      } else if (typeof target === "object" && "Application" in target) {
        const name = target.Application.name;
        sessions = sessions.map(ss => ss.name === name ? { ...ss, volume: v } : ss);
        await invoke("set_volume", { target, volume: v }).catch(console.error);
      } else if (typeof target === "object" && "Device" in target) {
        await invoke("set_volume", { target, volume: v }).catch(console.error);
      }
    }
  }

  async function handleMuteToggle(i: number) {
    const s = derivedStrips[i];
    if (!s) return;
    const t = primaryStripTarget(s.svTargets);
    if (!t || isDeviceTarget(t)) return;

    if (t === "master") {
      masterMuted = !masterMuted;
      await invoke("toggle_mute", { target: "SystemMaster" }).catch(console.error);
    } else {
      const name = (t as { app: string }).app;
      sessions = sessions.map(ss => ss.name === name ? { ...ss, muted: !ss.muted } : ss);
      await invoke("toggle_mute", { target: { Application: { name } } }).catch(console.error);
    }
    await sendLed(t, "mute", stripMuted[i]);
  }

  async function handleRClick(i: number) {
    const s = derivedStrips[i];
    if (!s) return;
    const t = primaryStripTarget(s.svTargets);
    if (!isDeviceTarget(t)) return;
    const deviceId = (t as { device: string }).device;
    await invoke("set_default_output", { deviceId }).catch(console.error);
    devices = await invoke<AudioDeviceInfo[]>("list_output_devices").catch(() => devices);
    await syncStripLeds(i);
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
      caps     = await invoke("get_capabilities");
      devices  = await invoke("list_output_devices");
      [masterVolume, masterMuted] = await Promise.all([
        invoke<number>("get_volume", { target: "SystemMaster" }),
        invoke<boolean>("get_muted", { target: "SystemMaster" }).catch(() => false),
      ]);
      if (caps.per_app_volume) sessions = await invoke("list_sessions");
    } catch (e) {
      console.error("Mixer: failed to load state", e);
    }
  }

  async function loadMappings() {
    [profiles, mappings] = await Promise.all([
      invoke<DeviceProfile[]>("list_profiles").catch(() => [] as DeviceProfile[]),
      invoke<Mapping[]>("get_mappings").catch(() => [] as Mapping[]),
    ]);
  }

  onMount(async () => {
    await Promise.all([loadState(), loadMappings()]);
    await syncAllLeds();

    refreshTimer = setInterval(async () => {
      [masterVolume, masterMuted] = await Promise.all([
        invoke<number>("get_volume", { target: "SystemMaster" }).catch(() => masterVolume),
        invoke<boolean>("get_muted", { target: "SystemMaster" }).catch(() => masterMuted),
      ]);
      if (caps.per_app_volume) {
        const prev = sessions;
        sessions = await invoke<AudioSessionInfo[]>("list_sessions").catch(() => sessions);
        if (sessions !== prev) await syncAllLeds();
      }
      // Refresh mappings so new entries added in the Mappings tab appear immediately
      await loadMappings();
    }, 3000);

    unlistens.push(
      await listen<{ target: unknown; volume: number }>("volume-changed", (e) => {
        if (e.payload.target === "SystemMaster") masterVolume = e.payload.volume;
      }),
      await listen<string>("device-connected", async () => {
        await loadState();
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
  {#if derivedStrips.length === 0}
    <div class="empty-state card">
      No fader mappings yet.<br>
      Go to <strong>Mappings</strong> and add a <em>Set Volume</em> action to see strips here.
    </div>
  {:else}
    <div class="strips-row">
      {#each derivedStrips as strip, i}
        {@const pt = primaryStripTarget(strip.svTargets)}
        <ChannelStrip
          label={targetsLabel(strip.svTargets)}
          volume={stripVolumes[i]}
          muted={stripMuted[i]}
          active={stripActive[i]}
          assigned={true}
          isMaster={pt === "master"}
          unavailable={isDeviceTarget(pt) ? !caps.device_switching : (pt !== "master" && !caps.per_app_volume)}
          showFader={!isDeviceTarget(pt)}
          showS={false}
          rClickable={isDeviceTarget(pt)}
          on:volume-change={(e) => handleVolumeChange(i, e.detail)}
          on:mute-toggle={() => handleMuteToggle(i)}
          on:r-click={() => handleRClick(i)}
        />
      {/each}
    </div>
  {/if}

  <!-- Output device selector -->
  {#if caps.device_switching && devices.filter(d => !d.is_input).length > 0}
    <div class="card device-row">
      <span class="device-label">Output</span>
      <select on:change={(e) => setDefaultOutput(e.currentTarget.value)}>
        {#each devices.filter(d => !d.is_input) as dev}
          <option value={dev.id} selected={dev.is_default}>{dev.name}</option>
        {/each}
      </select>
    </div>
  {/if}

  {#if !caps.per_app_volume}
    <div class="platform-note">
      Per-app volume requires the macOS Audio Tap API (14.2+) — coming soon.
      Master volume and device-level control are fully functional.
    </div>
  {/if}
</div>

<style>
  .mixer {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .strips-row {
    display: flex;
    gap: 8px;
    overflow-x: auto;
    padding-bottom: 6px;
  }

  .empty-state {
    text-align: center;
    padding: 32px;
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.7;
    max-width: 420px;
  }

  .device-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .device-label {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
  }
  .device-row select { flex: 1; }

  .platform-note {
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
