import { useQuery } from "@tanstack/react-query";
import { copilotGetUsage, copilotGetUsageForAccount } from "@/lib/api/copilot";
import { useWindowActive } from "@/lib/windowActivity";

const REFETCH_INTERVAL = 5 * 60 * 1000; // 5 minutes

interface CopilotQuota {
  plan: string | null;
  resetDate: string | null;
  utilization: number;
}

export function useCopilotQuota(accountId: string | null) {
  const active = useWindowActive();
  return useQuery<CopilotQuota>({
    queryKey: ["copilot", "quota", accountId ?? "default"],
    queryFn: async (): Promise<CopilotQuota> => {
      const usage = accountId
        ? await copilotGetUsageForAccount(accountId)
        : await copilotGetUsage();

      const premium = usage.quota_snapshots.premium_interactions;
      const utilization =
        premium.entitlement > 0
          ? ((premium.entitlement - premium.remaining) / premium.entitlement) *
            100
          : 0;

      return {
        plan: usage.copilot_plan,
        resetDate: usage.quota_reset_date,
        utilization,
      };
    },
    enabled: active,
    refetchInterval: active ? REFETCH_INTERVAL : false,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
    staleTime: REFETCH_INTERVAL,
    retry: 1,
  });
}
