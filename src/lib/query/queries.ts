import { useQuery, type UseQueryResult } from "@tanstack/react-query";
import { providersApi, settingsApi } from "@/lib/api";
import type { Provider, Settings } from "@/types";

interface ProvidersQueryData {
  providers: Record<string, Provider>;
  currentProviderId: string;
}

export const useProvidersQuery = (): UseQueryResult<ProvidersQueryData> =>
  useQuery({
    queryKey: ["providers", "codex"],
    queryFn: async () => {
      const [providers, currentProviderId] = await Promise.all([
        providersApi.getAll(),
        providersApi.getCurrent(),
      ]);
      return { providers, currentProviderId };
    },
  });

export const useSettingsQuery = (): UseQueryResult<Settings> =>
  useQuery({
    queryKey: ["settings"],
    queryFn: () => settingsApi.get(),
  });
