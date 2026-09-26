import { settingsApi } from "@/lib/api";

/** Refresh the app-owned model catalog after restoring local data. */
export async function syncCurrentProvidersLiveSafe(): Promise<{
  ok: boolean;
  error?: Error;
}> {
  try {
    await settingsApi.syncCurrentProvidersLive();
    return { ok: true };
  } catch (err) {
    const error = err instanceof Error ? err : new Error(String(err ?? ""));
    return { ok: false, error };
  }
}
