import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import type { AppId, ManagedAuthProvider } from "@/lib/api";
import type {
  Provider,
  ProviderMeta,
  CodexCatalogModel,
  CodexCopilotApiFormat,
} from "@/types";
import { CodexFormFields } from "./CodexFormFields";
import { useCopilotAuth } from "./hooks/useCopilotAuth";
import { mapCodexCatalogModelForForm } from "@/utils/codexModelCatalog";

export const normalizeCodexCatalogModelsForSave = (
  models: CodexCatalogModel[],
): CodexCatalogModel[] => {
  const seen = new Set<string>();
  const normalized: CodexCatalogModel[] = [];

  for (const item of models) {
    const model = item.model.trim();
    if (!model || seen.has(model)) continue;
    seen.add(model);

    const displayName = item.displayName?.trim();
    const rawContextWindow = String(item.contextWindow ?? "").replace(
      /[^\d]/g,
      "",
    );
    const contextWindow = rawContextWindow
      ? Number.parseInt(rawContextWindow, 10)
      : undefined;

    const inputModalities = item.inputModalities?.filter(
      (m) => typeof m === "string" && m.trim(),
    );

    const baseInstructions = item.baseInstructions?.trim();
    const reasoningLevels = item.reasoningLevels
      ?.filter((level) => typeof level === "string" && level.trim())
      .map((level) => level.trim());
    const defaultReasoningLevel = item.defaultReasoningLevel?.trim();

    normalized.push({
      model,
      ...(displayName ? { displayName } : {}),
      ...(contextWindow && contextWindow > 0 ? { contextWindow } : {}),
      // Native Responses profile overrides (ignored by the chat/proxy profile).
      ...(typeof item.supportsParallelToolCalls === "boolean"
        ? { supportsParallelToolCalls: item.supportsParallelToolCalls }
        : {}),
      ...(inputModalities && inputModalities.length > 0
        ? { inputModalities }
        : {}),
      ...(baseInstructions ? { baseInstructions } : {}),
      ...(reasoningLevels && reasoningLevels.length > 0
        ? { reasoningLevels }
        : {}),
      ...(defaultReasoningLevel ? { defaultReasoningLevel } : {}),
    });
  }

  return normalized;
};

export interface ProviderFormProps {
  appId: AppId;
  submitLabel: string;
  onSubmit: (values: ProviderFormValues) => Promise<void> | void;
  onCancel: () => void;
  onManageAuthAccounts?: (target: ManagedAuthProvider) => void;
  onSubmittingChange?: (value: boolean) => void;
  onSubmitReadyChange?: (value: boolean) => void;
  initialData?: Partial<Provider>;
  showButtons?: boolean;
}

export interface ProviderFormValues {
  name: string;
  settingsConfig: string;
  meta?: ProviderMeta;
}

export function ProviderForm({
  initialData,
  onSubmit,
  onCancel,
  submitLabel,
  onManageAuthAccounts,
  onSubmittingChange,
  onSubmitReadyChange,
  showButtons = true,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const settings = initialData?.settingsConfig ?? {
    auth: {},
    config: "",
    modelCatalog: { models: [] },
  };
  const initialMeta = initialData?.meta;
  const [saving, setSaving] = useState(false);
  const [accountId, setAccountId] = useState<string | null>(
    initialMeta?.authBinding?.accountId ?? initialMeta?.githubAccountId ?? null,
  );
  const { hasAnyAccount } = useCopilotAuth();
  const [format, setFormat] = useState<CodexCopilotApiFormat>(
    initialMeta?.codexCopilotApiFormat ?? "auto",
  );
  const [catalog, setCatalog] = useState<CodexCatalogModel[]>(() => {
    const value = settings.modelCatalog as { models?: unknown[] } | undefined;
    return (value?.models ?? []).map(mapCodexCatalogModelForForm);
  });
  useEffect(() => {
    onSubmittingChange?.(saving);
  }, [saving, onSubmittingChange]);
  useEffect(() => {
    onSubmitReadyChange?.(true);
  }, [onSubmitReadyChange]);

  const submit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!hasAnyAccount) {
      toast.error("Sign in to GitHub Copilot first.");
      return;
    }
    setSaving(true);
    try {
      const meta: ProviderMeta = {
        ...initialMeta,
        providerType: "github_copilot",
        commonConfigEnabled: false,
        apiFormat: format === "auto" ? "openai_chat" : format,
        codexCopilotApiFormat: format === "auto" ? undefined : format,
        githubAccountId: accountId ?? undefined,
        authBinding: {
          source: "managed_account",
          authProvider: "github_copilot",
          accountId: accountId ?? undefined,
        },
      };
      await onSubmit({
        name: initialData?.name ?? "GitHub Copilot",
        meta,
        settingsConfig: JSON.stringify({
          ...settings,
          modelCatalog: { models: normalizeCodexCatalogModelsForSave(catalog) },
        }),
      });
    } catch (error) {
      toast.error(String(error));
    } finally {
      setSaving(false);
    }
  };

  return (
    <form id="provider-form" onSubmit={submit} className="space-y-6">
      <p className="text-sm text-muted-foreground">
        {t("bridge.providerSettings")}
      </p>
      <CodexFormFields
        isCopilotAuthenticated={hasAnyAccount}
        selectedGitHubAccountId={accountId}
        onGitHubAccountSelect={setAccountId}
        onManageAuthAccounts={onManageAuthAccounts}
        copilotApiFormat={format}
        onCopilotApiFormatChange={setFormat}
        catalogModels={catalog}
        onCatalogModelsChange={setCatalog}
      />
      {showButtons && (
        <div className="flex justify-end gap-2">
          <Button
            type="button"
            variant="outline"
            disabled={saving}
            onClick={onCancel}
          >
            {t("common.cancel")}
          </Button>
          <Button type="submit" disabled={saving}>
            {saving ? t("common.saving") : submitLabel}
          </Button>
        </div>
      )}
    </form>
  );
}
