import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { proxyApi } from "@/lib/api/proxy";
import { proxyKeys, useProxyStatusQuery } from "@/lib/query/proxy";
import { extractErrorMessage } from "@/utils/errorUtils";

export function useProxyStatus() {
  const queryClient = useQueryClient();
  const query = useProxyStatusQuery();
  const toggle = useMutation({
    mutationFn: async (enabled: boolean) => {
      if (enabled) await proxyApi.startProxyServer();
      else await proxyApi.stopProxyServer();
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: proxyKeys.status });
      await queryClient.invalidateQueries({ queryKey: proxyKeys.globalConfig });
      await queryClient.invalidateQueries({
        queryKey: ["codex-setup-suggestion"],
      });
    },
    onError: (error) => toast.error(extractErrorMessage(error)),
  });
  return {
    status: query.data,
    isRunning: query.data?.running ?? false,
    isLoading: query.isLoading,
    isPending: toggle.isPending,
    toggleProxy: toggle.mutate,
    toggleProxyAsync: toggle.mutateAsync,
  };
}
