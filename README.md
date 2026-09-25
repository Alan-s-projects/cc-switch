# CC Switch Atlas

A Windows x64 desktop bridge from **Codex** to **GitHub Copilot**, with account
and model controls and a usage dashboard. Atlas starts at version **4.1.0**.

## Install and connect

Download `CC-Switch-atlas-4.1.0-Windows-x64.msi` from
[GitHub Releases](https://github.com/Alan-s-projects/cc-switch/releases).
Install it, sign in to GitHub Copilot, add a Copilot provider and start the proxy.

Open **Codex connection** using the file icon. Copy the suggested settings and
merge them into your Codex `config.toml`:

1. Put top-level fields before any TOML table headers.
2. Update the matching `model_providers` table instead of duplicating it.
3. Keep your model, reasoning, compaction, permissions, MCP and instruction settings.
4. Reload Codex to pick up the connection change.

**Codex configuration is read-only.** Starting or stopping the proxy, selecting
an account, saving settings, changing the listen port and restoring app backups
do not write Codex TOML, authentication, instructions, skills or session files.
Changes to the proxy address require manually updating the connection settings.

The generated model catalog belongs to CC Switch, at
`.cc-switch/copilot-model-catalog.json`. The suggestion includes its absolute
path. Model changes update this catalog without editing TOML.

When upgrading from a previous CC Switch version, exit the old app before
installing. That version may restore an old Codex configuration on exit; check
the connection suggestion after starting Atlas.

## Included

- Copilot login, accounts, model capabilities and protocol routing for Codex.
- Proxy controls, networking, failover and connectivity checks.
- Usage trends, request history, model/provider breakdowns, cache statistics,
  latency and estimated costs.
- App-data backups and sync.

Atlas is English-only. Other clients/providers and MCP, skill, instruction,
profile and session management are removed from the application. Existing
database records remain available for history and rollback. Usage is recorded
from proxy traffic; conversation files are not scanned. Cost estimates and
Copilot quota have different scopes and do not replace GitHub billing.

## Develop and build

Requires Windows x64, Node.js 22, pnpm 10, Rust (pinned in
`rust-toolchain.toml`), Visual Studio C++ Build Tools and WebView2.

```powershell
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test:unit --maxWorkers=4 --minWorkers=1
pnpm build
```

`scripts/build-msi.ps1` creates the MSI and its SHA256 file under `release/`.
The only workflow builds and tests Windows x64. An `atlas-<version>` tag
publishes the installer. All three manifests use the same version.

Run backend tests with isolated data:

```powershell
$atlasTestHome = Join-Path $env:TEMP ("atlas-tests-" + [guid]::NewGuid().ToString("N"))
$atlasTestData = Join-Path $atlasTestHome ".cc-switch"
New-Item -ItemType Directory -Path $atlasTestData -Force | Out-Null
[IO.File]::WriteAllBytes((Join-Path $atlasTestData "cc-switch.db"), [byte[]]@())
$env:CC_SWITCH_TEST_HOME = $atlasTestHome
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1
```

The bridge regression tests check that the provider/proxy lifecycle leaves
Codex files byte-for-byte unchanged, including when a legacy takeover backup
exists.

## License and credits

[MIT](LICENSE). Based on [CC Switch](https://github.com/farion1231/cc-switch)
and the [Copilot capability-routing work](https://github.com/ljie-PI/cc-switch/tree/feat/codex-copilot-capability-routing).
