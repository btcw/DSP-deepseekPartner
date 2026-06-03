import { invoke, isTauri } from "@tauri-apps/api/core";
import { writeText as writeTauriClipboardText } from "@tauri-apps/plugin-clipboard-manager";
import type { GatewayProfile, LogEntry, ProfileStatus } from "./types";

export const api = {
  listProfiles: () => invoke<GatewayProfile[]>("list_profiles"),
  saveProfile: (profile: GatewayProfile) => invoke<GatewayProfile[]>("save_profile", { profile }),
  deleteProfile: (id: string) => invoke<GatewayProfile[]>("delete_profile", { id }),
  startProfile: (id: string) => invoke<ProfileStatus>("start_profile", { id }),
  stopProfile: (id: string) => invoke<ProfileStatus>("stop_profile", { id }),
  startAll: () => invoke<ProfileStatus[]>("start_all"),
  stopAll: () => invoke<ProfileStatus[]>("stop_all"),
  statuses: () => invoke<ProfileStatus[]>("profile_statuses"),
  readLogs: (profileId: string, limit = 400) => invoke<LogEntry[]>("read_logs", { profileId, limit }),
  clearLogs: (profileId: string) => invoke<void>("clear_logs", { profileId }),
  copyProxyText: (profileId: string, kind: string) =>
    invoke<string>("copy_proxy_text", { profileId, kind })
};

export async function copyText(text: string) {
  if (isTauri()) {
    await writeTauriClipboardText(text);
    return;
  }

  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }

  throw new Error("Clipboard is not available in this browser context.");
}
