import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Form } from "@/components/ui/form";
import { Button } from "@/components/ui/button";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { AppId, ManagedAuthProvider } from "@/lib/api";
import type {
  Provider,
  ProviderMeta,
  ProviderCategory,
  CodexCatalogModel,
  CodexCopilotApiFormat,
} from "@/types";
import { CodexFormFields } from "./CodexFormFields";
import { BasicFormFields } from "./BasicFormFields";
import {
  ProviderAdvancedConfig,
  type PricingModelSourceOption,
} from "./ProviderAdvancedConfig";
import { useCopilotAuth } from "./hooks/useCopilotAuth";
import { mapCodexCatalogModelForForm } from "./hooks/useCodexConfigState";
import { codexProviderPresets } from "@/config/codexProviderPresets";

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
  providerId?: string;
  submitLabel: string;
  onSubmit: (values: ProviderFormValues) => Promise<void> | void;
  onCancel: () => void;
  onManageAuthAccounts?: (target: ManagedAuthProvider) => void;
  onSubmittingChange?: (value: boolean) => void;
  onSubmitReadyChange?: (value: boolean) => void;
  initialData?: Partial<Provider>;
  showButtons?: boolean;
  isProxyTakeover?: boolean;
}

export type ProviderFormValues = ProviderFormData & {
  presetId?: string;
  presetCategory?: ProviderCategory;
  meta?: ProviderMeta;
};

const noop = () => {};

export function ProviderForm({
  providerId,
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
  const preset = codexProviderPresets.find(
    (p) => p.providerType === "github_copilot",
  )!;
  const settings = initialData?.settingsConfig ?? {
    auth: {},
    config: preset.config,
    modelCatalog: { models: preset.modelCatalog ?? [] },
  };
  const initialMeta = initialData?.meta;
  const form = useForm<ProviderFormData>({
    defaultValues: {
      name: initialData?.name ?? "GitHub Copilot",
      websiteUrl:
        initialData?.websiteUrl ?? "https://github.com/features/copilot",
      notes: initialData?.notes ?? "",
      settingsConfig: JSON.stringify(settings),
      icon: initialData?.icon ?? "github",
      iconColor: initialData?.iconColor ?? "#000000",
    },
  });
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
  const [reasoning, setReasoning] = useState(
    initialMeta?.codexChatReasoning ?? {},
  );
  const [pricing, setPricing] = useState<{
    enabled: boolean;
    costMultiplier?: string;
    pricingModelSource: PricingModelSourceOption;
  }>({
    enabled:
      initialMeta?.costMultiplier !== undefined ||
      initialMeta?.pricingModelSource !== undefined,
    costMultiplier: initialMeta?.costMultiplier,
    pricingModelSource:
      initialMeta?.pricingModelSource === "request" ||
      initialMeta?.pricingModelSource === "response"
        ? initialMeta.pricingModelSource
        : "inherit",
  });
  const [cacheRouting, setCacheRouting] = useState(
    initialMeta?.promptCacheRouting ?? "auto",
  );
  const [userAgent, setUserAgent] = useState(
    initialMeta?.customUserAgent ?? "",
  );
  const [headers, setHeaders] = useState(
    initialMeta?.localProxyRequestOverrides?.headers
      ? JSON.stringify(initialMeta.localProxyRequestOverrides.headers, null, 2)
      : "",
  );
  const [body, setBody] = useState(
    initialMeta?.localProxyRequestOverrides?.body
      ? JSON.stringify(initialMeta.localProxyRequestOverrides.body, null, 2)
      : "",
  );
  useEffect(() => {
    onSubmittingChange?.(saving);
  }, [saving, onSubmittingChange]);
  useEffect(() => {
    onSubmitReadyChange?.(true);
  }, [onSubmitReadyChange]);

  const submit = form.handleSubmit(async (values) => {
    if (!values.name.trim()) {
      toast.error(t("provider.nameRequired"));
      return;
    }
    if (
      pricing.enabled &&
      pricing.costMultiplier?.trim() &&
      (!Number.isFinite(Number(pricing.costMultiplier)) ||
        Number(pricing.costMultiplier) < 0)
    ) {
      toast.error(t("settings.globalProxy.defaultCostMultiplierInvalid"));
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
        codexChatReasoning: reasoning,
        promptCacheRouting: cacheRouting,
        customUserAgent: userAgent || undefined,
        costMultiplier: pricing.enabled ? pricing.costMultiplier : undefined,
        pricingModelSource:
          pricing.enabled && pricing.pricingModelSource !== "inherit"
            ? pricing.pricingModelSource
            : undefined,
        localProxyRequestOverrides: {
          headers: headers.trim() ? JSON.parse(headers) : undefined,
          body: body.trim() ? JSON.parse(body) : undefined,
        },
      };
      await onSubmit({
        ...values,
        presetCategory: "third_party",
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
  });

  return (
    <Form {...form}>
      <form id="provider-form" onSubmit={submit} className="space-y-6">
        <BasicFormFields form={form} />
        <p className="text-sm text-muted-foreground">
          {t("bridge.providerSettings")}
        </p>
        <CodexFormFields
          appId="codex"
          providerId={providerId}
          isCopilotPreset
          isCopilotAuthenticated={hasAnyAccount}
          selectedGitHubAccountId={accountId}
          onGitHubAccountSelect={setAccountId}
          onManageAuthAccounts={onManageAuthAccounts}
          codexApiKey=""
          onApiKeyChange={noop}
          category="third_party"
          shouldShowApiKeyLink={false}
          websiteUrl="https://github.com/features/copilot"
          shouldShowSpeedTest={false}
          codexBaseUrl="https://api.githubcopilot.com"
          onBaseUrlChange={noop}
          isFullUrl={false}
          onFullUrlChange={noop}
          isEndpointModalOpen={false}
          onEndpointModalToggle={noop}
          autoSelect={false}
          onAutoSelectChange={noop}
          speedTestEndpoints={[]}
          apiFormat={format === "auto" ? "openai_chat" : format}
          onApiFormatChange={noop}
          copilotApiFormat={format}
          onCopilotApiFormatChange={setFormat}
          anthropicAuthField="ANTHROPIC_AUTH_TOKEN"
          onAnthropicAuthFieldChange={noop}
          impersonateClaudeCode={false}
          onImpersonateClaudeCodeChange={noop}
          maxOutputTokens=""
          onMaxOutputTokensChange={noop}
          codexChatReasoning={reasoning}
          onCodexChatReasoningChange={setReasoning}
          promptCacheRouting={cacheRouting}
          onPromptCacheRoutingChange={setCacheRouting}
          catalogModels={catalog}
          onCatalogModelsChange={setCatalog}
          customUserAgent={userAgent}
          onCustomUserAgentChange={setUserAgent}
          localProxyHeadersOverride={headers}
          onLocalProxyHeadersOverrideChange={setHeaders}
          localProxyBodyOverride={body}
          onLocalProxyBodyOverrideChange={setBody}
        />
        <ProviderAdvancedConfig
          pricingConfig={pricing}
          onPricingConfigChange={setPricing}
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
    </Form>
  );
}
