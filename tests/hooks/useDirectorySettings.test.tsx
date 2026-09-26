import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { useDirectorySettings } from "@/hooks/useDirectorySettings";
const api = vi.hoisted(() => ({
  getAppConfigDirOverride: vi.fn(),
  selectConfigDirectory: vi.fn(),
}));
vi.mock("@/lib/api", () => ({ settingsApi: api }));
vi.mock("@tauri-apps/api/path", () => ({
  homeDir: async () => "/home/mock",
  join: async (...segments: string[]) => segments.join("/"),
}));
beforeEach(() => {
  vi.clearAllMocks();
  api.getAppConfigDirOverride.mockResolvedValue(null);
  api.selectConfigDirectory.mockResolvedValue("/picked/atlas");
});
function mount() {
  return renderHook(() => useDirectorySettings());
}
it("resolves the Atlas data directory", async () => {
  api.getAppConfigDirOverride.mockResolvedValue("  /override/atlas  ");
  const { result } = mount();
  await waitFor(() => expect(result.current.isLoading).toBe(false));
  expect(result.current.resolvedDirs).toEqual({
    appConfig: "/override/atlas",
  });
  expect(api.getAppConfigDirOverride).toHaveBeenCalledTimes(1);
});
it("browses and resets the Atlas data directory", async () => {
  const { result } = mount();
  await waitFor(() => expect(result.current.isLoading).toBe(false));
  await act(() => result.current.browseAppConfigDir());
  expect(api.selectConfigDirectory).toHaveBeenCalledWith(
    "/home/mock/.copilot-bridge-atlas",
  );
  expect(result.current.appConfigDir).toBe("/picked/atlas");
  expect(result.current.resolvedDirs.appConfig).toBe("/picked/atlas");
  await act(() => result.current.resetAppConfigDir());
  expect(result.current.appConfigDir).toBeUndefined();
  expect(result.current.resolvedDirs.appConfig).toBe(
    "/home/mock/.copilot-bridge-atlas",
  );
});
it("leaves the preference alone when the picker is cancelled", async () => {
  api.getAppConfigDirOverride.mockResolvedValue("/existing/atlas");
  api.selectConfigDirectory.mockResolvedValue(null);
  const { result } = mount();
  await waitFor(() => expect(result.current.isLoading).toBe(false));
  await act(() => result.current.browseAppConfigDir());
  expect(result.current.appConfigDir).toBe("/existing/atlas");
  expect(result.current.resolvedDirs.appConfig).toBe("/existing/atlas");
});
