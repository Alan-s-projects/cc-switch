import { useQuery, type UseQueryResult } from "@tanstack/react-query";
import { providersApi, settingsApi, type AppId } from "@/lib/api";
import type { Provider, Settings } from "@/types";

export interface ProvidersQueryData {
  providers: Record<string, Provider>;
  currentProviderId: string;
}

export const useProvidersQuery = (
  appId: AppId,
): UseQueryResult<ProvidersQueryData> =>
  useQuery({
    queryKey: ["providers", appId],
    queryFn: async () => {
      const [providers, currentProviderId] = await Promise.all([
        providersApi.getAll(appId),
        providersApi.getCurrent(appId),
      ]);
      return { providers, currentProviderId };
    },
  });

export const useSettingsQuery = (): UseQueryResult<Settings> =>
  useQuery({
    queryKey: ["settings"],
    queryFn: () => settingsApi.get(),
  });
