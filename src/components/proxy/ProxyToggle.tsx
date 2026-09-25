import { Power } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useProxyStatus } from "@/hooks/useProxyStatus";

export function ProxyToggle() {
  const { isRunning, isLoading, isPending, toggleProxy } = useProxyStatus();
  return (
    <Button
      variant={isRunning ? "secondary" : "default"}
      disabled={isLoading || isPending}
      onClick={() => toggleProxy(!isRunning)}
    >
      <Power className="mr-2 h-4 w-4" />
      {isRunning ? "Stop proxy" : "Start proxy"}
    </Button>
  );
}
