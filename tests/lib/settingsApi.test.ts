import { beforeEach, describe, expect, it, vi } from "vitest";
import { settingsApi } from "@/lib/api/settings";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
}));

describe("settingsApi SQL import", () => {
  beforeEach(() => {
    mocks.invoke.mockReset();
  });

  it("calls the retained SQL import command and returns its result", async () => {
    const result = {
      success: true,
      message: "SQL imported successfully",
      backupId: "backup-1",
    };
    mocks.invoke.mockResolvedValue(result);

    await expect(
      settingsApi.importConfigFromFile("C:/backups/atlas.sql"),
    ).resolves.toEqual(result);
    expect(mocks.invoke).toHaveBeenCalledWith("import_config_from_file", {
      filePath: "C:/backups/atlas.sql",
    });
  });
});
