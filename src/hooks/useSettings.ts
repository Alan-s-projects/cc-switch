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
  updateSettings: (updates: Partial<SettingsFormState>) => void;
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

/**
 * useSettings - 组合层
 * 负责：
 * - 组合 useSettingsForm、useDirectorySettings
 * - 保存设置逻辑
 */
export function useSettings(): UseSettingsResult {
  const { t } = useTranslation();
  const { data } = useSettingsQuery();
  const saveMutation = useSaveSettingsMutation();

  // 1️⃣ 表单状态管理
  const {
    settings,
    isLoading: isFormLoading,
    updateSettings,
  } = useSettingsForm();

  // 2️⃣ 目录管理
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

  // 即时保存设置（用于 General 标签页的实时更新）
  // 保存基础配置 + 独立的系统 API 调用（开机自启）
  const autoSaveSettings = useCallback(
    async (updates: Partial<SettingsFormState>): Promise<SaveResult | null> => {
      const mergedSettings = settings ? { ...settings, ...updates } : null;
      if (!mergedSettings) return null;

      try {
        const payload: Settings = { ...mergedSettings };

        // 保存到配置文件
        await saveMutation.mutateAsync(payload);

        // 如果开机自启状态改变，调用系统 API
        if (
          payload.launchOnStartup !== undefined &&
          payload.launchOnStartup !== data?.launchOnStartup
        ) {
          try {
            await settingsApi.setAutoLaunch(payload.launchOnStartup);
          } catch (error) {
            console.error("Failed to update auto-launch:", error);
            toast.error(
              t("settings.autoLaunchFailed", {
                defaultValue: "Failed to set auto-launch",
              }),
            );
          }
        }

        // 更新托盘菜单
        try {
          await providersApi.updateTrayMenu();
        } catch (error) {
          console.warn("[useSettings] Failed to refresh tray menu", error);
        }

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
    [data, saveMutation, settings, t],
  );

  // 完整保存设置（用于 Advanced 标签页的手动保存）
  // 包含所有系统 API 调用和完整的验证流程
  const saveSettings = useCallback(async (): Promise<SaveResult | null> => {
    if (!settings || isDirectoryLoading) return null;
    try {
      const sanitizedAppDir = sanitizeDir(appConfigDir);
      const appDirChanged = sanitizedAppDir !== initialAppConfigDir;

      const payload: Settings = { ...settings };

      await saveMutation.mutateAsync(payload);

      if (appDirChanged) {
        await settingsApi.setAppConfigDirOverride(sanitizedAppDir ?? null);
      }

      // 只在开机自启状态真正改变时调用系统 API
      if (
        payload.launchOnStartup !== undefined &&
        payload.launchOnStartup !== data?.launchOnStartup
      ) {
        try {
          await settingsApi.setAutoLaunch(payload.launchOnStartup);
        } catch (error) {
          console.error("Failed to update auto-launch:", error);
          toast.error(
            t("settings.autoLaunchFailed", {
              defaultValue: "Failed to set auto-launch",
            }),
          );
        }
      }

      try {
        await providersApi.updateTrayMenu();
      } catch (error) {
        console.warn("[useSettings] Failed to refresh tray menu", error);
      }

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
    saveMutation,
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
    isSaving: saveMutation.isPending,
    appConfigDir,
    resolvedDirs,
    requiresRestart,
    updateSettings,
    updateAppConfigDir,
    browseAppConfigDir,
    resetAppConfigDir,
    saveSettings,
    autoSaveSettings,
    acknowledgeRestart,
  };
}
