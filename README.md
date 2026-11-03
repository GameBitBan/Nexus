# Nexus

![license](https://img.shields.io/badge/license-GPL--3.0-black) ![platform](https://img.shields.io/badge/windows%20%7C%20linux-black)

Minimal black & white local music player for your FLAC / lossless library. Album-grid home, Apple / Nothing inspired look, single-click play, album art everywhere, and nearly everything customizable live.

## install

### Windows — one line (recommended)
Open **PowerShell** (Start → type *PowerShell*) and paste:
```powershell
iwr -useb https://raw.githubusercontent.com/GameBitBan/Nexus/main/install.ps1 | iex
```
It installs Python if you don't have it, grabs the latest release, and adds **Nexus** to your Start Menu and Desktop (right-click → Pin to taskbar). Works with Smart App Control on. Uninstall from Settings → Apps → Installed apps → Nexus (your library and settings are kept).

### Windows — manual
1. Install Python 3.10+ from python.org (tick "Add python.exe to PATH").
2. Download `nexus.zip` from [Releases](https://github.com/GameBitBan/Nexus/releases/latest), extract it, double-click **`Install Nexus.pyw`**.

### NixOS
```
nix run github:GameBitBan/Nexus       # run once, straight from GitHub
nix run .                      # or from a clone
nix profile install .          # installs `nexus` + desktop entry + icon
```
Or add the flake as an input and put `inputs.nexus.packages.x86_64-linux.default` in `environment.systemPackages`. Wayland app_id / class is `nexus` for Hyprland window rules.

### other Linux / mac
`./run.sh`

On first launch it scans `~/Music` (or your old monotone folders). Add folders in settings → library.

## updating (no reinstall)

Nexus checks GitHub Releases on launch and shows a little *update + restart* button at the bottom when there's a new version (settings → update to turn that off or check now). You can also **drag a nexus.zip onto the window**. It swaps the app files in place, installs any new packages on the next start, keeps your library, playlists and settings, and restarts. The previous `nexus.py` is kept in `_previous/`. Soulseek downloads that are still running pause for the restart, then carry on and import by themselves (from 1.7.1 on).

Making a release: bump `VERSION` in `nexus.py` and push to `main`. GitHub Actions builds `nexus.zip`, tags it `v<VERSION>` and publishes the release; everyone's Nexus picks it up.

## sound

The **Nexus audio engine** (default) decodes with FFmpeg and outputs through PortAudio:

- **exclusive mode** (Windows, settings → sound): Nexus takes over the device, so audio skips the Windows mixer and its resampling. With EQ off and volume at 100% the output is bit-perfect. Other apps go silent while Nexus plays.
- **equalizer** (`ctrl+e` or the sliders icon in the player bar): 10 bands from 31 Hz to 16 kHz, ±12 dB, plus preamp and preset chips (bass boost, vocal, loudness, late night, hip-hop…). It fills the window: a big EQ curve over a live spectrum (drag the curve's points or the sliders), and the sliders below. Linear-phase, so it doesn't smear transients.
- **volume boost** up to 400% (settings → sound → max volume, default 200%), with a peak limiter so it gets louder without crackling.
- **gapless** playback between tracks.
- **ReplayGain** (track or album) for even loudness, if your files have the tags.

If the engine's packages (numpy, av, sounddevice) can't load, Nexus falls back to the basic Qt player and says so in settings → sound.

## discover

The compass in the rail (`ctrl+5`) recommends music from what you already have:
- **your top artists**: who you play most (Nexus counts a play after 30 s, or half of a short song), then who you own the most of.
- **artists you might like**: artists similar to several of yours, from Deezer's free public API (only artist names are sent). Each shows who it's *like*; ▶ plays a 30-second preview (your music pauses and comes back after), ⤓ searches Soulseek for them, *songs* opens their top tracks.
- **albums you don't have yet**: newer or older albums and EPs from artists in your library that aren't in it. **⤓ downloads it**: Nexus searches Soulseek, picks the best copy of that exact album (lossless first, skips other albums by the same artist), queues it with backups in case the first user never starts, and moves it into your library, adding the cover you saw if the download came without one. Same for the ⤓ next to a recommended artist's songs (downloads the album that song is on).
- **rediscover**: albums from your own library you haven't played in a month or more.

Results are cached and refresh every few days, or hit *refresh*.

## jam (listen together)

The people icon in the rail (`ctrl+j`). One of you **hosts**, the other **joins**:
- **host**: *start a jam* gives you a code like `K7Q-M2X` (*copy invite* makes a message to paste in Discord). Whatever you play streams to your friends live, about a second behind, up to 4 people. You see who's listening (right-click to remove someone) and a chat.
- **join**: type the code. Your own music pauses and you hear theirs, with the cover, song, progress, what's up next, your own volume, and the chat. The normal player bar follows along: it shows their song and how far in it is, **play/pause (or space) pauses just for you** (the others keep listening, and resuming drops you back in live), and the volume slider and mute are yours. Skipping asks the host (if they've allowed it); only the host can seek or go back. You can search their library and double-click a song to add it to their queue (the host can turn that off, and can let friends skip).

Audio goes straight between your PCs (WebRTC, encrypted, Opus at 320 kbps — near-transparent, not lossless). To find each other, both apps swap connection details through a public ntfy.sh channel named after the code, encrypted with a key made from the code, so the relay only sees scrambled data. If both of you are behind very strict routers (e.g. carrier-grade NAT on both ends) the direct connection can fail — one of you trying a different network (a phone hotspot) usually fixes it. Hosting needs the Nexus audio engine.

## visualizer & glow

- **visualizer** (`ctrl+b` or the bars icon in the player bar): full-window spectrum over a slowly moving blur of the cover. Styles: **dots** (Nothing-style dot matrix), **bars**, **mirror**, **wave** (oscilloscope), **ring** (cover in the middle, dots radiating out). **mono** / **cover colour** switches between the theme colour and a colour picked from the album art. **Immersive** (or double-click it) hides everything else and goes full screen; `esc` comes back.
- **record ring**: the dot ring around the spinning record in the now-playing panel dances to the music.
- **album-art glow**: a soft blur of the current cover behind the whole app, fading between songs. Strength is adjustable.

All of it is in settings → look → *background & visualizer* (style, colour, sensitivity, glow on/off + strength, record ring on/off). The visualizer reads what the Nexus audio engine is actually playing, after the EQ; with the basic Qt player it has nothing to show.

## downloading (Soulseek)

The download icon in the rail (`ctrl+d`) searches **Soulseek**. Nexus connects to the network itself, and finished files land straight on this PC and get imported. Results are grouped by album folder and ranked best-first: lossless before lossy, 24-bit first, free upload slot, fast peers. Hit **Download** on a row (or double-click it, or one track). Progress shows at the bottom.

Set it up in **settings → downloads**:
- **username + password**: any name works. The first login creates the account. Keep the password, because it's what proves the name is yours.
- **sharing**: by default Nexus shares your music folders back. Most Soulseek users won't upload to people who share nothing. Untick it or add extra folders there.
- **listening port / UPnP**: UPnP opens the port on your router automatically. If downloads from some people never start, forward that port (default 2234, and 2235 for obfuscated connections) manually.
- Finished downloads are tagged, moved into `<music>/<Album Artist>/<Album>/` with their cover, and show up in your library.

**When a download won't start:** Soulseek users can be offline, busy or have long queues. Each download shows what it's waiting on (e.g. *waiting in their queue (#37)*). If an album hasn't started after 5 minutes, or every file failed, Nexus moves it to another user who has the same album — from your search, or with a fresh search — and tells you. Right-click a download for *try another user*, *retry failed files*, *cancel* or *search for it again*. The timing (or turning it off) is in settings → downloads.

**Searching:** Soulseek only returns files whose names contain *every* word you type, so long searches often find nothing. Nexus tidies the search (dashes, brackets) and, if nothing comes back, retries without words like *deluxe / edition / remastered*, then with just the first few words. If there are still no results it checks why: whether other users can reach you (it asks slsknet.org's port test) and whether Windows Firewall is set up, and tells you which one it is. *Test port* in settings → downloads runs the same check.

**Windows Firewall:** other users have to be able to connect to you, or many downloads never start. The installer sets that up (one *Yes* on the admin prompt); if it's missing, the Download page shows a **fix firewall** button. It only opens Nexus's two Soulseek ports (2234/2235 by default) for Python. If Windows ever pops up *"Windows Defender Firewall has blocked some features of Python"*, tick **Private networks** and click **Allow**.

### watch folder
Settings → downloads → watch folder: point it at e.g. `Downloads` and tick *watch it*. Any new FLAC/MP3/etc. or **zip** that lands there is sorted into your library automatically. Files that were already in the folder are left alone, and half-finished browser downloads (`.crdownload`, `.part`) are skipped until they finish.

## discord ("Listening to Nexus")

Shows your friends what you're playing, Spotify-style: song, artist, album cover and a progress bar. Settings → **discord**:
1. Go to [discord.com/developers/applications](https://discord.com/developers/applications) → **New Application** → name it `Nexus`. That name is what friends see.
2. Copy its **Application ID** into Nexus and tick **show on discord**.
3. Optional: under *Rich Presence → Art Assets*, upload `icons/nexus-1024.png` and put its asset name (e.g. `nexus`) in Nexus → *fallback image asset*. It shows as the fallback image and as the little badge on the cover.

Needs the Discord desktop app (or Vesktop) running on the same PC. If Nexus says *discord accepted it* but nothing shows on your profile, turn on Discord → User Settings → **Activity Privacy → Share my activity**. Options: song title in your status line, hide when paused, and album cover. Discord can only show images that are online, so **cover from** is either:
- **your own cover** (default): the exact cover Nexus shows, uploaded once per album to catbox.moe (free image host, no account) and remembered. If the upload fails it falls back to matching online.
- **match it online**: nothing uploaded; finds the album on iTunes / Deezer by artist + album. Mixtapes and self-released stuff often won't match.

The Nexus logo is used when there's no cover, and as the little badge on it. Nexus picks your uploaded art asset automatically (named `nexus` if you have one, else the first one).

## inspect (is it really lossless?)

Right-click any song → **inspect (spectrum)**, press `ctrl+i` for the current song, or drop any audio file onto the window. You get a Spek-style spectrogram (time across, frequency up, dB colour scale; hover for a readout; `mono` / `color` toggle) and a verdict:

- **looks lossless / true hi-res**: content runs up to the top (~22 kHz for 44.1k, ~48 kHz for 96k).
- **lossy transcode**: a flat ceiling, marked with a dashed line. About 16 kHz is 128k MP3, ~19 kHz is 192–256k, ~20 kHz is 320k.
- **not true hi-res**: a 96k file with nothing above ~22–25 kHz (upsampled from CD).
- **padded 24-bit**: a 24-bit file that only uses 16 bits.

It's a strong hint, not proof. Some encoders (high-bitrate AAC, Opus) keep full bandwidth and leave no ceiling. Old or lo-fi recordings can have little treble without being lossy; Nexus only flags a sharp encoder cliff, not a gentle roll-off.

## editing tags & covers

Right-click songs → **edit tags & cover**, right-click an album → **edit album tags & cover**, the **edit tags** button on an album/playlist page, or `ctrl+t` for the current song. It opens inside the window:

- **fields** for title, artist, album, album artist, track, disc, year/date, genre, composer, comment and lyrics. They edit whatever is selected in the table below (everything by default), so changing *album* or *year* fixes the whole album at once; differing values show as ‹multiple values› and are left alone unless you type over them. Title, artist, track and disc can also be edited right in the table (double-click or just type). Changed cells go bold and show the old value on hover.
- **cover**: drop an image on it, paste one, choose a file, use the folder's cover.jpg, remove it, or **find online** (iTunes, then Deezer: up to 1200 px art plus year + genre you can apply). *Shrink to 1200 px* keeps big scans from bloating every file.
- **tools**: number 1…n, titles from file names (`03 - Song.flac` → Song), Title Case, find / replace in any field, and **rename files** from a pattern like `{track:02} {title}` or `{disc}-{track:02} {artist} - {title}` (playlists, queue and .lrc files follow along).
- Nothing touches your files until **Save**; **Revert** throws the edits away. Leave the page with unsaved edits and a reminder pops up at the bottom with a Save button.

Works for FLAC, ALAC/AAC (m4a), MP3, OGG/Opus, WavPack, APE, WAV, AIFF and DSF. If a file is playing on Windows it may be locked; save again after it finishes.

## lyrics

Click the speech-bubble icon in the player bar (or `ctrl+l`). It lights up when the song has lyrics:
- a `.lrc` file with the same name next to the song (e.g. `03 Roygbiv.lrc`), or
- lyrics embedded in the file's tags (FLAC/OGG `LYRICS` / `UNSYNCEDLYRICS`, MP3 `USLT`/`SYLT`, M4A `©lyr`).

Synced (timestamped) lyrics highlight the current line and scroll with the song, and clicking a line jumps there. Scroll manually and it pauses following for a few seconds. Plain lyrics show as text.

Settings, Equalizer and Lyrics open inside the main window. Hit **Done** (or the same button / shortcut again) to go back.

## layouts

**studio** (default)
- home is the **album grid**: hover a cover for its play button, click a cover to open the album
- icon rail on the left: albums, browse, songs, queue, + new playlist, then your playlists as cover icons (right-click one to rename/delete), rescan at the bottom
- **browse** = albums + tracks columns. Arrow keys move between them, enter plays, double-click an album to play it
- **now playing panel** on the right: round record that spins while playing, inside a dot ring that fills with progress. Click the ring to seek, click the record to play/pause. Big dot-matrix time + up next
- player bar: queue and panel toggles next to volume

**classic** — text sidebar instead of the rail (the terminal preset uses this).

Switch in settings → look → layout. Record size, spin speed, and spin on/off are there too.

## skins

Settings → **skins** (or the *change skin* button in Look). A skin changes the whole look: colours, fonts, corner radius, layout, buttons, progress-bar style, glyphs, visualizer, the glow, a **background image** and its own **stylesheet** (Qt CSS, the Spicetify-style bit).

- **use one**: click its card. Nexus comes with *Vapor*, *Glass*, *Paper* and *Amber CRT* plus the built-in looks.
- **install one**: drop a `.nexusskin` file on the Nexus window, or *import skin file*.
- **make one**: set things up in Look the way you like, then in Skins give it a name, optionally pick a background image and write some CSS (*preview stylesheet* tries it live), and hit **save as skin**. **save + export file** also writes a `.nexusskin` you can send to people. Every skin card has *export* and *delete* (with undo).

Skins carry no code: only look settings, a stylesheet, images and fonts, so they're safe to share. Anything else in a skin file is dropped on import.

### skin file format
A `.nexusskin` is a zip:
```
skin.json        name, author, description, colors, settings, background
style.qss        optional Qt stylesheet
assets/          images (background, anything the stylesheet uses)
fonts/           .ttf / .otf, loaded while the skin is active
```
```json
{
  "name": "Vapor", "author": "you", "description": "synthwave sunset",
  "background": "assets/background.jpg",
  "colors":   {"bg": "#0b0218", "panel": "#160630", "text": "#f6e9ff", "dim": "#a98bc9",
               "accent": "#ff3fb4", "select": "#2a0d4d", "border": "#3a1566"},
  "settings": {"corner_radius": 16, "progress_style": "line", "vis_style": "mirror",
               "skin_bg_opacity": 85, "font_family": "Inter", "layout": "studio"}
}
```
In `style.qss`, `@bg @panel @text @dim @accent @select @border` become the skin's colours and `url(skin:assets/x.png)` points at the skin's own files. Parts you can style: `#root #topbar #rail #card #nowpanel #npbar #sidebar #search #brand #h1 #h2 #eyebrow #dim #time #badge #pill #pillPrimary #chip #nav #settingsnav #eqpage #vispage #tagpage #lyric #bigtime #npBig #toast #coverdrop`. Example:
```css
QLabel#h1, QLabel#brand { color: @accent; }
QPushButton#pillPrimary { background: @accent; border-color: @accent; }
QWidget#card { border: 1px solid rgba(255, 63, 180, 0.25); }
```
Settings a skin may set: layout, now_panel, dial_size, dial_spin, dial_spin_seconds, theme, font_family, display_font, font_size, uppercase, brackets, bracket_padding, nav_marker, header_case, header_text, corner_radius, cover_size, list_cover_size, placeholder, ui_case, control_style, pill_buttons, progress_style, volume_style, progress_chars, glyphs, sidebar_width, ambient_background, ambient_strength, dial_visualizer, vis_style, vis_color, vis_sensitivity, background_alpha, skin_bg_opacity, skin_bg_blur.

`python make_skins.py` rebuilds the example skins in `skins/`.

## design presets

The built-in looks are in Settings → skins next to the skin files. Everything stays individually editable after applying one.

| preset | look |
|---|---|
| **nothing** (default) | studio layout, pure black, dot-matrix headings, dot progress bars, round play button, pill buttons |
| **nothing light** | same, black on off-white |
| **apple** | black, clean sans everywhere, thin line scrubber with knob |
| **terminal** | the original monospace `[ play ]` text look |

Fonts: body text uses SF Pro if installed, else bundled **Inter**. Headings use **Ndot 57/55** (Nothing's font) if you have it installed, else the bundled round-dot **Doto**. Both bundled fonts are OFL (licenses in `fonts/`).

## what it does

- formats: FLAC, ALAC, WAV, AIFF, WavPack, APE, DSF, OPUS, OGG, MP3, AAC
- album art from embedded tags or `cover/folder/front.jpg|png` next to files (cached + downscaled so huge embedded art doesn't lag)
- real quality per track: `FLAC 24/96`, `FLAC 16/44.1`, `MP3 320k`
- views: albums grid, artists, songs, queue, playlists, album page
- hover an album → play button on the cover; click the cover to open it
- click a song → plays that list from there (ctrl/shift-click selects instead; switch to double-click in settings → library)
- right-click anything → play next, add to queue, add to playlist, go to album/artist, open folder
- shuffle (keeps current track, restores order when off), repeat all / one, mute
- search title/artist/album/genre/year/format; multiple words = AND (`aphex 24`)
- remembers queue + position on exit
- incremental rescans
- now-playing text file for OBS (settings → player)

## customizing

Settings (`Ctrl+,`) open inside the window with the sections listed on the left. Everything applies instantly:

- themes: nothing, nothing light, mono, paper, midnight, amber, phosphor, nord, gruvbox, rose, catppuccin — plus per-color overrides
- body font, heading font, size, ui text case, column header case, uppercase mode
- corner radius, cover size, list thumb size, sidebar width, pill buttons on/off
- player controls: drawn icons or text glyphs (`>` `||` `<<` — or nerd font icons)
- progress / volume bar: dots, line, or text chars (`━─`, `█░`, `━─●`)
- song columns, album sort (artist / title / year / recently added)
- **window opacity** (restart): set below 100 for a see-through background; with `decoration { blur { enabled = true } }` in Hyprland you get frosted glass behind it while text stays fully opaque
- custom QSS for anything else (object names: `#rail #card #nowpanel #sidebar #npbar #search #nav #pill #pillPrimary #h1 #eyebrow #badge #time`)

Config: `~/.config/nexus/config.json` (Windows: `%APPDATA%\nexus`). `NEXUS_HOME=/some/dir` for portable mode.

## keys

| key | action |
|---|---|
| space | play / pause |
| ctrl ← / → | prev / next |
| ctrl ↑ / ↓ | volume (or scroll on the volume bar) |
| ctrl s / ctrl r | shuffle / repeat |
| ctrl f or `/` | search |
| esc | clear search |
| ctrl 1–5 | albums / browse / songs / queue / discover |
| ctrl j | jam (listen together) |
| alt ← | back |
| ctrl e | equalizer |
| ctrl b | visualizer (esc leaves immersive) |
| ctrl l | lyrics |
| ctrl d | download |
| ctrl i | inspect current song |
| ctrl t | edit tags of the current song |
| ctrl , | settings |

Click the right-hand time to toggle remaining time.

## notes

- Media keys work while focused; global media keys / waybar need MPRIS (not built in yet).

## icon

`icons/` has the Nexus icon at 16–512 px plus `nexus.ico`. `python make_icon.py` redraws them if you tweak the design.

## license

GPL-3.0 (see `LICENSE`) — Nexus uses [aioslsk](https://github.com/JurgenR/aioslsk), which is GPL-3.0. Bundled fonts (Inter, Doto) are under the SIL Open Font License (`fonts/`).
