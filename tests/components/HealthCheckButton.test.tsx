import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HealthCheckButton } from "@/components/providers/HealthCheckButton";
import type { StreamCheckResult } from "@/lib/api/connectivity-check";
import { createTestQueryClient } from "../utils/testQueryClient";

const mocks = vi.hoisted(() => ({
  probe: vi.fn(),
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
}));
vi.mock("@/lib/api/connectivity-check", () => ({
  streamCheckProvider: mocks.probe,
}));
vi.mock("sonner", () => ({
  toast: { success: mocks.success, warning: mocks.warning, error: mocks.error },
}));
const providerId = "current-copilot";
const reachable: StreamCheckResult = {
  status: "operational",
  success: true,
  message: "Reachable",
  responseTimeMs: 125,
  httpStatus: 401,
  testedAt: 1,
  retryCount: 0,
};
const connectivityOnly =
  "Connectivity only; sign-in and model requests are not tested.";
const renderButton = (props: { providerId?: string } = { providerId }) =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <HealthCheckButton {...props} />
    </QueryClientProvider>,
  );
const healthCheck = () => screen.getByRole("button", { name: "Health check" });

describe("Copilot connectivity health check", () => {
  beforeEach(() => {
    mocks.probe.mockReset().mockResolvedValue(reachable);
    mocks.success.mockClear();
    mocks.warning.mockClear();
    mocks.error.mockClear();
  });

  it("stays disabled without a provider and sends no probe", () => {
    renderButton({});
    expect(healthCheck()).toBeDisabled();
    fireEvent.click(healthCheck());
    expect(mocks.probe).not.toHaveBeenCalled();
  });

  it("probes only on click, blocks duplicates, and describes HTTP 401 as connectivity", async () => {
    let resolve!: (result: StreamCheckResult) => void;
    mocks.probe.mockReturnValue(
      new Promise<StreamCheckResult>((done) => {
        resolve = done;
      }),
    );
    renderButton();
    expect(healthCheck()).toHaveTextContent("Health check");
    expect(mocks.probe).not.toHaveBeenCalled();
    expect(healthCheck()).toBeEnabled();
    fireEvent.click(healthCheck());
    await waitFor(() => expect(healthCheck()).toBeDisabled());
    expect(healthCheck()).toHaveAttribute("aria-busy", "true");
    expect(mocks.probe).toHaveBeenCalledWith("codex", providerId);
    fireEvent.click(healthCheck());
    expect(mocks.probe).toHaveBeenCalledTimes(1);

    await act(async () => resolve(reachable));
    await waitFor(() =>
      expect(mocks.success).toHaveBeenCalledWith(
        "GitHub Copilot is reachable",
        expect.objectContaining({
          description: expect.stringContaining(connectivityOnly),
        }),
      ),
    );
    const description = mocks.success.mock.calls[0][1].description;
    expect(description).toContain("125 ms");
    expect(description).toContain("HTTP 401");
    expect(mocks.error).not.toHaveBeenCalled();
    expect(mocks.warning).not.toHaveBeenCalled();
    await waitFor(() => expect(healthCheck()).toBeEnabled());
    expect(healthCheck()).toHaveAttribute("aria-busy", "false");
  });

  it("reports slow connectivity as a warning with latency and HTTP status", async () => {
    mocks.probe.mockResolvedValue({
      ...reachable,
      status: "degraded",
      responseTimeMs: 2500,
      httpStatus: 200,
    });
    renderButton();
    fireEvent.click(healthCheck());
    await waitFor(() =>
      expect(mocks.warning).toHaveBeenCalledWith(
        "GitHub Copilot is reachable but slow",
        expect.objectContaining({
          description: expect.stringContaining(connectivityOnly),
        }),
      ),
    );
    const description = mocks.warning.mock.calls[0][1].description;
    expect(description).toContain("2500 ms");
    expect(description).toContain("HTTP 200");
    expect(mocks.success).not.toHaveBeenCalled();
    expect(mocks.error).not.toHaveBeenCalled();
    await waitFor(() => expect(healthCheck()).toBeEnabled());
  });

  it.each([
    {
      scenario: "an unsuccessful result",
      outcome: () =>
        Promise.resolve({
          ...reachable,
          status: "failed",
          success: false,
          message: "Connection timed out",
          httpStatus: undefined,
        }),
      title: "GitHub Copilot is unreachable",
      detail: "Connection timed out",
    },
    {
      scenario: "a rejected probe",
      outcome: () => Promise.reject(new Error("Network offline")),
      title: "Health check failed",
      detail: "Network offline",
    },
  ])(
    "reports $scenario and allows retry",
    async ({ outcome, title, detail }) => {
      mocks.probe
        .mockImplementationOnce(outcome)
        .mockResolvedValueOnce(reachable);
      renderButton();
      fireEvent.click(healthCheck());
      await waitFor(() =>
        expect(mocks.error).toHaveBeenCalledWith(
          title,
          expect.objectContaining({
            description: expect.stringContaining(detail),
          }),
        ),
      );
      expect(mocks.success).not.toHaveBeenCalled();
      await waitFor(() => expect(healthCheck()).toBeEnabled());
      fireEvent.click(healthCheck());
      await waitFor(() => expect(mocks.success).toHaveBeenCalledTimes(1));
      expect(mocks.probe).toHaveBeenCalledTimes(2);
      expect(mocks.probe).toHaveBeenLastCalledWith("codex", providerId);
      await waitFor(() => expect(healthCheck()).toBeEnabled());
    },
  );
});
