export interface Model {
  alias: string;
  label: string;
  context_limit: number | null;
  repo: string;
  preset: string;
  revision: string;
  state: string;
  loaded: boolean;
  registered: boolean;
  desired: boolean;
  error: string;
}
export interface Snapshot {
  models: Model[];
  mock: boolean;
  online: boolean;
  vram: { card: string; summary: string }[];
  progress: { stage: string; percent: number | null };
}
export interface Turn {
  role: "assistant" | "user";
  content: string;
}
export async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(path, options);
  if (!res.ok) {
    const message = await res.text();
    throw Error(message.startsWith("<") ? `Request failed (${res.status})` : message);
  }
  return res.status === 204 || res.status === 202 ? (undefined as T) : res.json();
}
export const post = <T>(path: string, data: unknown) =>
  request<T>(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
  });
export const snapshot = () => request<Snapshot>("/admin/state");
export const reply = (alias: string, prompt: string, history: Turn[]) =>
  post<{ turns: Turn[]; metrics: string; finish_reason: string | null }>(`/admin/chat/${alias}`, {
    prompt,
    history,
  });
export const saveModelSettings = (alias: string, label: string, context_limit: number | null) =>
  post<void>(`/admin/models/${alias}/settings`, { label, context_limit });
export const modelAction = (alias: string, op: string) =>
  post<void>(`/admin/models/${alias}/${op}`, {});
export const install = (repo: string, preset: string) =>
  post<void>("/admin/downloads", { repo, preset });
export const catalog = () => request<string[]>("/admin/catalog");
export const packagePresets = (repo: string, signal: AbortSignal) =>
  request<string[]>("/admin/presets?" + new URLSearchParams({ repo }), { signal });

export interface RuntimeStatus {
  runtime: string;
  setup: { phase: "preparing" | "ready" } | { phase: "failed"; error: string };
}
export const runtimeStatus = () => request<RuntimeStatus>("/admin/runtime");
export const retryRuntime = () => post<void>("/admin/runtime/retry", {});
export interface ApiInfo {
  base: string;
  keys: { id: number; name: string; created: string }[];
}
export const apiInfo = () => request<ApiInfo>("/admin/api");
export const createKey = (name: string) => post<{ secret: string }>("/admin/keys", { name });
export const revokeKey = (id: number) => post<void>(`/admin/keys/${id}/revoke`, {});
