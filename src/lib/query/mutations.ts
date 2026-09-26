import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { providersApi, settingsApi } from "@/lib/api";
import type { Provider, Settings } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";

export const useUpdateProviderMutation = () => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();
  return useMutation({
    mutationFn: async ({ provider }: { provider: Provider }) => {
      await providersApi.update(provider);
      return provider;
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
      await queryClient.invalidateQueries({ queryKey: ["copilot", "quota"] });
      await queryClient.invalidateQueries({
        queryKey: ["codex-setup-suggestion"],
      });
      toast.success(t("notifications.updateSuccess"));
    },
    onError: (error) => toast.error(extractErrorMessage(error)),
  });
};

export const useSaveSettingsMutation = () => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (settings: Settings) => settingsApi.save(settings),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["settings"] }),
  });
};
