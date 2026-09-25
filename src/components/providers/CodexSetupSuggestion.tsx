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
import { copyText } from "@/lib/clipboard";
import { ConfigDiff, type ConfigDiffLine } from "./ConfigDiff";

const PREVIEW_PATH_KEY = "cc-switch-codex-preview-path";
const normalizePath = (path: string) => path.trim().replace(/^"(.*)"$/, "$1");
const RECOMMENDATIONS = [
  {
    key: "modelContext",
    label: "Use model context defaults",
    setting: "model_context_window",
  },
  {
    key: "autoCompaction",
    label: "Use automatic compaction defaults",
    setting: "model_auto_compact_token_limit",
  },
  {
    key: "reasoning",
    label: "Use Codex default reasoning",
    setting: "model_reasoning_effort",
  },
] as const;

function readSavedPath(): string | null {
  try {
    return (
      normalizePath(window.localStorage.getItem(PREVIEW_PATH_KEY) ?? "") || null
    );
  } catch {
    return null;
  }
}

interface SetupSuggestion {
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
  const [recommendations, setRecommendations] = useState({
    modelContext: false,
    autoCompaction: false,
    reasoning: false,
  });
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
  const loadedPath = data?.configPath ?? selectedPath ?? "";
  const pathText = draftPath ?? loadedPath;
  const isPathEdited = normalizePath(pathText) !== normalizePath(loadedPath);

  useEffect(() => {
    if (!data || error || isFetching || !selectedPath || !data.configExists)
      return;
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
  const copy = async (text: string) => {
    try {
      await copyText(text);
      toast.success("Copied");
    } catch (error) {
      toast.error(String(error));
    }
  };
  return (
    <section className="space-y-5 overflow-y-auto px-6 pb-6">
      <p className="text-sm text-muted-foreground">
        Preview only. Atlas reads your configuration and never writes it. Apply
        changes yourself after reviewing them.
      </p>
      <div className="flex flex-wrap gap-2">
        <Button
          variant={target === "copilot" ? "default" : "outline"}
          aria-pressed={target === "copilot"}
          onClick={() => setTarget("copilot")}
        >
          Connect through Copilot
        </Button>
        <Button
          variant={target === "openai" ? "default" : "outline"}
          aria-pressed={target === "openai"}
          onClick={() => setTarget("openai")}
        >
          Return to OpenAI sign-in
        </Button>
      </div>
      <form
        className="space-y-2"
        onSubmit={(event) => {
          event.preventDefault();
          refresh();
        }}
      >
        <Label htmlFor="codex-toml-path">Codex TOML file</Label>
        <div className="flex flex-wrap items-center gap-2">
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
        <p role="status" className="text-sm text-muted-foreground">
          Refresh to preview the file at this location.
        </p>
      )}
      {error && (
        <p role="alert" className="text-destructive">
          {String(error)}
        </p>
      )}
      <fieldset className="space-y-3 rounded-xl border border-border p-4">
        <legend className="px-1 text-sm font-medium">
          Recommended settings
        </legend>
        <div className="flex flex-wrap gap-x-6 gap-y-3">
          {RECOMMENDATIONS.map(({ key, label, setting }) => (
            <div key={key} className="flex items-center gap-2">
              <Checkbox
                id={`recommend-${key}`}
                checked={recommendations[key]}
                onCheckedChange={(checked) =>
                  setRecommendations((previous) => ({
                    ...previous,
                    [key]: checked,
                  }))
                }
                aria-describedby="recommendations-hint"
                title={`Remove ${setting} from the proposed TOML`}
              />
              <Label
                htmlFor={`recommend-${key}`}
                title={`Remove ${setting} from the proposed TOML`}
              >
                {label}
              </Label>
            </div>
          ))}
        </div>
        <p id="recommendations-hint" className="text-xs text-muted-foreground">
          Optional. Checked choices remove the matching global overrides from
          the proposal so Codex can use its defaults.
        </p>
      </fieldset>
      {data && !error && !isPathEdited && (
        <>
          {!data.configExists && (
            <p role="status" className="text-sm text-muted-foreground">
              No config.toml was found at this location. Browse for your file or
              review the proposed new file below. Atlas will not create it.
            </p>
          )}
          <div className="space-y-1">
            <p className="text-sm text-muted-foreground">
              Current provider: {data.currentProvider}
            </p>
          </div>
          <p className="text-sm text-muted-foreground">
            {target === "copilot"
              ? `Update the active proxy connection to ${data.endpoint}. Review the connection changes and selected recommendations below.`
              : "Use Codex's built-in OpenAI provider and model catalog. After applying, sign in and choose a model in Codex if needed. Your authentication files remain untouched."}
          </p>
          <div className="overflow-hidden rounded-xl border">
            <div
              role="group"
              aria-label="Comparison view"
              className="flex flex-wrap gap-2 border-b bg-muted/40 p-2"
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
            {view === "config" ? (
              <pre
                aria-label="Proposed TOML"
                className="overflow-auto p-4 text-xs leading-6"
              >
                {proposed}
              </pre>
            ) : (
              <ConfigDiff lines={lines ?? []} layout={view} />
            )}
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              disabled={isFetching}
              onClick={() => void copy(proposed ?? "")}
            >
              <Copy className="mr-2 h-4 w-4" />
              Copy proposed TOML
            </Button>
            <Button
              variant="outline"
              disabled={!diff || isFetching}
              onClick={() => void copy(diff ?? "")}
            >
              Copy diff
            </Button>
          </div>
        </>
      )}
    </section>
  );
}
