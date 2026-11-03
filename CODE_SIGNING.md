# Code signing policy

Windows builds of Nexus (`NexusSetup.exe`, `Nexus.exe`) are built automatically by GitHub Actions from the source in this repository (`.github/workflows/release.yml`). Only builds made by that workflow, from a tagged release, are signed.

<!-- when signing is set up, keep the line that applies:
Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).
Signed with Azure Artifact Signing under the name of the maintainer.
-->

## Team

| role | who |
|---|---|
| author / committer | [GameBitBan](https://github.com/GameBitBan) |
| reviewer (changes from outside contributors) | [GameBitBan](https://github.com/GameBitBan) |
| approver (each signing request) | [GameBitBan](https://github.com/GameBitBan) |

All team members use multi-factor authentication on GitHub (and the signing service).

## What gets signed

Only Nexus's own executables and installer built from this repository. Third-party components bundled in the build (Python, Qt/PySide6, FFmpeg/PyAV, NumPy and the other libraries in `requirements.txt`) keep their own upstream signatures, or none.

## Privacy

See [PRIVACY.md](PRIVACY.md) — Nexus has no telemetry; every network connection is listed there with how to turn it off, and **offline mode** (settings → advanced) turns all web lookups off at once.

## Uninstalling

Settings → Apps → Installed apps → Nexus → Uninstall (or run `Install Nexus.pyw --uninstall` / the uninstaller in the install folder). Your music is never touched; Nexus's own data is in `%APPDATA%\nexus`.
