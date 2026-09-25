import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CopilotCard } from "@/components/providers/CopilotCard";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const mocks = vi.hoisted(() => ({
  auth: {
    accounts: [{ id: "account-1", login: "test-user" }],
    defaultAccountId: "account-1",
    isLoadingStatus: false,
  },
  quota: vi.fn(),
  probe: vi.fn(),
}));
vi.mock("@/components/providers/forms/hooks/useCopilotAuth", () => ({
  useCopilotAuth: () => mocks.auth,
}));
vi.mock("@/components/CopilotQuotaFooter", () => ({
  default: ({ meta }: { meta: Provider["meta"] }) => {
    mocks.quota(meta);
    return <span>Copilot quota</span>;
  },
}));
vi.mock("@/lib/api/connectivity-check", () => ({
  streamCheckProvider: mocks.probe,
}));

const provider: Provider = {
  id: "current-copilot",
  name: "GitHub Copilot",
  settingsConfig: { modelCatalog: { models: [{ model: "gpt-6-astra" }] } },
  meta: { providerType: "github_copilot", githubAccountId: "account-1" },
};

function renderCard() {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <CopilotCard provider={provider} />
    </QueryClientProvider>,
  );
}

describe("Copilot account card", () => {
  beforeEach(() => {
    mocks.auth.accounts = [{ id: "account-1", login: "test-user" }];
    mocks.auth.isLoadingStatus = false;
    mocks.quota.mockClear();
    mocks.probe.mockReset().mockResolvedValue({
      status: "operational",
      success: true,
      message: "Reachable",
      responseTimeMs: 125,
      httpStatus: 401,
    });
  });

  it("directs setup to Settings → Copilot and keeps connectivity available without signing in", () => {
    mocks.auth.accounts = [];
    renderCard();
    expect(screen.getByText("Needs setup")).toBeVisible();
    expect(
      screen.getByText(/Open Settings → Copilot to sign in/),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Health check" })).toBeEnabled();
    expect(
      screen.queryByRole("button", { name: "Edit" }),
    ).not.toBeInTheDocument();
    expect(mocks.probe).not.toHaveBeenCalled();
    expect(mocks.quota).not.toHaveBeenCalled();
  });

  it("keeps the account, model count, and quota, and runs health checks from the card", async () => {
    renderCard();
    expect(
      screen.getByText("test-user · 1 models available to Codex"),
    ).toBeVisible();
    expect(screen.queryByText("Needs setup")).not.toBeInTheDocument();
    expect(screen.getByText("Copilot quota")).toBeVisible();
    expect(mocks.quota).toHaveBeenCalledWith(provider.meta);
    const card = screen
      .getByRole("heading", { name: "Provider", level: 2 })
      .closest("section")!;
    expect(
      within(card).getByRole("heading", { name: "GitHub Copilot", level: 3 }),
    ).toBeVisible();
    expect(mocks.probe).not.toHaveBeenCalled();
    fireEvent.click(within(card).getByRole("button", { name: "Health check" }));
    await waitFor(() =>
      expect(mocks.probe).toHaveBeenCalledWith("codex", provider.id),
    );
  });
});
