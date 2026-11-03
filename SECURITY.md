# Security

## Reporting a problem

Please report vulnerabilities privately through GitHub: **Security → Report a vulnerability** on https://github.com/GameBitBan/Nexus (private advisory). Don't open a public issue for security bugs. You'll get an answer within a few days; fixes ship as a new release, which Nexus installs itself through its updater.

Supported: the latest release only.

## How Nexus protects you

Nexus deals with data from strangers: Soulseek peers, Jam guests and hosts, skins people share, tags in downloaded files, and web APIs. The rules it follows:

- **Updates are verified.** The updater only installs releases from this repository's GitHub Releases, checks the download against GitHub's SHA-256 checksum, and only replaces Nexus's own files. A zip you drop on the window is flagged as "replaces Nexus's program files" before anything happens. `install.ps1` verifies the same checksum.
- **Downloads stay in their folder.** Soulseek file names and paths come from the other side; Nexus rewrites them so nothing can be written outside the download folder, and only audio files (plus cover art) are imported into your library. Nothing downloaded is ever executed.
- **Jam is end-to-end encrypted.** Signalling goes through ntfy.sh encrypted with AES-GCM using a key derived from the jam code with scrypt (so a code can't be brute-forced from its relay topic). Audio and data go over WebRTC (DTLS). Listeners only receive files they asked for, only audio and cover images, and never more than announced. Every message from the other side is size-limited.
- **Skins are data, not code.** A `.nexusskin` may only contain settings, a Qt stylesheet, images, videos and fonts — anything else is dropped on import. Paths inside a skin can't point outside the skin (no absolute paths, drives, `..` or network shares), and stylesheet `url()`s may only reference the skin's own files.
- **Text is shown as text.** Song titles, tags, chat, lyrics, Soulseek names and tooltips are always rendered as plain text, so HTML in a tag can't load images from your disk or a network share.
- **Only http(s).** The networking layer refuses `file://` and `ftp://` URLs outright, whatever data a URL came from.
- **Secrets are protected.** The Soulseek password is encrypted with Windows DPAPI; on Linux the config file is only readable by you.
- **Single-instance socket** is restricted to your own user account.

## Known limits

- Soulseek is a public peer-to-peer network: other users see your username and the files in folders you share. Share only folders you're comfortable listing (settings → downloads).
- Audio files themselves are decoded by FFmpeg (through PyAV) and Qt; Nexus keeps those libraries current through their normal package updates.
- Python packages are installed from PyPI by name (see `requirements.txt`).
