import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  useGlobalProxyConfig,
  useUpdateGlobalProxyConfig,
  useProxyStatusQuery,
} from "@/lib/query/proxy";
import { copyText } from "@/lib/clipboard";

export function ProxyPanel() {
  const { data: config } = useGlobalProxyConfig();
  const { data: status } = useProxyStatusQuery();
  const save = useUpdateGlobalProxyConfig();
  const [address, setAddress] = useState("127.0.0.1");
  const [port, setPort] = useState("15722");
  useEffect(() => {
    if (config) {
      setAddress(config.listenAddress);
      setPort(String(config.listenPort));
    }
  }, [config]);
  const running = status?.running ?? false;
  const endpoint = `http://${address.includes(":") && !address.startsWith("[") ? `[${address}]` : address}:${port}/v1`;
  const saveAddress = async () => {
    const value = Number(port);
    if (
      !address.trim() ||
      !Number.isInteger(value) ||
      value < 1 ||
      value > 65535
    ) {
      toast.error("Enter an address and a port between 1 and 65535.");
      return;
    }
    if (config)
      await save.mutateAsync({
        ...config,
        listenAddress: address.trim(),
        listenPort: value,
      });
  };
  return (
    <section className="space-y-5 rounded-xl border bg-card p-6">
      <div className="flex items-center justify-between">
        <h3 className="font-semibold">Local OpenAI-compatible server</h3>
        <span className="text-sm text-muted-foreground">
          {running ? "Running" : "Stopped"}
        </span>
      </div>
      <div className="grid gap-4 sm:grid-cols-[1fr_8rem_auto]">
        <div className="space-y-2">
          <Label htmlFor="proxy-address">Listen address</Label>
          <Input
            id="proxy-address"
            value={address}
            disabled={running}
            onChange={(e) => setAddress(e.target.value)}
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="proxy-port">Port</Label>
          <Input
            id="proxy-port"
            value={port}
            disabled={running}
            onChange={(e) => setPort(e.target.value)}
          />
        </div>
        <Button
          className="self-end"
          disabled={running || save.isPending || !config}
          onClick={() => void saveAddress()}
        >
          Save
        </Button>
      </div>
      <p className="text-sm text-muted-foreground">
        Stop the proxy before changing its address. Review the Codex connection
        suggestion after any change.
      </p>
      <div className="flex items-center justify-between gap-3 rounded-lg bg-muted p-3">
        <code className="break-all text-sm">{endpoint}</code>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => void copyText(endpoint)}
        >
          Copy
        </Button>
      </div>
      <div className="flex items-center justify-between">
        <Label htmlFor="proxy-log">
          Record requests for the usage dashboard
        </Label>
        <Switch
          id="proxy-log"
          checked={config?.enableLogging ?? true}
          disabled={!config || save.isPending}
          onCheckedChange={(enabled) => {
            if (config) save.mutate({ ...config, enableLogging: enabled });
          }}
        />
      </div>
      <dl className="grid grid-cols-3 gap-3 border-t pt-4 text-sm">
        <div>
          <dt className="text-muted-foreground">Requests</dt>
          <dd>{status?.total_requests ?? 0}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Active connections</dt>
          <dd>{status?.active_connections ?? 0}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Success rate</dt>
          <dd>{(status?.success_rate ?? 0).toFixed(1)}%</dd>
        </div>
      </dl>
    </section>
  );
}
