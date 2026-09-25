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
    expect(screen.getByRole("button", { name: "Start proxy" })).toBeDisabled();
    state.isLoading = false;
    rerender(<ProxyToggle />);
    fireEvent.click(screen.getByRole("button", { name: "Start proxy" }));
    expect(state.toggleProxy).toHaveBeenCalledWith(true);
    state.isRunning = true;
    state.isPending = true;
    rerender(<ProxyToggle />);
    expect(screen.getByRole("button", { name: "Stop proxy" })).toBeDisabled();
    state.isPending = false;
    rerender(<ProxyToggle />);
    fireEvent.click(screen.getByRole("button", { name: "Stop proxy" }));
    expect(state.toggleProxy).toHaveBeenLastCalledWith(false);
  });
});
