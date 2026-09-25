import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { Provider } from "@/types";
import type { AppId, ManagedAuthProvider } from "@/lib/api";
import { ProviderForm } from "./forms/ProviderForm";
import { AuthSettingsPanel } from "./AuthSettingsPanel";

interface AddProviderDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  appId: AppId;
  onSubmit: (provider: Omit<Provider, "id">) => Promise<void> | void;
}

export function AddProviderDialog({
  open,
  onOpenChange,
  onSubmit,
}: AddProviderDialogProps) {
  const { t } = useTranslation();
  const [authTarget, setAuthTarget] = useState<ManagedAuthProvider | null>(
    null,
  );
  useEffect(() => {
    setAuthTarget(null);
  }, [open]);
  const close = () => {
    setAuthTarget(null);
    onOpenChange(false);
  };
  return (
    <>
      <FullScreenPanel
        isOpen={open}
        title={t("provider.addNewProvider")}
        onClose={close}
      >
        {open && (
          <ProviderForm
            appId="codex"
            submitLabel={t("common.add")}
            onCancel={close}
            onManageAuthAccounts={setAuthTarget}
            onSubmit={async (values) => {
              const { settingsConfig, meta, ...fields } = values;
              await onSubmit({
                ...fields,
                settingsConfig: JSON.parse(settingsConfig),
                meta,
                category: "third_party",
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
