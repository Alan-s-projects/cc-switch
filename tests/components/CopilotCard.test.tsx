import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CopilotCard } from "@/components/providers/CopilotCard";
import type { Provider } from "@/types";

const mocks = vi.hoisted(() => ({
  auth: {
    accounts: [{ id: "account-1", login: "test-user" }],
    defaultAccountId: "account-1",
    isLoadingStatus: false,
  },
  quota: vi.fn(),
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

const provider: Provider = {
  id: "current-copilot",
  name: "GitHub Copilot",
  settingsConfig: { modelCatalog: { models: [{ model: "gpt-6-astra" }] } },
  meta: { providerType: "github_copilot", githubAccountId: "account-1" },
};

describe("Copilot account card", () => {
  beforeEach(() => {
    mocks.auth.accounts = [{ id: "account-1", login: "test-user" }];
    mocks.auth.isLoadingStatus = false;
    mocks.quota.mockClear();
  });

  it("directs setup to Settings → Copilot without card actions", () => {
    mocks.auth.accounts = [];
    render(<CopilotCard provider={provider} />);
    expect(screen.getByText("Needs setup")).toBeVisible();
    expect(
      screen.getByText(/Open Settings → Copilot to sign in/),
    ).toBeVisible();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(mocks.quota).not.toHaveBeenCalled();
  });

  it("keeps the connected account, model count, and quota", () => {
    render(<CopilotCard provider={provider} />);
    expect(
      screen.getByText("test-user · 1 models available to Codex"),
    ).toBeVisible();
    expect(screen.queryByText("Needs setup")).not.toBeInTheDocument();
    expect(screen.getByText("Copilot quota")).toBeVisible();
    expect(mocks.quota).toHaveBeenCalledWith(provider.meta);
  });
});
