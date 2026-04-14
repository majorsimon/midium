<script lang="ts">
  interface Props {
    label?: string;
    volume?: number;
    muted?: boolean;
    active?: boolean;
    assigned?: boolean;
    isMaster?: boolean;
    unavailable?: boolean;
    showFader?: boolean;
    rClickable?: boolean;
    showS?: boolean;
    onVolumeChange?: (volume: number) => void;
    onMuteToggle?: () => void;
    onAssignClick?: () => void;
    onRClick?: () => void;
  }

  let {
    label = "",
    volume = 1.0,
    muted = false,
    active = false,
    assigned = false,
    isMaster = false,
    unavailable = false,
    showFader = true,
    rClickable = false,
    showS = true,
    onVolumeChange,
    onMuteToggle,
    onAssignClick,
    onRClick,
  }: Props = $props();

  let live = $derived((assigned || isMaster) && !unavailable);

  let localVol = $state(volume);
  let dragging = $state(false);
  $effect(() => {
    if (!dragging) localVol = volume;
  });

  function handleInput() {
    onVolumeChange?.(localVol);
  }
</script>

<div class="strip" class:muted class:dim={!assigned && !isMaster}>
  <!-- Label -->
  <div class="lbl" title={label}>
    {assigned || isMaster ? label || "—" : "—"}
  </div>

  <!-- Fader -->
  <div class="fader-wrap">
    {#if (assigned || isMaster) && showFader}
      <input
        class="fader"
        class:muted
        class:unavail={unavailable}
        type="range" min="0" max="1" step="0.005"
        bind:value={localVol}
        disabled={unavailable}
        onpointerdown={() => dragging = true}
        onpointerup={() => { dragging = false; localVol = localVol; }}
        oninput={handleInput}
      />
    {:else}
      <div class="fader-empty"></div>
    {/if}
  </div>

  <!-- Volume % -->
  <div class="vol-pct">
    {live && showFader ? Math.round(volume * 100) + "%" : ""}
  </div>

  <!-- S / M / R buttons -->
  <div class="btns">
    {#if showS}
      <button
        class="btn s"
        onclick={() => onAssignClick?.()}
        title={assigned || isMaster ? "Reassign" : "Assign app"}
      >S</button>
    {/if}

    <button
      class="btn m"
      class:on={muted}
      disabled={!live}
      onclick={() => onMuteToggle?.()}
      title={muted ? "Unmute" : "Mute"}
    >M</button>

    <!-- R: clickable "set as default" when rClickable, otherwise passive indicator -->
    {#if rClickable}
      <button
        class="btn r"
        class:on={active}
        onclick={() => onRClick?.()}
        title={active ? "Default output" : "Set as default output"}
      >R</button>
    {:else}
      <span class="btn r" class:on={active} title={active ? "Active" : "Silent"}>R</span>
    {/if}
  </div>
</div>

<style>
  .strip {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 68px;
    min-width: 68px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px 6px;
    gap: 4px;
    transition: background 0.15s, border-color 0.15s;
    user-select: none;
  }

  .strip.muted {
    background: color-mix(in srgb, var(--danger) 8%, var(--surface));
    border-color: color-mix(in srgb, var(--danger) 30%, var(--border));
  }

  .strip.dim { opacity: 0.4; }

  .lbl {
    font-size: 10px;
    font-weight: 600;
    color: var(--text);
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    width: 100%;
    line-height: 1.3;
  }

  .fader-wrap {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 4px 0;
  }

  .fader {
    -webkit-appearance: slider-vertical;
    writing-mode: vertical-lr;
    direction: rtl;
    height: 110px;
    width: 28px;
    cursor: pointer;
    accent-color: var(--accent);
    margin: 0;
  }

  .fader.muted { accent-color: var(--danger); }

  .fader.unavail,
  .fader:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .fader-empty {
    width: 4px;
    height: 110px;
    background: var(--border);
    border-radius: 2px;
    opacity: 0.4;
  }

  .vol-pct {
    font-size: 10px;
    color: var(--text-muted);
    min-height: 14px;
    text-align: center;
  }

  .btns {
    display: flex;
    gap: 3px;
    align-items: center;
  }

  .btn {
    width: 18px;
    height: 18px;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0;
    border-radius: 3px;
    border: 1px solid var(--border);
    background: var(--surface2);
    color: var(--text-muted);
    padding: 0;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    transition: background 0.1s, color 0.1s, border-color 0.1s;
    line-height: 1;
  }

  .btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  /* S — assign */
  .btn.s {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
  }
  .btn.s:hover {
    background: color-mix(in srgb, var(--accent) 14%, var(--surface2));
  }

  /* M — mute */
  .btn.m.on {
    background: color-mix(in srgb, var(--danger) 18%, var(--surface2));
    color: var(--danger);
    border-color: var(--danger);
  }
  .btn.m:not(:disabled):not(.on):hover {
    background: color-mix(in srgb, var(--danger) 10%, var(--surface2));
    color: var(--danger);
  }

  /* R — active indicator / default-output selector */
  .btn.r {
    cursor: default;
  }
  button.btn.r {
    cursor: pointer;
  }
  button.btn.r:not(.on):hover {
    background: color-mix(in srgb, var(--success) 12%, var(--surface2));
    color: var(--success);
  }
  .btn.r.on {
    background: color-mix(in srgb, var(--success) 22%, var(--surface2));
    color: var(--success);
    border-color: var(--success);
    animation: pulse 2s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50%       { opacity: 0.5; }
  }
</style>
