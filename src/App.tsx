import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Settings,
  ArrowLeft,
  BarChart2,
  FileText,
  Loader2,
} from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { useProvidersQuery } from "@/lib/query";
import { providersApi } from "@/lib/api";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { isTextEditableTarget } from "@/utils/domUtils";
import { CopilotCard } from "@/components/providers/CopilotCard";
import { HealthCheckButton } from "@/components/providers/HealthCheckButton";
import { SettingsPage } from "@/components/settings/SettingsPage";
import { UsagePage } from "@/components/usage/UsagePage";
import { BridgeOverview } from "@/components/overview/BridgeOverview";
import { CodexSetupSuggestion } from "@/components/providers/CodexSetupSuggestion";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
import { RoutingActivationBrand } from "@/components/proxy/RoutingActivationBrand";
import { Button } from "@/components/ui/button";

type View = "provider" | "settings" | "setup" | "usage";

export default function App() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [view, setView] = useState<View>("provider");
  const { isRunning, status } = useProxyStatus();
  const { data, isLoading, refetch } = useProvidersQuery("codex");
  const provider =
    data?.providers[data.currentProviderId] ??
    Object.values(data?.providers ?? {})[0];

  useEffect(() => {
    let off: (() => void) | undefined;
    let active = true;
    void providersApi
      .onSwitched(() => {
        void refetch();
      })
      .then((unsubscribe) => {
        if (active) off = unsubscribe;
        else unsubscribe();
      });
    return () => {
      active = false;
      off?.();
    };
  }, [refetch]);

  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "," && (event.ctrlKey || event.metaKey)) {
        event.preventDefault();
        setView("settings");
      } else if (
        event.key === "Escape" &&
        !event.defaultPrevented &&
        document.body.style.overflow !== "hidden" &&
        !isTextEditableTarget(event.target)
      ) {
        setView("provider");
      }
    };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, []);

  const refreshData = async () => {
    await queryClient.invalidateQueries();
    await providersApi.updateTrayMenu();
  };

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-background text-foreground">
      <header className="flex h-16 shrink-0 items-center justify-between gap-3 px-6">
        <div className="flex items-center gap-2">
          {view === "provider" ? (
            <RoutingActivationBrand
              active={isRunning}
              contextKey="codex"
              ready={status !== undefined}
            />
          ) : (
            <>
              <Button
                variant="outline"
                size="sm"
                aria-label={t("common.back")}
                onClick={() => setView("provider")}
              >
                <ArrowLeft aria-hidden className="mr-2 h-4 w-4" />
                {t("common.back")}
              </Button>
              <h1 className="text-lg font-semibold">
                {view === "setup"
                  ? t("bridge.setup")
                  : view === "usage"
                    ? t("usage.title")
                    : t("settings.title")}
              </h1>
            </>
          )}
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="sm"
            title={t("usage.title")}
            onClick={() => setView("usage")}
          >
            <BarChart2 aria-hidden className="mr-2 h-4 w-4" />
            Usage
          </Button>
          <Button
            variant="ghost"
            size="sm"
            title={t("common.settings")}
            onClick={() => setView("settings")}
          >
            <Settings aria-hidden className="mr-2 h-4 w-4" />
            {t("common.settings")}
          </Button>
          <HealthCheckButton providerId={provider?.id} />
          <Button
            variant="ghost"
            size="sm"
            title={t("bridge.setup")}
            onClick={() => setView("setup")}
          >
            <FileText aria-hidden className="mr-2 h-4 w-4" />
            Connect
          </Button>
          <ProxyToggle />
        </div>
      </header>
      <main
        className={`flex min-h-0 flex-1 flex-col ${view === "setup" ? "overflow-hidden" : "overflow-y-auto"}`}
      >
        {view === "settings" ? (
          <SettingsPage
            open
            onOpenChange={() => setView("provider")}
            onImportSuccess={refreshData}
          />
        ) : view === "setup" ? (
          <CodexSetupSuggestion />
        ) : view === "usage" ? (
          <UsagePage />
        ) : (
          <div className="space-y-5 px-6 pb-6 pt-4">
            {isLoading ? (
              <Loader2 className="h-6 w-6 animate-spin" />
            ) : provider ? (
              <>
                <CopilotCard provider={provider} />
                <BridgeOverview status={status} />
              </>
            ) : (
              <Button variant="outline" onClick={() => void refetch()}>
                Reload GitHub Copilot
              </Button>
            )}
          </div>
        )}
      </main>
    </div>
  );
}
