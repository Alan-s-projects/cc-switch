import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useSettings } from "@/hooks/useSettings";

const mutateAsync = vi.fn();
const setAppConfigDirOverride = vi.fn();
const setAutoLaunch = vi.fn();
const updateTrayMenu = vi.fn();
let form: any;
let directories: any;
let saved: any;

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
vi.mock("@/hooks/useSettingsForm", () => ({ useSettingsForm: () => form }));
vi.mock("@/hooks/useDirectorySettings", () => ({
  useDirectorySettings: () => directories,
}));
vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: saved, isLoading: false }),
  useSaveSettingsMutation: () => ({ mutateAsync, isPending: false }),
}));
vi.mock("@/lib/api", () => ({
  settingsApi: {
    setAppConfigDirOverride: (...args: unknown[]) =>
      setAppConfigDirOverride(...args),
    setAutoLaunch: (...args: unknown[]) => setAutoLaunch(...args),
  },
  providersApi: {
    updateTrayMenu: (...args: unknown[]) => updateTrayMenu(...args),
  },
}));

describe("useSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    saved = {
      showInTray: true,
      launchOnStartup: false,
      language: "en",
      backupRetainCount: 10,
    };
    form = {
      settings: saved,
      isLoading: false,
      updateSettings: vi.fn(),
    };
    directories = {
      appConfigDir: undefined,
      resolvedDirs: { appConfig: "/home/mock/.copilot-bridge-atlas" },
      isLoading: false,
      initialAppConfigDir: undefined,
      updateAppConfigDir: vi.fn(),
      browseAppConfigDir: vi.fn(),
      resetAppConfigDir: vi.fn(),
    };
    mutateAsync.mockResolvedValue(true);
    setAppConfigDirOverride.mockResolvedValue(true);
    setAutoLaunch.mockResolvedValue(true);
    updateTrayMenu.mockResolvedValue(true);
  });

  it("saves app preferences and flags a changed data directory for restart", async () => {
    directories.appConfigDir = "  /custom/atlas  ";
    const { result } = renderHook(() => useSettings());
    await act(async () => {
      await result.current.saveSettings();
    });
    expect(mutateAsync).toHaveBeenCalledWith(saved);
    expect(setAppConfigDirOverride).toHaveBeenCalledWith("/custom/atlas");
    expect(result.current.requiresRestart).toBe(true);
  });

  it.each([
    { description: "the saved override", override: "/existing/atlas" },
    { description: "an unavailable override", override: undefined },
  ])(
    "waits for discovery and preserves $description when unchanged",
    async ({ override }) => {
      directories.isLoading = true;
      const { result, rerender } = renderHook(() => useSettings());
      expect(result.current.isLoading).toBe(true);
      await expect(result.current.saveSettings()).resolves.toBeNull();
      expect(mutateAsync).not.toHaveBeenCalled();
      expect(setAppConfigDirOverride).not.toHaveBeenCalled();

      directories = {
        ...directories,
        isLoading: false,
        appConfigDir: override,
        initialAppConfigDir: override,
      };
      rerender();
      expect(result.current.isLoading).toBe(false);
      await act(async () => {
        await expect(result.current.saveSettings()).resolves.toEqual({
          requiresRestart: false,
        });
      });
      expect(mutateAsync).toHaveBeenCalledWith(saved);
      expect(setAppConfigDirOverride).not.toHaveBeenCalled();
      expect(result.current.requiresRestart).toBe(false);
    },
  );

  it("can explicitly reset a loaded override to the default directory", async () => {
    directories.initialAppConfigDir = "/existing/atlas";
    const { result } = renderHook(() => useSettings());
    await act(async () => {
      await result.current.saveSettings();
    });
    expect(setAppConfigDirOverride).toHaveBeenCalledWith(null);
    expect(result.current.requiresRestart).toBe(true);
  });

  it("autosaves startup changes without editing a data directory", async () => {
    const { result } = renderHook(() => useSettings());
    await act(async () => {
      await result.current.autoSaveSettings({ launchOnStartup: true });
    });
    expect(setAutoLaunch).toHaveBeenCalledWith(true);
    expect(setAppConfigDirOverride).not.toHaveBeenCalled();
    expect(mutateAsync).toHaveBeenCalledWith({
      ...saved,
      launchOnStartup: true,
    });
  });

  it("returns null when preferences have not loaded", async () => {
    form.settings = null;
    const { result } = renderHook(() => useSettings());
    await expect(result.current.saveSettings()).resolves.toBeNull();
    expect(mutateAsync).not.toHaveBeenCalled();
    expect(setAppConfigDirOverride).not.toHaveBeenCalled();
  });

  it("keeps the current directory when saving preferences fails", async () => {
    directories.appConfigDir = "/custom/atlas";
    mutateAsync.mockRejectedValueOnce(new Error("save failed"));
    const { result } = renderHook(() => useSettings());
    await expect(result.current.saveSettings()).rejects.toThrow("save failed");
    expect(setAppConfigDirOverride).not.toHaveBeenCalled();
    expect(result.current.requiresRestart).toBe(false);
  });
});
