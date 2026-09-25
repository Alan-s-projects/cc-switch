import { http, HttpResponse } from "msw";
import type { AppId } from "@/lib/api/types";
import type { Provider, Settings } from "@/types";
import { MODELS_DEV_API_URL } from "@/lib/modelsDevPricing";
import {
  getProviders,
  getCurrentProviderId,
  updateProvider,
  getSettings,
  setSettings,
  getAppConfigDirOverride,
  setAppConfigDirOverrideState,
} from "./state";
const root = "http://tauri.local";
const success = <T>(data: T) => HttpResponse.json(data as never);
const body = async <T>(request: Request): Promise<T> => {
  const text = await request.text();
  return text ? (JSON.parse(text) as T) : ({} as T);
};
export const handlers = [
  http.get(MODELS_DEV_API_URL, () => success({})),
  http.post(`${root}/get_providers`, async ({ request }) =>
    success(getProviders((await body<{ app: AppId }>(request)).app)),
  ),
  http.post(`${root}/get_current_provider`, async ({ request }) =>
    success(getCurrentProviderId((await body<{ app: AppId }>(request)).app)),
  ),
  http.post(`${root}/update_provider`, async ({ request }) => {
    const data = await body<{ app: AppId; provider: Provider }>(request);
    updateProvider(data.app, data.provider);
    return success(true);
  }),
  http.post(`${root}/get_settings`, () => success(getSettings())),
  http.post(`${root}/save_settings`, async ({ request }) => {
    setSettings((await body<{ settings: Settings }>(request)).settings);
    return success(true);
  }),
  http.post(`${root}/get_app_config_dir_override`, () =>
    success(getAppConfigDirOverride()),
  ),
  http.post(`${root}/set_app_config_dir_override`, async ({ request }) => {
    setAppConfigDirOverrideState(
      (await body<{ path: string | null }>(request)).path ?? null,
    );
    return success(true);
  }),
  http.post(`${root}/pick_directory`, async ({ request }) => {
    const { defaultPath } = await body<{ defaultPath?: string }>(request);
    return success(
      defaultPath ? `${defaultPath}/picked` : "/mock/selected-dir",
    );
  }),
  http.post(`${root}/open_file_dialog`, () =>
    success("/mock/import-settings.sql"),
  ),
  http.post(`${root}/save_file_dialog`, () =>
    success("/mock/export-settings.sql"),
  ),
  http.post(`${root}/import_config_from_file`, async ({ request }) => {
    const { filePath } = await body<{ filePath: string }>(request);
    return success(
      filePath
        ? { success: true, backupId: "backup-123" }
        : { success: false, message: "Missing file" },
    );
  }),
  http.post(`${root}/export_config_to_file`, async ({ request }) => {
    const { filePath } = await body<{ filePath: string }>(request);
    return success(
      filePath
        ? { success: true, filePath }
        : { success: false, message: "Invalid destination" },
    );
  }),
  http.post(`${root}/get_proxy_status`, () =>
    success({
      running: false,
      address: "127.0.0.1",
      port: 15721,
      active_connections: 0,
      total_requests: 0,
      success_requests: 0,
      failed_requests: 0,
      success_rate: 0,
      uptime_seconds: 0,
      current_provider: null,
      current_provider_id: null,
      last_request_at: null,
      last_error: null,
      active_targets: [],
    }),
  ),
  http.post(`${root}/get_default_cost_multiplier`, () => success("1")),
  http.post(`${root}/get_pricing_model_source`, () => success("response")),
  http.post(`${root}/get_global_proxy_config`, () =>
    success({
      proxyEnabled: false,
      listenAddress: "127.0.0.1",
      listenPort: 15721,
      enableLogging: true,
    }),
  ),
  ...["get_migration_result", "is_portable_mode", "get_auto_launch_status"].map(
    (command) => http.post(`${root}/${command}`, () => success(false)),
  ),
  ...["restart_app", "update_tray_menu", "set_auto_launch"].map((command) =>
    http.post(`${root}/${command}`, () => success(true)),
  ),
  http.post(`${root}/sync_current_providers_live`, () =>
    success({ success: true }),
  ),
  http.post(`${root}/list_db_backups`, () => success([])),
];
