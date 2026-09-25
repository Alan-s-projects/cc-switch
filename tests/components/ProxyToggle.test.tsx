import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
const state = vi.hoisted(() => ({
  isRunning: false,
  isLoading: true,
  isPending: false,
  toggleProxy: vi.fn(),
}));
vi.mock("@/hooks/useProxyStatus", () => ({ useProxyStatus: () => state }));
describe("single-server control", () => {
  it("waits for status and prevents repeated clicks during a change", () => {
    const { rerender } = render(<ProxyToggle />);
    const control = screen.getByRole("switch", { name: "Proxy server" });
    expect(screen.getByText("Checking")).toBeVisible();
    expect(screen.queryByText("Proxy Stopped")).not.toBeInTheDocument();
    expect(control).not.toBeChecked();
    expect(control).toBeDisabled();
    fireEvent.click(control);
    expect(state.toggleProxy).not.toHaveBeenCalled();
    state.isLoading = false;
    rerender(<ProxyToggle />);
    expect(screen.getByText("Proxy Stopped")).toBeVisible();
    expect(control).toBeEnabled();
    fireEvent.click(control);
    expect(state.toggleProxy).toHaveBeenCalledWith(true);
    state.isRunning = true;
    state.isPending = true;
    rerender(<ProxyToggle />);
    expect(screen.getByText("Proxy Running")).toBeVisible();
    expect(control).toBeChecked();
    expect(control).toBeDisabled();
    fireEvent.click(control);
    expect(state.toggleProxy).toHaveBeenCalledTimes(1);
    state.isPending = false;
    rerender(<ProxyToggle />);
    expect(control).toBeEnabled();
    fireEvent.click(control);
    expect(state.toggleProxy).toHaveBeenLastCalledWith(false);
    state.isRunning = false;
    rerender(<ProxyToggle />);
    expect(screen.getByText("Proxy Stopped")).toBeVisible();
    expect(control).not.toBeChecked();
  });
});
