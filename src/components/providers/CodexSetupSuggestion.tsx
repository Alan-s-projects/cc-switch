import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { Copy, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { copyText } from "@/lib/clipboard";

interface SetupSuggestion {
  configPath: string;
  currentProvider: string;
  endpoint: string;
  copilotConfig: string;
  copilotDiff: string;
  openaiConfig: string;
  openaiDiff: string;
}

export function CodexSetupSuggestion() {
  const [target, setTarget] = useState<"copilot" | "openai">("copilot");
  const [view, setView] = useState<"diff" | "config">("diff");
  const { data, error, isFetching, refetch } = useQuery({
    queryKey: ["codex-setup-suggestion"],
    queryFn: () => invoke<SetupSuggestion>("get_codex_setup_suggestion"),
    staleTime: 0,
  });
  const diff = target === "copilot" ? data?.copilotDiff : data?.openaiDiff;
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
      {error && (
        <p role="alert" className="text-destructive">
          {String(error)}
        </p>
      )}
      {data && (
        <>
          <div className="space-y-1">
            <p className="break-all font-mono text-sm">{data.configPath}</p>
            <p className="text-sm text-muted-foreground">
              Current provider: {data.currentProvider}
            </p>
          </div>
          <p className="text-sm text-muted-foreground">
            {target === "copilot"
              ? `Update the active proxy connection to ${data.endpoint}. Conflicting proxy authentication and catalog settings are replaced; other preferences are preserved.`
              : "Use Codex's built-in OpenAI provider and model catalog. After applying, sign in and choose a model in Codex if needed. Your authentication files remain untouched."}
          </p>
          <div className="overflow-hidden rounded-xl border">
            <div className="flex gap-2 border-b bg-muted/40 p-2">
              <Button
                size="sm"
                variant={view === "diff" ? "secondary" : "ghost"}
                aria-pressed={view === "diff"}
                onClick={() => setView("diff")}
              >
                Git-style diff
              </Button>
              <Button
                size="sm"
                variant={view === "config" ? "secondary" : "ghost"}
                aria-pressed={view === "config"}
                onClick={() => setView("config")}
              >
                Proposed TOML
              </Button>
            </div>
            {view === "diff" ? (
              diff ? (
                <pre
                  aria-label="Configuration diff"
                  className="overflow-auto py-3 text-xs leading-6"
                >
                  {diff.split("\n").map((line, index) => (
                    <div
                      key={index}
                      className={`min-w-max px-4 ${line.startsWith("+++") || line.startsWith("---") ? "text-muted-foreground" : line.startsWith("+") ? "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300" : line.startsWith("-") ? "bg-red-500/10 text-red-700 dark:text-red-300" : line.startsWith("@@") ? "bg-blue-500/10 text-blue-600 dark:text-blue-300" : ""}`}
                    >
                      {line || " "}
                    </div>
                  ))}
                </pre>
              ) : (
                <p className="p-4 text-sm text-muted-foreground">
                  No changes needed in this file.
                </p>
              )
            ) : (
              <pre
                aria-label="Proposed TOML"
                className="overflow-auto p-4 text-xs leading-6"
              >
                {proposed}
              </pre>
            )}
          </div>
          <div className="flex flex-wrap gap-2">
            <Button onClick={() => void copy(proposed ?? "")}>
              <Copy className="mr-2 h-4 w-4" />
              Copy proposed TOML
            </Button>
            <Button
              variant="outline"
              disabled={!diff}
              onClick={() => void copy(diff ?? "")}
            >
              Copy diff
            </Button>
          </div>
        </>
      )}
      <Button
        variant="outline"
        disabled={isFetching}
        onClick={() => void refetch()}
      >
        <RefreshCw
          className={`mr-2 h-4 w-4 ${isFetching ? "animate-spin" : ""}`}
        />
        Refresh
      </Button>
    </section>
  );
}
