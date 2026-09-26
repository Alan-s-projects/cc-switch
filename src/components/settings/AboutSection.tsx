import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { Github, Download, ExternalLink } from "lucide-react";
import { settingsApi } from "@/lib/api";
import { Button } from "@/components/ui/button";

export function AboutSection() {
  const [version, setVersion] = useState("");
  useEffect(() => {
    void getVersion().then(setVersion);
  }, []);
  return (
    <section className="space-y-5 rounded-xl border bg-card p-6">
      <h2 className="text-xl font-semibold">Copilot Bridge Atlas {version}</h2>
      <p className="text-sm text-muted-foreground">
        A local OpenAI-compatible server connecting Codex to GitHub Copilot,
        with usage statistics and read-only configuration suggestions.
      </p>
      <div className="flex flex-wrap gap-3">
        <Button
          variant="outline"
          onClick={() =>
            void settingsApi.openExternal(
              "https://github.com/Alan-s-projects/copilot-bridge-atlas",
            )
          }
        >
          <Github className="mr-2 h-4 w-4" />
          GitHub
        </Button>
        <Button
          variant="outline"
          onClick={() =>
            void settingsApi.openExternal(
              `https://github.com/Alan-s-projects/copilot-bridge-atlas/releases${version ? `/tag/atlas-${version}` : ""}`,
            )
          }
        >
          <ExternalLink className="mr-2 h-4 w-4" />
          Release notes
        </Button>
        <Button onClick={() => void settingsApi.checkUpdates()}>
          <Download className="mr-2 h-4 w-4" />
          Download MSI
        </Button>
      </div>
    </section>
  );
}
