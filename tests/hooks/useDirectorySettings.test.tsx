import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { useDirectorySettings } from "@/hooks/useDirectorySettings";
const api = vi.hoisted(() => ({
  getAppConfigDirOverride: vi.fn(),
  getConfigDir: vi.fn(),
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
  api.getConfigDir.mockResolvedValue("/remote/codex");
  api.selectConfigDirectory.mockResolvedValue("/picked/codex");
});
function mount() {
  const onUpdateSettings = vi.fn();
  const hook = renderHook(() =>
    useDirectorySettings({
      settings: {
        showInTray: true,
        codexConfigDir: undefined,
        language: "en",
      },
      onUpdateSettings,
    }),
  );
  return { ...hook, onUpdateSettings };
}
it("resolves only Atlas and the Codex read directory", async () => {
  api.getAppConfigDirOverride.mockResolvedValue("  /override/atlas  ");
  const { result } = mount();
  await waitFor(() => expect(result.current.isLoading).toBe(false));
  expect(result.current.resolvedDirs).toEqual({
    appConfig: "/override/atlas",
    codex: "/remote/codex",
  });
  expect(api.getConfigDir).toHaveBeenCalledTimes(1);
  expect(api.getConfigDir).toHaveBeenCalledWith("codex");
});
it("changes the saved read-path preference without writing Codex configuration", async () => {
  const { result, onUpdateSettings } = mount();
  await waitFor(() => expect(result.current.isLoading).toBe(false));
  await act(() => result.current.browseDirectory("codex"));
  expect(api.selectConfigDirectory).toHaveBeenCalledWith("/remote/codex");
  expect(onUpdateSettings).toHaveBeenCalledWith({
    codexConfigDir: "/picked/codex",
  });
  expect(result.current.resolvedDirs.codex).toBe("/picked/codex");
  await act(() => result.current.resetDirectory("codex"));
  expect(onUpdateSettings).toHaveBeenLastCalledWith({
    codexConfigDir: undefined,
  });
  expect(result.current.resolvedDirs.codex).toBe("/home/mock/.codex");
});
it("leaves the preference alone when the picker is cancelled", async () => {
  api.selectConfigDirectory.mockResolvedValue(null);
  const { result, onUpdateSettings } = mount();
  await waitFor(() => expect(result.current.isLoading).toBe(false));
  await act(() => result.current.browseDirectory("codex"));
  expect(onUpdateSettings).not.toHaveBeenCalled();
});
it("resets Atlas data and Codex read directories independently", async () => {
  const { result } = mount();
  await waitFor(() => expect(result.current.isLoading).toBe(false));
  act(() => result.current.updateAppConfigDir(" /new/atlas "));
  expect(result.current.resolvedDirs.appConfig).toBe("/new/atlas");
  await act(() => result.current.resetAppConfigDir());
  expect(result.current.resolvedDirs.appConfig).toBe("/home/mock/.cc-switch");
  act(() => result.current.resetAllDirectories({ codex: "/new/codex" }));
  expect(result.current.resolvedDirs.codex).toBe("/new/codex");
});
