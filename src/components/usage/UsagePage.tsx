import { Loader2 } from "lucide-react";
import { useSettings } from "@/hooks/useSettings";
import { UsageDashboard } from "./UsageDashboard";

export function UsagePage() {
  const { settings, isLoading, autoSaveSettings } = useSettings();

  if (isLoading || !settings) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="px-6 pb-6 pt-4">
      <UsageDashboard
        refreshIntervalMs={settings.usageDashboardRefreshIntervalMs}
        onRefreshIntervalChange={async (usageDashboardRefreshIntervalMs) =>
          (await autoSaveSettings({ usageDashboardRefreshIntervalMs })) !== null
        }
      />
    </div>
  );
}
