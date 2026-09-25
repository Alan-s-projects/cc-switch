import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Provider } from "@/types";
import type { AppId } from "./types";

export interface ProviderSwitchEvent {
  appType: AppId;
  providerId: string;
}
export const providersApi = {
  getAll: (appId: AppId): Promise<Record<string, Provider>> =>
    invoke("get_providers", { app: appId }),
  getCurrent: (appId: AppId): Promise<string> =>
    invoke("get_current_provider", { app: appId }),
  update: (provider: Provider, appId: AppId): Promise<boolean> =>
    invoke("update_provider", { provider, app: appId }),
  updateTrayMenu: (): Promise<boolean> => invoke("update_tray_menu"),
  onSwitched: (
    handler: (event: ProviderSwitchEvent) => void,
  ): Promise<UnlistenFn> =>
    listen<ProviderSwitchEvent>("provider-switched", (event) =>
      handler(event.payload),
    ),
};
