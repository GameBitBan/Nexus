# Privacy

Nexus is a local music player. It has **no account, no telemetry, no analytics and no ads**, and it never sends your library, listening history or files anywhere unless you use a feature that obviously needs to (Soulseek, Jam). Everything it knows about you stays in its data folder on your PC:

- Windows: `%APPDATA%\nexus\`
- Linux: `~/.config/nexus/` (or `$XDG_CONFIG_HOME/nexus`)

Delete that folder and Nexus forgets everything.

## What Nexus connects to, and why

| feature | talks to | what it sends | how to turn it off |
|---|---|---|---|
| update check | `api.github.com`, `github.com` | nothing about you (a normal web request for the latest release) | settings → update → *check for updates on launch* |
| Discover, album-cover and tag lookups | `api.deezer.com`, `itunes.apple.com` | the **artist / album / song names** being looked up | don't open Discover / don't press *look up*; or offline mode |
| Discord status | the Discord app on your own PC (local only) | what you're playing, to your own Discord | settings → discord → *show on discord* (off by default) |
| Discord album art | `itunes.apple.com` / `api.deezer.com` (default), or `catbox.moe` if you pick *cover from: your own cover* | artist + album names; with *your own cover*, the **cover image** is uploaded to catbox.moe (a public image host) so Discord can show it | settings → discord → *cover from* / *album cover* |
| Soulseek downloads | the Soulseek network (`vps.slsknet.org` + other users) | your Soulseek username, your searches, and — like every Soulseek client — the **list of files in the folders you share** (your music folders by default) | don't set a Soulseek login; settings → downloads → *share my music folders* |
| Internet Archive / Jamendo (Download page) | `archive.org`, `api.jamendo.com` + its file servers | your search words and the albums you download | just don't pick them as a source; or offline mode |
| firewall / port test | `www.slsknet.org` | your Soulseek port number, only when you press *test* | — |
| Jam (listen together) | `ntfy.sh` (to find each other), STUN servers at Google / Cloudflare (to find your public address), then a **direct encrypted connection** to your friend | connection details, **end-to-end encrypted** with a key made from the jam code (ntfy only sees scrambled data); audio, now-playing info, chat and files you choose to share go straight to the people in your jam | only active while you host or join a jam |

**Offline mode** (settings → advanced) blocks every web lookup in one switch: no updates, Discover, cover/tag lookups or Discord art. Soulseek and Jam still connect when you use them, because that's what they're for.

## Things Nexus stores

- `config.json`: your settings. Your Soulseek password is encrypted with Windows DPAPI (only your Windows account can read it). On Linux the file is readable only by you.
- `library.json`, `playlists.json`, `stats.json` (play counts for Discover), `imports.json` (what you imported from Spotify / Apple Music, so playlists can fill in later), `covers/` (thumbnails), `soulseek/` (download queue and share index), `crash.log` (errors only — no personal data beyond file paths).

## Imports

Spotify / Apple Music / CSV exports you import are read on your PC and never uploaded.

## Contact

Questions or concerns: open an issue at https://github.com/GameBitBan/Nexus/issues.
