Atlas 4.1.1 removes the **Advanced request settings** section from Copilot setup.

- Removes custom User-Agent/presets, manual header/body overrides, and the section's manual Chat cache/thinking switches.
- Deletes the supporting frontend and backend code and obsolete tests. Previously saved override fields are ignored.
- Retains protocol selection, model catalog and per-model reasoning controls, automatic Copilot compatibility, usage statistics, and read-only TOML previews.

Download **CC-Switch-atlas-4.1.1-Windows-x64.msi** and its SHA256 file. Close CC Switch before installing. This is an unsigned Windows x64 installer and upgrades Atlas 4.1.0. No Codex TOML changes are required for this update.

Validation: TypeScript checks, all 187 frontend tests and 1,634 Rust tests passed. Four optional platform/performance checks remain skipped. A regression test verifies that retired settings are ignored while account and pricing metadata is preserved.
