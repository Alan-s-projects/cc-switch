import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  Check,
  ChevronsUpDown,
  Loader2,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { CopilotAuthSection } from "./CopilotAuthSection";
import {
  copilotGetModels,
  copilotGetModelsForAccount,
  type CopilotModel,
} from "@/lib/api/copilot";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isGptModel } from "@/utils/codexModelCatalog";
import type { CodexCatalogModel, CodexCopilotApiFormat } from "@/types";
import type { ManagedAuthProvider } from "@/lib/api";

export function isCopilotModelSupportedByCodex(
  model: CopilotModel,
  format: CodexCopilotApiFormat = "auto",
): boolean {
  if (!isGptModel(model.id)) return false;
  const endpoints =
    format === "openai_responses"
      ? ["/responses", "/v1/responses"]
      : format === "openai_chat"
        ? ["/chat/completions", "/v1/chat/completions"]
        : [
            "/responses",
            "/v1/responses",
            "/chat/completions",
            "/v1/chat/completions",
          ];
  return (model.supported_endpoints ?? []).some((endpoint) =>
    endpoints.includes(
      endpoint.split("?")[0].replace(/\/+$/, "").toLowerCase(),
    ),
  );
}

export function resolveCopilotCatalogContextWindow(
  current: CodexCatalogModel["contextWindow"],
  reported: number | undefined,
): CodexCatalogModel["contextWindow"] {
  const configured = Number(current);
  if (!Number.isFinite(configured) || configured <= 0) return reported;
  return reported && reported > 0 ? Math.min(configured, reported) : current;
}

const DEFAULT_NEW_MODEL_REASONING_LEVELS = [
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
] as const;

export function mergeCopilotModelCapabilities(
  model: CopilotModel,
  existing?: CodexCatalogModel,
): CodexCatalogModel {
  return {
    ...existing,
    model: model.id,
    displayName: model.name || model.id,
    contextWindow: resolveCopilotCatalogContextWindow(
      existing?.contextWindow,
      model.context_window,
    ),
    // Live capabilities supersede old inferred flags. An omitted declaration
    // preserves the saved value instead of guessing that a feature is absent.
    supportsParallelToolCalls:
      model.supports_parallel_tool_calls ?? existing?.supportsParallelToolCalls,
    inputModalities:
      model.supports_vision === undefined
        ? existing?.inputModalities
        : model.supports_vision
          ? ["text", "image"]
          : ["text"],
    // Reasoning levels are editable preferences. Live data seeds an unset
    // list, but must not replace a saved choice or its default.
    reasoningLevels: existing
      ? existing.reasoningLevels?.length
        ? existing.reasoningLevels
        : model.reasoning_efforts
      : model.reasoning_efforts?.length
        ? model.reasoning_efforts
        : [...DEFAULT_NEW_MODEL_REASONING_LEVELS],
    defaultReasoningLevel: existing?.defaultReasoningLevel,
  };
}

interface CodexFormFieldsProps {
  isCopilotAuthenticated?: boolean;
  selectedGitHubAccountId?: string | null;
  onGitHubAccountSelect?: (id: string | null) => void;
  onManageAuthAccounts?: (target: ManagedAuthProvider) => void;
  copilotApiFormat: CodexCopilotApiFormat;
  onCopilotApiFormatChange: (value: CodexCopilotApiFormat) => void;
  catalogModels: CodexCatalogModel[];
  onCatalogModelsChange: (models: CodexCatalogModel[]) => void;
}

const CODEX_REASONING_LEVELS = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
] as const;

// Sentinel for the default-level Select: Radix Select forbids empty item
// values, so "back to Auto" needs a non-empty value mapped to undefined.
const AUTO_DEFAULT_REASONING_LEVEL = "__auto__";

