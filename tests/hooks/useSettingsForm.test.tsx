import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import i18n from "i18next";
import { useSettingsForm } from "@/hooks/useSettingsForm";

const useSettingsQueryMock = vi.fn();

vi.mock("@/lib/query", () => ({
  useSettingsQuery: (...args: unknown[]) => useSettingsQueryMock(...args),
}));

let changeLanguageSpy: ReturnType<typeof vi.spyOn<any, any>>;

beforeEach(() => {
  useSettingsQueryMock.mockReset();
  window.localStorage.clear();
  (i18n as any).language = "zh";
  changeLanguageSpy = vi
    .spyOn(i18n, "changeLanguage")
    .mockImplementation(async (lang?: string) => {
      (i18n as any).language = lang;
      return i18n.t;
    });
});

afterEach(() => {
  changeLanguageSpy.mockRestore();
});

describe("useSettingsForm Hook", () => {
  it("always uses English regardless of the stored language", () => {
    useSettingsQueryMock.mockReturnValue({
      data: null,
      isLoading: false,
    });
    window.localStorage.setItem("language", "ja");

    const { result } = renderHook(() => useSettingsForm());

    const lang = result.current.readPersistedLanguage();
    expect(lang).toBe("en");
    expect(changeLanguageSpy).not.toHaveBeenCalled();
  });

  it("should update fields and sync language when language changes in updateSettings", () => {
    useSettingsQueryMock.mockReturnValue({
      data: null,
      isLoading: false,
    });

    const { result } = renderHook(() => useSettingsForm());

    act(() => {
      result.current.updateSettings({ showInTray: false });
    });

    expect(result.current.settings?.showInTray).toBe(false);

    changeLanguageSpy.mockClear();
    act(() => {
      result.current.updateSettings({ language: "en" });
    });

    expect(result.current.settings?.language).toBe("en");
    expect(changeLanguageSpy).toHaveBeenCalledWith("en");
  });
});
