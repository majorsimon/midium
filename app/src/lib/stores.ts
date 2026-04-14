import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import type { AudioSessionInfo } from "./types";

export const masterVolume = writable(0.7);
export const masterMuted = writable(false);
export const focusedVolume = writable(0.0);
export const focusedMuted = writable(false);
export const sessions = writable<AudioSessionInfo[]>([]);

let initialized = false;

/**
 * One-time setup: subscribe to backend-pushed Tauri events so stores update
 * automatically without polling. Safe to call multiple times (idempotent).
 */
export async function initStoreListeners(): Promise<void> {
  if (initialized) return;
  initialized = true;

  await listen<{ target: unknown; volume: number }>("volume-changed", (e) => {
    if (e.payload.target === "SystemMaster") {
      masterVolume.set(e.payload.volume);
    } else if (e.payload.target === "FocusedApplication") {
      focusedVolume.set(e.payload.volume);
    }
  });

  await listen<{ target: unknown; muted: boolean }>("mute-changed", (e) => {
    if (e.payload.target === "SystemMaster") {
      masterMuted.set(e.payload.muted);
    } else if (e.payload.target === "FocusedApplication") {
      focusedMuted.set(e.payload.muted);
    }
  });
}
