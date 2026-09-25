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
import { useProvidersQuery, useUpdateProviderMutation } from "@/lib/query";
import { providersApi } from "@/lib/api";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { isTextEditableTarget } from "@/utils/domUtils";
import { CopilotCard } from "@/components/providers/CopilotCard";
import { EditProviderDialog } from "@/components/providers/EditProviderDialog";
import { SettingsPage } from "@/components/settings/SettingsPage";
import { CodexSetupSuggestion } from "@/components/providers/CodexSetupSuggestion";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
import { RoutingActivationBrand } from "@/components/proxy/RoutingActivationBrand";
import { Button } from "@/components/ui/button";

type View = "provider" | "settings" | "setup";

export default function App() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [view, setView] = useState<View>("provider");
  const [settingsTab, setSettingsTab] = useState("general");
  const [editing, setEditing] = useState(false);
  const { isRunning, status } = useProxyStatus();
  const { data, isLoading, refetch } = useProvidersQuery("codex");
  const provider =
    data?.providers[data.currentProviderId] ??
    Object.values(data?.providers ?? {})[0];
  const update = useUpdateProviderMutation("codex");

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
        setSettingsTab("general");
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

  const openSettings = (tab: string) => {
    setSettingsTab(tab);
    setView("settings");
  };
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
                size="icon"
                aria-label={t("common.back")}
                onClick={() => setView("provider")}
              >
                <ArrowLeft className="h-4 w-4" />
              </Button>
              <h1 className="text-lg font-semibold">
                {view === "setup" ? t("bridge.setup") : t("settings.title")}
              </h1>
            </>
          )}
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="icon"
            title={t("usage.title")}
            onClick={() => openSettings("usage")}
          >
            <BarChart2 className="h-4 w-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            title={t("bridge.setup")}
            onClick={() => setView("setup")}
          >
            <FileText className="h-4 w-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            title={t("common.settings")}
            onClick={() => openSettings("general")}
          >
            <Settings className="h-4 w-4" />
          </Button>
          <ProxyToggle />
        </div>
      </header>
      <main className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        {view === "settings" ? (
          <SettingsPage
            open
            onOpenChange={() => setView("provider")}
            defaultTab={settingsTab}
            onImportSuccess={refreshData}
          />
        ) : view === "setup" ? (
          <CodexSetupSuggestion />
        ) : (
          <div className="px-6 pb-6 pt-4">
            {isLoading ? (
              <Loader2 className="h-6 w-6 animate-spin" />
            ) : provider ? (
              <CopilotCard
                provider={provider}
                onEdit={() => setEditing(true)}
              />
            ) : (
              <Button variant="outline" onClick={() => void refetch()}>
                Reload GitHub Copilot
              </Button>
            )}
          </div>
        )}
      </main>
      <EditProviderDialog
        open={editing}
        provider={provider ?? null}
        appId="codex"
        onOpenChange={setEditing}
        onSubmit={async ({ provider: next }) => {
          await update.mutateAsync({ provider: next });
          setEditing(false);
          await providersApi.updateTrayMenu();
        }}
      />
    </div>
  );
}
