import type { Model } from "./admin";

const prefixes = ["lum.chat.", "lum.draft.", "lum.installation."];
export function clearLocalChats() {
  try {
    for (const key of Object.keys(sessionStorage))
      if (prefixes.some((p) => key.startsWith(p))) sessionStorage.removeItem(key);
  } catch {
    /* optional storage */
  }
}
export function forgetChat(alias: string) {
  try {
    for (const prefix of prefixes) sessionStorage.removeItem(prefix + alias);
  } catch {
    /* optional storage */
  }
}

// Each tab owns its drafts. Also catch deletion in another tab or while this
// one was closed/suspended, including removal and reinstall between polls.
export function reconcileChats(models: Model[]) {
  try {
    const installed = new Map(
      models.filter((m) => m.state === "installed").map((m) => [m.alias, m.installation]),
    );
    for (const key of Object.keys(sessionStorage)) {
      const prefix = prefixes.find((p) => key.startsWith(p));
      if (prefix && !installed.has(key.slice(prefix.length))) forgetChat(key.slice(prefix.length));
    }
    for (const [alias, id] of installed) {
      const key = "lum.installation." + alias,
        previous = sessionStorage.getItem(key);
      if (previous !== id) {
        if (previous !== null) forgetChat(alias);
        sessionStorage.setItem(key, id);
      }
    }
  } catch {
    /* storage optional */
  }
}
