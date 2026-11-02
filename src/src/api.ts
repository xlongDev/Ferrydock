import { invoke } from "@tauri-apps/api/core";
import type {
  AppConfig,
  HealthResponse,
  ModelEntry,
  StatusResponse,
} from "./types";

const GATEWAY: string =
  (import.meta.env.VITE_GATEWAY_URL as string) || "http://127.0.0.1:41980";

let apiKey = "";

function inTauri(): boolean {
  return (
    typeof window !== "undefined" &&
    "__TAURI_INTERNALS__" in (window as unknown as Record<string, unknown>)
  );
}

/** Fetch the gateway API key from the Rust side (only meaningful in Tauri). */
export async function initApiKey(): Promise<string> {
  if (inTauri()) {
    try {
      apiKey = await invoke<string>("get_api_key");
    } catch {
      apiKey = "";
    }
  }
  return apiKey;
}

async function apiFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const headers = new Headers(init.headers);
  // /health and /admin/status are public on the local gateway.
  if (!path.startsWith("/health") && !path.startsWith("/admin/status")) {
    headers.set("Authorization", `Bearer ${apiKey}`);
  }
  return fetch(`${GATEWAY}${path}`, { ...init, headers });
}

export async function getHealth(): Promise<HealthResponse> {
  const r = await fetch(`${GATEWAY}/health`);
  if (!r.ok) throw new Error(`health ${r.status}`);
  return r.json();
}

export async function getStatus(): Promise<StatusResponse> {
  const r = await apiFetch("/admin/status");
  if (!r.ok) throw new Error(`status ${r.status}`);
  return r.json();
}

export async function getModels(): Promise<{ data: ModelEntry[] }> {
  const r = await apiFetch("/v1/models");
  if (!r.ok) throw new Error(`models ${r.status}`);
  return r.json();
}

export async function getConfig(): Promise<AppConfig> {
  const r = await apiFetch("/admin/config");
  if (!r.ok) throw new Error(`config ${r.status}`);
  return r.json();
}

export async function updateConfig(patch: Partial<AppConfig>): Promise<void> {
  const r = await apiFetch("/admin/config", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(patch),
  });
  if (!r.ok) throw new Error(await r.text());
}

export async function importNow(): Promise<void> {
  const r = await apiFetch("/admin/import", { method: "POST" });
  if (!r.ok) throw new Error(await r.text());
}

export async function openPath(path: string): Promise<void> {
  if (inTauri()) {
    await invoke("open_path", { path });
  } else {
    console.warn("openPath is only available inside the Tauri app:", path);
  }
}

export async function quitApp(): Promise<void> {
  if (inTauri()) {
    await invoke("quit_app");
  }
}

export async function getDataDir(): Promise<string> {
  if (inTauri()) {
    return await invoke<string>("get_data_dir");
  }
  return "";
}

export const gatewayUrl = GATEWAY;
