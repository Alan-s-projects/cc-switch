import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { AnimatePresence, motion } from "framer-motion";
import { Plus, Settings, ArrowLeft, BarChart2, FileText } from "lucide-react";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import type { Provider } from "@/types";
import { useProvidersQuery, useSettingsQuery } from "@/lib/query";
import { providersApi, settingsApi } from "@/lib/api";
import { useProviderActions } from "@/hooks/useProviderActions";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { useUsageCacheBridge } from "@/hooks/useUsageCacheBridge";
import { useLastValidValue } from "@/hooks/useLastValidValue";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isTextEditableTarget } from "@/utils/domUtils";
import { deepClone } from "@/utils/deepClone";
import { ProviderList } from "@/components/providers/ProviderList";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import { EditProviderDialog } from "@/components/providers/EditProviderDialog";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { SettingsPage } from "@/components/settings/SettingsPage";
import { CodexSetupSuggestion } from "@/components/providers/CodexSetupSuggestion";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
import { FailoverToggle } from "@/components/proxy/FailoverToggle";
import { RoutingActivationBrand } from "@/components/proxy/RoutingActivationBrand";
import UsageScriptModal from "@/components/UsageScriptModal";
import { Button } from "@/components/ui/button";

type View = "providers" | "settings" | "setup";

