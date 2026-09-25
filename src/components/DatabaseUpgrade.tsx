import { invoke } from "@tauri-apps/api/core";
import { exit } from "@tauri-apps/plugin-process";
import { AlertTriangle, ExternalLink, FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";

interface DatabaseUpgradeProps {
  payload: {
    path?: string;
    error?: string;
    kind?: string;
    db_version?: number;
    supported_version?: number;
  };
}

export function DatabaseUpgrade({ payload }: DatabaseUpgradeProps) {
  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-6 text-foreground">
      <div className="w-full max-w-lg space-y-5 rounded-2xl border bg-card p-7 shadow-xl">
        <AlertTriangle className="h-8 w-8 text-amber-500" />
        <h1 className="text-xl font-semibold">
          A compatible app version is required
        </h1>
        <p className="text-sm text-muted-foreground">
          This database uses version {payload.db_version}; this app supports
          version {payload.supported_version}. Your database has not been
          changed. Check the Atlas releases for a compatible MSI.
        </p>
        {payload.path && (
          <pre className="overflow-x-auto text-xs">{payload.path}</pre>
        )}
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => void invoke("check_for_updates")}>
            <ExternalLink className="mr-2 h-4 w-4" />
            Atlas releases
          </Button>
          <Button
            variant="outline"
            onClick={() => void invoke("open_app_config_folder")}
          >
            <FolderOpen className="mr-2 h-4 w-4" />
            Open data folder
          </Button>
          <Button variant="ghost" onClick={() => void exit(0)}>
            Quit
          </Button>
        </div>
      </div>
    </div>
  );
}

export default DatabaseUpgrade;
