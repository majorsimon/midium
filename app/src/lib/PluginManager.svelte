<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  interface PluginInfo {
    name: string;
    action_count: number;
    enabled: boolean;
  }

  let plugins: PluginInfo[] = [];
  let loading = true;

  onMount(async () => {
    plugins = await invoke<PluginInfo[]>("list_plugins").catch(() => []);
    loading = false;
  });
</script>

<div style="padding: 20px; max-width: 700px;">
  <div class="card">
    <div class="section-title" style="margin-bottom: 12px;">
      Loaded Plugins ({plugins.length})
    </div>

    {#if loading}
      <div style="color: var(--text-muted); font-size: 12px;">Loading…</div>
    {:else if plugins.length === 0}
      <div class="empty">
        No plugins found. Drop <code>.lua</code> files into the
        <code>plugins/</code> directory next to the app and restart.
      </div>
    {:else}
      <table>
        <thead>
          <tr>
            <th>Plugin</th>
            <th>Custom Actions</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          {#each plugins as p}
            <tr>
              <td>
                <div class="plugin-name-cell">
                  <span
                    class="status-dot"
                    class:dot-enabled={p.enabled}
                    class:dot-disabled={!p.enabled}
                    title={p.enabled ? "Enabled" : "Disabled"}
                  ></span>
                  <span style="font-weight: 500;">{p.name}</span>
                </div>
              </td>
              <td>
                {#if p.action_count > 0}
                  <span class="tag active">{p.action_count}</span>
                {:else}
                  <span style="color: var(--text-muted); font-size: 11px;">none</span>
                {/if}
              </td>
              <td>
                <span class="tag" class:active={p.enabled}>
                  {p.enabled ? "Active" : "Disabled"}
                </span>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>

  <div class="card config-notice" style="margin-top: 16px;">
    <div class="config-notice-inner">
      <span class="config-icon" aria-hidden="true">&#9432;</span>
      <div>
        Plugins are enabled/disabled via the <code>plugins.enabled</code> list in
        <code>config.toml</code>. Changes take effect on next restart.
      </div>
    </div>
  </div>

  <div class="card" style="margin-top: 16px;">
    <div class="section-title" style="margin-bottom: 8px;">Plugin Directories</div>
    <div style="font-size: 11px; color: var(--text-muted); line-height: 1.8;">
      Plugins are loaded from these directories at startup (first match wins):
      <ul style="margin: 8px 0 0 16px; padding: 0;">
        <li><code>./plugins/</code> (relative to working directory)</li>
        <li><code>&lt;exe&gt;/plugins/</code></li>
        <li><code>~/Library/Application Support/midium/plugins/</code> (macOS)</li>
        <li><code>~/.config/midium/plugins/</code> (Linux)</li>
        <li><code>%APPDATA%\midium\plugins\</code> (Windows)</li>
      </ul>
    </div>
  </div>

  <div class="card" style="margin-top: 16px;">
    <div class="section-title" style="margin-bottom: 8px;">Plugin API Reference</div>
    <pre class="api-ref"
>midium.log(msg)                      -- log a message
midium.audio.get_volume(target)      -- returns 0.0–1.0
midium.audio.set_volume(target, v)   -- set volume
midium.audio.is_muted(target)        -- returns bool
midium.audio.set_mute(target, muted) -- set mute
midium.audio.list_sessions()         -- [&#123;name, volume, muted&#125;]
midium.audio.list_devices()          -- [&#123;id, name, is_default&#125;]
midium.state.get(key)                -- per-plugin persistent state
midium.state.set(key, value)
midium.register_action(name, desc, fn)

-- Targets: "master", "focused", "app:Spotify", "device:&lt;id&gt;"

-- Lifecycle hooks (return a table or define globals):
function on_load() end
function on_midi_event(event) end    -- event.device, event.channel,
                                     -- event.message.&#123;cc,note,pitch_bend&#125;
function on_unload() end</pre>
  </div>
</div>

<style>
  .empty {
    font-size: 12px;
    color: var(--text-muted);
    padding: 16px 0;
  }
  code {
    font-family: monospace;
    background: var(--surface2);
    padding: 1px 4px;
    border-radius: 3px;
    font-size: 11px;
  }
  .api-ref {
    font-size: 11px;
    color: var(--text-muted);
    background: var(--surface2);
    border-radius: var(--radius);
    padding: 12px;
    margin: 0;
    overflow-x: auto;
    line-height: 1.6;
    white-space: pre;
  }
  ul { list-style: disc; }
  li { margin-bottom: 2px; }

  .plugin-name-cell {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .status-dot {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    transition: background 0.2s;
  }
  .dot-enabled  { background: var(--success); box-shadow: 0 0 4px color-mix(in srgb, var(--success) 60%, transparent); }
  .dot-disabled { background: var(--text-muted); }

  .config-notice {
    border-color: color-mix(in srgb, var(--accent) 30%, var(--border));
    background: color-mix(in srgb, var(--accent) 4%, var(--surface));
  }
  .config-notice-inner {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.5;
  }
  .config-icon {
    font-size: 16px;
    color: var(--accent);
    flex-shrink: 0;
    line-height: 1.2;
  }
</style>
