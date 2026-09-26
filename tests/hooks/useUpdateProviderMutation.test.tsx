import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { expect, it, vi } from "vitest";
import { useUpdateProviderMutation } from "@/lib/query/mutations";
import { createTestQueryClient } from "../utils/testQueryClient";
const update = vi.hoisted(() => vi.fn().mockResolvedValue(true));
vi.mock("@/lib/api", () => ({ providersApi: { update }, settingsApi: {} }));
it("refreshes Copilot, quota and the connection preview after editing", async () => {
  const client = createTestQueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const { result } = renderHook(() => useUpdateProviderMutation(), {
    wrapper,
  });
  const provider = {
    id: "copilot",
    name: "GitHub Copilot",
    settingsConfig: {},
  };
  await act(() => result.current.mutateAsync({ provider }));
  expect(update).toHaveBeenCalledWith(provider);
  for (const queryKey of [
    ["providers", "codex"],
    ["copilot", "quota"],
    ["codex-setup-suggestion"],
  ]) {
    expect(invalidate).toHaveBeenCalledWith({ queryKey });
  }
});
