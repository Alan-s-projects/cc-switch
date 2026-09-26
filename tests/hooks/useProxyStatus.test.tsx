import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { expect, it, vi } from "vitest";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { createTestQueryClient } from "../utils/testQueryClient";
const invoke = vi.hoisted(() =>
  vi
    .fn()
    .mockImplementation((command: string) =>
      Promise.resolve(
        command === "get_proxy_status" ? { running: false } : null,
      ),
    ),
);
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
it("starts and stops only the proxy without client takeover or restoration", async () => {
  const client = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const { result } = renderHook(() => useProxyStatus(), { wrapper });
  await waitFor(() => expect(result.current.isLoading).toBe(false));
  act(() => result.current.toggleProxy(true));
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("start_proxy_server"),
  );
  await waitFor(() => expect(result.current.isPending).toBe(false));
  act(() => result.current.toggleProxy(false));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("stop_proxy_server"));
  await waitFor(() => expect(result.current.isPending).toBe(false));
  expect(
    invoke.mock.calls.every(([command]) =>
      ["get_proxy_status", "start_proxy_server", "stop_proxy_server"].includes(
        command,
      ),
    ),
  ).toBe(true);
});
