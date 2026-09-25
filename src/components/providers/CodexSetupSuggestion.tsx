import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Copy, FolderSearch, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { copyText } from "@/lib/clipboard";
import { ConfigDiff, type ConfigDiffLine } from "./ConfigDiff";

const PREVIEW_PATH_KEY = "cc-switch-codex-preview-path";
const normalizePath = (path: string) => path.trim().replace(/^"(.*)"$/, "$1");
const DEFAULT_RECOMMENDATIONS = {
  context1m: false,
  approvalPolicy: false,
  sandboxMode: false,
  reasoning: false,
};
const CODEX_DEFAULTS_URL =
  "https://learn.chatgpt.com/docs/config-file/config-sample";

interface SourceSettings {
  contextPreset: {
    model: string | null;
    currentContextWindow: string | null;
    currentAutoCompactTokenLimit: string | null;
    contextWindow: number;
    autoCompactTokenLimit: number;
    copilotContextWindow: number;
    copilotAutoCompactTokenLimit: number;
    copilotModelLimit: number | null;
  };
  settingDefaults: Array<{
    option: "approvalPolicy" | "sandboxMode" | "reasoning";
    key: string;
    currentValue: string;
    defaultValue: string;
  }>;
}

function readSavedPath(): string | null {
  try {
    return (
      normalizePath(window.localStorage.getItem(PREVIEW_PATH_KEY) ?? "") || null
    );
  } catch {
    return null;
  }
}

interface SetupSuggestion extends SourceSettings {
  configPath: string;
  configExists: boolean;
  currentProvider: string;
  endpoint: string;
  copilotConfig: string;
  copilotDiff: string;
  copilotLines: ConfigDiffLine[];
  openaiConfig: string;
  openaiDiff: string;
  openaiLines: ConfigDiffLine[];
}

