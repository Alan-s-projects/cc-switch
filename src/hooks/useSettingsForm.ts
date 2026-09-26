import { useCallback, useEffect, useState } from "react";
import { useSettingsQuery } from "@/lib/query";
import type { Settings } from "@/types";

export type SettingsFormState = Settings;

const normalizeSettings = (settings: Settings): SettingsFormState => ({
  ...settings,
  showInTray: settings.showInTray ?? true,
  language: "en",
});

export function useSettingsForm() {
  const { data, isLoading } = useSettingsQuery();
  const [settings, setSettings] = useState<SettingsFormState | null>(null);

  useEffect(() => {
    if (data) setSettings(normalizeSettings(data));
  }, [data]);

  const updateSettings = useCallback((updates: Partial<SettingsFormState>) => {
    setSettings((previous) =>
      normalizeSettings({ showInTray: true, ...previous, ...updates }),
    );
  }, []);

  const resetSettings = useCallback((saved: Settings | null) => {
    if (saved) setSettings(normalizeSettings(saved));
  }, []);

  return { settings, isLoading, updateSettings, resetSettings };
}
