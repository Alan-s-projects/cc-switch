import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { providersApi, settingsApi } from "@/lib/api";
import { useSettingsQuery, useSaveSettingsMutation } from "@/lib/query";
import type { Settings } from "@/types";
import { useSettingsForm, type SettingsFormState } from "./useSettingsForm";
import {
  useDirectorySettings,
  type ResolvedDirectories,
} from "./useDirectorySettings";

interface SaveResult {
  requiresRestart: boolean;
}

export interface UseSettingsResult {
  settings: SettingsFormState | null;
  isLoading: boolean;
  isSaving: boolean;
  appConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  requiresRestart: boolean;
  updateAppConfigDir: (value?: string) => void;
  browseAppConfigDir: () => Promise<void>;
  resetAppConfigDir: () => Promise<void>;
  saveSettings: () => Promise<SaveResult | null>;
  autoSaveSettings: (
    updates: Partial<SettingsFormState>,
  ) => Promise<SaveResult | null>;
  acknowledgeRestart: () => void;
}

export type { SettingsFormState, ResolvedDirectories };

const sanitizeDir = (value?: string | null): string | undefined => {
  if (!value) return undefined;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
};

// Settings and Usage can mount separate consumers while an earlier save is
// pending. Keep all preference writes in order across page navigation.
let settingsSaveQueue: Promise<void> = Promise.resolve();

export function useSettings(): UseSettingsResult {
  const { t } = useTranslation();
  const { data } = useSettingsQuery();
  const saveMutation = useSaveSettingsMutation();
  const [saveState, setSaveState] = useState({ pending: 0, completed: 0 });

  const {
    settings,
    isLoading: isFormLoading,
    updateSettings,
  } = useSettingsForm(saveState.pending > 0, saveState.completed);

  const {
    appConfigDir,
    resolvedDirs,
    isLoading: isDirectoryLoading,
    initialAppConfigDir,
    updateAppConfigDir,
    browseAppConfigDir,
    resetAppConfigDir,
  } = useDirectorySettings();

  const [requiresRestart, setRequiresRestart] = useState(false);
  const acknowledgeRestart = useCallback(() => setRequiresRestart(false), []);

  const runSave = useCallback(async <T>(save: () => Promise<T>): Promise<T> => {
    setSaveState((state) => ({ ...state, pending: state.pending + 1 }));
    const result = settingsSaveQueue.then(save);
    settingsSaveQueue = result.then(
      () => undefined,
      () => undefined,
    );
    try {
      return await result;
    } finally {
      setSaveState((state) => ({
        pending: state.pending - 1,
        completed: state.completed + 1,
      }));
    }
  }, []);

  const savePreferences = useCallback(
    async (updates: Partial<SettingsFormState>) => {
      // Read inside the queue: a render snapshot can omit an earlier save, or
      // contain an optimistic change whose write failed.
      const previous = await settingsApi.get();
      const payload: Settings = { ...previous, ...updates };
      const startupChanged =
        payload.launchOnStartup !== undefined &&
        payload.launchOnStartup !== previous.launchOnStartup;

      if (startupChanged) {
        await settingsApi.setAutoLaunch(payload.launchOnStartup!);
      }
      try {
        await saveMutation.mutateAsync(payload);
      } catch (error) {
        if (startupChanged) {
          try {
            await settingsApi.setAutoLaunch(previous.launchOnStartup ?? false);
          } catch (rollbackError) {
            console.error("Failed to restore auto-launch:", rollbackError);
            toast.error(
              t("settings.autoLaunchFailed", {
                defaultValue: "Failed to set auto-launch",
              }),
            );
          }
        }
        throw error;
      }

      try {
        await providersApi.updateTrayMenu();
      } catch (error) {
        console.warn("[useSettings] Failed to refresh tray menu", error);
      }
    },
    [saveMutation, t],
  );

  const autoSaveSettings = useCallback(
    async (updates: Partial<SettingsFormState>): Promise<SaveResult | null> => {
      if (!settings) return null;
      const changes = { ...updates };
      updateSettings(changes);

      try {
        await runSave(() => savePreferences(changes));
        return { requiresRestart: false };
      } catch (error) {
        console.error("[useSettings] Failed to auto-save settings", error);
        toast.error(
          t("notifications.settingsSaveFailed", {
            defaultValue: "Failed to save settings: {{error}}",
            error: (error as Error)?.message ?? String(error),
          }),
        );
        throw error;
      }
    },
    [runSave, savePreferences, settings, t, updateSettings],
  );

  const saveSettings = useCallback(async (): Promise<SaveResult | null> => {
    if (!settings || isDirectoryLoading) return null;
    try {
      const sanitizedAppDir = sanitizeDir(appConfigDir);
      const appDirChanged = sanitizedAppDir !== initialAppConfigDir;

      const updates = Object.fromEntries(
        Object.entries(settings).filter(
          ([key, value]) => value !== data?.[key as keyof Settings],
        ),
      );

      await runSave(async () => {
        await savePreferences(updates);
        if (appDirChanged) {
          await settingsApi.setAppConfigDirOverride(sanitizedAppDir ?? null);
        }
      });

      setRequiresRestart(appDirChanged);

      toast.success(
        t("notifications.settingsSaved", {
          defaultValue: "Settings saved",
        }),
        { closeButton: true },
      );

      return { requiresRestart: appDirChanged };
    } catch (error) {
      console.error("[useSettings] Failed to save settings", error);
      toast.error(
        t("notifications.settingsSaveFailed", {
          defaultValue: "Failed to save settings: {{error}}",
          error: (error as Error)?.message ?? String(error),
        }),
      );
      throw error;
    }
  }, [
    appConfigDir,
    data,
    initialAppConfigDir,
    isDirectoryLoading,
    runSave,
    savePreferences,
    settings,
    setRequiresRestart,
    t,
  ]);

  const isLoading = useMemo(
    () => isFormLoading || isDirectoryLoading,
    [isFormLoading, isDirectoryLoading],
  );

  return {
    settings,
    isLoading,
    isSaving: saveState.pending > 0,
    appConfigDir,
    resolvedDirs,
    requiresRestart,
    updateAppConfigDir,
    browseAppConfigDir,
    resetAppConfigDir,
    saveSettings,
    autoSaveSettings,
    acknowledgeRestart,
  };
}