export function CodexSetupSuggestion() {
  const [target, setTarget] = useState<"copilot" | "openai">("copilot");
  const [view, setView] = useState<"split" | "inline" | "config">("split");
  const [selectedPath, setSelectedPath] = useState<string | null>(
    readSavedPath,
  );
  const [draftPath, setDraftPath] = useState<string | null>(null);
  const [recommendations, setRecommendations] = useState(
    DEFAULT_RECOMMENDATIONS,
  );
  const [sourceSettings, setSourceSettings] = useState<
    | (SourceSettings & { selectedPath: string | null; configPath: string })
    | null
  >(null);
  const { data, error, isFetching, refetch } = useQuery({
    queryKey: ["codex-setup-suggestion", selectedPath, recommendations],
    queryFn: () =>
      invoke<SetupSuggestion>("get_codex_setup_suggestion", {
        configPath: selectedPath,
        ...(Object.values(recommendations).some(Boolean)
          ? { recommendations }
          : {}),
      }),
    staleTime: 0,
    retry: false,
  });
  const previousSource =
    sourceSettings?.selectedPath === selectedPath ? sourceSettings : null;
  const source = data ?? previousSource;
  const loadedPath = source?.configPath ?? selectedPath ?? "";
  const pathText = draftPath ?? loadedPath;
  const isPathEdited = normalizePath(pathText) !== normalizePath(loadedPath);

  useEffect(() => {
    if (!data || error || isFetching) return;
    setSourceSettings({
      selectedPath,
      configPath: data.configPath,
      contextPreset: data.contextPreset,
      settingDefaults: data.settingDefaults,
    });
    if (!selectedPath || !data.configExists) return;
    try {
      window.localStorage.setItem(PREVIEW_PATH_KEY, data.configPath);
    } catch {
      // The selected file can still be previewed when local storage is unavailable.
    }
  }, [data, error, isFetching, selectedPath]);

  const choosePath = (path: string | null) => {
    const nextPath = path ? normalizePath(path) || null : null;
    setDraftPath(null);
    if (nextPath === null) {
      try {
        window.localStorage.removeItem(PREVIEW_PATH_KEY);
      } catch {
        // Auto-detection remains available without persisted preferences.
      }
    }
    if (nextPath === selectedPath) {
      if (!isFetching) void refetch();
    } else {
      setRecommendations(DEFAULT_RECOMMENDATIONS);
      setSelectedPath(nextPath);
    }
  };
  const refresh = () => {
    if (isPathEdited) {
      if (normalizePath(pathText)) choosePath(pathText);
    } else if (!isFetching) {
      setDraftPath(null);
      void refetch();
    }
  };
  const browse = async () => {
    try {
      const path = await open({
        directory: false,
        multiple: false,
        filters: [{ name: "TOML", extensions: ["toml"] }],
        defaultPath: normalizePath(pathText) || undefined,
      });
      if (typeof path === "string") choosePath(path);
    } catch (error) {
      toast.error(String(error));
    }
  };
  const diff = target === "copilot" ? data?.copilotDiff : data?.openaiDiff;
  const lines = target === "copilot" ? data?.copilotLines : data?.openaiLines;
  const proposed =
    target === "copilot" ? data?.copilotConfig : data?.openaiConfig;
  const preset = source?.contextPreset;
  const contextWindow =
    target === "copilot" ? preset?.copilotContextWindow : preset?.contextWindow;
  const compactionLimit =
    target === "copilot"
      ? preset?.copilotAutoCompactTokenLimit
      : preset?.autoCompactTokenLimit;
  const formatTokens = (value: number) => value.toLocaleString("en-US");
  const copy = async (text: string) => {
    try {
      await copyText(text);
      toast.success("Copied");
    } catch (error) {
      toast.error(String(error));
    }
  };
  return (
    <section className="flex min-h-0 flex-1 flex-col gap-2 overflow-hidden px-6 pb-4">
      <div className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2">
        <div className="flex shrink-0 gap-2">
          <Button
            size="sm"
            variant={target === "copilot" ? "default" : "outline"}
            aria-pressed={target === "copilot"}
            onClick={() => setTarget("copilot")}
          >
            Connect through Copilot
          </Button>
          <Button
            size="sm"
            variant={target === "openai" ? "default" : "outline"}
            aria-pressed={target === "openai"}
            onClick={() => setTarget("openai")}
          >
            Return to OpenAI sign-in
          </Button>
        </div>
        <p className="min-w-40 flex-1 text-xs text-muted-foreground">
          Preview only. Atlas reads your configuration and never writes it.
          Apply changes yourself after reviewing them.
        </p>
      </div>
      <form
        className="flex shrink-0 flex-wrap items-center gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          refresh();
        }}
      >
        <Label htmlFor="codex-toml-path" className="shrink-0 text-xs">
          Codex TOML file
        </Label>
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
          <Input
            id="codex-toml-path"
            className="min-w-48 flex-1 font-mono text-sm"
            value={pathText}
            placeholder="Automatically detect config.toml"
            onChange={(event) => setDraftPath(event.target.value)}
          />
          <Button
            type="button"
            variant="outline"
            size="icon"
            title="Browse for TOML file"
            aria-label="Browse for TOML file"
            onClick={() => void browse()}
          >
            <FolderSearch className="h-4 w-4" />
          </Button>
          <Button
            type="button"
            variant="outline"
            onClick={() => choosePath(null)}
          >
            Auto-detect
          </Button>
          <Button
            type="submit"
            variant="outline"
            disabled={
              (isFetching && !isPathEdited) ||
              (isPathEdited && !normalizePath(pathText))
            }
          >
            <RefreshCw
              className={`mr-2 h-4 w-4 ${isFetching ? "animate-spin" : ""}`}
            />
            Refresh
          </Button>
        </div>
      </form>
      {isPathEdited && (
        <p role="status" className="shrink-0 text-sm text-muted-foreground">
          Refresh to preview the file at this location.
        </p>
      )}
      {error && (
        <p role="alert" className="shrink-0 text-destructive">
          {String(error)}
        </p>
      )}
      <fieldset className="shrink-0 space-y-1 rounded-xl border border-border px-3 pb-2">
        <legend className="px-1 text-xs font-medium">
          Optional TOML changes
        </legend>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
          <div className="flex shrink-0 items-center gap-2">
            <Checkbox
              id="recommend-context1m"
              checked={recommendations.context1m}
              disabled={!preset || isPathEdited || Boolean(error)}
              onCheckedChange={(checked) =>
                setRecommendations((previous) => ({
                  ...previous,
                  context1m: checked,
                }))
              }
              aria-describedby="context-preset-description"
            />
            <Label htmlFor="recommend-context1m">Use 1M context</Label>
          </div>
          <p
            id="context-preset-description"
            className="min-w-48 flex-1 text-[11px] leading-4 text-muted-foreground"
          >
            {contextWindow != null && compactionLimit != null
              ? `Context ${formatTokens(contextWindow)} · Compact at ${formatTokens(compactionLimit)} tokens.`
              : "Up to 1,000,000 context and 900,000 compaction tokens."}
            {target === "copilot" &&
              preset?.copilotModelLimit != null &&
              preset.copilotModelLimit < 1_000_000 &&
              ` Capped to the ${formatTokens(preset.copilotModelLimit)}-token saved Copilot catalog limit${preset.model ? ` for ${preset.model}` : ""}.`}
            {(target === "openai" || preset?.copilotModelLimit == null) &&
              " Actual model and provider limits still apply."}
            {preset && (
              <span className="ml-3">
                Current file: context{" "}
                {preset.currentContextWindow ?? "model default"} · compaction{" "}
                {preset.currentAutoCompactTokenLimit ?? "automatic"}.
              </span>
            )}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
          {!!source?.settingDefaults.length && (
            <Popover>
              <PopoverTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-6 px-0 text-xs"
                  disabled={isPathEdited || Boolean(error)}
                >
                  Compare with Codex defaults
                </Button>
              </PopoverTrigger>
              <PopoverContent
                align="start"
                className="max-h-64 w-[min(52rem,calc(100vw-3rem))] overflow-auto p-3 text-xs"
              >
                <table className="w-full text-left">
                  <thead className="text-muted-foreground">
                    <tr>
                      <th scope="col" className="pb-1 font-normal">
                        Setting
                      </th>
                      <th scope="col" className="pb-1 font-normal">
                        Current value
                      </th>
                      <th scope="col" className="pb-1 font-normal">
                        Codex default
                      </th>
                      <th scope="col" className="pb-1 font-normal">
                        Use default
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {source.settingDefaults.map((setting) => (
                      <tr key={setting.option}>
                        <th
                          scope="row"
                          className="py-1 pr-3 font-mono font-normal"
                        >
                          {setting.key}
                        </th>
                        <td className="py-1 pr-3 font-mono">
                          {setting.currentValue}
                        </td>
                        <td className="py-1 pr-3 font-mono">
                          {setting.defaultValue}
                        </td>
                        <td className="py-1">
                          <Checkbox
                            aria-label={`Use default for ${setting.key}`}
                            checked={recommendations[setting.option]}
                            disabled={isPathEdited || Boolean(error)}
                            onCheckedChange={(checked) =>
                              setRecommendations((previous) => ({
                                ...previous,
                                [setting.option]: checked,
                              }))
                            }
                          />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </PopoverContent>
            </Popover>
          )}
          <p className="min-w-64 flex-1 text-[11px] leading-4 text-muted-foreground">
            File values only; profiles or Codex app choices can override them.
            Proposal only.{" "}
            <a
              href={CODEX_DEFAULTS_URL}
              className="underline underline-offset-2"
              onClick={(event) => {
                event.preventDefault();
                void invoke("open_external", { url: CODEX_DEFAULTS_URL }).catch(
                  (error) => toast.error(String(error)),
                );
              }}
            >
              Official Codex defaults
            </a>
          </p>
        </div>
      </fieldset>
      {data && !error && !isPathEdited && (
        <>
          {!data.configExists && (
            <p role="status" className="shrink-0 text-sm text-muted-foreground">
              No config.toml was found at this location. Browse for your file or
              review the proposed new file below. Atlas will not create it.
            </p>
          )}
          <p className="shrink-0 text-xs text-muted-foreground">
            <span className="font-medium">
              Current provider: {data.currentProvider}.
            </span>{" "}
            {target === "copilot"
              ? `Proposed endpoint: ${data.endpoint}. Review the connection changes and selected options below.`
              : "Use Codex's built-in OpenAI provider and model catalog. After applying, sign in and choose a model in Codex if needed. Your authentication files remain untouched."}
          </p>
          <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border">
            <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b bg-muted/40 p-2">
              <div
                role="group"
                aria-label="Comparison view"
                className="flex flex-wrap gap-2"
              >
                {(
                  [
                    ["split", "Side by side"],
                    ["inline", "Inline"],
                    ["config", "Proposed TOML"],
                  ] as const
                ).map(([mode, label]) => (
                  <Button
                    key={mode}
                    size="sm"
                    variant={view === mode ? "outline" : "ghost"}
                    className={
                      view === mode
                        ? "bg-background text-foreground shadow-sm"
                        : ""
                    }
                    aria-pressed={view === mode}
                    onClick={() => setView(mode)}
                  >
                    {label}
                  </Button>
                ))}
              </div>
              <div className="ml-auto flex flex-wrap gap-2">
                <Button
                  size="sm"
                  disabled={isFetching}
                  onClick={() => void copy(proposed ?? "")}
                >
                  <Copy className="mr-2 h-4 w-4" />
                  Copy proposed TOML
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={!diff || isFetching}
                  onClick={() => void copy(diff ?? "")}
                >
                  Copy diff
                </Button>
              </div>
            </div>
            {view === "config" ? (
              <pre
                aria-label="Proposed TOML"
                className="min-h-0 flex-1 overflow-auto p-4 text-xs leading-6"
              >
                {proposed}
              </pre>
            ) : (
              <ConfigDiff lines={lines ?? []} layout={view} />
            )}
          </div>
        </>
      )}
    </section>
  );
}
