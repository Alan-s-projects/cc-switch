Atlas 4.1.2 moves Codex TOML file selection onto the read-only connection preview.

- Removes the separate Codex configuration-directory override section from Advanced settings and cleans up its client-directory controls and callbacks.
- Adds a TOML location field, Browse, and Auto-detect directly above the diff. Refresh or Enter loads a typed path; selecting a file loads it immediately.
- Remembers the last valid manually selected file for previews only. Existing saved directory preferences remain available to automatic detection.
- Reports missing or unreadable files and invalid TOML. A missing automatically detected file can still show a new-file suggestion; a missing manual selection is an error.
- Hides stale comparisons while a different path is being edited or a read has failed. Atlas never creates or modifies the selected file, authentication, or other Codex data.
- Preserves Atlas's own data-directory controls, model catalog, global pricing, usage dashboard, and health checks.

Download **CC-Switch-atlas-4.1.2-Windows-x64.msi** and its SHA256 file. Use the tray menu's **Quit** action before installing. This unsigned Windows x64 installer upgrades existing Atlas installations.

Validation: TypeScript checks, all 204 frontend tests, all 1,625 Rust tests, and the production renderer build passed. Four optional platform/performance checks remain skipped. Regression tests cover file selection, remembered paths, missing/invalid files, stale previews, read-only file handling, and Atlas data-directory settings. The file picker and recovery flow were also checked visually with sample data.
