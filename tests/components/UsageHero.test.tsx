import { render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageHero } from "@/components/usage/UsageHero";
import type { ReactNode } from "react";

const summaryMock = vi.hoisted(() => vi.fn());
vi.mock("@/lib/query/usage", () => ({
  useUsageSummary: summaryMock,
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string) => fallback ?? key,
  }),
}));
vi.mock("framer-motion", () => ({
  motion: {
    div: ({
      children,
      className,
    }: {
      children: ReactNode;
      className?: string;
    }) => <div className={className}>{children}</div>,
  },
}));

beforeEach(() => {
  summaryMock.mockReset().mockReturnValue({
    isLoading: false,
    data: {
      totalInputTokens: 200,
      totalOutputTokens: 50,
      totalCacheReadTokens: 800,
      realTotalTokens: 1050,
      cacheHitRate: 0.8,
      totalCost: "1.25",
      totalRequests: 12,
      successRate: 91.7,
      avgLatencyMs: 1250,
    },
  });
});

describe("Usage summary", () => {
  it("shows a compact unified summary with every metric and no client branding", () => {
    render(
      <UsageHero
        range={{ preset: "today" }}
        model="gpt-6-astra"
        refreshIntervalMs={0}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "Usage summary" }),
    ).toBeVisible();
    expect(screen.queryByText("Codex")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("img", { name: "Codex" }),
    ).not.toBeInTheDocument();
    expect(screen.getByTitle((1050).toLocaleString())).toBeVisible();
    expect(screen.getByText("Tokens Processed")).toBeVisible();
    expect(screen.getByText("Requests")).toBeVisible();
    expect(screen.getByText("$1")).toBeVisible();
    expect(screen.getByText("80.0%")).toBeVisible();
    expect(screen.getByText("Success Rate")).toBeVisible();
    expect(screen.getByText("91.7%")).toBeVisible();
    expect(screen.getByText("Average Latency")).toBeVisible();
    expect(screen.getByText("1.25s")).toBeVisible();
    expect(screen.queryByText("Creation")).not.toBeInTheDocument();
    expect(screen.getByText("Hit")).toBeVisible();
    expect(screen.getByText("800")).toBeVisible();
    const summary = screen.getByRole("region", { name: "Usage summary" });
    expect(screen.getAllByRole("region")).toHaveLength(1);
    expect(
      within(summary)
        .getAllByRole("term")
        .map((term) => term.textContent),
    ).toEqual([
      "Total Cost",
      "Tokens Processed",
      "Requests",
      "Fresh Input",
      "Output",
      "Hit",
      "Cache Hit Rate",
      "Average Latency",
      "Success Rate",
    ]);
    const tokenDetails = within(summary).getByRole("group", {
      name: "Token details",
    });
    const requestDetails = within(summary).getByRole("group", {
      name: "Request details",
    });
    expect(tokenDetails.querySelector("dl")).toHaveClass(
      "grid-cols-2",
      "md:grid-cols-4",
    );
    expect(requestDetails.querySelector("dl")).toHaveClass("grid-cols-2");
    expect(within(tokenDetails).getByText("80.0%")).toHaveClass(
      "text-emerald-700",
    );
    expect(within(requestDetails).getByText("91.7%")).toHaveClass(
      "text-emerald-700",
    );
    expect(summaryMock).toHaveBeenLastCalledWith(
      { preset: "today" },
      { appType: "codex", providerName: undefined, model: "gpt-6-astra" },
      { refetchInterval: false },
    );
  });

  it("keeps a compact loading state until the summary is available", () => {
    summaryMock.mockReturnValue({ isLoading: true, data: undefined });
    render(<UsageHero range={{ preset: "today" }} refreshIntervalMs={5000} />);

    expect(
      screen.queryByRole("heading", { name: "Usage summary" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Loading usage" })).toHaveClass(
      "min-h-24",
    );
    expect(summaryMock).toHaveBeenLastCalledWith(
      { preset: "today" },
      { appType: "codex", providerName: undefined, model: undefined },
      { refetchInterval: 5000 },
    );
  });

  it("does not invent success or latency when there are no requests", () => {
    summaryMock.mockReturnValue({
      isLoading: false,
      data: { totalRequests: 0, successRate: 0, avgLatencyMs: 0 },
    });
    render(<UsageHero range={{ preset: "today" }} refreshIntervalMs={0} />);
    expect(
      screen.getByText("Success Rate").nextElementSibling,
    ).toHaveTextContent("--");
    expect(
      screen.getByText("Average Latency").nextElementSibling,
    ).toHaveTextContent("--");
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  });

  it.each([
    ["514.5064", "$515"],
    ["514.49", "$514"],
    ["0.125", "$0"],
    ["invalid", "--"],
  ])("displays %s as %s without modifying the stored cost", (raw, display) => {
    const summary = { totalRequests: 0, totalCost: raw };
    summaryMock.mockReturnValue({ isLoading: false, data: summary });
    render(<UsageHero range={{ preset: "today" }} refreshIntervalMs={0} />);
    const usageSummary = screen.getByRole("region", { name: "Usage summary" });
    const displayedCost = within(usageSummary).getAllByRole("definition")[0];
    expect(displayedCost).toHaveTextContent(display);
    expect(displayedCost).toBeVisible();
    expect(summary.totalCost).toBe(raw);
    expect(within(usageSummary).queryByText("USD")).not.toBeInTheDocument();
    expect(usageSummary.firstElementChild).toHaveClass("p-0");
    expect(usageSummary.querySelector("dl")).toHaveClass(
      "grid-cols-[minmax(0,0.9fr)_minmax(0,1.2fr)_minmax(0,0.9fr)]",
    );
  });
});
