import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProxyPanel } from "@/components/proxy/ProxyPanel";

const mocks = vi.hoisted(() => ({
  save: vi.fn(),
  copy: vi.fn(),
  config: {
    proxyEnabled: true,
    listenAddress: "127.0.0.1",
    listenPort: 15722,
    enableLogging: true,
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/lib/query/proxy", () => ({
  useGlobalProxyConfig: () => ({ data: mocks.config }),
  useProxyStatusQuery: () => ({
    data: {
      running: false,
      total_requests: 0,
      active_connections: 0,
      success_rate: 100,
    },
  }),
  useUpdateGlobalProxyConfig: () => ({
    mutateAsync: mocks.save,
    mutate: mocks.save,
    isPending: false,
  }),
}));
vi.mock("@/lib/clipboard", () => ({ copyText: mocks.copy }));

describe("ProxyPanel auto-save", () => {
  beforeEach(() => {
    mocks.save.mockReset().mockResolvedValue(undefined);
  });

  it("auto-saves listener edits and removes the Save button", async () => {
    render(<ProxyPanel />);
    const address = screen.getByRole("textbox", { name: "Listen address" });
    await waitFor(() => expect(address).toHaveValue("127.0.0.1"));

    fireEvent.change(address, { target: { value: "0.0.0.0" } });

    expect(
      screen.queryByRole("button", { name: "Save" }),
    ).not.toBeInTheDocument();
    await waitFor(
      () =>
        expect(mocks.save).toHaveBeenCalledWith({
          ...mocks.config,
          listenAddress: "0.0.0.0",
          listenPort: 15722,
        }),
      { timeout: 1500 },
    );
  });

  it("auto-saves the request logging switch", async () => {
    render(<ProxyPanel />);
    fireEvent.click(
      screen.getByRole("switch", {
        name: "Record requests for the usage dashboard",
      }),
    );

    await waitFor(() =>
      expect(mocks.save).toHaveBeenCalledWith({
        ...mocks.config,
        enableLogging: false,
      }),
    );
    expect(await screen.findByText("settings.saved")).toBeVisible();
  });

  it("flushes a pending listener edit when the settings panel closes", async () => {
    const { unmount } = render(<ProxyPanel />);
    const address = screen.getByRole("textbox", { name: "Listen address" });
    fireEvent.change(address, { target: { value: "0.0.0.0" } });

    unmount();

    await waitFor(() =>
      expect(mocks.save).toHaveBeenCalledWith({
        ...mocks.config,
        listenAddress: "0.0.0.0",
        listenPort: 15722,
      }),
    );
  });
});
