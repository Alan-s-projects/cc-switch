import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { Provider } from "@/types";
import type { AppId, ManagedAuthProvider } from "@/lib/api";
import { ProviderForm } from "./forms/ProviderForm";
import { AuthSettingsPanel } from "./AuthSettingsPanel";

interface EditProviderDialogProps {
  open: boolean;
  provider: Provider | null;
  onOpenChange: (open: boolean) => void;
  onSubmit: (payload: { provider: Provider }) => Promise<void> | void;
  appId: AppId;
}

export function EditProviderDialog({
  open,
  provider,
  onOpenChange,
  onSubmit,
}: EditProviderDialogProps) {
  const { t } = useTranslation();
  const [authTarget, setAuthTarget] = useState<ManagedAuthProvider | null>(
    null,
  );
  useEffect(() => {
    setAuthTarget(null);
  }, [open, provider?.id]);
  const close = () => {
    setAuthTarget(null);
    onOpenChange(false);
  };
  return (
    <>
      <FullScreenPanel
        isOpen={open}
        title="GitHub Copilot setup"
        onClose={close}
      >
        {open && provider && (
          <ProviderForm
            key={provider.id}
            appId="codex"
            initialData={provider}
            submitLabel={t("common.save")}
            onCancel={close}
            onManageAuthAccounts={setAuthTarget}
            onSubmit={async (values) => {
              await onSubmit({
                provider: {
                  ...provider,
                  settingsConfig: JSON.parse(values.settingsConfig),
                  meta: values.meta,
                },
              });
              close();
            }}
          />
        )}
      </FullScreenPanel>
      <AuthSettingsPanel
        target={authTarget}
        onClose={() => setAuthTarget(null)}
      />
    </>
  );
}
