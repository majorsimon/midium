<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import Mixer from "$lib/Mixer.svelte";
  import MappingEditor from "$lib/MappingEditor.svelte";
  import Devices from "$lib/Devices.svelte";
  import Settings from "$lib/Settings.svelte";
  import PluginManager from "$lib/PluginManager.svelte";
  import type { MidiEvent } from "$lib/types";

  type Tab = "mixer" | "mappings" | "devices" | "plugins" | "settings";
  let activeTab: Tab = "mixer";

  // When Devices tab fires open-mapping, navigate to Mappings with pre-fill.
  let mappingPrefill: {
    device: string;
    channel: number;
    controlTypeName: "CC" | "Note" | "PitchBend";
    controlNumber: number;
  } | null = null;

  function handleOpenMapping(e: CustomEvent<typeof mappingPrefill>) {
    mappingPrefill = e.detail;
    activeTab = "mappings";
  }

  let connectedDevices: string[] = [];
  let lastMidiEvent: (MidiEvent & { ts: number }) | null = null;
  let unlistens: UnlistenFn[] = [];

  onMount(async () => {
    // Track connected MIDI devices
    unlistens.push(
      await listen<string>("device-connected", (e) => {
        if (!connectedDevices.includes(e.payload)) {
          connectedDevices = [...connectedDevices, e.payload];
        }
      }),
      await listen<string>("device-disconnected", (e) => {
        connectedDevices = connectedDevices.filter(d => d !== e.payload);
      }),
      await listen<MidiEvent>("midi-event", (e) => {
        lastMidiEvent = { ...e.payload, ts: Date.now() };
        // fade out after 2s
        setTimeout(() => {
          if (lastMidiEvent && Date.now() - lastMidiEvent.ts >= 1900) {
            lastMidiEvent = null;
          }
        }, 2000);
      })
    );

    // Populate initial device list from currently connected ports
    const ports = await invoke<string[]>("list_midi_ports").catch(() => [] as string[]);
    connectedDevices = ports;
  });

  onDestroy(() => unlistens.forEach(u => u()));

  function midiEventLabel(e: MidiEvent): string {
    const msg = e.message;
    if (msg.ControlChange) return `CC${msg.ControlChange.control}=${msg.ControlChange.value}`;
    if (msg.NoteOn) return `Note ${msg.NoteOn.note} on`;
    if (msg.NoteOff) return `Note ${msg.NoteOff.note} off`;
    if (msg.PitchBend) return `Pitch ${msg.PitchBend.value}`;
    return "?";
  }
</script>

<div class="shell">
  <!-- Sidebar -->
  <aside class="sidebar">
    <div class="logo">
      <svg width="22" height="22" viewBox="0 0 22 22" fill="none">
        <rect width="22" height="22" rx="5" fill="#7c6af7"/>
        <rect x="5" y="6" width="3" height="10" rx="1.5" fill="white"/>
        <rect x="9.5" y="4" width="3" height="14" rx="1.5" fill="white" opacity="0.7"/>
        <rect x="14" y="7" width="3" height="8" rx="1.5" fill="white" opacity="0.5"/>
      </svg>
      <span>Midium</span>
    </div>

    <nav>
      <button class:active={activeTab === "mixer"} on:click={() => activeTab = "mixer"}>
        <span class="nav-icon">🎚</span> Mixer
      </button>
      <button class:active={activeTab === "devices"} on:click={() => activeTab = "devices"}>
        <span class="nav-icon">🎹</span> Devices
      </button>
      <button class:active={activeTab === "mappings"} on:click={() => activeTab = "mappings"}>
        <span class="nav-icon">⚡</span> Mappings
      </button>
      <button class:active={activeTab === "plugins"} on:click={() => activeTab = "plugins"}>
        <span class="nav-icon">🧩</span> Plugins
      </button>
      <button class:active={activeTab === "settings"} on:click={() => activeTab = "settings"}>
        <span class="nav-icon">⚙</span> Settings
      </button>
    </nav>

    <div class="sidebar-bottom">
      <!-- Connected devices -->
      <div class="section-title">MIDI Devices</div>
      {#if connectedDevices.length === 0}
        <div class="no-devices">No devices connected</div>
      {:else}
        {#each connectedDevices as device}
          <div class="device-chip" title={device}>
            <span class="dot"></span>
            <span class="device-name">{device}</span>
          </div>
        {/each}
      {/if}

      <!-- Last MIDI event indicator -->
      {#if lastMidiEvent}
        <div class="midi-indicator">
          <span class="dot active"></span>
          <span style="font-size: 10px;">
            {lastMidiEvent.device.split(" ")[0]} · {midiEventLabel(lastMidiEvent)}
          </span>
        </div>
      {/if}
    </div>
  </aside>

  <!-- Content -->
  <main class="content">
    {#if activeTab === "mixer"}
      <Mixer />
    {:else if activeTab === "devices"}
      <Devices
        {connectedDevices}
        on:open-mapping={handleOpenMapping}
      />
    {:else if activeTab === "mappings"}
      <MappingEditor bind:prefill={mappingPrefill} />
    {:else if activeTab === "plugins"}
      <PluginManager />
    {:else}
      <Settings />
    {/if}
  </main>
</div>

<style>
  .shell {
    display: flex;
    height: 100vh;
    overflow: hidden;
  }

  /* Sidebar */
  .sidebar {
    width: 200px;
    min-width: 200px;
    background: var(--surface);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    padding: 16px 0;
  }

  .logo {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 16px 20px;
    font-size: 15px;
    font-weight: 700;
    letter-spacing: -0.02em;
    border-bottom: 1px solid var(--border);
    margin-bottom: 8px;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 8px;
  }
  nav button {
    background: transparent;
    border: none;
    text-align: left;
    padding: 8px 10px;
    border-radius: var(--radius);
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-muted);
    width: 100%;
    transition: background 0.1s, color 0.1s;
  }
  nav button:hover { background: var(--surface2); color: var(--text); }
  nav button.active {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
    font-weight: 500;
  }
  .nav-icon { font-size: 14px; }

  .sidebar-bottom {
    margin-top: auto;
    padding: 16px;
    border-top: 1px solid var(--border);
  }

  .no-devices { font-size: 11px; color: var(--text-muted); }

  .device-chip {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 0;
  }
  .device-name {
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 140px;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--success);
    flex-shrink: 0;
  }
  .dot.active { background: var(--accent); animation: blink 0.8s ease-in-out 3; }
  @keyframes blink { 0%, 100% { opacity: 1; } 50% { opacity: 0.2; } }

  .midi-indicator {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
    padding: 4px 6px;
    background: var(--surface2);
    border-radius: var(--radius);
    color: var(--text-muted);
  }

  /* Content */
  .content {
    flex: 1;
    overflow-y: auto;
    background: var(--bg);
  }
</style>
