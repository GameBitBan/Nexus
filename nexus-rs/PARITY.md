# Nexus 2 (Rust) — parity checklist

The Rust version ships only when every box below is ticked. It reads and writes the same data folder as the
Python app (`config.json`, `library.json`, `covers/`, `playlists/`, `stats.json`, `skins/`, `imports/`), so
switching between them is lossless in both directions.

Legend: `[x]` done and tested · `[~]` started · `[ ]` not started

## Stage 1 — core (`nexus-core`)
- [x] Data dir (`NEXUS_HOME` / `MONOTONE_HOME`, `%APPDATA%\nexus`, `$XDG_CONFIG_HOME/nexus`), resource dir lookup for Nix `share/nexus`
- [x] Config — Python `DEFAULTS` merged with saved file, atomic saves, 0600 on unix
- [x] Secrets — DPAPI `dpapi:` values on Windows, compatible with Python's `protect_secret`
- [x] Library — `library.json` schema, album keys (`md5(albumartist|album)[:16]`), search blobs, sorting
- [x] Scanner — FLAC/ALAC/WAV/AIFF/WV/APE/DSF/Opus/OGG/MP3/AAC tags + quality, incremental by mtime, parallel
- [x] Covers — embedded or folder art, 800 px JPEG q90 in `covers/<album_key>.jpg`
- [x] Audio engine — FFmpeg decode, ring buffer, cpal output at the file's own rate when the device allows
- [x] Gapless — bit-exact rollover (tested: 198 450 / 198 450 frames, max diff 0.0)
- [x] Seek / pause / resume / stop / end-of-queue events
- [x] 10-band linear-phase FIR EQ + preamp (same impulse as Python), click-free curve changes
- [x] ReplayGain (track / album + preamp), volume curve, soft limiter
- [x] Device loss → reopen
- [ ] WASAPI exclusive mode (Windows)
- [x] Output device list / pick in settings

## Stage 2 — GUI shell (`nexus`, egui)
- [x] Window, bundled fonts (Doto / Inter, or installed ones), themes, text-style look, case + bracket rules
- [x] Studio layout: nav rail, album grid, song table, now panel with the spinning record + dot ring
- [x] Player bar: transport, seek (dots / line / text), volume + boost, shuffle/repeat, quality badge, lossy colour
- [x] Album / playlist / artists / songs / queue pages, column browser (with arrow keys)
- [x] Search, sorting, keyboard shortcuts, right-click menus, playlists (new / rename in place / delete + undo)
- [x] Play counts, resume last session (queue + position), window size, save on logout/kill
- [x] Layouts: studio, classic, tabs, focus, mini (+ pin on top)
- [x] Ambient cover glow behind the window, toasts
- [x] Single instance on Linux/macOS (talks to a running Python Nexus too)
- [ ] Single instance on Windows (named pipe)
- [~] File drops + `nexus FILE…` (skins, exports, audio, cover images; album zips and updates still to do)

## Stage 3 — look & feel
- [x] Visualizer page + analyzer (dots, bars, mirror, wave, ring; cover colour; immersive)
- [x] Equalizer page (curve over live spectrum, presets, preamp, dot sliders)
- [x] Lyrics page (`.lrc` synced + embedded SYLT / LRC / plain; click a line to jump)
- [x] Inspect page (spectrogram, lossless / fake hi-res / padded 24-bit verdict — same results as Python)
- [x] Settings page (every Python option; buttons for Soulseek/Discord/updates wait for their stages)
- [x] Skins: `.nexusskin` import/export/drop, built-in skins + looks, skin cards, skin maker (stylesheet subset: panel borders, main buttons, heading colour)
- [x] Skin changes keep the layout
- [x] Backgrounds: image or looping video (blur, opacity, paused while minimised), cover glow

## Stage 4 — library tools & integrations
- [x] Tag editor (single + batch, inline cells, cover drop / paste / folder / online lookup, tools, rename files) — keeps unknown tags; FLAC/Opus/OGG/M4A/ALAC/MP3/WAV/AIFF/WV/APE/DSF
- [x] Import from Spotify / Apple Music / CSV / M3U exports, library matcher, rematch after scans ("get missing albums" waits for Soulseek)
- [ ] Discover page (Deezer charts, previews, net images)
- [ ] Internet Archive + Jamendo sources, web downloads
- [ ] Watch folder
- [ ] Discord Rich Presence (app id 1566983013445931018)
- [ ] MPRIS (Linux media keys / desktop widgets)
- [x] OBS now-playing file
- [~] Updater (GitHub release digests) + offline mode / network policy — `net.rs` done (http(s) only, size caps, proxy, system certs, offline allow-list)
- [ ] Problem log, uncaught-error log, `--selftest`

## Stage 5 — Soulseek
- [ ] Native client: login, search, browse, download queue, resume
- [ ] Shares + uploads, distributed network
- [ ] UPnP, port check, firewall helper, VPN detection + advice
- [ ] Download page (grouped results, broader queries, quality filters)

## Stage 6 — Jam
- [ ] ntfy signalling with scrypt + AES-GCM (`nexusjam2` topics) — interoperable with Python Nexus
- [ ] WebRTC audio (Opus) host → guests, sync, latency
- [ ] Guest controls (local pause, volume, position), host queue/state
- [ ] Song / album requests and file transfer (size limits, allow/decline)

## Stage 7 — packaging
- [ ] Nix flake: package, dev shell, NixOS module-free install (desktop file, icon, MPRIS)
- [ ] Windows: release build, installer, code signing hook (SignPath)
- [ ] CI: Linux + Windows builds and tests on the `rust` branch
- [ ] README / migration notes