function App() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [currentView, setCurrentView] = useState<View>("providers");
  const [settingsDefaultTab, setSettingsDefaultTab] = useState("general");
  const [isAddOpen, setIsAddOpen] = useState(false);
  const [editingProvider, setEditingProvider] = useState<Provider | null>(null);
  const [usageProvider, setUsageProvider] = useState<Provider | null>(null);
  const [deletingProvider, setDeletingProvider] = useState<Provider | null>(
    null,
  );
  const effectiveEditingProvider = useLastValidValue(editingProvider);
  const effectiveUsageProvider = useLastValidValue(usageProvider);
  const { data: settingsData } = useSettingsQuery();
  const { isRunning, status } = useProxyStatus();
  const { data, isLoading, refetch } = useProvidersQuery("codex", {
    isProxyRunning: isRunning,
  });
  const providers = useMemo(() => data?.providers ?? {}, [data]);
  const currentProviderId = data?.currentProviderId ?? "";
  const {
    addProvider,
    updateProvider,
    switchProvider,
    deleteProvider,
    saveUsageScript,
  } = useProviderActions("codex", isRunning, isRunning);
  useUsageCacheBridge();

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let active = true;
    void providersApi
      .onSwitched(async () => {
        await refetch();
      })
      .then((off) => {
        if (active) unsubscribe = off;
        else off();
      })
      .catch(console.error);
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [refetch]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "," && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        setCurrentView("settings");
      } else if (
        event.key === "Escape" &&
        !event.defaultPrevented &&
        document.body.style.overflow !== "hidden" &&
        !isTextEditableTarget(event.target)
      ) {
        setCurrentView("providers");
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  useTauriEvent<{ source?: string; status?: string; error?: string }>(
    "webdav-sync-status-updated",
    async (payload) => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      if (payload?.source === "auto" && payload.status === "error") {
        toast.error(
          t("settings.webdavSync.autoSyncFailedToast", {
            error: payload.error,
          }),
        );
      }
    },
  );
  useTauriEvent<{ source?: string; status?: string; error?: string }>(
    "s3-sync-status-updated",
    async (payload) => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      if (payload?.source === "auto" && payload.status === "error") {
        toast.error(
          t("settings.s3Sync.autoSyncFailedToast", { error: payload.error }),
        );
      }
    },
  );

  const openSettings = (tab: string) => {
    setSettingsDefaultTab(tab);
    setCurrentView("settings");
  };
  const handleImportSuccess = async () => {
    await queryClient.invalidateQueries({ queryKey: ["providers"] });
    await queryClient.invalidateQueries({ queryKey: ["proxy"] });
    await providersApi.updateTrayMenu();
  };
  const handleDuplicate = async (provider: Provider) => {
    const { id: _id, createdAt: _createdAt, ...copy } = deepClone(provider);
    await addProvider({ ...copy, name: `${provider.name} copy` });
  };

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-background text-foreground">
      <header className="flex h-16 shrink-0 items-center justify-between gap-3 px-6">
        <div className="flex items-center gap-2">
          {currentView === "providers" ? (
            <>
              <RoutingActivationBrand
                active={isRunning}
                contextKey="codex"
                ready={status !== undefined}
              />
              <span className="text-sm text-muted-foreground">
                Codex · GitHub Copilot
              </span>
            </>
          ) : (
            <>
              <Button
                variant="outline"
                size="icon"
                aria-label={t("common.back")}
                onClick={() => setCurrentView("providers")}
              >
                <ArrowLeft className="h-4 w-4" />
              </Button>
              <h1 className="text-lg font-semibold">
                {currentView === "setup"
                  ? t("bridge.setup")
                  : t("settings.title")}
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
            onClick={() => setCurrentView("setup")}
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
          <ProxyToggle activeApp="codex" />
          {settingsData?.enableFailoverToggle && (
            <FailoverToggle activeApp="codex" />
          )}
          {currentView === "providers" && (
            <Button
              size="icon"
              onClick={() => setIsAddOpen(true)}
              aria-label={t("provider.addNewProvider")}
            >
              <Plus className="h-5 w-5" />
            </Button>
          )}
        </div>
      </header>
      <main className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        <AnimatePresence mode="wait">
          <motion.div
            key={currentView}
            className="flex min-h-0 flex-1 flex-col"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            {currentView === "settings" ? (
              <SettingsPage
                open
                onOpenChange={() => setCurrentView("providers")}
                onImportSuccess={handleImportSuccess}
                defaultTab={settingsDefaultTab}
              />
            ) : currentView === "setup" ? (
              <CodexSetupSuggestion />
            ) : (
              <div className="overflow-y-auto px-6 pb-12">
                <ProviderList
                  providers={providers}
                  currentProviderId={currentProviderId}
                  appId="codex"
                  isLoading={isLoading}
                  isProxyRunning={isRunning}
                  isProxyTakeover={isRunning}
                  activeProviderId={currentProviderId}
                  onSwitch={switchProvider}
                  onEdit={setEditingProvider}
                  onDelete={setDeletingProvider}
                  onDuplicate={handleDuplicate}
                  onConfigureUsage={setUsageProvider}
                  onCreate={() => setIsAddOpen(true)}
                  onOpenWebsite={async (url) => {
                    try {
                      await settingsApi.openExternal(url);
                    } catch (error) {
                      toast.error(extractErrorMessage(error));
                    }
                  }}
                />
              </div>
            )}
          </motion.div>
        </AnimatePresence>
      </main>
      <AddProviderDialog
        open={isAddOpen}
        onOpenChange={setIsAddOpen}
        appId="codex"
        onSubmit={addProvider}
      />
      <EditProviderDialog
        open={Boolean(editingProvider)}
        provider={effectiveEditingProvider}
        onOpenChange={(open) => {
          if (!open) setEditingProvider(null);
        }}
        appId="codex"
        onSubmit={async ({ provider }) => {
          await updateProvider(provider);
          setEditingProvider(null);
        }}
      />
      {effectiveUsageProvider && (
        <UsageScriptModal
          key={effectiveUsageProvider.id}
          provider={effectiveUsageProvider}
          appId="codex"
          isOpen={Boolean(usageProvider)}
          onClose={() => setUsageProvider(null)}
          onSave={(script) => {
            if (usageProvider) void saveUsageScript(usageProvider, script);
          }}
        />
      )}
      <ConfirmDialog
        isOpen={Boolean(deletingProvider)}
        title={t("confirm.deleteProvider")}
        message={t("confirm.deleteProviderMessage", {
          name: deletingProvider?.name,
        })}
        onConfirm={() => {
          if (deletingProvider)
            void deleteProvider(deletingProvider.id).then(() =>
              setDeletingProvider(null),
            );
        }}
        onCancel={() => setDeletingProvider(null)}
      />
    </div>
  );
}

export default App;