function ReasoningLevelsEditor({
  levels,
  defaultLevel,
  onLevelsChange,
  onDefaultLevelChange,
}: {
  levels?: string[];
  defaultLevel?: string;
  onLevelsChange: (levels: string[] | undefined) => void;
  onDefaultLevelChange: (level: string | undefined) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const selected = (levels ?? []).filter((level) =>
    (CODEX_REASONING_LEVELS as readonly string[]).includes(level),
  );

  const toggleLevel = (level: string) => {
    const picked = selected.includes(level)
      ? selected.filter((item) => item !== level)
      : [...selected, level];
    // Store in canonical ascending-depth order (not click order): the Codex
    // picker and the generated catalog both follow array order.
    const next = (CODEX_REASONING_LEVELS as readonly string[]).filter((item) =>
      picked.includes(item),
    );
    onLevelsChange(next.length > 0 ? next : undefined);
  };

  const triggerLabel =
    selected.length > 0
      ? selected.join(", ")
      : t("codexConfig.reasoningLevelsNotSet", {
          defaultValue: "Not set",
        });

  return (
    <Popover modal open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          role="combobox"
          aria-label="Reasoning levels"
          aria-expanded={open}
          className="flex h-9 w-full items-center justify-between gap-1 rounded-md border border-border-default bg-background px-3 py-1 text-sm shadow-sm focus:outline-none focus-visible:outline-none focus:border-border-default focus-visible:border-border-default focus:ring-0 focus-visible:ring-0 disabled:cursor-not-allowed disabled:opacity-50"
        >
          <span
            className={cn(
              "truncate",
              selected.length === 0 && "text-muted-foreground",
            )}
          >
            {triggerLabel}
          </span>
          <ChevronsUpDown className="h-3.5 w-3.5 shrink-0 opacity-50" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="bottom"
        align="start"
        sideOffset={6}
        avoidCollisions
        collisionPadding={8}
        className="z-[1000] w-[var(--radix-popover-trigger-width)] p-0 border-border-default"
      >
        <Command>
          <CommandInput
            placeholder={t("codexConfig.reasoningLevelsSearch", {
              defaultValue: "Search reasoning levels...",
            })}
          />
          <CommandList>
            <CommandEmpty>
              {t("codexConfig.reasoningLevelsEmpty", {
                defaultValue: "No levels",
              })}
            </CommandEmpty>
            <CommandGroup>
              {CODEX_REASONING_LEVELS.map((level) => (
                <CommandItem
                  key={level}
                  value={level}
                  onSelect={() => toggleLevel(level)}
                >
                  <Check
                    className={cn(
                      "mr-2 h-4 w-4",
                      selected.includes(level) ? "opacity-100" : "opacity-0",
                    )}
                  />
                  <span className="flex-1">{level}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
        {selected.length > 0 && (
          <div className="border-t border-border-default p-2">
            <span className="text-xs text-muted-foreground">
              {t("codexConfig.defaultReasoningLevelLabel", {
                defaultValue: "Default level",
              })}
            </span>
            <Select
              value={defaultLevel ?? AUTO_DEFAULT_REASONING_LEVEL}
              onValueChange={(value) =>
                onDefaultLevelChange(
                  value === AUTO_DEFAULT_REASONING_LEVEL ? undefined : value,
                )
              }
            >
              <SelectTrigger className="mt-1 h-8 w-full">
                <SelectValue
                  placeholder={t(
                    "codexConfig.defaultReasoningLevelPlaceholder",
                    { defaultValue: "Auto" },
                  )}
                />
              </SelectTrigger>
              {/* Must render above the enclosing z-[1000] popover: the
                  default SelectContent z-[100] would hide the menu behind
                  the panel when it flips upward. */}
              <SelectContent className="z-[1100]">
                <SelectItem value={AUTO_DEFAULT_REASONING_LEVEL}>
                  {t("codexConfig.defaultReasoningLevelPlaceholder", {
                    defaultValue: "Auto",
                  })}
                </SelectItem>
                {selected.map((level) => (
                  <SelectItem key={level} value={level}>
                    {level}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}

export function CodexFormFields({
  isCopilotAuthenticated,
  selectedGitHubAccountId,
  onGitHubAccountSelect,
  onManageAuthAccounts,
  copilotApiFormat,
  onCopilotApiFormatChange,
  catalogModels,
  onCatalogModelsChange,
}: CodexFormFieldsProps) {
  const { t } = useTranslation();
  const [fetching, setFetching] = useState(false);
  const fetchSequence = useRef(0);
  useEffect(() => {
    fetchSequence.current += 1;
    setFetching(false);
    return () => {
      fetchSequence.current += 1;
    };
  }, [selectedGitHubAccountId, isCopilotAuthenticated, copilotApiFormat]);

  const fetchModels = async () => {
    if (!isCopilotAuthenticated) {
      toast.error("Sign in to GitHub Copilot first.");
      return;
    }
    const sequence = ++fetchSequence.current;
    setFetching(true);
    try {
      const models = selectedGitHubAccountId
        ? await copilotGetModelsForAccount(selectedGitHubAccountId)
        : await copilotGetModels();
      if (sequence !== fetchSequence.current) return;
      const existing = new Map(
        catalogModels.map((model) => [model.model.trim().toLowerCase(), model]),
      );
      const usable = models.filter((model) =>
        isCopilotModelSupportedByCodex(model, copilotApiFormat),
      );
      if (!usable.length) {
        toast.error(
          "No GPT models support the selected protocol. Try Automatic.",
        );
        return;
      }
      onCatalogModelsChange(
        usable.map((model) =>
          mergeCopilotModelCapabilities(
            model,
            existing.get(model.id.trim().toLowerCase()),
          ),
        ),
      );
      toast.success(`Loaded ${usable.length} Copilot GPT models.`);
    } catch (error) {
      if (sequence === fetchSequence.current)
        toast.error(extractErrorMessage(error));
    } finally {
      if (sequence === fetchSequence.current) setFetching(false);
    }
  };
  const updateModel = (index: number, patch: Partial<CodexCatalogModel>) =>
    onCatalogModelsChange(
      catalogModels.map((model, position) =>
        position === index ? { ...model, ...patch } : model,
      ),
    );
  return (
    <div className="space-y-6">
      <CopilotAuthSection
        mode="select"
        selectedAccountId={selectedGitHubAccountId}
        onAccountSelect={onGitHubAccountSelect}
        onManageAccounts={
          onManageAuthAccounts
            ? () => onManageAuthAccounts("github_copilot")
            : undefined
        }
      />
      <div className="space-y-2">
        <Label htmlFor="codex-upstream-format">Upstream format</Label>
        <Select
          value={copilotApiFormat}
          onValueChange={(value) =>
            onCopilotApiFormatChange(value as CodexCopilotApiFormat)
          }
        >
          <SelectTrigger id="codex-upstream-format">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="auto">Automatic (recommended)</SelectItem>
            <SelectItem value="openai_responses">Responses</SelectItem>
            <SelectItem value="openai_chat">Chat Completions</SelectItem>
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          Automatic uses each model's supported Copilot endpoint, preferring
          Responses. Codex always connects to Atlas using Responses.
        </p>
      </div>
      <section className="space-y-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h3 className="text-sm font-medium">Model catalog</h3>
          <div className="flex gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={fetching}
              onClick={() => void fetchModels()}
            >
              {fetching ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <RefreshCw className="mr-2 h-4 w-4" />
              )}
              Refresh models
            </Button>
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() =>
                onCatalogModelsChange([...catalogModels, { model: "" }])
              }
            >
              <Plus className="mr-2 h-4 w-4" />
              Add model
            </Button>
          </div>
        </div>
        <p className="text-xs text-muted-foreground">
          Refresh updates image and parallel-tool capabilities. New models
          default to Copilot-reported reasoning levels, or the standard set if
          none are reported. Saved choices survive refresh. Input limits cap the
          catalog context window.
        </p>
        {catalogModels.length === 0 && (
          <p role="status" className="text-sm text-muted-foreground">
            {isCopilotAuthenticated
              ? t("codexConfig.catalogEmptySignedIn", {
                  defaultValue:
                    "No models are in this catalog yet. Refresh models to load those available to your GitHub Copilot account.",
                })
              : t("codexConfig.catalogEmptySignedOut", {
                  defaultValue:
                    "No models are available while GitHub Copilot is signed out. Sign in to GitHub Copilot, then refresh models to load the models your account can use.",
                })}
          </p>
        )}
        {catalogModels.map((model, index) => (
          <div key={index} className="space-y-2 rounded-lg border p-3">
            <div className="grid gap-2 md:grid-cols-[1fr_1fr_140px_1fr_36px]">
              <Input
                aria-label="Display name"
                value={model.displayName ?? ""}
                placeholder="GPT-6 Astra"
                onChange={(event) =>
                  updateModel(index, { displayName: event.target.value })
                }
              />
              <Input
                aria-label="Model ID"
                value={model.model}
                placeholder="gpt-6-astra"
                onChange={(event) =>
                  updateModel(index, { model: event.target.value })
                }
              />
              <Input
                aria-label="Context window"
                type="number"
                min={1}
                value={model.contextWindow ?? ""}
                placeholder="Input tokens"
                onChange={(event) =>
                  updateModel(index, {
                    contextWindow: event.target.value.replace(/[^\d]/g, ""),
                  })
                }
              />
              <ReasoningLevelsEditor
                levels={model.reasoningLevels}
                defaultLevel={model.defaultReasoningLevel}
                onLevelsChange={(reasoningLevels) =>
                  updateModel(index, {
                    reasoningLevels,
                    defaultReasoningLevel: reasoningLevels?.includes(
                      model.defaultReasoningLevel ?? "",
                    )
                      ? model.defaultReasoningLevel
                      : undefined,
                  })
                }
                onDefaultLevelChange={(defaultReasoningLevel) =>
                  updateModel(index, { defaultReasoningLevel })
                }
              />
              <Button
                type="button"
                size="icon"
                variant="ghost"
                aria-label={`Remove model ${model.model || index + 1}`}
                onClick={() =>
                  onCatalogModelsChange(
                    catalogModels.filter((_, position) => position !== index),
                  )
                }
              >
                <Trash2 className="h-4 w-4" />
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              Images:{" "}
              {model.inputModalities
                ? model.inputModalities.includes("image")
                  ? "supported"
                  : "not supported"
                : "not reported"}{" "}
              · Parallel tools:{" "}
              {model.supportsParallelToolCalls === undefined
                ? "not reported"
                : model.supportsParallelToolCalls
                  ? "supported"
                  : "not supported"}
            </p>
          </div>
        ))}
      </section>
    </div>
  );
}
