Atlas 4.1.1 simplifies Copilot setup and makes the read-only Codex configuration comparison easier to review.

- Removes custom User-Agent/presets, manual header/body overrides, and the section's manual Chat cache/thinking switches.
- Deletes the supporting frontend and backend code and obsolete tests. Previously saved override fields are ignored.
- Shows current TOML on the left and proposed TOML on the right by default, with aligned changes and line numbers. Switch to an inline comparison or the complete proposed TOML.
- Uses the same comparison for Copilot setup and restoring OpenAI sign-in. Copy and refresh remain available; configuration files stay read-only.
- Retains protocol selection, model catalog and per-model reasoning controls, automatic Copilot compatibility, and usage statistics.

Download **CC-Switch-atlas-4.1.1-Windows-x64.msi** and its SHA256 file. Close CC Switch before installing. This is an unsigned Windows x64 installer and upgrades Atlas 4.1.0. No Codex TOML changes are required for this update.

Validation: TypeScript checks, all 192 frontend tests, the production renderer build, and all 10 focused Rust connection-preview tests passed. Coverage includes both complete file snapshots, unequal change blocks, line numbers, CRLF and missing final newlines, target/layout switching, and read-only copy/refresh behavior. The Windows pipeline runs the full Rust suite and MSI build before release.
