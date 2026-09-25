import { Loader2, Radio } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import { useProxyStatus } from "@/hooks/useProxyStatus";

export function ProxyToggle() {
  const { isRunning, isLoading, isPending, toggleProxy } = useProxyStatus();
  return (
    <div
      className="flex h-8 items-center gap-2 rounded-lg bg-muted/50 px-2"
      title={isRunning ? "Stop proxy" : "Start proxy"}
    >
      {isLoading || isPending ? (
        <Loader2
          aria-hidden
          className="h-4 w-4 animate-spin text-muted-foreground"
        />
      ) : (
        <Radio
          aria-hidden
          className={`h-4 w-4 ${isRunning ? "text-emerald-500" : "text-muted-foreground"}`}
        />
      )}
      <span className="text-xs font-medium">Proxy</span>
      <Switch
        checked={isRunning}
        disabled={isLoading || isPending}
        onCheckedChange={toggleProxy}
        aria-label="Proxy server"
        aria-busy={isLoading || isPending}
      />
    </div>
  );
}
