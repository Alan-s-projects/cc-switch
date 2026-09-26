import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useManagedAuth } from "@/components/providers/forms/hooks/useManagedAuth";

const apiMocks = vi.hoisted(() => ({
  authGetStatus: vi.fn(),
  authStartLogin: vi.fn(),
  authPollForAccount: vi.fn(),
  authRemoveAccount: vi.fn(),
}));
const toastMocks = vi.hoisted(() => ({ success: vi.fn() }));

vi.mock("@/lib/api", () => ({
  authApi: apiMocks,
  settingsApi: { openExternal: vi.fn().mockResolvedValue(undefined) },
}));
vi.mock("@/lib/clipboard", () => ({
  copyText: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("sonner", () => ({ toast: toastMocks }));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

const code = {
  provider: "github_copilot",
  device_code: "device-1",
  user_code: "ABCD-EFGH",
  verification_uri: "https://github.com/login/device",
  expires_in: 600,
  interval: 5,
};

describe("GitHub Copilot device authentication", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.authGetStatus.mockResolvedValue({
      provider: "github_copilot",
      authenticated: true,
      default_account_id: "acct-1",
      accounts: [
        {
          id: "acct-1",
          provider: "github_copilot",
          login: "user",
          is_default: true,
        },
      ],
    });
    apiMocks.authStartLogin.mockReset().mockResolvedValue(code);
    apiMocks.authPollForAccount.mockReset().mockResolvedValue(null);
    apiMocks.authRemoveAccount.mockResolvedValue(undefined);
  });

  it("starts the GitHub device flow with the selected domain", async () => {
    const { result } = renderHook(
      () => useManagedAuth("github_copilot", "example.ghe.com"),
      {
        wrapper: createWrapper(),
      },
    );
    act(() => result.current.addAccount());
    await waitFor(() => expect(result.current.isPolling).toBe(true));
    expect(apiMocks.authStartLogin).toHaveBeenCalledWith(
      "github_copilot",
      "example.ghe.com",
    );
    await waitFor(() =>
      expect(apiMocks.authPollForAccount).toHaveBeenCalledWith(
        "github_copilot",
        "device-1",
        "example.ghe.com",
      ),
    );
  });

  it("ignores a pending login response after cancellation", async () => {
    let resolveStart!: (value: typeof code) => void;
    apiMocks.authStartLogin.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveStart = resolve;
        }),
    );
    const { result } = renderHook(() => useManagedAuth("github_copilot"), {
      wrapper: createWrapper(),
    });
    act(() => result.current.addAccount());
    await waitFor(() => expect(apiMocks.authStartLogin).toHaveBeenCalled());
    act(() => result.current.cancelAuth());
    await act(async () => resolveStart(code));
    expect(result.current.pollingState).toBe("idle");
    expect(result.current.deviceCode).toBeNull();
    expect(apiMocks.authPollForAccount).not.toHaveBeenCalled();
  });

  it("ignores a late result from a cancelled polling request", async () => {
    let resolvePoll!: (value: object) => void;
    apiMocks.authPollForAccount.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolvePoll = resolve;
        }),
    );
    const { result } = renderHook(() => useManagedAuth("github_copilot"), {
      wrapper: createWrapper(),
    });
    await waitFor(() => expect(result.current.isStatusSuccess).toBe(true));
    act(() => result.current.addAccount());
    await waitFor(() => expect(apiMocks.authPollForAccount).toHaveBeenCalled());
    act(() => result.current.cancelAuth());
    const statusCalls = apiMocks.authGetStatus.mock.calls.length;
    await act(async () => resolvePoll({ id: "late-account" }));
    expect(result.current.pollingState).toBe("idle");
    expect(result.current.deviceCode).toBeNull();
    expect(apiMocks.authGetStatus).toHaveBeenCalledTimes(statusCalls);
  });

  it("ignores a pending login response after the view closes", async () => {
    let resolveStart!: (value: typeof code) => void;
    apiMocks.authStartLogin.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveStart = resolve;
        }),
    );
    const { result, unmount } = renderHook(
      () => useManagedAuth("github_copilot"),
      { wrapper: createWrapper() },
    );
    act(() => result.current.addAccount());
    await waitFor(() => expect(apiMocks.authStartLogin).toHaveBeenCalled());
    unmount();
    await act(async () => resolveStart(code));
    expect(apiMocks.authPollForAccount).not.toHaveBeenCalled();
  });

  it("removes a Copilot account and refreshes status", async () => {
    const { result } = renderHook(() => useManagedAuth("github_copilot"), {
      wrapper: createWrapper(),
    });
    await waitFor(() => expect(result.current.isStatusSuccess).toBe(true));
    const statusCalls = apiMocks.authGetStatus.mock.calls.length;
    act(() => result.current.removeAccount("acct-1"));
    await waitFor(() =>
      expect(apiMocks.authRemoveAccount).toHaveBeenCalledWith(
        "github_copilot",
        "acct-1",
      ),
    );
    await waitFor(() =>
      expect(apiMocks.authGetStatus.mock.calls.length).toBeGreaterThan(
        statusCalls,
      ),
    );
    expect(toastMocks.success).toHaveBeenCalled();
  });
});
