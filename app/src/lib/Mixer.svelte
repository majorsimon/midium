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
    FaderGroup,
  } from "./types";
  import {
    masterVolume as masterVolumeStore,
    masterMuted as masterMutedStore,
    focusedVolume as focusedVolumeStore,
    focusedMuted as focusedMutedStore,
    sessions as sessionsStore,
    initStoreListeners,
  } from "./stores";

  // ---------------------------------------------------------------------------
  // Audio state (local reactive copies, kept in sync with stores)
  // ---------------------------------------------------------------------------

  let masterVolume = $state(0.7);
  let masterMuted = $state(false);
  let focusedVolume = $state(0.0);
  let focusedMuted = $state(false);
  let sessions: AudioSessionInfo[] = $state([]);
  let devices: AudioDeviceInfo[] = $state([]);
  let caps: AudioCapabilities = $state({
    per_app_volume: false,
    device_switching: false,
    input_device_switching: false,
  });

  // ---------------------------------------------------------------------------
  // Mapping + profile + fader group state
  // ---------------------------------------------------------------------------

  let profiles: DeviceProfile[] = [];
  let mappings: Mapping[] = $state([]);
  let faderGroups: FaderGroup[] = $state([]);

  // Live state for fader group strips
  let groupVolumes: number[] = $state([]);
  let groupMuted: boolean[] = $state([]);
  let groupIsDefault: boolean[] = $state([]);

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
    if (t === "FocusedApplication") return "focused";
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
  type StripTarget = null | "master" | "focused" | { app: string } | { device: string };

  let derivedStrips = $derived(mappings
    .map(m => ({ mapping: m, svTargets: getSetVolumeTargets(m.action) }))
    .filter(s => s.svTargets.length > 0));

  // ---------------------------------------------------------------------------
  // Unified ordered strip list — sorted by CC/control number ascending.
  // Mapping strips use the CC number from their control; fader group strips
  // use their group number (shifted high so they sort after mappings with
  // the same number, but still in group order).
  // ---------------------------------------------------------------------------

  type UnifiedStrip =
    | { kind: "mapping"; mappingIdx: number; sortKey: number }
    | { kind: "group"; groupIdx: number; sortKey: number };

  function mappingCc(m: Mapping): number {
    const ct = m.control.control_type;
    if (typeof ct === "object" && "CC" in ct) return (ct as { CC: number }).CC;
    if (typeof ct === "object" && "Note" in ct) return (ct as { Note: number }).Note;
    return Infinity;
  }

  let stripOrder = $derived([
    ...derivedStrips.map((_, i) => ({
      kind: "mapping" as const,
      mappingIdx: i,
      sortKey: mappingCc(derivedStrips[i].mapping),
    })),
    ...faderGroups.map((g, i) => ({
      kind: "group" as const,
      groupIdx: i,
      sortKey: 10000 + g.group,
    })),
  ].sort((a, b) => a.sortKey - b.sortKey));

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

  let stripVolumes = $derived(derivedStrips.map(s => {
    const t = primaryStripTarget(s.svTargets);
    if (!t || isDeviceTarget(t)) return 0;
    if (t === "master") return masterVolume;
    if (t === "focused") return focusedVolume;
    return sessions.find(ss => ss.name === (t as { app: string }).app)?.volume ?? 0.8;
  }));

  let stripMuted = $derived(derivedStrips.map(s => {
    const t = primaryStripTarget(s.svTargets);
    if (!t || isDeviceTarget(t)) return false;
    if (t === "master") return masterMuted;
    if (t === "focused") return focusedMuted;
    return sessions.find(ss => ss.name === (t as { app: string }).app)?.muted ?? false;
  }));

  let stripActive = $derived(derivedStrips.map(s => {
    const t = primaryStripTarget(s.svTargets);
    if (!t) return false;
    if (t === "master" || t === "focused") return true;
    if (isDeviceTarget(t))
      return devices.find(d => d.id === (t as { device: string }).device)?.is_default ?? false;
    return sessions.some(ss => ss.name === (t as { app: string }).app);
  }));

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
      if (target === "focused") return t === "FocusedApplication";
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
        focusedVolume = v;
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
    } else if (t === "focused") {
      focusedMuted = !focusedMuted;
      await invoke("toggle_mute", { target: "FocusedApplication" }).catch(console.error);
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

  async function setDefaultInput(id: string) {
    await invoke("set_default_input", { deviceId: id }).catch(console.error);
    const inputs = await invoke<AudioDeviceInfo[]>("list_input_devices").catch(() => []);
    devices = [...devices.filter(d => !d.is_input), ...inputs];
  }

  // ---------------------------------------------------------------------------
  // Fader group strip handlers
  // ---------------------------------------------------------------------------

  async function handleGroupVolumeChange(i: number, v: number) {
    const g = faderGroups[i];
    groupVolumes[i] = v;
    await invoke("set_volume", { target: g.target, volume: v }).catch(console.error);
  }

  async function handleGroupMuteToggle(i: number) {
    const g = faderGroups[i];
    groupMuted[i] = !groupMuted[i];
    groupMuted = groupMuted;
    await invoke("toggle_mute", { target: g.target }).catch(console.error);
  }

  async function handleGroupRClick(i: number) {
    const devId = groupDeviceId(faderGroups[i]);
    if (!devId) return;
    await invoke("set_default_output", { deviceId: devId }).catch(console.error);
    await refreshGroupState();
  }

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  let unlistens: UnlistenFn[] = [];
  let refreshTimer: ReturnType<typeof setInterval>;

  async function loadState() {
    try {
      caps     = await invoke("get_capabilities");
      const outputs: AudioDeviceInfo[] = await invoke("list_output_devices");
      const inputs: AudioDeviceInfo[] = caps.input_device_switching
        ? await invoke<AudioDeviceInfo[]>("list_input_devices").catch(() => [])
        : [];
      devices = [...outputs, ...inputs];
      [masterVolume, masterMuted] = await Promise.all([
        invoke<number>("get_volume", { target: "SystemMaster" }),
        invoke<boolean>("get_muted", { target: "SystemMaster" }).catch(() => false),
      ]);
      masterVolumeStore.set(masterVolume);
      masterMutedStore.set(masterMuted);
      if (caps.per_app_volume) {
        sessions = await invoke("list_sessions");
        sessionsStore.set(sessions);
      }
    } catch (e) {
      console.error("Mixer: failed to load state", e);
    }
  }

  async function loadMappings() {
    [profiles, mappings, faderGroups] = await Promise.all([
      invoke<DeviceProfile[]>("list_profiles").catch(() => [] as DeviceProfile[]),
      invoke<Mapping[]>("get_mappings").catch(() => [] as Mapping[]),
      invoke<FaderGroup[]>("get_fader_groups").catch(() => [] as FaderGroup[]),
    ]);
    await refreshGroupState();
  }

  function groupDeviceId(g: FaderGroup): string | null {
    if (typeof g.target === "object" && "Device" in g.target) return g.target.Device.id;
    return null;
  }

  async function refreshGroupState() {
    if (faderGroups.length === 0) return;
    [groupVolumes, groupMuted] = await Promise.all([
      Promise.all(faderGroups.map(g => invoke<number>("get_volume", { target: g.target }).catch(() => 0.8))),
      Promise.all(faderGroups.map(g => invoke<boolean>("get_muted", { target: g.target }).catch(() => false))),
    ]);
    const allDevices = await invoke<AudioDeviceInfo[]>("list_output_devices").catch(() => devices);
    groupIsDefault = faderGroups.map(g => {
      const devId = groupDeviceId(g);
      if (!devId) return false;
      return allDevices.find(d => d.id === devId)?.is_default ?? false;
    });
  }

  let groupLabels = $derived(faderGroups.map(g => {
    if (g.target === "SystemMaster") return "Master";
    if (g.target === "FocusedApplication") return "Focused";
    if (typeof g.target === "object" && "Application" in g.target) return g.target.Application.name;
    if (typeof g.target === "object" && "Device" in g.target)
      return devices.find(d => d.id === (g.target as { Device: { id: string } }).Device.id)?.name
        ?? (g.target as { Device: { id: string } }).Device.id;
    return "?";
  }));

  onMount(async () => {
    await initStoreListeners();

    // Subscribe to stores to keep local reactive variables in sync
    const unsubs = [
      masterVolumeStore.subscribe(v => masterVolume = v),
      masterMutedStore.subscribe(v => masterMuted = v),
      focusedVolumeStore.subscribe(v => focusedVolume = v),
      focusedMutedStore.subscribe(v => focusedMuted = v),
      sessionsStore.subscribe(v => sessions = v),
    ];
    unlistens.push(...unsubs.map(u => u as unknown as UnlistenFn));

    await loadState();
    await loadMappings();
    await syncAllLeds();

    // Fallback polling at 30s to catch any missed push events (e.g. per-app
    // session changes, mapping edits from another tab).
    refreshTimer = setInterval(async () => {
      [masterVolume, masterMuted, focusedVolume, focusedMuted] = await Promise.all([
        invoke<number>("get_volume", { target: "SystemMaster" }).catch(() => masterVolume),
        invoke<boolean>("get_muted", { target: "SystemMaster" }).catch(() => masterMuted),
        invoke<number>("get_volume", { target: "FocusedApplication" }).catch(() => focusedVolume),
        invoke<boolean>("get_muted", { target: "FocusedApplication" }).catch(() => focusedMuted),
      ]);
      masterVolumeStore.set(masterVolume);
      masterMutedStore.set(masterMuted);
      focusedVolumeStore.set(focusedVolume);
      focusedMutedStore.set(focusedMuted);
      if (caps.per_app_volume) {
        const prev = sessions;
        sessions = await invoke<AudioSessionInfo[]>("list_sessions").catch(() => sessions);
        sessionsStore.set(sessions);
        if (sessions !== prev) await syncAllLeds();
      }
      await loadMappings();
      await refreshGroupState();
    }, 30_000);

    unlistens.push(
      await listen<string>("device-connected", async () => {
        await loadState();
        await refreshGroupState();
        await syncAllLeds();
      }),
      await listen<string>("device-disconnected", () => loadState()),
      await listen("default-device-changed", async () => {
        devices = [
          ...await invoke<AudioDeviceInfo[]>("list_output_devices").catch(() => devices.filter(d => !d.is_input)),
          ...(caps.input_device_switching
            ? await invoke<AudioDeviceInfo[]>("list_input_devices").catch(() => devices.filter(d => d.is_input))
            : devices.filter(d => d.is_input)),
        ];
        await refreshGroupState();
        await syncAllLeds();
      }),
    );
  });

  onDestroy(() => {
    clearInterval(refreshTimer);
    unlistens.forEach(u => u());
  });
</script>

<div class="mixer">
  {#if derivedStrips.length === 0 && faderGroups.length === 0}
    <div class="empty-state card">
      No fader mappings yet.<br>
      Go to <strong>Mappings</strong> and add a <em>Set Volume</em> action, or configure <strong>Groups</strong> to see strips here.
    </div>
  {:else}
    <div class="strips-row">
      {#each stripOrder as s}
        {#if s.kind === "mapping"}
          {@const strip = derivedStrips[s.mappingIdx]}
          {@const pt = primaryStripTarget(strip.svTargets)}
          <ChannelStrip
            label={targetsLabel(strip.svTargets)}
            volume={stripVolumes[s.mappingIdx]}
            muted={stripMuted[s.mappingIdx]}
            active={stripActive[s.mappingIdx]}
            assigned={true}
            isMaster={pt === "master"}
            unavailable={isDeviceTarget(pt) ? !caps.device_switching : (pt !== "master" && pt !== "focused" && !caps.per_app_volume)}
            showFader={!isDeviceTarget(pt)}
            showS={false}
            rClickable={isDeviceTarget(pt)}
            onVolumeChange={(v) => handleVolumeChange(s.mappingIdx, v)}
            onMuteToggle={() => handleMuteToggle(s.mappingIdx)}
            onRClick={() => handleRClick(s.mappingIdx)}
          />
        {:else}
          {@const g = faderGroups[s.groupIdx]}
          {@const devId = groupDeviceId(g)}
          {@const isDevice = devId !== null}
          <ChannelStrip
            label={groupLabels[s.groupIdx]}
            volume={groupVolumes[s.groupIdx] ?? 0.8}
            muted={groupMuted[s.groupIdx] ?? false}
            active={isDevice ? (groupIsDefault[s.groupIdx] ?? false) : !(groupMuted[s.groupIdx] ?? false)}
            assigned={true}
            isMaster={g.target === "SystemMaster"}
            unavailable={isDevice ? !caps.device_switching : false}
            showFader={true}
            showS={false}
            rClickable={isDevice}
            onVolumeChange={(v) => handleGroupVolumeChange(s.groupIdx, v)}
            onMuteToggle={() => handleGroupMuteToggle(s.groupIdx)}
            onRClick={() => handleGroupRClick(s.groupIdx)}
          />
        {/if}
      {/each}
    </div>
  {/if}

  <!-- Output device selector -->
  {#if caps.device_switching && devices.filter(d => !d.is_input).length > 0}
    <div class="card device-row">
      <span class="device-label">Output</span>
      <select value={devices.find(d => !d.is_input && d.is_default)?.id ?? ""}
              onchange={(e) => setDefaultOutput(e.currentTarget.value)}>
        {#each devices.filter(d => !d.is_input) as dev}
          <option value={dev.id}>{dev.name}</option>
        {/each}
      </select>
    </div>
  {/if}

  <!-- Input device selector -->
  {#if caps.input_device_switching && devices.filter(d => d.is_input).length > 0}
    <div class="card device-row">
      <span class="device-label">Input</span>
      <select value={devices.find(d => d.is_input && d.is_default)?.id ?? ""}
              onchange={(e) => setDefaultInput(e.currentTarget.value)}>
        {#each devices.filter(d => d.is_input) as dev}
          <option value={dev.id}>{dev.name}</option>
        {/each}
      </select>
    </div>
  {/if}

  {#if !caps.per_app_volume}
    <div class="platform-note">
      Per-app volume requires macOS 14.2+ (Audio Tap API).
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
