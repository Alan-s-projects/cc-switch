import { useState } from "react";
import { useIsFetching, useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { bridgeOverviewKey } from "@/hooks/useBridgeOverview";
import { proxyKeys } from "@/lib/query/proxy";
import { Button } from "@/components/ui/button";

export function OverviewRefreshButton() {
  const queryClient = useQueryClient();
  const [refreshing, setRefreshing] = useState(false);
  const overviewFetching = useIsFetching({ queryKey: bridgeOverviewKey });
  const connectionFetching = useIsFetching({
    queryKey: ["codex-setup-suggestion", "connection-check"],
  });
  const refresh = async () => {
    setRefreshing(true);
    try {
      await Promise.all(
        [
          bridgeOverviewKey,
          proxyKeys.status,
          proxyKeys.globalConfig,
          ["codex-setup-suggestion", "connection-check"],
          ["copilot", "quota"],
          ["managed-auth-status", "github_copilot"],
          ["providers", "codex"],
        ].map((queryKey) => queryClient.invalidateQueries({ queryKey })),
      );
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <Button
      variant="outline"
      size="sm"
      className="shrink-0"
      disabled={refreshing || overviewFetching > 0 || connectionFetching > 0}
      onClick={() => void refresh()}
    >
      <RefreshCw aria-hidden className="h-4 w-4" />
      Refresh overview
    </Button>
  );
}
