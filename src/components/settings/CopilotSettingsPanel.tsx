import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { useProvidersQuery, useUpdateProviderMutation } from "@/lib/query";
import { providersApi, type ManagedAuthProvider } from "@/lib/api";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { AuthSettingsPanel } from "@/components/providers/AuthSettingsPanel";
import { Button } from "@/components/ui/button";

export function CopilotSettingsPanel({ onCancel }: { onCancel: () => void }) {
  const { t } = useTranslation();
  const { data, isLoading, error, refetch } = useProvidersQuery("codex");
  const update = useUpdateProviderMutation("codex");
  const [authTarget, setAuthTarget] = useState<ManagedAuthProvider | null>(
    null,
  );
  const provider =
    data?.providers[data.currentProviderId] ??
    Object.values(data?.providers ?? {})[0];

  if (isLoading) {
    return <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />;
  }
  if (!provider) {
    return (
      <div className="space-y-3">
        {error && <p role="alert">{String(error)}</p>}
        <Button variant="outline" onClick={() => void refetch()}>
          Reload GitHub Copilot
        </Button>
      </div>
    );
  }

  return (
    <>
      <ProviderForm
        key={provider.id}
        initialData={provider}
        submitLabel={t("common.save")}
        onCancel={onCancel}
        onManageAuthAccounts={setAuthTarget}
        onSubmit={async (values) => {
          await update.mutateAsync({
            provider: {
              ...provider,
              settingsConfig: JSON.parse(values.settingsConfig),
              meta: values.meta,
            },
          });
          await providersApi.updateTrayMenu();
        }}
      />
      <AuthSettingsPanel
        target={authTarget}
        onClose={() => setAuthTarget(null)}
      />
    </>
  );
}
