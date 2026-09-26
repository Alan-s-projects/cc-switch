import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Copy, Eye, FolderSearch, RefreshCw } from "lucide-react";
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

const PREVIEW_PATH_KEY = "copilot-bridge-atlas-codex-preview-path";
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
    contextWindow: number;
    copilotContextWindow: number;
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
    <section className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden px-6 pb-4 pt-4">
      <div className="flex shrink-0 items-start justify-between gap-6">
        <div className="min-w-0 space-y-1">
          <h1 className="text-2xl font-bold tracking-tight">Connect</h1>
          <p className="text-sm text-muted-foreground">
            Preview Codex configuration for Copilot or OpenAI sign-in
          </p>
        </div>
        <div className="flex max-w-sm flex-1 items-start gap-2 rounded-lg bg-muted/40 px-3 py-2 text-xs leading-4 text-muted-foreground">
          <Eye aria-hidden className="mt-0.5 h-4 w-4 shrink-0" />
          <p>
            Preview only. Atlas reads your configuration and never writes it.
            Apply changes yourself after reviewing them.
          </p>
        </div>
      </div>
      <div className="shrink-0 rounded-xl border border-border bg-card p-3 shadow-sm">
        <div className="flex items-center gap-4">
          <div
            role="group"
            aria-label="Connection destination"
            className="flex shrink-0 gap-2"
          >
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
          {data && !error && !isPathEdited && (
            <div className="min-w-0 flex-1 border-l border-border pl-4 text-xs leading-4 text-muted-foreground">
              <p
                className="truncate"
                title={`Current provider: ${data.currentProvider}`}
              >
                Current provider:{" "}
                <span className="font-medium text-foreground">
                  {data.currentProvider}
                </span>
              </p>
              {target === "copilot" ? (
                <p className="truncate" title={data.endpoint}>
                  Proposed endpoint:{" "}
                  <span className="font-mono text-foreground">
                    {data.endpoint}
                  </span>
                </p>
              ) : (
                <p>
                  Use Codex&apos;s built-in OpenAI provider and catalog. After
                  applying, sign in and choose a model in Codex. Authentication
                  files stay untouched.
                </p>
              )}
            </div>
          )}
        </div>
        <form
          className="mt-3 flex items-center gap-3 border-t border-border pt-3"
          onSubmit={(event) => {
            event.preventDefault();
            refresh();
          }}
        >
          <Label htmlFor="codex-toml-path" className="shrink-0 text-xs">
            Codex TOML file
          </Label>
          <div className="flex min-w-0 flex-1 items-center gap-2">
            <Input
              id="codex-toml-path"
              className="min-w-0 flex-1 font-mono text-sm"
              value={pathText}
              placeholder="Automatically detect config.toml"
              onChange={(event) => setDraftPath(event.target.value)}
            />
            <Button
              type="button"
              variant="outline"
              size="icon"
              className="shrink-0"
              title="Browse for TOML file"
              aria-label="Browse for TOML file"
              onClick={() => void browse()}
            >
              <FolderSearch aria-hidden className="h-4 w-4" />
            </Button>
            <Button
              type="button"
              variant="outline"
              className="shrink-0"
              onClick={() => choosePath(null)}
            >
              Auto-detect
            </Button>
            <Button
              type="submit"
              variant="outline"
              className="shrink-0"
              disabled={
                (isFetching && !isPathEdited) ||
                (isPathEdited && !normalizePath(pathText))
              }
            >
              <RefreshCw
                aria-hidden
                className={`h-4 w-4 ${isFetching ? "animate-spin" : ""}`}
              />
              Refresh
            </Button>
          </div>
        </form>
        {isPathEdited && (
          <p role="status" className="mt-2 text-xs text-muted-foreground">
            Refresh to preview the file at this location.
          </p>
        )}
        {error && (
          <p role="alert" className="mt-2 break-words text-sm text-destructive">
            {String(error)}
          </p>
        )}
        {data && !error && !isPathEdited && !data.configExists && (
          <p role="status" className="mt-2 text-xs text-muted-foreground">
            No config.toml was found at this location. Browse for your file or
            review the proposed new file below. Atlas will not create it.
          </p>
        )}
      </div>
      <fieldset className="shrink-0 rounded-xl border border-border bg-muted/20 px-3 pb-3">
        <legend className="px-1 text-xs font-medium">
          Optional TOML changes
        </legend>
        <div className="flex items-start gap-4 pt-1">
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
            className="min-w-0 flex-1 text-[11px] leading-4 text-muted-foreground"
          >
            {contextWindow != null
              ? `Context window ${formatTokens(contextWindow)} tokens.`
              : "Context window up to 1,000,000 tokens."}
            {" Auto-compaction is unchanged."}
            {target === "copilot" &&
              preset?.copilotModelLimit != null &&
              preset.copilotModelLimit < 1_000_000 &&
              ` Capped to the ${formatTokens(preset.copilotModelLimit)}-token saved Copilot catalog limit${preset.model ? ` for ${preset.model}` : ""}.`}
            {(target === "openai" || preset?.copilotModelLimit == null) &&
              " Actual model and provider limits still apply."}
            {preset && (
              <span className="ml-3">
                Current file: context{" "}
                {preset.currentContextWindow ?? "model default"}.
              </span>
            )}
          </p>
        </div>
        <div className="mt-2 flex items-center gap-4">
          {!!source?.settingDefaults.length && (
            <Popover>
              <PopoverTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-6 shrink-0 px-0 text-xs"
                  disabled={isPathEdited || Boolean(error)}
                >
                  Compare with Codex defaults
                </Button>
              </PopoverTrigger>
              <PopoverContent
                align="start"
                className="max-h-64 w-[min(52rem,calc(100vw-3rem))] overflow-auto p-4 text-xs"
              >
                <table className="w-full text-left">
                  <thead className="text-muted-foreground">
                    <tr>
                      <th scope="col" className="pb-2 font-normal">
                        Setting
                      </th>
                      <th scope="col" className="pb-2 font-normal">
                        Current value
                      </th>
                      <th scope="col" className="pb-2 font-normal">
                        Codex default
                      </th>
                      <th scope="col" className="pb-2 font-normal">
                        Use default
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {source.settingDefaults.map((setting) => (
                      <tr key={setting.option}>
                        <th
                          scope="row"
                          className="border-t border-border py-2 pr-4 font-mono font-normal"
                        >
                          {setting.key}
                        </th>
                        <td className="border-t border-border py-2 pr-4 font-mono">
                          {setting.currentValue}
                        </td>
                        <td className="border-t border-border py-2 pr-4 font-mono">
                          {setting.defaultValue}
                        </td>
                        <td className="border-t border-border py-2">
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
          <p className="min-w-0 flex-1 text-[11px] leading-4 text-muted-foreground">
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
