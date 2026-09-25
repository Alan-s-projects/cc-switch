Atlas 4.1.4 introduces persistent navigation and clearer overview cards, with Windows installers built locally.

- Keeps **Overview**, **Usage**, **Connect**, and **Settings** at the top left on every page, highlighting the selected page. Removes the previous changing title, Back button, and decorative brand animation.
- Keeps the original green proxy switch on the right, labeled **Proxy Running** or **Proxy Stopped**.
- Groups the overview into large **Provider**, **Proxy**, **Today's usage**, and **Requests** cards, each with its title at the upper left. Removes the redundant Overview heading.
- Moves **Health check** into the Provider card. **Refresh overview**, connection warnings, usage metrics, and recent requests remain available.
- Preserves read-only Codex previews and the existing low-CPU update behavior; this layout change adds no polling or background work.
- Removes the GitHub Actions workflow. Tests, renderer builds, and Windows MSI packaging run locally; releases receive the verified MSI and checksum directly.

Download **CC-Switch-atlas-4.1.4-Windows-x64.msi** and its SHA256 file. Use the tray menu's **Quit** action before installing. This unsigned Windows x64 installer upgrades existing Atlas installations.

Local validation: TypeScript, all 222 frontend tests, and all 1,535 Rust tests passed; four optional Rust checks remain skipped. Browser checks covered the normal and minimum 900×600 layouts, all page selections, card grouping, Health check placement, and both proxy status labels. The MSI is built locally and its version, architecture, upgrade identity, and checksum are checked before publication.
