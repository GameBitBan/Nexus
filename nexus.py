#!/usr/bin/env python3
"""
Nexus — a minimal black & white local lossless music player.

Plays FLAC / ALAC / WAV / AIFF / WavPack / APE / OPUS / OGG / MP3 / AAC from your
own folders. Spotify-style layout, Apple / Nothing inspired look (or a terminal look via presets),
album art everywhere, and almost every visual detail is editable live in settings (Ctrl+,).

deps:  pip install PySide6 mutagen
run:   python nexus.py
"""
import copy
import hashlib
import json
import math
import os
import random
import re
import shutil
import sys
from collections import OrderedDict
from pathlib import Path

import mutagen
from PySide6.QtCore import (QAbstractTableModel, QBuffer, QPoint, QByteArray, QEvent, QIODevice, QModelIndex, QObject, QPointF, QRect,
                            QRectF, QSize, QSortFilterProxyModel, Qt, QThread,
                            QTimer, QUrl, QVariantAnimation, Signal)
from PySide6.QtGui import (QColor, QDesktopServices, QFont, QFontDatabase,
                           QFontMetrics, QGuiApplication, QImage, QImageReader, QKeySequence, QLinearGradient,
                           QPainter, QPainterPath, QPen, QPixmap, QShortcut)
from PySide6.QtMultimedia import QAudioOutput, QMediaPlayer
from PySide6.QtWidgets import (QAbstractItemView, QApplication, QCheckBox,
                               QColorDialog, QComboBox, QDialog, QFileDialog,
                               QFontComboBox, QFormLayout, QFrame, QGridLayout, QLayout,
                               QHBoxLayout, QHeaderView, QInputDialog, QLabel,
                               QLineEdit, QListView, QListWidget,
                               QListWidgetItem, QMainWindow, QMenu, QMessageBox,
                               QPlainTextEdit, QPushButton, QScrollArea,
                               QSizePolicy, QSpinBox, QSplitter, QStackedWidget,
                               QStyle, QStyledItemDelegate, QStyleOption, QTableView, QTableWidget, QTableWidgetItem,
                               QTabWidget, QVBoxLayout, QWidget)

APP = "nexus"
APP_NAME = "Nexus"
OLD_APP = "monotone"
VERSION = "1.12.1"
GITHUB_REPO = ""            # "owner/repo" — Nexus checks its GitHub Releases for updates

AUDIO_EXT = {".flac", ".m4a", ".alac", ".wav", ".aiff", ".aif", ".wv", ".ape",
             ".dsf", ".opus", ".ogg", ".oga", ".mp3", ".mp4", ".aac"}
IMG_EXT = {".jpg", ".jpeg", ".png", ".webp", ".bmp"}
COVER_NAMES = {"cover", "folder", "front", "album", "albumart", "art", "albumartsmall"}

# ------------------------------------------------------------------ themes
COLOR_KEYS = ["bg", "panel", "text", "dim", "accent", "select", "border"]
THEMES = {
    "nothing":  dict(bg="#000000", panel="#0a0a0a", text="#ffffff", dim="#7d7d7d",
                     accent="#ffffff", select="#161616", border="#1c1c1c"),
    "nothing light": dict(bg="#f1f1f1", panel="#fafafa", text="#000000", dim="#8c8c8c",
                          accent="#000000", select="#e4e4e4", border="#dddddd"),
    "midnight": dict(bg="#0e0f13", panel="#14161c", text="#d7dae0", dim="#6b7080",
                     accent="#7aa2f7", select="#1d2130", border="#23262f"),
    "mono":     dict(bg="#000000", panel="#0a0a0a", text="#f2f2f2", dim="#6f6f6f",
                     accent="#ffffff", select="#1c1c1c", border="#1a1a1a"),
    "paper":    dict(bg="#f4f1ea", panel="#ebe6db", text="#2b2a28", dim="#8a857b",
                     accent="#c2410c", select="#e0d9ca", border="#d6cfc0"),
    "amber":    dict(bg="#0c0a05", panel="#110e06", text="#ffb000", dim="#7a5a10",
                     accent="#ffd26a", select="#2a2008", border="#241b07"),
    "phosphor": dict(bg="#050a05", panel="#081008", text="#33ff66", dim="#1d7a35",
                     accent="#b4ffc8", select="#0e2a14", border="#0f2a16"),
    "nord":     dict(bg="#2e3440", panel="#353c4a", text="#e5e9f0", dim="#7b8496",
                     accent="#88c0d0", select="#434c5e", border="#434c5e"),
    "gruvbox":  dict(bg="#1d2021", panel="#282828", text="#ebdbb2", dim="#928374",
                     accent="#fabd2f", select="#3c3836", border="#3c3836"),
    "rose":     dict(bg="#191724", panel="#1f1d2e", text="#e0def4", dim="#6e6a86",
                     accent="#ebbcba", select="#26233a", border="#26233a"),
    "catppuccin": dict(bg="#1e1e2e", panel="#181825", text="#cdd6f4", dim="#6c7086",
                       accent="#cba6f7", select="#313244", border="#313244"),
}

MONO_FONTS = ["JetBrainsMono Nerd Font", "JetBrains Mono", "Iosevka", "Iosevka Term",
              "Fira Code", "Cascadia Mono", "Cascadia Code", "Hack", "Source Code Pro",
              "DejaVu Sans Mono", "Consolas", "Menlo", "Monaco"]
UI_FONTS = ["SF Pro Text", "SF Pro Display", "Inter", "Helvetica Neue", "Segoe UI Variable Text",
            "Segoe UI", "Cantarell", "Noto Sans", "DejaVu Sans"]
DISPLAY_FONTS = ["Ndot 57", "Ndot 55", "NDot 57", "NType 82", "Doto"]

# a preset sets many keys at once; everything stays individually editable after
PRESETS = {
    "nothing": dict(theme="nothing", font_family="", display_font="", font_size=11,
                    uppercase=False, brackets="", nav_marker="", corner_radius=12,
                    cover_size=172, list_cover_size=36, progress_style="dots",
                    volume_style="dots", control_style="icons", pill_buttons=True,
                    placeholder="", glyphs={"now": "●"}, header_case="upper", ui_case="sentence",
                    layout="studio", now_panel=True),
    "nothing light": dict(theme="nothing light", font_family="", display_font="", font_size=11,
                          uppercase=False, brackets="", nav_marker="", corner_radius=12,
                          cover_size=172, list_cover_size=36, progress_style="dots",
                          volume_style="dots", control_style="icons", pill_buttons=True,
                          placeholder="", glyphs={"now": "●"}, header_case="upper", ui_case="sentence",
                    layout="studio", now_panel=True),
    "apple": dict(theme="mono", font_family="", display_font="Inter", font_size=11,
                  uppercase=False, brackets="", nav_marker="", corner_radius=10,
                  cover_size=180, list_cover_size=36, progress_style="line",
                  volume_style="line", control_style="icons", pill_buttons=True,
                  placeholder="♪", glyphs={"now": "♪"}, header_case="title", ui_case="sentence",
                  layout="studio", now_panel=True),
    "terminal": dict(theme="midnight", font_family="DejaVu Sans Mono", display_font="DejaVu Sans Mono",
                     font_size=11, uppercase=False, brackets="[]", nav_marker=">",
                     corner_radius=0, cover_size=160, list_cover_size=28, progress_style="text",
                     volume_style="text", control_style="text", pill_buttons=False,
                     placeholder="♪", glyphs={"now": "▶"}, header_case="lower", ui_case="lower",
                     layout="classic", now_panel=False),
}

COLS = OrderedDict([("#", "#"), ("title", "title"), ("artist", "artist"),
                    ("album", "album"), ("year", "year"), ("genre", "genre"),
                    ("duration", "time"), ("format", "format")])

PAGES = ("discover", "tags", "visualizer", "albums", "artists", "songs", "queue", "album", "playlist", "browse", "settings", "eq", "lyrics", "download",
         "inspect")

DEFAULTS = {
    "library_folders": [],
    "scan_on_startup": True,
    "preset": "nothing",
    "layout": "studio",                # studio (icon rail + columns + dial) | classic (sidebar)
    "now_panel": True,
    "dial_size": 290,
    "dial_spin": True,
    "dial_spin_seconds": 12,
    "theme": "nothing",
    "color_overrides": {},
    "font_family": "",                 # body font; empty = auto (SF Pro / Inter / Segoe…)
    "display_font": "",                # headings; empty = auto (Ndot if installed, else bundled Doto)
    "font_size": 11,
    "uppercase": False,
    "brackets": "",
    "bracket_padding": True,
    "nav_marker": "",
    "header_case": "upper",            # table headers: upper | lower | title
    "header_text": "nexus",
    "corner_radius": 12,
    "cover_size": 172,
    "list_cover_size": 36,
    "placeholder": "",
    "ui_case": "sentence",             # sentence | lower  (uppercase toggle overrides)
    "control_style": "icons",          # icons | text
    "pill_buttons": True,
    "progress_style": "dots",          # dots | line | text
    "volume_style": "dots",
    "progress_chars": "━─",            # text mode: filled, empty, [optional head]
    "background_alpha": 100,           # <100 = see-through window (hyprland blur), restart needed
    "glyphs": {"play": ">", "pause": "||", "prev": "<<", "next": ">>",
               "shuffle": "shuf", "repeat": "rep", "repeat_one": "rep1", "now": "●"},
    "song_columns": ["#", "title", "artist", "album", "year", "duration", "format"],
    "album_sort": "artist",            # artist | title | year | recent
    "start_view": "albums",
    "single_click_play": True,         # one click on a song plays it
    "cfg_version": 2,
    "show_remaining": False,
    "sidebar_width": 210,
    "now_playing_file": "",
    "now_playing_format": "{artist} - {title}",
    "volume": 0.8,
    "volume_max": 200,                 # % — above 100 boosts, the limiter stops distortion
    "audio_engine": "nexus",           # nexus (EQ, boost, gapless, exclusive) | qt (basic)
    "audio_device": "",                # empty = system default
    "exclusive_mode": False,           # windows WASAPI exclusive: bypasses the windows mixer
    "gapless": True,
    "replaygain": "off",               # off | track | album
    "replaygain_preamp": 0,
    "eq": {"on": False, "preset": "flat", "preamp": 0.0, "bands": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]},
    "update_url": "",                  # optional: url of a version.json (instead of GitHub Releases)
    "update_repo": "",                 # optional: "owner/repo" to take updates from (default: GITHUB_REPO)
    "update_check": True,              # look for a new version on launch
    "import_root": "",                 # where imported music goes (default: first library folder)
    "import_move": True,
    "watch_folder": "",                # e.g. your Downloads folder
    "watch_enabled": False,
    "slsk_user": "",
    "slsk_password": "",
    "slsk_server": "vps.slsknet.org",
    "slsk_server_port": 2271,
    "slsk_port": 2234,                 # listening port (upnp opens it on your router when it can)
    "slsk_upnp": True,
    "slsk_share_library": True,        # share your music folders back (most peers won't upload to non-sharers)
    "slsk_share_extra": "",            # more folders to share, separated by ;
    "slsk_upload_slots": 2,
    "slsk_download_dir": "",           # temp landing spot before import (default: inside Nexus's data folder)
    "dl_lossless_only": True,
    "dl_auto_switch": True,            # a download that hasn't started after a while moves to another user
    "dl_stall_minutes": 5,
    "ambient_background": True,        # blurred album-art glow behind the ui
    "ambient_strength": 45,
    "dial_visualizer": True,           # the record's dot ring dances to the music
    "vis_style": "dots",               # dots | bars | mirror | wave | ring
    "vis_color": "mono",               # mono | cover (colour picked from the album art)
    "vis_sensitivity": 100,
    "rename_pattern": "{track:02} {title}",
    "discord_enabled": False,
    "discord_client_id": "",           # your discord application id (its name shows as "Listening to <name>")
    "discord_song_in_status": True,    # show the song title in your status line instead of the app name
    "discord_hide_paused": True,
    "discord_cover_lookup": True,      # cover art on discord at all
    "discord_cover_source": "upload",  # upload = your own cover (exact) · online = match on itunes/deezer only
    "discord_large_image": "nexus",    # fallback art asset key uploaded to your discord app
    "discord_small_image": "nexus",
    "dl_search_seconds": 15,
    "shuffle": False,
    "repeat": "off",                   # off | all | one
    "remember_session": True,
    "custom_qss": "",
    "skin": "",                        # folder name of the active skin in <config>/skins ("" = built-in look)
    "skin_bg_opacity": 60,
    "skin_bg_blur": False,
    "skins_installed": [],
    "geometry": "",
    "session": {},
}


# ------------------------------------------------------------------ helpers
def resource_dir():
    """folder holding fonts/ and icons/ (works from source and from a pyinstaller build)."""
    if getattr(sys, "frozen", False):
        return Path(getattr(sys, "_MEIPASS", Path(sys.executable).parent))
    return Path(__file__).resolve().parent


def _app_base(name):
    if sys.platform.startswith("win"):
        return Path(os.environ.get("APPDATA", Path.home() / "AppData" / "Roaming")) / name
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / name
    return Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config")) / name


def app_dir():
    home = os.environ.get("NEXUS_HOME") or os.environ.get("MONOTONE_HOME")
    if home:
        base = Path(home)
    else:
        base = _app_base(APP)
        old = _app_base(OLD_APP)
        if not base.exists() and old.is_dir():          # carry over library, covers, playlists
            import shutil
            try:
                shutil.copytree(old, base)
            except OSError as e:
                print("couldn't migrate old config:", e)
    (base / "covers").mkdir(parents=True, exist_ok=True)
    return base


def load_json(path, default):
    try:
        return json.loads(Path(path).read_text("utf-8"))
    except Exception:
        return copy.deepcopy(default)


def save_json(path, data):
    path = Path(path)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(data, indent=1, ensure_ascii=False), "utf-8")
    tmp.replace(path)


def fmt_time(sec):
    sec = max(0, int(sec or 0))
    h, r = divmod(sec, 3600)
    m, s = divmod(r, 60)
    return f"{h}:{m:02}:{s:02}" if h else f"{m}:{s:02}"


def quality(t):
    if not t:
        return ""
    fmt = t.get("fmt", "")
    if t.get("bits") and t.get("sr"):
        return f"{fmt} {t['bits']}/{t['sr'] / 1000:g}"
    if t.get("bitrate"):
        return f"{fmt} {t['bitrate'] // 1000}k"
    return fmt


class Config(dict):
    def __init__(self, path):
        super().__init__(copy.deepcopy(DEFAULTS))
        self.path = path
        raw = load_json(path, {})
        self.fresh = not raw
        self.raw_keys = set(raw)
        for k, v in raw.items():
            if isinstance(v, dict) and isinstance(self.get(k), dict) and k not in ("session", "color_overrides"):
                self[k].update(v)
            else:
                self[k] = v

    def save(self):
        try:
            save_json(self.path, self)
        except OSError as e:
            print("config save failed:", e)


def theme_colors(cfg):
    c = dict(THEMES.get(cfg["theme"], THEMES["midnight"]))
    c.update({k: v for k, v in cfg.get("color_overrides", {}).items() if k in COLOR_KEYS})
    return c


_FONTS_LOADED = False


def load_bundled_fonts():
    global _FONTS_LOADED
    if _FONTS_LOADED:
        return
    _FONTS_LOADED = True
    here = resource_dir() / "fonts"
    if here.is_dir():
        for p in sorted(here.glob("*.[ot]tf")):
            QFontDatabase.addApplicationFont(str(p))


def resolve_font(cfg):
    """returns (body QFont, display family name)."""
    fams = set(QFontDatabase.families())
    fam = cfg.get("font_family") or ""
    if fam not in fams:
        fam = next((f for f in UI_FONTS if f in fams), "")
    if not fam:
        fam = QFontDatabase.systemFont(QFontDatabase.GeneralFont).family()
    f = QFont(fam)
    f.setPointSizeF(float(cfg.get("font_size", 11)))
    disp = cfg.get("display_font") or ""
    if disp not in fams:
        disp = next((d for d in DISPLAY_FONTS if d in fams), fam)
    return f, disp


# ------------------------------------------------------------------ tag reading
def _first(tags, key):
    try:
        v = tags.get(key)
    except Exception:
        return ""
    if not v:
        return ""
    if isinstance(v, (list, tuple)):
        v = v[0] if v else ""
    return str(v).strip()


def _int(s):
    try:
        return int(str(s).split("/")[0].strip() or 0)
    except ValueError:
        return 0


def detect_format(f):
    n = type(f).__name__.upper()
    if "FLAC" in n:
        return "FLAC"
    if "MP4" in n:
        return "ALAC" if "alac" in str(getattr(f.info, "codec", "")).lower() else "AAC"
    if "MP3" in n:
        return "MP3"
    if "OPUS" in n:
        return "OPUS"
    if "VORBIS" in n:
        return "OGG"
    for key, name in (("WAVPACK", "WV"), ("WAVE", "WAV"), ("AIFF", "AIFF"),
                      ("MONKEY", "APE"), ("DSF", "DSF"), ("AAC", "AAC")):
        if key in n:
            return name
    return n[:5]


_NON_EASY = {"WAVE", "AIFF", "DSF", "DSDIFF", "WavPack", "MonkeysAudio", "Musepack", "OptimFROG", "TrueAudio", "TAK"}


def read_track(path):
    try:
        f = mutagen.File(path, easy=True)
    except Exception:
        return None
    if f is None:
        return None
    tags = f.tags if f.tags is not None else {}
    if type(f).__name__ in _NON_EASY:          # wav/aiff/dsf (id3) and wavpack/ape (apev2): use the full reader
        try:
            rt = read_tags(path, folder=False)
            tags = {k: [rt[k].split(";")[0].strip()] for k in rt if not k.startswith("_") and rt[k]}
        except Exception:
            pass
    info = f.info
    p = Path(path)
    artist = _first(tags, "artist") or "Unknown Artist"
    album = _first(tags, "album") or p.parent.name
    aa = _first(tags, "albumartist") or artist
    year = (_first(tags, "date") or _first(tags, "year"))[:4]
    key = hashlib.md5(f"{aa}|{album}".lower().encode()).hexdigest()[:16]
    return dict(
        path=str(path), title=_first(tags, "title") or p.stem, artist=artist,
        album=album, albumartist=aa, year=year, genre=_first(tags, "genre"),
        track=_int(_first(tags, "tracknumber")), disc=_int(_first(tags, "discnumber")) or 1,
        duration=float(getattr(info, "length", 0) or 0),
        sr=int(getattr(info, "sample_rate", 0) or 0),
        bits=int(getattr(info, "bits_per_sample", 0) or 0),
        channels=int(getattr(info, "channels", 0) or 0),
        bitrate=int(getattr(info, "bitrate", 0) or 0),
        fmt=detect_format(f), album_key=key, cover="",
    )


def cover_bytes(path):
    """embedded art first, then cover.jpg/folder.png/etc next to the file."""
    data = None
    try:
        f = mutagen.File(path)
    except Exception:
        f = None
    if f is not None:
        pics = getattr(f, "pictures", None)          # FLAC
        if pics:
            front = [p for p in pics if p.type == 3]
            data = (front or pics)[0].data
        elif f.tags is not None:
            tags = f.tags
            try:
                apic = tags.getall("APIC")           # ID3
                if apic:
                    data = apic[0].data
            except AttributeError:
                pass
            try:
                if data is None and "covr" in tags:  # MP4
                    data = bytes(tags["covr"][0])
                if data is None and "metadata_block_picture" in tags:  # OGG/OPUS
                    import base64
                    from mutagen.flac import Picture
                    data = Picture(base64.b64decode(tags["metadata_block_picture"][0])).data
            except Exception:
                pass
    if data:
        return data
    try:
        for p in Path(path).parent.iterdir():
            if p.suffix.lower() in IMG_EXT and p.stem.lower() in COVER_NAMES:
                return p.read_bytes()
    except OSError:
        pass
    return None


def save_cover(path, dest):
    data = cover_bytes(path)
    if not data:
        return False
    img = QImage()
    if not img.loadFromData(data):
        return False
    if max(img.width(), img.height()) > 800:
        img = img.scaled(800, 800, Qt.KeepAspectRatio, Qt.SmoothTransformation)
    return img.save(str(dest), "JPG", 90)


def track_sort_key(t):
    return (t["albumartist"].lower(), t["album"].lower(), t["disc"], t["track"], t["title"].lower())


# ------------------------------------------------------------------ scanner
class Scanner(QThread):
    progress = Signal(int, int)
    done = Signal(object)

    def __init__(self, folders, cache, cover_dir, full=False):
        super().__init__()
        self.folders, self.cache, self.cover_dir, self.full = folders, cache, cover_dir, full

    def run(self):
        files, seen = [], set()
        for folder in self.folders:
            for root, _dirs, names in os.walk(folder, followlinks=True):
                for n in names:
                    if os.path.splitext(n)[1].lower() in AUDIO_EXT:
                        p = os.path.join(root, n)
                        rp = os.path.realpath(p)
                        if rp not in seen:
                            seen.add(rp)
                            files.append(p)
        old = {} if self.full else {t["path"]: t for t in self.cache}
        out, have_cover, total = [], set(), len(files)
        for i, p in enumerate(files):
            if self.isInterruptionRequested():
                return
            try:
                mtime = os.path.getmtime(p)
            except OSError:
                continue
            t = old.get(p)
            if not t or t.get("mtime") != mtime:
                t = read_track(p)
                if not t:
                    continue
                t["mtime"] = mtime
            key = t["album_key"]
            if key not in have_cover:
                cp = self.cover_dir / f"{key}.jpg"
                if (cp.exists() and not self.full) or save_cover(p, cp):
                    have_cover.add(key)
            out.append(t)
            if i % 20 == 0:
                self.progress.emit(i, total)
        for t in out:
            t["cover"] = t["album_key"] if t["album_key"] in have_cover else ""
        out.sort(key=track_sort_key)
        self.done.emit(out)


# ------------------------------------------------------------------ covers
class Covers:
    """scaled, rounded, hi-dpi cover pixmaps with a bounded LRU cache."""

    def __init__(self, win, limit=700):
        self.win, self.limit = win, limit
        self.cache = OrderedDict()

    def clear(self):
        self.cache.clear()

    def dpr(self):
        s = QGuiApplication.primaryScreen()
        return s.devicePixelRatio() if s else 1.0

    def radius(self, size):
        return min(float(self.win.cfg["corner_radius"]), size / 5)

    def get(self, key, size):
        size = int(size)
        k = (key, size)
        if k in self.cache:
            self.cache.move_to_end(k)
            return self.cache[k]
        pm = self._load(key, size) if key else None
        if pm is None:
            pm = self._placeholder(size)
        self.cache[k] = pm
        if len(self.cache) > self.limit:
            self.cache.popitem(last=False)
        return pm

    def _load(self, key, size):
        path = self.win.dir / "covers" / f"{key}.jpg"
        if not path.exists():
            return None
        dpr = self.dpr()
        target = max(1, int(size * dpr))
        r = QImageReader(str(path))
        sz = r.size()
        if sz.isValid() and sz.width() and sz.height():
            sc = max(target / sz.width(), target / sz.height())
            if sc < 1:
                r.setScaledSize(QSize(math.ceil(sz.width() * sc), math.ceil(sz.height() * sc)))
        img = r.read()
        if img.isNull():
            return None
        img = img.scaled(target, target, Qt.KeepAspectRatioByExpanding, Qt.SmoothTransformation)
        img = img.copy((img.width() - target) // 2, (img.height() - target) // 2, target, target)
        pm = QPixmap.fromImage(img)
        rad = self.radius(size) * dpr
        if rad > 0:
            out = QPixmap(target, target)
            out.fill(Qt.transparent)
            p = QPainter(out)
            p.setRenderHint(QPainter.Antialiasing)
            path_ = QPainterPath()
            path_.addRoundedRect(QRectF(0, 0, target, target), rad, rad)
            p.setClipPath(path_)
            p.drawPixmap(0, 0, pm)
            p.end()
            pm = out
        pm.setDevicePixelRatio(dpr)
        return pm

    def _placeholder(self, size):
        c, dpr = self.win.c, self.dpr()
        pm = QPixmap(max(1, int(size * dpr)), max(1, int(size * dpr)))
        pm.setDevicePixelRatio(dpr)
        pm.fill(Qt.transparent)
        p = QPainter(pm)
        p.setRenderHint(QPainter.Antialiasing)
        rect = QRectF(0.5, 0.5, size - 1, size - 1)
        path_ = QPainterPath()
        rad = self.radius(size)
        path_.addRoundedRect(rect, rad, rad)
        p.fillPath(path_, QColor(c["panel"]))
        if size >= 48:                       # dot-matrix texture
            p.save()
            p.setClipPath(path_)
            p.setPen(Qt.NoPen)
            dot = QColor(c["dim"])
            dot.setAlphaF(0.18)
            p.setBrush(dot)
            step = max(6.0, size / 16)
            rr = max(0.7, step / 9)
            y = step / 2
            while y < size:
                x = step / 2
                while x < size:
                    p.drawEllipse(QRectF(x - rr, y - rr, rr * 2, rr * 2))
                    x += step
                y += step
            p.restore()
        p.setPen(QColor(c["border"]))
        p.drawPath(path_)
        f = QFont(QApplication.font())
        f.setPixelSize(max(6, int(size * 0.3)))
        p.setFont(f)
        p.setPen(QColor(c["dim"]))
        p.drawText(rect, Qt.AlignCenter, self.win.cfg.get("placeholder", "♪"))
        p.end()
        return pm


# ------------------------------------------------------------------ widgets
class ClickLabel(QLabel):
    clicked = Signal()

    def mousePressEvent(self, e):
        if e.button() == Qt.LeftButton:
            self.clicked.emit()
        super().mousePressEvent(e)


class TextBar(QWidget):
    """progress / volume bar: dot-matrix, apple-style line with knob, or characters."""
    changed = Signal(float)
    released = Signal(float)

    def __init__(self, wheel=False, key="progress_style"):
        super().__init__()
        self.v, self.dragging, self.wheel, self.key = 0.0, False, wheel, key
        self.mode, self.chars, self.hover = "text", "━─", False
        self.fg, self.bg = QColor("#fff"), QColor("#555")
        self.setCursor(Qt.PointingHandCursor)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.setAttribute(Qt.WA_Hover, True)

    def sizeHint(self):
        return QSize(200, max(18, self.fontMetrics().height() + 6))

    def minimumSizeHint(self):
        return QSize(40, max(18, self.fontMetrics().height() + 6))

    def restyle(self, cfg, c):
        self.mode = cfg.get(self.key, "text")
        self.chars = cfg.get("progress_chars") or "━─"
        self.fg = QColor(c["accent"])
        self.bg = QColor(c["dim"])
        if self.mode != "text":
            self.bg.setAlphaF(0.35)
        self.updateGeometry()
        self.update()

    def set_value(self, v):
        if not self.dragging:
            self.v = min(1.0, max(0.0, v))
            self.update()

    def enterEvent(self, e):
        self.hover = True
        self.update()
        super().enterEvent(e)

    def leaveEvent(self, e):
        self.hover = False
        self.update()
        super().leaveEvent(e)

    def paintEvent(self, _e):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        w, h = self.width(), self.height()
        active = self.hover or self.dragging
        if self.mode == "line":
            th = 4 if active else 3
            knob = 6 if active else 0
            x0, x1 = knob, w - knob
            y = h / 2
            span = max(1, x1 - x0)
            p.setPen(Qt.NoPen)
            p.setBrush(self.bg)
            p.drawRoundedRect(QRectF(x0, y - th / 2, span, th), th / 2, th / 2)
            p.setBrush(self.fg)
            p.drawRoundedRect(QRectF(x0, y - th / 2, span * self.v, th), th / 2, th / 2)
            if knob:
                p.drawEllipse(QRectF(x0 + span * self.v - knob, y - knob, knob * 2, knob * 2))
            return
        if self.mode == "dots":
            d = 3.2 if not active else 3.8
            gap = 3.4
            n = max(1, int((w + gap) // (d + gap)))
            filled = int(round(n * self.v))
            x = (w - (n * d + (n - 1) * gap)) / 2
            y = h / 2 - d / 2
            p.setPen(Qt.NoPen)
            for i in range(n):
                p.setBrush(self.fg if i < filled else self.bg)
                p.drawEllipse(QRectF(x + i * (d + gap), y, d, d))
            return
        fm = self.fontMetrics()
        fc = self.chars[0]
        ec = self.chars[1] if len(self.chars) > 1 else " "
        hc = self.chars[2] if len(self.chars) > 2 else None
        cw = max(fm.horizontalAdvance(fc), fm.horizontalAdvance(ec), 1)
        n = max(1, w // cw)
        filled = int(round(n * self.v))
        s1 = fc * filled
        if hc and filled > 0:
            s1 = fc * (filled - 1) + hc
        s2 = ec * (n - filled)
        base = (h + fm.ascent() - fm.descent()) // 2
        p.setPen(self.fg)
        p.drawText(0, base, s1)
        p.setPen(self.bg)
        p.drawText(fm.horizontalAdvance(s1), base, s2)

    def _set_from(self, x):
        self.v = min(1.0, max(0.0, x / max(1, self.width())))
        self.update()
        self.changed.emit(self.v)

    def mousePressEvent(self, e):
        if e.button() == Qt.LeftButton:
            self.dragging = True
            self._set_from(e.position().x())

    def mouseMoveEvent(self, e):
        if self.dragging:
            self._set_from(e.position().x())

    def mouseReleaseEvent(self, _e):
        if self.dragging:
            self.dragging = False
            self.released.emit(self.v)

    def wheelEvent(self, e):
        if not self.wheel:
            e.ignore()
            return
        self.v = min(1.0, max(0.0, self.v + e.angleDelta().y() / 120 * 0.05))
        self.update()
        self.changed.emit(self.v)
        self.released.emit(self.v)


class IconButton(QPushButton):
    """transport button. icons mode: vector icons drawn with QPainter (apple music style).
    text mode: draws the configured glyph with brackets, like the terminal look."""

    def __init__(self, win, kind, size=30):
        super().__init__()
        self.win, self.kind, self.base = win, kind, size
        self.active = False
        self.rail = False      # rounded-square tile, filled when active (icon rail)
        self.round = False     # circular chip (home button)
        self.setCursor(Qt.PointingHandCursor)
        self.setFocusPolicy(Qt.NoFocus)
        self.setAttribute(Qt.WA_Hover, True)
        self.setFlat(True)
        self.relayout()

    def text_label(self):
        g = self.win.cfg["glyphs"]
        k = {"play": "play", "pause": "pause", "prev": "prev", "next": "next",
             "shuffle": "shuffle", "repeat": "repeat", "repeat_one": "repeat_one"}.get(self.kind)
        if k:
            return self.win.B(g.get(k, k))
        return self.win.B({"back": "<", "volume": "vol", "mute": "mute", "columns": "browse", "grid": "albums",
                           "list": "songs", "plus": "+", "search": "/", "gear": "settings",
                           "refresh": "rescan", "panel": "panel", "eq": "eq", "lyrics": "lyrics", "download": "download",
                           "vis": "vis", "discover": "discover"}.get(self.kind, self.kind))

    def relayout(self):
        if self.win.cfg.get("control_style") == "text":
            fm = self.fontMetrics()
            self.setFixedSize(fm.horizontalAdvance(self.text_label()) + 10, fm.height() + 10)
        else:
            self.setFixedSize(self.base, self.base)
        self.update()

    def set_kind(self, kind):
        if kind != self.kind:
            self.kind = kind
            self.relayout()

    EXTRA = {"columns", "grid", "list", "queue", "plus", "home", "search", "gear", "refresh", "panel", "eq", "lyrics",
             "download", "vis", "discover"}

    def draw_extra(self, p, cx, cy, s, col):
        pen = QPen(col, max(1.6, s / 17), Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin)
        p.setPen(pen)
        p.setBrush(Qt.NoBrush)
        k = self.kind
        if k == "columns":
            cw, ch, gap = s * 0.13, s * 0.42, s * 0.055
            x0 = cx - (cw * 3 + gap * 2) / 2
            for i in range(3):
                p.drawRoundedRect(QRectF(x0 + i * (cw + gap), cy - ch / 2, cw, ch), 1.5, 1.5)
        elif k == "grid":
            q, gap = s * 0.16, s * 0.07
            for dx in (-1, 1):
                for dy in (-1, 1):
                    x = cx + (gap / 2 if dx > 0 else -gap / 2 - q)
                    y = cy + (gap / 2 if dy > 0 else -gap / 2 - q)
                    p.drawRoundedRect(QRectF(x, y, q, q), 1.8, 1.8)
        elif k in ("list", "queue"):
            for i, dy in enumerate((-0.15, 0, 0.15)):
                y = cy + s * dy
                x1 = cx + s * (0.24 if k == "list" or i < 2 else 0.02)
                p.drawLine(QPointF(cx - s * 0.12, y), QPointF(x1, y))
                if k == "list":
                    p.drawPoint(QPointF(cx - s * 0.22, y))
            if k == "queue":
                p.drawLine(QPointF(cx - s * 0.24, cy - s * 0.15), QPointF(cx - s * 0.2, cy - s * 0.15))
                p.drawLine(QPointF(cx - s * 0.24, cy), QPointF(cx - s * 0.2, cy))
                p.drawLine(QPointF(cx - s * 0.24, cy + s * 0.15), QPointF(cx - s * 0.2, cy + s * 0.15))
                tri = QPainterPath()
                tri.moveTo(cx + s * 0.1, cy + s * 0.07)
                tri.lineTo(cx + s * 0.1, cy + s * 0.23)
                tri.lineTo(cx + s * 0.24, cy + s * 0.15)
                tri.closeSubpath()
                p.setBrush(col)
                p.drawPath(tri)
        elif k == "plus":
            p.drawLine(QPointF(cx - s * 0.17, cy), QPointF(cx + s * 0.17, cy))
            p.drawLine(QPointF(cx, cy - s * 0.17), QPointF(cx, cy + s * 0.17))
        elif k == "home":
            path_ = QPainterPath()
            path_.moveTo(cx - s * 0.21, cy - s * 0.01)
            path_.lineTo(cx, cy - s * 0.21)
            path_.lineTo(cx + s * 0.21, cy - s * 0.01)
            path_.moveTo(cx - s * 0.15, cy - s * 0.07)
            path_.lineTo(cx - s * 0.15, cy + s * 0.2)
            path_.lineTo(cx + s * 0.15, cy + s * 0.2)
            path_.lineTo(cx + s * 0.15, cy - s * 0.07)
            p.drawPath(path_)
        elif k == "search":
            r = s * 0.14
            p.drawEllipse(QRectF(cx - s * 0.04 - r, cy - s * 0.04 - r, r * 2, r * 2))
            p.drawLine(QPointF(cx + s * 0.07, cy + s * 0.07), QPointF(cx + s * 0.2, cy + s * 0.2))
        elif k == "gear":
            r = s * 0.13
            p.drawEllipse(QRectF(cx - r, cy - r, r * 2, r * 2))
            for i in range(8):
                a = i * math.pi / 4
                p.drawLine(QPointF(cx + math.cos(a) * s * 0.17, cy + math.sin(a) * s * 0.17),
                           QPointF(cx + math.cos(a) * s * 0.23, cy + math.sin(a) * s * 0.23))
            d = s * 0.035
            p.drawEllipse(QRectF(cx - d, cy - d, d * 2, d * 2))
        elif k == "refresh":
            r = s * 0.18
            p.drawArc(QRectF(cx - r, cy - r, r * 2, r * 2), 60 * 16, 290 * 16)
            ax, ay = cx + r * math.cos(math.radians(60)), cy - r * math.sin(math.radians(60))
            ah = s * 0.08
            p.drawLine(QPointF(ax, ay), QPointF(ax - ah, ay - ah * 0.2))
            p.drawLine(QPointF(ax, ay), QPointF(ax + ah * 0.1, ay - ah))
        elif k == "download":
            p.drawLine(QPointF(cx, cy - s * 0.2), QPointF(cx, cy + s * 0.07))
            p.drawLine(QPointF(cx - s * 0.1, cy - s * 0.03), QPointF(cx, cy + s * 0.07))
            p.drawLine(QPointF(cx + s * 0.1, cy - s * 0.03), QPointF(cx, cy + s * 0.07))
            tray = QPainterPath()
            tray.moveTo(cx - s * 0.2, cy + s * 0.05)
            tray.lineTo(cx - s * 0.2, cy + s * 0.18)
            tray.lineTo(cx + s * 0.2, cy + s * 0.18)
            tray.lineTo(cx + s * 0.2, cy + s * 0.05)
            p.drawPath(tray)
        elif k == "lyrics":
            bub = QPainterPath()
            bub.addRoundedRect(QRectF(cx - s * 0.22, cy - s * 0.19, s * 0.44, s * 0.32), s * 0.08, s * 0.08)
            bub.moveTo(cx - s * 0.1, cy + s * 0.13)
            bub.lineTo(cx - s * 0.16, cy + s * 0.23)
            bub.lineTo(cx + s * 0.0, cy + s * 0.13)
            p.drawPath(bub)
            p.drawLine(QPointF(cx - s * 0.12, cy - s * 0.07), QPointF(cx + s * 0.12, cy - s * 0.07))
            p.drawLine(QPointF(cx - s * 0.12, cy + s * 0.02), QPointF(cx + s * 0.05, cy + s * 0.02))
        elif k == "eq":
            for i, (dx, ky) in enumerate(((-0.15, 0.06), (0, -0.09), (0.15, 0.03))):
                x = cx + s * dx
                p.drawLine(QPointF(x, cy - s * 0.2), QPointF(x, cy + s * 0.2))
                p.setBrush(QColor(self.win.c["panel"]) if not self.rail else Qt.NoBrush)
                p.drawEllipse(QRectF(x - s * 0.05, cy + s * ky - s * 0.05, s * 0.1, s * 0.1))
        elif k == "discover":                          # compass
            p.drawEllipse(QPointF(cx, cy), s * 0.22, s * 0.22)
            needle = QPainterPath()
            needle.moveTo(cx + s * 0.1, cy - s * 0.1)
            needle.lineTo(cx + s * 0.035, cy + s * 0.035)
            needle.lineTo(cx - s * 0.1, cy + s * 0.1)
            needle.lineTo(cx - s * 0.035, cy - s * 0.035)
            needle.closeSubpath()
            p.setBrush(col)
            p.drawPath(needle)
            p.setBrush(Qt.NoBrush)
        elif k == "vis":
            for dx, hh in ((-0.18, 0.16), (-0.06, 0.34), (0.06, 0.24), (0.18, 0.4)):
                x = cx + s * dx
                p.drawLine(QPointF(x, cy + s * 0.2), QPointF(x, cy + s * 0.2 - s * hh))
        elif k == "panel":
            rw, rh = s * 0.46, s * 0.36
            rect = QRectF(cx - rw / 2, cy - rh / 2, rw, rh)
            p.drawRoundedRect(rect, 2.5, 2.5)
            x = rect.right() - rw * 0.34
            p.drawLine(QPointF(x, rect.top()), QPointF(x, rect.bottom()))
        if self.active and not self.rail and k in ("queue", "panel", "eq", "lyrics", "vis"):
            p.setPen(Qt.NoPen)
            p.setBrush(col)
            p.drawEllipse(QRectF(cx - 1.6, self.height() - 4.5, 3.2, 3.2))

    def paintEvent(self, _e):
        c = self.win.c
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        hover = self.underMouse()
        fg, bg, dim = QColor(c["text"]), QColor(c["bg"]), QColor(c["dim"])
        if not self.isEnabled():
            fg = QColor(c["border"])
            hover = False
        toggle = self.kind in ("shuffle", "repeat", "repeat_one", "mute")
        col = fg if (not toggle or self.active) else dim
        if hover and toggle and not self.active:
            col = fg
        if self.win.cfg.get("control_style") == "text":
            if hover or (toggle and self.active):
                col = QColor(c["accent"])
            p.setPen(col)
            p.drawText(self.rect(), Qt.AlignCenter, self.text_label())
            return
        w, h = self.width(), self.height()
        s = min(w, h)
        cx, cy = w / 2, h / 2
        p.setPen(Qt.NoPen)

        if self.rail or self.round:
            tile = QPainterPath()
            if self.rail:
                rad = min(12.0, float(self.win.cfg["corner_radius"]))
                tile.addRoundedRect(QRectF(0, 0, w, h), rad, rad)
            else:
                tile.addEllipse(QRectF(0.5, 0.5, w - 1, h - 1))
            if self.rail and self.active:
                p.fillPath(tile, fg)
                col = QColor(bg)
            else:
                if hover or self.round:
                    p.fillPath(tile, QColor(c["select"] if hover else c["panel"]))
                col = fg if (hover or self.round or self.active) else dim
            s = s * 0.8
        elif self.kind in self.EXTRA:
            col = fg if (hover or self.active) else dim
        if self.kind in self.EXTRA:
            self.draw_extra(p, cx, cy, s, col)
            return

        if self.kind in ("play", "pause"):
            scale = 1.0 if not self.isDown() else 0.94
            r = s / 2 * scale - 0.5
            p.setBrush(fg if not hover else fg.darker(110) if fg.lightness() > 128 else fg.lighter(140))
            p.drawEllipse(QRectF(cx - r, cy - r, r * 2, r * 2))
            p.setBrush(bg)
            if self.kind == "play":
                path_ = QPainterPath()
                path_.moveTo(cx - s * 0.11, cy - s * 0.17)
                path_.lineTo(cx - s * 0.11, cy + s * 0.17)
                path_.lineTo(cx + s * 0.19, cy)
                path_.closeSubpath()
                p.drawPath(path_)
            else:
                bw, bh = s * 0.085, s * 0.32
                for dx in (-s * 0.085, s * 0.085):
                    p.drawRoundedRect(QRectF(cx + dx - bw / 2, cy - bh / 2, bw, bh), 1.5, 1.5)
            return

        if not hover and self.kind in ("prev", "next", "back"):
            col = QColor(fg)
            col.setAlphaF(0.85)
        p.setBrush(col)
        if self.kind in ("prev", "next"):
            d = -1 if self.kind == "prev" else 1
            th, tw = s * 0.30, s * 0.20
            for off in (-tw / 2 - 1, tw / 2 + 1):
                path_ = QPainterPath()
                bx = cx + off * d
                path_.moveTo(bx - tw / 2 * d, cy - th / 2)
                path_.lineTo(bx - tw / 2 * d, cy + th / 2)
                path_.lineTo(bx + tw / 2 * d, cy)
                path_.closeSubpath()
                p.drawPath(path_)
            return

        stroke = max(1.6, s / 16)
        pen = QPen(col, stroke, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin)
        p.setPen(pen)
        p.setBrush(Qt.NoBrush)

        if self.kind == "back":
            p.drawLine(QPointF(cx + s * 0.06, cy - s * 0.16), QPointF(cx - s * 0.08, cy))
            p.drawLine(QPointF(cx - s * 0.08, cy), QPointF(cx + s * 0.06, cy + s * 0.16))
            return

        if self.kind == "shuffle":
            l, r, t, b = cx - s * 0.24, cx + s * 0.24, cy - s * 0.15, cy + s * 0.15
            a = QPainterPath()
            a.moveTo(l, b)
            a.cubicTo(cx, b, cx, t, r, t)
            p.drawPath(a)
            bpath = QPainterPath()
            bpath.moveTo(l, t)
            bpath.cubicTo(cx, t, cx, b, r, b)
            p.drawPath(bpath)
            ah = s * 0.07
            for yy in (t, b):
                p.drawLine(QPointF(r, yy), QPointF(r - ah, yy - ah))
                p.drawLine(QPointF(r, yy), QPointF(r - ah, yy + ah))
        elif self.kind in ("repeat", "repeat_one"):
            rw, rh = s * 0.44, s * 0.28
            rect = QRectF(cx - rw / 2, cy - rh / 2, rw, rh)
            path_ = QPainterPath()
            path_.moveTo(rect.left() + rw * 0.35, rect.bottom())
            path_.lineTo(rect.left() + rh / 2, rect.bottom())
            path_.arcTo(QRectF(rect.left(), rect.top(), rh, rh), 270, -180)
            path_.lineTo(rect.right() - rh / 2, rect.top())
            path_.arcTo(QRectF(rect.right() - rh, rect.top(), rh, rh), 90, -180)
            path_.lineTo(rect.right() - rw * 0.35, rect.bottom())
            p.drawPath(path_)
            ah = s * 0.065
            ax, ay = rect.left() + rw * 0.35, rect.bottom()
            p.drawLine(QPointF(ax, ay), QPointF(ax - ah, ay - ah))
            p.drawLine(QPointF(ax, ay), QPointF(ax - ah, ay + ah))
            bx, by = rect.right() - rw * 0.35, rect.top()
            p.drawLine(QPointF(bx, by), QPointF(bx + ah, by - ah))
            p.drawLine(QPointF(bx, by), QPointF(bx + ah, by + ah))
            if self.kind == "repeat_one":
                f = QFont(self.font())
                f.setPixelSize(max(7, int(s * 0.24)))
                f.setBold(True)
                p.setFont(f)
                p.drawText(QRectF(cx - s * 0.12, cy - s * 0.13, s * 0.24, s * 0.26), Qt.AlignCenter, "1")
        elif self.kind in ("volume", "mute"):
            p.setPen(Qt.NoPen)
            p.setBrush(col)
            spk = QPainterPath()
            x0 = cx - s * 0.22
            spk.moveTo(x0, cy - s * 0.07)
            spk.lineTo(x0 + s * 0.09, cy - s * 0.07)
            spk.lineTo(x0 + s * 0.2, cy - s * 0.17)
            spk.lineTo(x0 + s * 0.2, cy + s * 0.17)
            spk.lineTo(x0 + s * 0.09, cy + s * 0.07)
            spk.lineTo(x0, cy + s * 0.07)
            spk.closeSubpath()
            p.drawPath(spk)
            p.setPen(pen)
            p.setBrush(Qt.NoBrush)
            if self.kind == "mute":
                xm = cx + s * 0.12
                p.drawLine(QPointF(xm - s * 0.07, cy - s * 0.07), QPointF(xm + s * 0.07, cy + s * 0.07))
                p.drawLine(QPointF(xm - s * 0.07, cy + s * 0.07), QPointF(xm + s * 0.07, cy - s * 0.07))
            else:
                vol = float(self.win.cfg.get("volume", 1))
                for i, rr in enumerate((0.11, 0.19)):
                    if vol > (0.0 if i == 0 else 0.5):
                        p.drawArc(QRectF(cx - s * 0.02 - rr * s, cy - rr * s, rr * s * 2, rr * s * 2),
                                  -45 * 16, 90 * 16)
        if toggle and self.active and self.kind != "mute":
            p.setPen(Qt.NoPen)
            p.setBrush(col)
            p.drawEllipse(QRectF(cx - 1.6, h - 4.5, 3.2, 3.2))


# ------------------------------------------------------------------ songs table
class SongModel(QAbstractTableModel):
    def __init__(self, win, mode):
        super().__init__()
        self.win, self.mode = win, mode
        self.tracks, self.cols, self.thumbs, self.plain = [], [], True, False

    def set_tracks(self, tracks, cols):
        self.beginResetModel()
        self.tracks, self.cols = list(tracks), list(cols)
        self.endResetModel()

    def rowCount(self, parent=QModelIndex()):
        return 0 if parent.isValid() else len(self.tracks)

    def columnCount(self, parent=QModelIndex()):
        return 0 if parent.isValid() else len(self.cols)

    def headerData(self, section, orient, role=Qt.DisplayRole):
        if orient == Qt.Horizontal and role == Qt.DisplayRole and section < len(self.cols):
            name = COLS[self.cols[section]]
            case = self.win.cfg.get("header_case", "lower")
            if self.win.cfg["uppercase"] or case == "upper":
                return name.upper()
            return name.title() if case == "title" else name
        if orient == Qt.Horizontal and role == Qt.TextAlignmentRole and section < len(self.cols):
            if self.cols[section] in ("#", "duration", "year"):
                return int(Qt.AlignRight | Qt.AlignVCenter)
            return int(Qt.AlignLeft | Qt.AlignVCenter)
        return None

    def playing(self, row):
        if self.mode == "queue":
            return row == self.win.qi
        return self.tracks[row]["path"] == self.win.current_path

    def data(self, index, role=Qt.DisplayRole):
        if not index.isValid():
            return None
        t = self.tracks[index.row()]
        c = self.cols[index.column()]
        if role == Qt.DisplayRole:
            if c == "#":
                if self.playing(index.row()):
                    return self.win.cfg["glyphs"].get("now", "▶")
                if self.mode == "queue":
                    return str(index.row() + 1)
                return str(t["track"]) if t["track"] else ""
            if c == "duration":
                return fmt_time(t["duration"])
            if c == "format":
                return quality(t)
            return t.get(c, "")
        if role == Qt.UserRole:
            if c == "#":
                return index.row() if self.mode == "queue" else t["disc"] * 1000 + t["track"]
            if c == "duration":
                return t["duration"]
            if c == "format":
                return (t["fmt"], t["bits"], t["sr"])
            return str(t.get(c, "")).lower()
        if role == Qt.DecorationRole and c == "title" and self.thumbs:
            s = int(self.win.cfg["list_cover_size"])
            return self.win.covers.get(t["cover"], s) if s > 0 else None
        if role == Qt.FontRole and self.plain and self.playing(index.row()):
            f = QFont()
            f.setBold(True)
            return f
        if role == Qt.ForegroundRole and not self.plain:
            if self.playing(index.row()):
                return QColor(self.win.c["accent"])
            if c in ("#", "duration", "format", "year", "genre"):
                return QColor(self.win.c["dim"])
        if role == Qt.TextAlignmentRole:
            if c in ("#", "duration", "year"):
                return int(Qt.AlignRight | Qt.AlignVCenter)
            return int(Qt.AlignLeft | Qt.AlignVCenter)
        if role == Qt.ToolTipRole:
            return f"{t['path']}\n{quality(t)}  {t['channels']}ch"
        return None


class SongProxy(QSortFilterProxyModel):
    def __init__(self):
        super().__init__()
        self.words = []
        self.setSortRole(Qt.UserRole)

    def set_query(self, q):
        self.words = q.lower().split()
        self.invalidateFilter()

    def filterAcceptsRow(self, row, parent):
        if not self.words:
            return True
        t = self.sourceModel().tracks[row]
        blob = t.get("_blob")
        if blob is None:
            blob = t["_blob"] = " ".join(str(t.get(k, "")) for k in (
                "title", "artist", "album", "albumartist", "genre", "year", "fmt")).lower()
        return all(w in blob for w in self.words)


class SongTable(QTableView):
    def __init__(self, win, mode="list", hide=(), thumbs=True):
        super().__init__()
        self.win, self.mode, self.hide = win, mode, set(hide)
        self.model_ = SongModel(win, mode)
        self.model_.thumbs = thumbs
        self.proxy = SongProxy()
        self.proxy.setSourceModel(self.model_)
        self.setModel(self.proxy)
        self.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.setShowGrid(False)
        self.setWordWrap(False)
        self.setVerticalScrollMode(QAbstractItemView.ScrollPerPixel)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.verticalHeader().hide()
        self.verticalHeader().setSectionResizeMode(QHeaderView.Fixed)
        hh = self.horizontalHeader()
        hh.setHighlightSections(False)
        hh.setSectionsClickable(mode != "queue")
        if mode != "queue":
            hh.setSortIndicator(-1, Qt.AscendingOrder)
            self.setSortingEnabled(True)
        self.setContextMenuPolicy(Qt.CustomContextMenu)
        self.customContextMenuRequested.connect(self.menu)
        self.doubleClicked.connect(lambda i: None if self.win.cfg.get("single_click_play", True) else self.activate(i))
        self.clicked.connect(self.on_click)
        win.tables.append(self)

    def set_tracks(self, tracks):
        cols = [c for c in self.win.cfg["song_columns"] if c in COLS and c not in self.hide]
        if "title" not in cols:
            cols.insert(0, "title")
        self.model_.set_tracks(tracks, cols)
        self.fit()

    def fit(self):
        s = int(self.win.cfg["list_cover_size"]) if self.model_.thumbs else 0
        fm = self.fontMetrics()
        self.verticalHeader().setDefaultSectionSize(max(s + 8, fm.height() + 12))
        self.setIconSize(QSize(s, s))
        zw = fm.horizontalAdvance("0")
        widths = {"#": 5, "year": 7, "duration": 8, "format": 13, "genre": 16}
        hh = self.horizontalHeader()
        for i, c in enumerate(self.model_.cols):
            if c in ("title", "artist", "album"):
                hh.setSectionResizeMode(i, QHeaderView.Stretch)
            else:
                hh.setSectionResizeMode(i, QHeaderView.Interactive)
                hh.resizeSection(i, zw * widths.get(c, 10) + 16)
        self.viewport().update()

    def src_row(self, proxy_row):
        return self.proxy.mapToSource(self.proxy.index(proxy_row, 0)).row()

    def visible_tracks(self):
        return [self.model_.tracks[self.src_row(r)] for r in range(self.proxy.rowCount())]

    def selected_rows(self):
        rows = sorted({i.row() for i in self.selectionModel().selectedRows()})
        return [self.src_row(r) for r in rows]

    def on_click(self, index):
        if not self.win.cfg.get("single_click_play", True):
            return
        if QApplication.keyboardModifiers() & (Qt.ControlModifier | Qt.ShiftModifier):
            return                       # ctrl/shift-click = select for right-click actions
        import time
        now = time.monotonic()
        last = getattr(self, "_last_click", (None, 0.0))
        self._last_click = (index.row(), now)
        if last[0] == index.row() and now - last[1] < 0.5:
            return                       # second half of a double-click: don't restart the song
        self.activate(index)

    def activate(self, index):
        if self.mode == "queue":
            self.win.play_queue_index(self.src_row(index.row()))
        else:
            self.win.play_list(self.visible_tracks(), index.row())

    def menu(self, pos):
        rows = self.selected_rows()
        if not rows:
            return
        sel = [self.model_.tracks[r] for r in rows]
        w, L = self.win, self.win.L
        m = QMenu(self)
        if self.mode != "queue":
            m.addAction(L("play"), lambda: w.play_list(sel, 0))
        m.addAction(L("play next"), lambda: w.queue_next(sel))
        m.addAction(L("add to queue"), lambda: w.queue_add(sel))
        pm = m.addMenu(L("add to playlist"))
        for name in w.playlists:
            pm.addAction(name, lambda n=name: w.playlist_add(n, sel))
        pm.addSeparator()
        pm.addAction(L("+ new playlist"), lambda: w.playlist_new(sel))
        if self.mode == "playlist":
            m.addAction(L("remove from playlist"), lambda: w.playlist_remove_rows(w.page_arg, rows))
        if self.mode == "queue":
            m.addAction(L("remove from queue"), lambda: w.queue_remove(rows))
        m.addSeparator()
        m.addAction(L("go to album"), lambda: w.go("album", sel[0]["album_key"]))
        m.addAction(L("go to artist"), lambda: w.go("artists", sel[0]["albumartist"]))
        m.addAction(L("inspect (spectrum)"), lambda: w.go("inspect", sel[0]["path"]))
        m.addAction(L("edit tags & cover"), lambda: w.go("tags", [t["path"] for t in sel]))
        m.addAction(L("open folder"), lambda: QDesktopServices.openUrl(
            QUrl.fromLocalFile(str(Path(sel[0]["path"]).parent))))
        m.addAction(L("copy path"), lambda: QApplication.clipboard().setText(
            "\n".join(t["path"] for t in sel)))
        m.exec(self.viewport().mapToGlobal(pos))


# ------------------------------------------------------------------ album grid
class AlbumDelegate(QStyledItemDelegate):
    def __init__(self, win, parent):
        super().__init__(parent)
        self.win = win

    def sizeHint(self, opt, idx):
        return self.parent().cell_size()

    def paint(self, p, opt, idx):
        w = self.win
        a = w.albums.get(idx.data(Qt.UserRole))
        if not a:
            return
        c, s = w.c, int(w.cfg["cover_size"])
        r = opt.rect
        p.save()
        p.setRenderHint(QPainter.Antialiasing)
        hot = bool(opt.state & QStyle.State_MouseOver)
        x, y = r.x() + (r.width() - s) // 2, r.y() + 10
        p.drawPixmap(QRect(x, y, s, s), w.covers.get(a["cover"], s))
        playing = a["key"] == w.playing_album()
        if hot:
            shade = QPainterPath()
            rad = w.covers.radius(s)
            shade.addRoundedRect(QRectF(x, y, s, s), rad, rad)
            p.fillPath(shade, QColor(0, 0, 0, 70))
            pr = self.play_rect(QRect(x, y, s, s))
            p.setPen(Qt.NoPen)
            p.setBrush(QColor("#ffffff"))
            p.drawEllipse(QRectF(pr))
            p.setBrush(QColor("#000000"))
            cx, cy, d = pr.center().x() + 0.5, pr.center().y() + 0.5, pr.width()
            tri = QPainterPath()
            tri.moveTo(cx - d * 0.12, cy - d * 0.18)
            tri.lineTo(cx - d * 0.12, cy + d * 0.18)
            tri.lineTo(cx + d * 0.2, cy)
            tri.closeSubpath()
            p.drawPath(tri)
        if playing:            # small dot badge on the album that's playing
            p.setPen(Qt.NoPen)
            p.setBrush(QColor(c["bg"]))
            p.drawEllipse(QRectF(x + 8, y + 8, 14, 14))
            p.setBrush(QColor(c["accent"]))
            p.drawEllipse(QRectF(x + 11.5, y + 11.5, 7, 7))
        fm = opt.fontMetrics
        lh = fm.height()
        title = fm.elidedText(a["album"], Qt.ElideRight, s)
        sub = " · ".join(v for v in (a["artist"], a["year"]) if v)
        tf = QFont(opt.font)
        tf.setWeight(QFont.Medium)
        p.setFont(tf)
        p.setPen(QColor(c["text"]))
        p.drawText(QRect(x, y + s + 8, s, lh), Qt.AlignLeft | Qt.AlignVCenter, title)
        p.setFont(opt.font)
        p.setPen(QColor(c["dim"]))
        p.drawText(QRect(x, y + s + 8 + lh, s, lh), Qt.AlignLeft | Qt.AlignVCenter,
                   fm.elidedText(sub, Qt.ElideRight, s))
        p.restore()

    @staticmethod
    def play_rect(cover):
        d = max(34, min(48, cover.width() // 4))
        return QRect(cover.right() - d - 9, cover.bottom() - d - 9, d, d)


class AlbumGrid(QListWidget):
    def __init__(self, win):
        super().__init__()
        self.win = win
        self.setViewMode(QListView.IconMode)
        self.setResizeMode(QListView.Adjust)
        self.setMovement(QListView.Static)
        self.setWrapping(True)
        self.setUniformItemSizes(True)
        self.setSelectionMode(QAbstractItemView.SingleSelection)
        self.setVerticalScrollMode(QAbstractItemView.ScrollPerPixel)
        self.verticalScrollBar().setSingleStep(30)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setMouseTracking(True)
        self.viewport().setAttribute(Qt.WA_Hover, True)
        self.setItemDelegate(AlbumDelegate(win, self))
        self.itemClicked.connect(lambda it: win.go("album", it.data(Qt.UserRole)))
        self.setContextMenuPolicy(Qt.CustomContextMenu)
        self.customContextMenuRequested.connect(self.menu)
        self.setContentsMargins(12, 12, 12, 12)

    def cell_size(self):
        s = int(self.win.cfg["cover_size"])
        return QSize(s + 28, s + self.fontMetrics().height() * 2 + 34)

    def mousePressEvent(self, e):
        idx = self.indexAt(e.position().toPoint())
        if idx.isValid() and e.button() == Qt.LeftButton:
            r, s = self.visualRect(idx), int(self.win.cfg["cover_size"])
            cover = QRect(r.x() + (r.width() - s) // 2, r.y() + 10, s, s)
            if AlbumDelegate.play_rect(cover).contains(e.position().toPoint()):
                a = self.win.albums.get(idx.data(Qt.UserRole))
                if a:
                    self.win.play_list(a["tracks"], 0)
                return
        super().mousePressEvent(e)

    def relayout(self):
        self.setGridSize(self.cell_size())
        self.viewport().update()

    def set_albums(self, albums):
        self.clear()
        self.relayout()
        for a in albums:
            it = QListWidgetItem(f"{a['album']} {a['artist']}")
            it.setData(Qt.UserRole, a["key"])
            self.addItem(it)

    def menu(self, pos):
        it = self.itemAt(pos)
        if not it:
            return
        a = self.win.albums.get(it.data(Qt.UserRole))
        if not a:
            return
        w, L = self.win, self.win.L
        m = QMenu(self)
        m.addAction(L("play"), lambda: w.play_list(a["tracks"], 0))
        m.addAction(L("shuffle"), lambda: w.play_shuffled(a["tracks"]))
        m.addAction(L("play next"), lambda: w.queue_next(a["tracks"]))
        m.addAction(L("add to queue"), lambda: w.queue_add(a["tracks"]))
        pm = m.addMenu(L("add to playlist"))
        for name in w.playlists:
            pm.addAction(name, lambda n=name: w.playlist_add(n, a["tracks"]))
        pm.addSeparator()
        pm.addAction(L("+ new playlist"), lambda: w.playlist_new(a["tracks"]))
        m.addAction(L("edit album tags & cover"), lambda: w.go("tags", [t["path"] for t in a["tracks"]]))
        m.addAction(L("go to artist"), lambda: w.go("artists", a["artist"]))
        m.exec(self.viewport().mapToGlobal(pos))


# ------------------------------------------------------------------ pages
class ArtistsPage(QSplitter):
    def __init__(self, win):
        super().__init__(Qt.Horizontal)
        self.win = win
        self.list = QListWidget()
        self.list.setObjectName("artists")
        self.grid = AlbumGrid(win)
        self.addWidget(self.list)
        self.addWidget(self.grid)
        self.setStretchFactor(1, 1)
        self.setSizes([240, 900])
        self.setHandleWidth(1)
        self.list.currentItemChanged.connect(self.show_artist)

    def refresh(self, select=None):
        cur = select or (self.list.currentItem().data(Qt.UserRole) if self.list.currentItem() else None)
        self.list.blockSignals(True)
        self.list.clear()
        target = None
        for name in sorted(self.win.artists, key=str.lower):
            n = len(self.win.artists[name])
            it = QListWidgetItem(f" {name}  ({n})")
            it.setData(Qt.UserRole, name)
            self.list.addItem(it)
            if name == cur:
                target = it
        self.list.blockSignals(False)
        if target is None and self.list.count():
            target = self.list.item(0)
        if target:
            self.list.setCurrentItem(target)
            self.show_artist(target, None)
        else:
            self.grid.clear()

    def show_artist(self, cur, _prev):
        if cur:
            self.grid.set_albums(self.win.sorted_albums(self.win.artists.get(cur.data(Qt.UserRole), [])))


class CollectionPage(QWidget):
    """album page and playlist page."""

    def __init__(self, win):
        super().__init__()
        self.win, self.kind, self.key, self.tracks = win, None, None, []
        v = QVBoxLayout(self)
        v.setContentsMargins(32, 28, 28, 0)
        head = QHBoxLayout()
        head.setSpacing(28)
        self.cover = QLabel()
        head.addWidget(self.cover, 0, Qt.AlignTop)
        info = QVBoxLayout()
        info.setSpacing(4)
        self.kind_l = QLabel(objectName="eyebrow")
        self.title = ClickLabel(objectName="h1")
        self.title.setWordWrap(True)
        self.title.clicked.connect(lambda: self.kind == "playlist" and self.start_rename())
        self.title_edit = QLineEdit(objectName="titleedit")
        self.title_edit.hide()
        self.title_edit.returnPressed.connect(self.finish_rename)
        self.title_edit.editingFinished.connect(self.finish_rename)
        self.meta = QLabel(objectName="dim")
        self.meta.setWordWrap(True)
        btns = QHBoxLayout()
        btns.setSpacing(8)
        self.b_play = win.tbtn("play", lambda: win.play_list(self.tracks, 0), "pillPrimary")
        self.b_shuf = win.tbtn("shuffle", lambda: win.play_shuffled(self.tracks), "pill")
        self.b_queue = win.tbtn("add to queue", lambda: win.queue_add(self.tracks), "pill")
        self.b_rename = win.tbtn("rename", self.start_rename, "pill")
        self.b_delete = win.tbtn("delete", lambda: win.playlist_delete(self.key), "pill")
        self.b_tags = win.tbtn("edit tags", lambda: win.go("tags", [t["path"] for t in self.tracks]), "pill")
        for b in (self.b_play, self.b_shuf, self.b_queue, self.b_tags, self.b_rename, self.b_delete):
            btns.addWidget(b)
        btns.addStretch()
        info.addStretch()
        for wdg in (self.kind_l, self.title, self.title_edit, self.meta):
            info.addWidget(wdg)
        info.addSpacing(14)
        info.addLayout(btns)
        head.addLayout(info, 1)
        v.addLayout(head)
        v.addSpacing(22)
        self.table = SongTable(win, "album", hide={"album"}, thumbs=False)
        v.addWidget(self.table, 1)

    # ---- rename right here on the page (no pop-up)
    def start_rename(self):
        if self.kind != "playlist":
            return
        self.title_edit.setText(self.key)
        self.title.hide()
        self.title_edit.show()
        self.title_edit.setFocus()
        self.title_edit.selectAll()

    def renaming(self):
        return self.title_edit.isVisible()

    def cancel_rename(self):
        self.title_edit.hide()
        self.title.show()

    def finish_rename(self):
        if not self.title_edit.isVisible():
            return
        new = self.title_edit.text().strip()
        if new and new != self.key and new in self.win.playlists:
            self.win.toast(f"there's already a playlist called “{new}”")
            self.title_edit.setFocus()
            self.title_edit.selectAll()
            return
        self.cancel_rename()
        if new and new != self.key:
            self.win.playlist_rename_to(self.key, new)

    def show_album(self, key):
        a = self.win.albums.get(key)
        if not a:
            return False
        self.kind, self.key, self.tracks = "album", key, a["tracks"]
        total = sum(t["duration"] for t in self.tracks)
        meta = [a["artist"], a["year"], f"{len(self.tracks)} tracks", fmt_time(total),
                quality(self.tracks[0]) if self.tracks else ""]
        self._render("album", a["album"], "  ·  ".join(m for m in meta if m), a["cover"],
                     mode="album", hide={"album"}, thumbs=False)
        return True

    def show_playlist(self, name):
        if name not in self.win.playlists:
            return False
        bp = self.win.by_path
        self.kind, self.key = "playlist", name
        self.tracks = [bp[p] for p in self.win.playlists[name] if p in bp]
        total = sum(t["duration"] for t in self.tracks)
        cover = next((t["cover"] for t in self.tracks if t["cover"]), "")
        self._render("playlist", name, f"{len(self.tracks)} tracks  ·  {fmt_time(total)}", cover,
                     mode="playlist", hide=set(), thumbs=True)
        return True

    def rerender(self):
        if self.kind == "album":
            return self.show_album(self.key)
        if self.kind == "playlist":
            return self.show_playlist(self.key)
        return False

    def _render(self, kind, title, meta, cover, mode, hide, thumbs):
        s = int(int(self.win.cfg["cover_size"]) * 1.4)
        self.cover.setFixedSize(s, s)
        self.cover.setPixmap(self.win.covers.get(cover, s))
        self.kind_l.setText(kind.upper())
        if self.title_edit.isVisible():
            self.cancel_rename()
        self.title.setText(title)
        self.meta.setText(meta)
        self.b_rename.setVisible(kind == "playlist")
        self.b_delete.setVisible(kind == "playlist")
        t = self.table
        t.mode = t.model_.mode = mode
        t.hide = hide
        t.model_.thumbs = thumbs
        t.horizontalHeader().setSortIndicator(-1, Qt.AscendingOrder)
        t.set_tracks(self.tracks)


class QueuePage(QWidget):
    def __init__(self, win):
        super().__init__()
        self.win = win
        v = QVBoxLayout(self)
        v.setContentsMargins(32, 28, 28, 0)
        h = QHBoxLayout()
        self.title = QLabel(objectName="h1")
        self.info = QLabel(objectName="dim")
        h.addWidget(self.title)
        h.addSpacing(12)
        h.addWidget(self.info, 0, Qt.AlignBottom)
        h.addStretch()
        h.addWidget(win.tbtn("clear", win.queue_clear, "pill"))
        v.addLayout(h)
        v.addSpacing(16)
        self.table = SongTable(win, "queue")
        v.addWidget(self.table, 1)

    def refresh(self):
        w = self.win
        self.title.setText(w.L("queue"))
        left = sum(t["duration"] for t in w.queue[max(0, w.qi):])
        self.info.setText(f"{len(w.queue)} tracks  ·  {fmt_time(left)} left")
        self.table.set_tracks(w.queue)
        if 0 <= w.qi < len(w.queue):
            self.table.scrollTo(self.table.proxy.index(w.qi, 0), QAbstractItemView.PositionAtCenter)


# ------------------------------------------------------------------ audio engine
# decode (ffmpeg via PyAV) → ReplayGain → EQ (linear-phase FIR, numpy FFT) → buffer →
# output callback (volume / boost, peak limiter) → PortAudio (WASAPI exclusive on windows).
# gapless: the decoder rolls straight into the queued next track inside the same stream.
try:
    import numpy as np
    import av
    _ENGINE_ERR = ""
except Exception as _e:          # missing deps → app falls back to the Qt player
    np = av = None
    _ENGINE_ERR = f"{type(_e).__name__}: {_e}"
try:
    import sounddevice as sd
    _SD_ERR = ""
except Exception as _e:
    sd = None
    _SD_ERR = f"{type(_e).__name__}: {_e}"

import collections
import threading
import time as _time

EQ_BANDS = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]
EQ_PRESETS = {
    "flat":          [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    "bass boost":    [6, 5, 4, 2, 0, 0, 0, 0, 0, 0],
    "bass + treble": [5, 4, 2, 0, -1, -1, 0, 2, 4, 5],
    "treble boost":  [0, 0, 0, 0, 0, 1, 2, 4, 5, 6],
    "vocal":         [-2, -2, -1, 1, 3, 4, 3, 1, 0, -1],
    "loudness":      [5, 4, 1, 0, -1, 0, 1, 2, 4, 4],
    "hip-hop":       [5, 4, 2, 3, -1, -1, 1, 0, 2, 3],
    "electronic":    [4, 4, 1, 0, -2, 1, 0, 1, 3, 4],
    "rock":          [4, 3, -1, -2, -1, 1, 3, 4, 4, 4],
    "acoustic":      [3, 3, 2, 1, 1, 1, 2, 2, 2, 1],
    "late night":    [-3, -2, 0, 1, 2, 2, 1, 0, -2, -3],
}


def engine_available():
    if np is None or av is None:
        return False, _ENGINE_ERR
    if sd is None and not os.environ.get("NEXUS_NULL_AUDIO"):
        return False, _SD_ERR
    return True, ""


def eq_impulse(sr, gains, preamp):
    """linear-phase FIR matching a 10-band graphic EQ curve (dB interpolated on log-frequency)."""
    n = 8192 if sr > 50000 else 4096
    freqs = np.fft.rfftfreq(n, 1.0 / sr)
    db = np.interp(np.log10(np.maximum(freqs, 1.0)), np.log10(EQ_BANDS), gains) + preamp
    h = np.fft.irfft(10 ** (db / 20.0), n)
    return (np.roll(h, n // 2) * np.hanning(n)).astype(np.float32)


class FIRFilter:
    """streaming overlap-add convolution, any block size in, whole L-blocks out."""

    def __init__(self, h, ch=2):
        self.N = len(h)
        self.M = 1 << int(math.ceil(math.log2(2 * self.N)))
        self.L = self.M - self.N + 1
        self.H = np.fft.rfft(h, self.M)[:, None].astype(np.complex64)
        self.ch = ch
        self.reset()

    def reset(self):
        self.tail = np.zeros((self.N - 1, self.ch), np.float32)
        self.inbuf = np.zeros((0, self.ch), np.float32)

    def process(self, x):
        self.inbuf = np.concatenate([self.inbuf, x]) if len(self.inbuf) else x
        out = []
        L, N, M = self.L, self.N, self.M
        while len(self.inbuf) >= L:
            blk, self.inbuf = self.inbuf[:L], self.inbuf[L:]
            y = np.fft.irfft(np.fft.rfft(blk, M, axis=0) * self.H, M, axis=0).astype(np.float32)
            y[:N - 1] += self.tail
            out.append(y[:L])
            self.tail = y[L:L + N - 1].copy()
        return np.concatenate(out) if out else np.zeros((0, self.ch), np.float32)

    def flush(self):
        pad = np.zeros((self.L, self.ch), np.float32)
        return self.process(pad)


class PCMBuffer:
    def __init__(self, ch=2):
        self.ch = ch
        self.lock = threading.Lock()
        self.chunks = collections.deque()
        self.frames = 0
        self.pushed = 0      # total frames ever pushed
        self.pulled = 0      # total frames ever consumed (or discarded)

    def push(self, a):
        with self.lock:
            self.chunks.append(a)
            self.frames += len(a)
            self.pushed += len(a)

    def pull(self, n):
        out = []
        with self.lock:
            while n > 0 and self.chunks:
                c = self.chunks[0]
                if len(c) <= n:
                    self.chunks.popleft()
                    out.append(c)
                    n -= len(c)
                else:
                    out.append(c[:n])
                    self.chunks[0] = c[n:]
                    n = 0
            got = sum(len(c) for c in out)
            self.frames -= got
            self.pulled += got
        if not out:
            return np.zeros((0, self.ch), np.float32)
        return out[0] if len(out) == 1 else np.concatenate(out)

    def clear(self):
        with self.lock:
            self.chunks.clear()
            self.pulled += self.frames
            self.frames = 0


class _NullStream:
    """fake output used for headless testing (NEXUS_NULL_AUDIO=1)."""

    def __init__(self, samplerate, callback, **_kw):
        self.samplerate, self.cb, self.active = samplerate, callback, False

    def start(self):
        self.active = True
        threading.Thread(target=self._run, daemon=True).start()

    def _run(self):
        bs = 1024
        buf = np.zeros((bs, 2), np.float32)
        t = _time.monotonic()
        while self.active:
            self.cb(buf, bs, None, None)
            t += bs / self.samplerate
            d = t - _time.monotonic()
            if d > 0:
                _time.sleep(d)

    def stop(self):
        self.active = False

    close = stop


class _Decoder(threading.Thread):
    def __init__(self, eng, path, start_ms):
        super().__init__(daemon=True)
        self.eng, self.path, self.start_ms = eng, path, start_ms
        self.stop_ev = threading.Event()
        self.lock = threading.Lock()
        self.seek_ms = None

    def request_seek(self, ms):
        with self.lock:
            self.seek_ms = ms

    def stop(self):
        self.stop_ev.set()

    def run(self):
        eng = self.eng
        st = {"rate": None, "fir": None, "fir_ver": -1}
        path, pos = self.path, self.start_ms
        try:
            while path and not self.stop_ev.is_set():
                if self._play(path, pos, st) != "eof":
                    break
                nxt = eng._take_next(st["rate"])
                if not nxt:
                    break
                path, pos = nxt, 0
            if st["fir"] is not None and not self.stop_ev.is_set():
                eng.buf.push(st["fir"].flush())
        except Exception as e:
            if not self.stop_ev.is_set():
                eng._fail(f"{type(e).__name__}: {e}")
        finally:
            eng._decoder_finished(self)

    def _play(self, path, pos, st):
        eng = self.eng
        c = av.open(path)
        try:
            s = c.streams.audio[0]
            file_rate = s.rate or s.codec_context.sample_rate or 44100
            rate = eng._choose_rate(file_rate)
            eng._ensure_stream(rate)
            if st["rate"] is not None and st["rate"] != rate and st["fir"] is not None:
                st["fir"] = None
                st["fir_ver"] = -1
            st["rate"] = rate
            if s.duration and s.time_base:
                dur_ms = int(s.duration * s.time_base * 1000)
            else:
                dur_ms = int((c.duration or 0) / 1000)
            rg = eng._rg_factor(path)

            def resampler():
                return av.AudioResampler(format="flt", layout="stereo", rate=rate)
            res = resampler()
            pending_mark = pos
            if pos > 0:
                c.seek(int(pos * 1000))
            gen = c.decode(s)
            while not self.stop_ev.is_set():
                with self.lock:
                    sk, self.seek_ms = self.seek_ms, None
                if sk is not None:
                    c.seek(max(0, int(sk * 1000)))
                    eng.buf.clear()
                    if st["fir"] is not None:
                        st["fir"].reset()
                    res = resampler()
                    gen = c.decode(s)
                    pending_mark = sk
                    eng._reset_marks()
                    continue
                if eng.buf.frames > rate * eng.BUFFER_SEC:
                    self.stop_ev.wait(0.01)
                    continue
                try:
                    frame = next(gen)
                except StopIteration:
                    break
                except av.error.InvalidDataError:
                    continue
                if pending_mark is not None:
                    t0 = frame.time
                    eng._mark(path, dur_ms, int(t0 * 1000) if t0 is not None and pending_mark else pending_mark)
                    pending_mark = None
                if eng.eq_version != st["fir_ver"]:
                    st["fir"] = eng._make_fir(rate, st["fir"])
                    st["fir_ver"] = eng.eq_version
                for rf in res.resample(frame):
                    self._emit(rf, rg, st["fir"])
            else:
                return "stopped"
            if pending_mark is not None:
                eng._mark(path, dur_ms, pending_mark)
            for rf in res.resample(None):
                self._emit(rf, rg, st["fir"])
            return "eof"
        finally:
            c.close()

    def _emit(self, rf, rg, fir):
        a = rf.to_ndarray().reshape(-1, 2)
        if a.dtype != np.float32:
            a = a.astype(np.float32)
        if rg != 1.0:
            a = a * np.float32(rg)
        if fir is not None:
            a = fir.process(a)
        if len(a):
            self.eng.buf.push(a)


class NexusEngine(QObject):
    """drop-in for the bits of QMediaPlayer + QAudioOutput the app uses."""
    positionChanged = Signal(int)
    durationChanged = Signal(int)
    mediaStatusChanged = Signal(QMediaPlayer.MediaStatus)
    playbackStateChanged = Signal(QMediaPlayer.PlaybackState)
    errorOccurred = Signal(QMediaPlayer.Error, str)
    advanced = Signal(str)          # gapless rollover into the queued next track
    BUFFER_SEC = 0.25

    def __init__(self, win):
        super().__init__(win)
        self.win = win
        self.buf = PCMBuffer()
        self._src, self._state, self._dur = QUrl(), QMediaPlayer.StoppedState, 0
        self._err, self._err_pending, self._loaded_pending, self._ended = "", None, False, False
        self.dec, self.dec_done = None, True
        self.marks = collections.deque()     # (abs frame, path, dur_ms, offset_ms)
        self.mark_lock = threading.Lock()
        self.stream, self.stream_rate, self.out_info = None, None, ""
        self.gain, self.gain_cur, self.lim, self._muted = 1.0, 1.0, 1.0, False
        self.next_path, self.next_rate = None, 0
        self.eq_version = 0
        self.eq = dict(win.cfg.get("eq") or {})
        self.vis_ring = np.zeros(8192, np.float32)       # last few thousand output samples (mono) for visualizers
        self.vis_w = 0
        self.last_pos = -1
        self.timer = QTimer(self)
        self.timer.setInterval(50)
        self.timer.timeout.connect(self._poll)
        self.timer.start()

    # ---- QAudioOutput-ish
    def setVolume(self, g):
        self.gain = max(0.0, float(g))

    def setMuted(self, m):
        self._muted = bool(m)

    def isMuted(self):
        return self._muted

    # ---- QMediaPlayer-ish
    def source(self):
        return self._src

    def errorString(self):
        return self._err

    def playbackState(self):
        return self._state

    def duration(self):
        return self._dur

    def setSource(self, url):
        self._kill_decoder()
        self.buf.clear()
        self._reset_marks(all_=True)
        self._src = QUrl(url)
        self._dur, self._ended, self.last_pos = 0, False, -1
        self.mediaStatusChanged.emit(QMediaPlayer.LoadingMedia)
        if not self._src.isEmpty():
            self._start_decoder(0)

    def play(self):
        if self._src.isEmpty():
            return
        if self.dec is None or (self.dec_done and self.buf.frames == 0):
            self._kill_decoder()
            self.buf.clear()
            self._reset_marks(all_=True)
            self._ended = False
            self._start_decoder(0)
        self._set_state(QMediaPlayer.PlayingState)

    def pause(self):
        if self._state == QMediaPlayer.PlayingState:
            self._set_state(QMediaPlayer.PausedState)

    def stop(self):
        self._kill_decoder()
        self.buf.clear()
        self._reset_marks(all_=True)
        self._set_state(QMediaPlayer.StoppedState)
        self.positionChanged.emit(0)

    def position(self):
        m = self._current_mark()
        if not m or not self.stream_rate:
            return 0
        return int(m[3] + (self.buf.pulled - m[0]) * 1000 / self.stream_rate)

    def setPosition(self, ms):
        ms = max(0, int(ms))
        self._ended = False
        if self.dec is not None and not self.dec_done:
            self.dec.request_seek(ms)
        elif not self._src.isEmpty():
            self._kill_decoder()
            self.buf.clear()
            self._reset_marks(all_=True)
            self._start_decoder(ms)
        self.last_pos = -1

    # ---- extras
    def queue_next(self, path, rate=0):
        self.next_path, self.next_rate = path, rate

    def set_eq(self, eq):
        self.eq = dict(eq)
        self.eq_version += 1

    def restart_output(self):
        """reopen the device (after changing device / exclusive mode) at the current spot."""
        pos, was = self.position(), self._state
        src = QUrl(self._src)
        self._kill_decoder()
        self._close_stream()
        if not src.isEmpty():
            self.setSource(src)
            self.setPosition(pos)
            if was == QMediaPlayer.PlayingState:
                self.play()

    def shutdown(self):
        self.timer.stop()
        self._kill_decoder()
        self._close_stream()

    @staticmethod
    def output_devices():
        """[(name, hostapi name)] for the output devices worth offering."""
        if sd is None:
            return []
        try:
            apis, devs = sd.query_hostapis(), sd.query_devices()
        except Exception:
            return []
        wi = next((i for i, a in enumerate(apis) if "WASAPI" in a["name"]), None) \
            if sys.platform.startswith("win") else None
        return [d["name"] for d in devs
                if d["max_output_channels"] > 0 and (wi is None or d["hostapi"] == wi)]

    # ---- internals (some run on the decoder thread)
    def _start_decoder(self, ms):
        self.dec_done = False
        self.dec = _Decoder(self, self._src.toLocalFile(), ms)
        self.dec.start()

    def _kill_decoder(self):
        d, self.dec = self.dec, None
        if d is not None:
            d.stop()
            d.join(1.5)
        self.dec_done = True

    def _decoder_finished(self, dec):
        if dec is self.dec:
            self.dec_done = True

    def _fail(self, msg):
        self._err = msg
        self._err_pending = msg

    def _set_state(self, s):
        if s != self._state:
            self._state = s
            self.playbackStateChanged.emit(s)

    def _mark(self, path, dur, offset):
        with self.mark_lock:
            self.marks.append((self.buf.pushed, path, dur, offset))
        self._loaded_pending = True

    def _reset_marks(self, all_=False):
        with self.mark_lock:
            self.marks.clear()

    def _current_mark(self):
        with self.mark_lock:
            while len(self.marks) > 1 and self.marks[1][0] <= self.buf.pulled:
                self.marks.popleft()
            return self.marks[0] if self.marks else None

    def _take_next(self, rate):
        cfg = self.win.cfg
        p, r = self.next_path, self.next_rate
        if not cfg.get("gapless", True) or not p or not os.path.exists(p):
            return None
        if r and self._choose_rate(r) != rate:
            return None          # sample-rate change: can't be gapless, play normally
        self.next_path = None
        return p

    def _choose_rate(self, file_rate):
        cfg = self.win.cfg
        if sys.platform.startswith("win") and not cfg.get("exclusive_mode", False) and sd is not None:
            idx = self._device_index()
            try:
                return int(sd.query_devices(idx)["default_samplerate"]) if idx is not None else int(file_rate)
            except Exception:
                return int(file_rate)
        return int(file_rate)

    def _device_index(self):
        if sd is None:
            return None
        try:
            apis, devs = sd.query_hostapis(), sd.query_devices()
        except Exception:
            return None
        want = self.win.cfg.get("audio_device") or ""
        wi = next((i for i, a in enumerate(apis) if "WASAPI" in a["name"]), None) \
            if sys.platform.startswith("win") else None
        for i, d in enumerate(devs):
            if want and d["name"] == want and d["max_output_channels"] > 0 and (wi is None or d["hostapi"] == wi):
                return i
        if wi is not None and apis[wi].get("default_output_device", -1) >= 0:
            return apis[wi]["default_output_device"]
        dflt = sd.default.device[1] if isinstance(sd.default.device, (list, tuple)) else sd.default.device
        return dflt if dflt is not None and dflt >= 0 else None

    def _ensure_stream(self, rate):
        if self.stream is not None and self.stream_rate == rate:
            return
        self._close_stream()
        if sd is None:
            self.stream = _NullStream(samplerate=rate, callback=self._callback)
            self.out_info = f"null output · {rate / 1000:g} kHz"
        else:
            idx = self._device_index()
            excl = sys.platform.startswith("win") and self.win.cfg.get("exclusive_mode", False)
            extra = sd.WasapiSettings(exclusive=True) if excl else None
            try:
                self.stream = sd.OutputStream(device=idx, samplerate=rate, channels=2, dtype="float32",
                                              callback=self._callback, latency="high", extra_settings=extra)
                mode = "exclusive" if excl else "shared"
            except Exception:
                if not excl:
                    raise
                self.stream = sd.OutputStream(device=idx, samplerate=rate, channels=2, dtype="float32",
                                              callback=self._callback, latency="high")
                mode = "shared (exclusive refused)"
            name = sd.query_devices(idx)["name"] if idx is not None else "default"
            self.out_info = f"{name} · {mode} · {rate / 1000:g} kHz"
        self.stream_rate = rate
        self.stream.start()

    def _close_stream(self):
        s, self.stream = self.stream, None
        if s is not None:
            try:
                s.stop()
                s.close()
            except Exception:
                pass
        self.stream_rate = None

    def _make_fir(self, rate, old):
        eq = self.eq
        if not eq.get("on"):
            return None
        gains = list(eq.get("bands") or [0] * 10)[:10]
        gains += [0] * (10 - len(gains))
        f = FIRFilter(eq_impulse(rate, gains, float(eq.get("preamp", 0))))
        if old is not None and old.N == f.N:
            f.tail = old.tail
        return f

    def _rg_factor(self, path):
        mode = self.win.cfg.get("replaygain", "off")
        if mode == "off":
            return 1.0
        try:
            tags = mutagen.File(path, easy=True)
            tags = tags.tags if tags is not None and tags.tags is not None else {}
            key = "replaygain_album_gain" if mode == "album" else "replaygain_track_gain"
            v = _first(tags, key) or _first(tags, "replaygain_track_gain")
            db = float(v.lower().replace("db", "").strip()) if v else 0.0
            db += float(self.win.cfg.get("replaygain_preamp", 0))
            return 10 ** (db / 20)
        except Exception:
            return 1.0

    def _callback(self, outdata, frames, _t, _status):
        if self._state != QMediaPlayer.PlayingState:
            outdata.fill(0)
            return
        data = self.buf.pull(frames)
        n = len(data)
        if n:
            try:
                m = data[:, 0] if data.shape[1] == 1 else (data[:, 0] + data[:, 1]) * 0.5
                L = len(self.vis_ring)
                m = m[-L:]
                k = len(m)
                w = self.vis_w % L
                first = min(k, L - w)
                self.vis_ring[w:w + first] = m[:first]
                if k > first:
                    self.vis_ring[:k - first] = m[first:]
                self.vis_w = (w + k) % L
            except Exception:
                pass
            g = 0.0 if self._muted else self.gain
            if abs(g - self.gain_cur) > 1e-5:
                y = data * np.linspace(self.gain_cur, g, n, dtype=np.float32)[:, None]
                self.gain_cur = g
            else:
                y = data * np.float32(g)
            peak = float(np.abs(y).max()) if n else 0.0
            target = min(1.0, 0.97 / peak) if peak > 0 else 1.0
            new = target if target < self.lim else min(target, self.lim + 0.0015 * n / 256)   # fast attack, slow release
            if new != 1.0 or self.lim != 1.0:
                y *= np.linspace(self.lim, new, n, dtype=np.float32)[:, None]
            self.lim = new
            np.clip(y, -1.0, 1.0, out=y)
            outdata[:n] = y
        if n < frames:
            outdata[n:] = 0

    def vis_samples(self, n):
        L = len(self.vis_ring)
        w = self.vis_w
        idx = (np.arange(w - n, w) % L)
        return self.vis_ring[idx]

    def _poll(self):
        if self._err_pending:
            msg, self._err_pending = self._err_pending, None
            self._set_state(QMediaPlayer.StoppedState)
            self.errorOccurred.emit(QMediaPlayer.ResourceError, msg)
            return
        m = self._current_mark()
        if self._loaded_pending and m:
            self._loaded_pending = False
            if m[2] != self._dur:
                self._dur = m[2]
                self.durationChanged.emit(self._dur)
            self.mediaStatusChanged.emit(QMediaPlayer.LoadedMedia)
        if m and m[1] != self._src.toLocalFile():       # gapless rollover happened
            self._src = QUrl.fromLocalFile(m[1])
            self._dur = m[2]
            self.durationChanged.emit(self._dur)
            self.advanced.emit(m[1])
        if m and m[2] != self._dur:
            self._dur = m[2]
            self.durationChanged.emit(self._dur)
        pos = self.position()
        if pos != self.last_pos:
            self.last_pos = pos
            self.positionChanged.emit(pos)
        if self._state == QMediaPlayer.PlayingState and self.dec_done and self.buf.frames == 0 and not self._ended:
            self._ended = True
            self._set_state(QMediaPlayer.StoppedState)
            self.mediaStatusChanged.emit(QMediaPlayer.EndOfMedia)


class ToggleSwitch(QCheckBox):
    """pill on/off switch (apple / nothing style) — a QCheckBox, so it drops in anywhere."""

    def __init__(self, win, text=""):
        super().__init__(text)
        self.win = win
        self.setCursor(Qt.PointingHandCursor)
        self._pos = 0.0
        self.anim = QVariantAnimation(self)
        self.anim.setDuration(140)
        self.anim.valueChanged.connect(self._step)
        self.toggled.connect(self._animate)

    def setChecked(self, on):
        super().setChecked(on)
        self.anim.stop()
        self._pos = 1.0 if on else 0.0
        self.update()

    def _animate(self, on):
        self.anim.stop()
        self.anim.setStartValue(float(self._pos))
        self.anim.setEndValue(1.0 if on else 0.0)
        self.anim.start()

    def _step(self, v):
        self._pos = float(v)
        self.update()

    TW, TH = 38, 22

    def sizeHint(self):
        w = self.TW
        if self.text():
            w += 9 + self.fontMetrics().horizontalAdvance(self.text())
        return QSize(w + 2, max(self.TH + 2, self.fontMetrics().height() + 4))

    minimumSizeHint = sizeHint

    def hitButton(self, _pos):
        return True

    def paintEvent(self, _e):
        c = self.win.c
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        y = (self.height() - self.TH) / 2
        t = self._pos
        on, off = QColor(c["text"]), QColor(c["select"])
        track = QColor(int(off.red() + (on.red() - off.red()) * t), int(off.green() + (on.green() - off.green()) * t),
                       int(off.blue() + (on.blue() - off.blue()) * t))
        p.setPen(QPen(QColor(c["border"]), 1) if t < 0.5 else Qt.NoPen)
        p.setBrush(track)
        p.drawRoundedRect(QRectF(0.5, y + 0.5, self.TW - 1, self.TH - 1), self.TH / 2, self.TH / 2)
        k = self.TH - 6
        kx = 3 + t * (self.TW - 6 - k)
        p.setPen(Qt.NoPen)
        knob = QColor(c["bg"]) if t >= 0.5 else QColor(c["dim"])
        p.setBrush(knob)
        p.drawEllipse(QRectF(kx, y + 3, k, k))
        if not self.isEnabled():
            p.setBrush(QColor(0, 0, 0, 110))
            p.drawRoundedRect(QRectF(0, y, self.TW, self.TH), self.TH / 2, self.TH / 2)
        if self.text():
            p.setPen(QColor(c["text"] if self.isEnabled() else c["dim"]))
            p.drawText(QRectF(self.TW + 9, 0, self.width() - self.TW - 9, self.height()),
                       Qt.AlignVCenter | Qt.AlignLeft, self.text())


class Analyzer(QObject):
    """turns what the engine is playing right now into spectrum bands + a waveform for the visualizers.
    only runs while a visualizer is on screen."""
    frame = Signal()
    N = 96

    def __init__(self, win):
        super().__init__(win)
        self.win = win
        self.users = set()
        self.bands = np.zeros(self.N, np.float32) if np is not None else []
        self.peaks = np.zeros(self.N, np.float32) if np is not None else []
        self.wave = np.zeros(512, np.float32) if np is not None else []
        self.level = 0.0
        self.timer = QTimer(self)
        self.timer.setInterval(16)
        self.timer.timeout.connect(self.tick)
        self._edges = None
        self._idle = False

    def available(self):
        return np is not None and isinstance(self.win.player, NexusEngine)

    def attach(self, w):
        self.users.add(w)
        if self.available() and not self.timer.isActive():
            self.timer.start()

    def detach(self, w):
        self.users.discard(w)
        if not self.users:
            self.timer.stop()

    def listen(self, w):
        """widget repaints every analyzer frame while shown."""
        self.attach(w)
        if not getattr(w, "_listening", False):
            self.frame.connect(w.update)
            w._listening = True

    def unlisten(self, w):
        if getattr(w, "_listening", False):
            try:
                self.frame.disconnect(w.update)
            except (RuntimeError, TypeError):
                pass
            w._listening = False
        self.detach(w)

    def tick(self):
        eng = self.win.player
        n = 2048
        playing = eng.playbackState() == QMediaPlayer.PlayingState
        sens = max(0.3, float(self.win.cfg.get("vis_sensitivity", 100)) / 100)
        if playing:
            x = eng.vis_samples(n)
            sr = float(eng.stream_rate or 44100)
            if self._edges is None or self._edges[0] != sr:
                hi = min(16000.0, sr / 2 * 0.95)
                edges = np.geomspace(32.0, hi, self.N + 1)
                bins = np.clip((edges * n / sr).astype(int), 1, n // 2)
                self._edges = (sr, bins)
            bins = self._edges[1]
            spec = np.abs(np.fft.rfft(x * np.hanning(n).astype(np.float32)))
            spec = spec / (n / 4)
            out = np.empty(self.N, np.float32)
            for i in range(self.N):
                a, b = bins[i], max(bins[i] + 1, bins[i + 1])
                out[i] = spec[a:b].max()
            tilt = np.linspace(0, 9, self.N)          # pink-ish tilt so highs aren't always flat
            db = 20 * np.log10(out + 1e-7) + tilt
            v = np.clip((db + 57) / 54 * sens, 0, 1)
            self.level = float(np.sqrt(np.mean(x[-1024:] ** 2)) * 3 * sens)
            w = x[-1024::2]
            self.wave = np.clip(w * 2.2 * sens, -1, 1)
        else:
            v = np.zeros(self.N, np.float32)
            self.wave = self.wave * 0.8
            self.level *= 0.8
        up = v > self.bands
        self.bands = np.where(up, self.bands + (v - self.bands) * 0.65, self.bands * 0.88 + v * 0.12)
        self.peaks = np.maximum(self.peaks - 0.012, self.bands)
        quiet = not playing and float(self.peaks.max()) < 0.002 and self.level < 0.002
        if quiet and self._idle:
            return                               # paused and settled: stop repainting
        self._idle = quiet
        self.frame.emit()


def cover_color(win, key):
    """a lively colour picked from the album art (for 'cover' colour mode)."""
    cache = win.__dict__.setdefault("_cover_colors", {})
    if key in cache:
        return cache[key]
    col = None
    if key:
        img = win.covers.get(key, 24).toImage().scaled(16, 16, Qt.IgnoreAspectRatio, Qt.SmoothTransformation)
        best = -1
        for yy in range(img.height()):
            for xx in range(img.width()):
                q = QColor(img.pixel(xx, yy))
                score = q.hsvSaturationF() * 0.7 + q.valueF() * 0.3
                if q.valueF() > 0.25 and score > best:
                    best, col = score, q
        if col is not None and col.hsvSaturationF() < 0.18:
            col = None
        if col is not None:
            col = QColor.fromHsvF(col.hsvHueF(), min(1.0, col.hsvSaturationF() * 1.1), max(0.75, col.valueF()))
    cache[key] = col
    return col


class Visualizer(QWidget):
    """spectrum / waveform drawn from the Analyzer. styles: dots, bars, mirror, wave, ring."""
    STYLES = ("dots", "bars", "mirror", "wave", "ring")

    def __init__(self, win, style=None, pitch=11, cfg_key="vis_style"):
        super().__init__()
        self.win, self.style_, self.pitch, self.cfg_key = win, style, pitch, cfg_key
        self.center_cover = False
        self.setAttribute(Qt.WA_TransparentForMouseEvents, False)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)

    def style(self):
        return self.style_ or self.win.cfg.get(self.cfg_key, "dots")

    def showEvent(self, e):
        super().showEvent(e)
        self.win.analyzer.listen(self)

    def hideEvent(self, e):
        self.win.analyzer.unlisten(self)
        super().hideEvent(e)

    def color(self):
        c = self.win.c
        if self.win.cfg.get("vis_color", "mono") == "cover":
            t = self.win.queue[self.win.qi] if 0 <= self.win.qi < len(self.win.queue) else None
            col = cover_color(self.win, t["cover"]) if t else None
            if col is not None:
                return col
        return QColor(c["text"])

    @staticmethod
    def resample(v, n):
        if n <= 0:
            return v[:0]
        if np is None:
            return [0.0] * n
        xs = np.linspace(0, len(v) - 1, n)
        return np.interp(xs, np.arange(len(v)), v)

    def paintEvent(self, _e):
        an = self.win.analyzer
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        fg = self.color()
        dim = QColor(self.win.c["dim"])
        dim.setAlphaF(0.13)
        st = self.style()
        W, H = self.width(), self.height()
        if not an.available():
            p.setPen(QColor(self.win.c["dim"]))
            p.drawText(self.rect(), Qt.AlignCenter, "the visualizer needs the Nexus audio engine (settings → sound)")
            return
        getattr(self, "_" + (st if st in self.STYLES else "dots"))(p, an, fg, dim, W, H)

    def _dots(self, p, an, fg, dim, W, H):
        pitch = self.pitch
        cols, rows = max(4, int(W // pitch)), max(3, int(H // pitch))
        v = self.resample(an.bands, cols)
        pk = self.resample(an.peaks, cols)
        x0 = (W - cols * pitch) / 2 + pitch / 2
        y0 = H - (H - rows * pitch) / 2 - pitch / 2
        d = pitch * 0.46
        p.setPen(Qt.NoPen)
        for i in range(cols):
            lit = int(round(v[i] * rows))
            peak = min(rows - 1, int(round(pk[i] * rows)))
            x = x0 + i * pitch
            for j in range(rows):
                y = y0 - j * pitch
                if j < lit:
                    p.setBrush(fg)
                elif j == peak and peak > 0:
                    q = QColor(fg)
                    q.setAlphaF(0.55)
                    p.setBrush(q)
                else:
                    p.setBrush(dim)
                p.drawEllipse(QRectF(x - d / 2, y - d / 2, d, d))

    def _bars(self, p, an, fg, dim, W, H):
        n = max(8, int(W // max(6, self.pitch)))
        v = self.resample(an.bands, n)
        bw = W / n
        p.setPen(Qt.NoPen)
        for i in range(n):
            h = max(2.0, v[i] * H)
            p.setBrush(fg)
            p.drawRoundedRect(QRectF(i * bw + bw * 0.18, H - h, bw * 0.64, h), bw * 0.3, bw * 0.3)

    def _mirror(self, p, an, fg, dim, W, H):
        n = max(8, int(W // max(6, self.pitch)))
        half = self.resample(an.bands, (n + 1) // 2)
        v = list(half[::-1]) + list(half)
        n = len(v)
        bw = W / n
        cy = H / 2
        p.setPen(Qt.NoPen)
        for i in range(n):
            h = max(2.0, v[i] * H * 0.92)
            p.setBrush(fg)
            p.drawRoundedRect(QRectF(i * bw + bw * 0.2, cy - h / 2, bw * 0.6, h), bw * 0.3, bw * 0.3)

    def _wave(self, p, an, fg, dim, W, H):
        w = an.wave
        n = len(w)
        if not n:
            return
        cy = H / 2
        path = QPainterPath()
        for i in range(n):
            x = i / (n - 1) * W
            y = cy - float(w[i]) * H * 0.45
            path.moveTo(x, y) if i == 0 else path.lineTo(x, y)
        glow = QColor(fg)
        glow.setAlphaF(0.18)
        p.setBrush(Qt.NoBrush)
        p.setPen(QPen(glow, 7, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin))
        p.drawPath(path)
        p.setPen(QPen(fg, 2, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin))
        p.drawPath(path)

    def _ring(self, p, an, fg, dim, W, H):
        cx, cy = W / 2, H / 2
        R = min(W, H) * 0.27
        n = 72
        half = self.resample(an.bands, n // 2)
        v = list(half) + list(half[::-1])
        if self.center_cover:
            t = self.win.queue[self.win.qi] if 0 <= self.win.qi < len(self.win.queue) else None
            pm = self.win.covers.get(t["cover"] if t else "", int(R * 1.7))
            r = R * 0.86 * (1 + min(0.06, an.level * 0.05))
            clip = QPainterPath()
            clip.addEllipse(QRectF(cx - r, cy - r, r * 2, r * 2))
            p.save()
            p.setClipPath(clip)
            p.drawPixmap(QRectF(cx - r, cy - r, r * 2, r * 2), pm, QRectF(0, 0, pm.width(), pm.height()))
            p.restore()
        ext = min(W, H) / 2 - R - 6
        step = 9.0
        p.setPen(Qt.NoPen)
        for i in range(n):
            a = -math.pi / 2 + i / n * 2 * math.pi
            k = max(1, int(v[i] * ext / step) + 1)
            for j in range(int(ext / step)):
                rr = R + j * step
                p.setBrush(fg if j < k else dim)
                d = 4.4 if j < k else 3.2
                p.drawEllipse(QRectF(cx + rr * math.cos(a) - d / 2, cy + rr * math.sin(a) - d / 2, d, d))


class EqGraph(QWidget):
    """the EQ curve over a live spectrum. drag a band's node up/down to change it."""
    bandDragged = Signal(int, float)

    def __init__(self, win, page):
        super().__init__()
        self.win, self.page = win, page
        self.drag = None
        self.setMinimumHeight(170)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        self.setCursor(Qt.PointingHandCursor)
        self.setToolTip("drag a point to change that band")

    def showEvent(self, e):
        super().showEvent(e)
        self.win.analyzer.listen(self)

    def hideEvent(self, e):
        self.win.analyzer.unlisten(self)
        super().hideEvent(e)

    M = 26

    def fx(self, f):
        lo, hi = math.log10(20), math.log10(20000)
        return self.M + (math.log10(f) - lo) / (hi - lo) * (self.width() - 2 * self.M)

    def dy(self, db):
        top, bot = 18, self.height() - 26
        return top + (12 - db) / 24 * (bot - top)

    def paintEvent(self, _e):
        c = self.win.c
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        W, H = self.width(), self.height()
        an = self.win.analyzer
        dimc = QColor(c["dim"])
        # live spectrum behind
        if an.available() and len(an.bands):
            pitch = 9
            x_lo, x_hi = self.fx(32), self.fx(16000)
            cols = max(8, int((x_hi - x_lo) // pitch))
            v = Visualizer.resample(an.bands, cols)
            rows = int((H - 26 - 18) // pitch)
            q = QColor(c["text"])
            q.setAlphaF(0.16)
            p.setPen(Qt.NoPen)
            p.setBrush(q)
            for i in range(cols):
                x = x_lo + i * pitch + pitch / 2
                for j in range(int(v[i] * rows)):
                    y = H - 26 - j * pitch - pitch / 2
                    p.drawEllipse(QRectF(x - 1.7, y - 1.7, 3.4, 3.4))
        # grid
        p.setFont(QFont(self.font().family(), max(7, self.font().pointSize() - 2)))
        for db in (12, 6, 0, -6, -12):
            y = self.dy(db)
            g = QColor(c["border"]) if db else QColor(c["dim"])
            if not db:
                g.setAlphaF(0.5)
            p.setPen(QPen(g, 1, Qt.SolidLine if not db else Qt.DotLine))
            p.drawLine(QPointF(self.M, y), QPointF(W - self.M, y))
            p.setPen(dimc)
            p.drawText(QRectF(0, y - 8, self.M - 4, 16), Qt.AlignRight | Qt.AlignVCenter, f"{db:+d}" if db else "0")
        for f, lab in ((50, "50"), (100, "100"), (200, "200"), (500, "500"), (1000, "1k"), (2000, "2k"),
                       (5000, "5k"), (10000, "10k")):
            x = self.fx(f)
            p.setPen(QPen(QColor(c["border"]), 1, Qt.DotLine))
            p.drawLine(QPointF(x, 18), QPointF(x, H - 26))
            p.setPen(dimc)
            p.drawText(QRectF(x - 20, H - 22, 40, 18), Qt.AlignCenter, lab)
        # curve
        cur = self.page.current()
        gains = [g + cur["preamp"] for g in cur["bands"]]
        on = cur["on"]
        pts = [QPointF(self.fx(f), self.dy(max(-12, min(12, g)))) for f, g in zip(EQ_BANDS, gains)]
        path = QPainterPath(QPointF(self.M, pts[0].y()))
        path.lineTo(pts[0])
        for i in range(len(pts) - 1):
            a, b = pts[i], pts[i + 1]
            mx = (a.x() + b.x()) / 2
            path.cubicTo(QPointF(mx, a.y()), QPointF(mx, b.y()), b)
        path.lineTo(QPointF(W - self.M, pts[-1].y()))
        fg = QColor(c["text"])
        if not on:
            fg = QColor(c["dim"])
        fill = QPainterPath(path)
        fill.lineTo(W - self.M, self.dy(0))
        fill.lineTo(self.M, self.dy(0))
        fill.closeSubpath()
        fc = QColor(fg)
        fc.setAlphaF(0.08)
        p.setPen(Qt.NoPen)
        p.setBrush(fc)
        p.drawPath(fill)
        p.setPen(QPen(fg, 2.2, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin))
        p.setBrush(Qt.NoBrush)
        p.drawPath(path)
        for i, pt in enumerate(pts):
            big = self.drag == i
            r = 7 if big else 5
            p.setPen(Qt.NoPen)
            p.setBrush(fg)
            p.drawEllipse(pt, r, r)
            p.setBrush(QColor(c["panel"]))
            p.drawEllipse(pt, r - 2.5, r - 2.5)

    def _band_at(self, x):
        xs = [self.fx(f) for f in EQ_BANDS]
        i = min(range(len(xs)), key=lambda k: abs(xs[k] - x))
        return i if abs(xs[i] - x) < 40 else None

    def _db_at(self, y):
        top, bot = 18, self.height() - 26
        return 12 - (y - top) / max(1, bot - top) * 24 - self.page.current()["preamp"]

    def mousePressEvent(self, e):
        self.drag = self._band_at(e.position().x())
        if self.drag is not None:
            self.bandDragged.emit(self.drag, self._db_at(e.position().y()))
        self.update()

    def mouseMoveEvent(self, e):
        if self.drag is not None and e.buttons() & Qt.LeftButton:
            self.bandDragged.emit(self.drag, self._db_at(e.position().y()))

    def mouseReleaseEvent(self, _e):
        self.drag = None
        self.update()

    def mouseDoubleClickEvent(self, e):
        i = self._band_at(e.position().x())
        if i is not None:
            self.bandDragged.emit(i, 0.0)


class DotSlider(QWidget):
    """vertical dot-column slider for the EQ (-12…+12 dB). grows with the window."""
    valueChanged = Signal(float)

    def __init__(self, win, lo=-12.0, hi=12.0):
        super().__init__()
        self.win, self.lo, self.hi, self.v = win, lo, hi, 0.0
        self.setMinimumSize(30, 120)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        self.setCursor(Qt.PointingHandCursor)
        self.setToolTip("drag · scroll · double-click resets")

    def set_value(self, v, emit=False):
        v = round(min(self.hi, max(self.lo, float(v))) * 2) / 2
        if v != self.v:
            self.v = v
            self.update()
            if emit:
                self.valueChanged.emit(v)

    def _y(self, v):
        m = 10
        return m + (self.hi - v) / (self.hi - self.lo) * (self.height() - 2 * m)

    def paintEvent(self, _e):
        c = self.win.c
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        fg, dim = QColor(c["text"]), QColor(c["dim"])
        dim.setAlphaF(0.3)
        cx = self.width() / 2
        steps = 24 if self.height() < 330 else 48
        yv = self._y(self.v)
        p.setPen(Qt.NoPen)
        for i in range(steps + 1):
            val = self.hi - i * (self.hi - self.lo) / steps
            y = self._y(val)
            on = (0 <= val <= self.v) or (self.v <= val <= 0)
            d = 4.2 if abs(val) > 1e-9 else 6
            p.setBrush(fg if on and self.v != 0 else dim if abs(val) > 1e-9 else QColor(c["dim"]))
            p.drawEllipse(QRectF(cx - d / 2, y - d / 2, d, d))
        p.setBrush(fg)
        p.drawEllipse(QRectF(cx - 8, yv - 8, 16, 16))
        p.setBrush(QColor(c["panel"]))
        p.drawEllipse(QRectF(cx - 3, yv - 3, 6, 6))

    def _from_y(self, y):
        m = 10
        frac = (y - m) / max(1, self.height() - 2 * m)
        self.set_value(self.hi - frac * (self.hi - self.lo), emit=True)

    def mousePressEvent(self, e):
        self._from_y(e.position().y())

    def mouseMoveEvent(self, e):
        if e.buttons() & Qt.LeftButton:
            self._from_y(e.position().y())

    def mouseDoubleClickEvent(self, _e):
        self.set_value(0, emit=True)

    def wheelEvent(self, e):
        self.set_value(self.v + (0.5 if e.angleDelta().y() > 0 else -0.5), emit=True)


class EqualizerPage(QWidget):
    def __init__(self, win):
        super().__init__(objectName="eqpage")
        self.win = win
        eq = dict(win.cfg.get("eq") or {})
        page = QVBoxLayout(self)
        page.setContentsMargins(32, 28, 28, 22)
        page.setSpacing(0)
        top = QHBoxLayout()
        top.addWidget(QLabel("Equalizer", objectName="h1"))
        top.addStretch()
        self.on = ToggleSwitch(win, "on")
        self.on.setChecked(bool(eq.get("on")))
        self.on.toggled.connect(self.changed)
        top.addWidget(self.on)
        top.addSpacing(14)
        top.addWidget(win.tbtn("reset", lambda: self.pick_preset("flat", force=True), "pill"))
        top.addSpacing(6)
        top.addWidget(win.tbtn("done", win.back, "pillPrimary"))
        page.addLayout(top)
        page.addSpacing(16)
        if not isinstance(win.player, NexusEngine):
            note = QLabel("The EQ needs the Nexus audio engine (settings → sound). It's currently using the basic Qt player.",
                          objectName="dim")
            note.setWordWrap(True)
            page.addWidget(note)
            page.addSpacing(8)
        # preset chips
        chips = QHBoxLayout()
        chips.setSpacing(6)
        chips.setContentsMargins(0, 3, 0, 5)
        self.chips = {}
        for name in list(EQ_PRESETS) + ["custom"]:
            b = QPushButton(win.L(name), objectName="chip")
            b.setCheckable(True)
            b.setCursor(Qt.PointingHandCursor)
            if name == "custom":
                b.setEnabled(False)
            else:
                b.clicked.connect(lambda _=False, n=name: self.pick_preset(n))
            self.chips[name] = b
            chips.addWidget(b)
        chips.addStretch()
        cw = QWidget()
        cw.setLayout(chips)
        cs = QScrollArea()
        cs.setWidget(cw)
        cs.setWidgetResizable(True)
        cs.setFrameShape(QFrame.NoFrame)
        cs.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)       # scroll wheel still slides it on narrow windows
        cs.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        cs.setFixedHeight(54)
        page.addWidget(cs)
        page.addSpacing(10)
        # graph
        self.sliders, self.vals = [], []
        self.graph = EqGraph(win, self)
        self.graph.bandDragged.connect(self.graph_drag)
        page.addWidget(self.graph, 5)
        page.addSpacing(14)
        # sliders: preamp | 10 bands, spread across the full width
        row = QHBoxLayout()
        row.setSpacing(0)
        bands = list(eq.get("bands") or [0] * 10)
        cols = [("pre", float(eq.get("preamp", 0)))] + [
            (f"{b // 1000}k" if b >= 1000 else str(b), bands[i] if i < len(bands) else 0) for i, b in enumerate(EQ_BANDS)]
        for i, (lab, val) in enumerate(cols):
            col = QVBoxLayout()
            col.setSpacing(6)
            vl = QLabel(objectName="eqval")
            vl.setAlignment(Qt.AlignCenter)
            s = DotSlider(win)
            s.set_value(val)
            s.valueChanged.connect(self.slider_moved)
            bl = QLabel(lab if i else "preamp", objectName="eyebrow")
            bl.setAlignment(Qt.AlignCenter)
            col.addWidget(vl)
            col.addWidget(s, 1)
            col.addWidget(bl)
            row.addLayout(col, 1)
            if i == 0:
                row.addSpacing(8)
                row.addWidget(vline())
                row.addSpacing(8)
            self.sliders.append(s)
            self.vals.append(vl)
        page.addLayout(row, 4)
        page.addSpacing(10)
        hint = QLabel("drag the dots or the curve · scroll on a slider for fine steps · double-click resets a band. "
                      "Boosting can clip — the limiter catches it, but a negative preamp sounds cleaner.",
                      objectName="time")
        hint.setWordWrap(True)
        page.addWidget(hint)
        self.preset_name = eq.get("preset", "flat")
        self.save_timer = QTimer(self)
        self.save_timer.setSingleShot(True)
        self.save_timer.setInterval(400)
        self.save_timer.timeout.connect(win.cfg.save)
        self.refresh_labels()

    def refresh_labels(self):
        for s, l in zip(self.sliders, self.vals):
            l.setText(f"{s.v:+g}" if s.v else "0")
        for n, b in self.chips.items():
            b.setChecked(n == self.preset_name)
        self.graph.update()

    def current(self):
        return {"on": self.on.isChecked(), "preset": self.preset_name,
                "preamp": self.sliders[0].v if self.sliders else 0.0,
                "bands": [s.v for s in self.sliders[1:]] if self.sliders else [0] * 10}

    def pick_preset(self, name, force=False):
        if name not in EQ_PRESETS:
            return
        for s, g in zip(self.sliders[1:], EQ_PRESETS[name]):
            s.set_value(g)
        if force:
            self.sliders[0].set_value(0)
        self.preset_name = name
        if name != "flat" and not self.on.isChecked():
            self.on.setChecked(True)
        self.changed()

    def graph_drag(self, i, db):
        self.sliders[i + 1].set_value(db, emit=True)

    def slider_moved(self, _v):
        cur = [s.v for s in self.sliders[1:]]
        self.preset_name = next((n for n, g in EQ_PRESETS.items() if g == cur), "custom")
        if any(cur) and not self.on.isChecked():
            self.on.blockSignals(True)
            self.on.setChecked(True)
            self.on.blockSignals(False)
        self.changed()

    def changed(self, *_):
        eq = self.current()
        self.win.cfg["eq"] = eq
        if isinstance(self.win.player, NexusEngine):
            self.win.player.set_eq(eq)
        self.refresh_labels()
        self.save_timer.start()
        self.win.update_controls()


# ------------------------------------------------------------------ lyrics
import re as _re

_LRC_TIME = _re.compile(r"\[(\d{1,3}):(\d{1,2}(?:[.:]\d{1,3})?)\]")
_LRC_OFFSET = _re.compile(r"\[offset:\s*([+-]?\d+)\s*\]", _re.I)


def parse_lrc(text):
    """→ sorted [(ms, line)] or None if the text has no timestamps."""
    if not text or not _LRC_TIME.search(text):
        return None
    off = _LRC_OFFSET.search(text)
    offset = int(off.group(1)) if off else 0
    out = []
    for raw in text.splitlines():
        stamps = list(_LRC_TIME.finditer(raw))
        if not stamps:
            continue
        line = _LRC_TIME.sub("", raw).strip()
        line = _re.sub(r"<\d{1,3}:\d{1,2}(?:[.:]\d{1,3})?>", "", line).strip()   # enhanced-lrc word stamps
        for m in stamps:
            sec = float(m.group(2).replace(":", "."))
            out.append((max(0, int((int(m.group(1)) * 60 + sec) * 1000) - offset), line))
    out.sort(key=lambda x: x[0])
    return out or None


def read_lyrics(path):
    """→ {"synced": [(ms, line)] | None, "plain": str | None, "source": str} or None."""
    p = Path(path)
    # 1) sidecar .lrc next to the song (same name, any case)
    try:
        for f in p.parent.iterdir():
            if f.suffix.lower() == ".lrc" and f.stem.lower() == p.stem.lower():
                text = f.read_text("utf-8", errors="replace")
                synced = parse_lrc(text)
                return {"synced": synced, "plain": None if synced else text.strip() or None, "source": ".lrc file"}
    except OSError:
        pass
    # 2) embedded
    try:
        f = mutagen.File(path)
    except Exception:
        f = None
    if f is None or f.tags is None:
        return None
    tags = f.tags
    texts = []
    try:                                   # ID3: synced SYLT first, then USLT
        sylt = tags.getall("SYLT")
        for fr in sylt:
            lines = [(int(t), str(txt).strip()) for txt, t in fr.text]
            if lines and getattr(fr, "format", 2) == 2:
                return {"synced": sorted(lines), "plain": None, "source": "tag (SYLT)"}
        texts += [fr.text for fr in tags.getall("USLT")]
    except AttributeError:
        pass
    try:
        if "\xa9lyr" in tags:              # MP4
            texts += [str(x) for x in tags["\xa9lyr"]]
    except Exception:
        pass
    try:                                   # FLAC / OGG / OPUS vorbis comments
        keys = {k.lower(): k for k in tags.keys()} if hasattr(tags, "keys") else {}
        for k in ("syncedlyrics", "lyrics", "unsyncedlyrics", "unsynced lyrics", "lyric"):
            if k in keys:
                v = tags[keys[k]]
                texts += [str(x) for x in (v if isinstance(v, list) else [v])]
    except Exception:
        pass
    for t in texts:
        synced = parse_lrc(t)
        if synced:
            return {"synced": synced, "plain": None, "source": "tag"}
    for t in texts:
        if t and t.strip():
            return {"synced": None, "plain": t.strip(), "source": "tag"}
    return None


class LyricsPage(QWidget):
    """apple-music-ish lyrics: synced lines light up and scroll; click a line to jump there."""

    def __init__(self, win):
        super().__init__(objectName="lyricspage")
        self.win = win
        self.path, self.data, self.lines, self.cur = None, None, [], -1
        self.times = []
        self.user_until = 0.0
        v = QVBoxLayout(self)
        v.setContentsMargins(40, 26, 28, 0)
        v.setSpacing(6)
        head = QHBoxLayout()
        self.cover = QLabel()
        self.cover.setFixedSize(52, 52)
        head.addWidget(self.cover)
        head.addSpacing(10)
        tv = QVBoxLayout()
        tv.setSpacing(1)
        self.kind = QLabel("LYRICS", objectName="eyebrow")
        self.title = QLabel(objectName="npTitle")
        self.sub = QLabel(objectName="dim")
        for wdg in (self.kind, self.title, self.sub):
            tv.addWidget(wdg)
        head.addLayout(tv, 1)
        head.addWidget(win.tbtn("done", win.back, "pillPrimary"), 0, Qt.AlignTop)
        v.addLayout(head)
        v.addSpacing(10)
        self.area = QScrollArea()
        self.area.setWidgetResizable(True)
        self.area.setFrameShape(QFrame.NoFrame)
        self.area.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.area.viewport().installEventFilter(self)
        self.inner = QWidget()
        self.col = QVBoxLayout(self.inner)
        self.col.setContentsMargins(0, 40, 40, 260)
        self.col.setSpacing(14)
        self.area.setWidget(self.inner)
        v.addWidget(self.area, 1)
        from PySide6.QtCore import QPropertyAnimation, QEasingCurve
        self.anim = QPropertyAnimation(self.area.verticalScrollBar(), b"value", self)
        self.anim.setDuration(420)
        self.anim.setEasingCurve(QEasingCurve.OutCubic)

    def eventFilter(self, obj, e):
        if e.type() == QEvent.Wheel:
            self.user_until = _time.monotonic() + 4.0     # let them scroll, resume following after a bit
        return False

    def load(self, t):
        path = t["path"] if t else None
        if path == self.path and self.lines:
            return
        self.path = path
        self.data = self.win.lyrics_for(t) if t else None
        while self.col.count():
            it = self.col.takeAt(0)
            if it.widget():
                it.widget().deleteLater()
        self.lines, self.times, self.cur = [], [], -1
        if t:
            self.cover.setPixmap(self.win.covers.get(t["cover"], 52))
            self.title.setText(t["title"])
            self.sub.setText(f"{t['artist']} — {t['album']}")
        else:
            self.cover.clear()
            self.title.setText(self.win.L("nothing playing"))
            self.sub.setText("")
        d = self.data
        if not d:
            self.kind.setText("LYRICS")
            msg = QLabel(self.win.L("no lyrics for this song.") + "\n\n" +
                         self.win.L("drop a .lrc file with the same name next to the song, or embed lyrics in its tags."),
                         objectName="dim")
            msg.setWordWrap(True)
            self.col.addWidget(msg)
            self.col.addStretch()
            return
        synced = d.get("synced")
        self.kind.setText("LYRICS · SYNCED" if synced else "LYRICS")
        rows = synced if synced else [(None, ln) for ln in (d.get("plain") or "").splitlines()]
        for ms, text in rows:
            lab = ClickLabel(text or " ", objectName="lyric")
            lab.setWordWrap(True)
            lab.setProperty("state", "future" if synced else "plain")
            if ms is not None:
                lab.setCursor(Qt.PointingHandCursor)
                lab.clicked.connect(lambda m=ms: self.seek(m))
            self.col.addWidget(lab)
            self.lines.append(lab)
            self.times.append(ms)
        self.col.addStretch()
        self.area.verticalScrollBar().setValue(0)
        if synced:
            self.set_pos(self.win.player.position(), force=True)

    def seek(self, ms):
        self.win.player.setPosition(ms)
        self.user_until = 0
        if self.win.player.playbackState() != QMediaPlayer.PlayingState:
            self.win.player.play()

    def set_pos(self, ms, force=False):
        if not self.lines or not self.times or self.times[0] is None:
            return
        import bisect
        i = bisect.bisect_right(self.times, ms + 150) - 1
        if i == self.cur and not force:
            return
        for j in {self.cur, i} | (set(range(len(self.lines))) if force else set()):
            if 0 <= j < len(self.lines):
                lab = self.lines[j]
                lab.setProperty("state", "current" if j == i else ("past" if j < i else "future"))
                lab.style().unpolish(lab)
                lab.style().polish(lab)
        self.cur = i
        if 0 <= i < len(self.lines) and _time.monotonic() > self.user_until:
            lab = self.lines[i]
            target = max(0, lab.y() + lab.height() // 2 - self.area.viewport().height() // 3)
            self.anim.stop()
            self.anim.setStartValue(self.area.verticalScrollBar().value())
            self.anim.setEndValue(target)
            self.anim.start()


# ------------------------------------------------------------------ soulseek downloads + importer
import subprocess
import urllib.error
import urllib.parse
import urllib.request
import uuid

LOSSLESS_EXT = {".flac", ".wav", ".aiff", ".aif", ".wv", ".ape", ".alac", ".dsf"}
class DownloadError(Exception):
    pass


FIREWALL_RULE = "Nexus Soulseek"


def firewall_ok():
    """windows: is our inbound rule there? (None = can't tell / not windows)"""
    if not sys.platform.startswith("win"):
        return None
    try:
        r = subprocess.run(["netsh", "advfirewall", "firewall", "show", "rule", f"name={FIREWALL_RULE}"],
                           capture_output=True, text=True, timeout=8, creationflags=0x08000000)
        return r.returncode == 0 and FIREWALL_RULE in (r.stdout or "")
    except Exception:
        return None


def firewall_fix(port):
    """one admin prompt: (re)creates inbound allow rules for python on just the soulseek ports."""
    import ctypes
    exe_dir = Path(sys.executable).parent
    progs = [str(p) for p in dict.fromkeys((exe_dir / "pythonw.exe", exe_dir / "python.exe", Path(sys.executable)))
             if Path(p).exists()]
    ports = f"{port},{port + 1}"
    cmds = [f'netsh advfirewall firewall delete rule name="{FIREWALL_RULE}"']
    for prog in progs:
        cmds.append(f'netsh advfirewall firewall add rule name="{FIREWALL_RULE}" dir=in action=allow '
                    f'program="{prog}" protocol=TCP localport={ports} enable=yes profile=any')
    for i in range(1, 4):                                   # older Nexus versions made these broader rules
        cmds.append(f'netsh advfirewall firewall delete rule name="Nexus Soulseek {i}"')
    rc = ctypes.windll.shell32.ShellExecuteW(None, "runas", "cmd.exe", "/c " + " & ".join(cmds), None, 0)
    return rc > 32


def remote_split(path):
    p = path.replace("\\", "/")
    d, _, base = p.rpartition("/")
    return d, base


def group_results(responses, lossless_only=True):
    """soulseek responses → album-ish rows (one per user+folder), best first."""
    rows = []
    for r in responses:
        dirs = collections.OrderedDict()
        for f in r.get("files") or []:
            d, base = remote_split(f.get("filename", ""))
            dirs.setdefault(d, []).append(f)
        for d, fs in dirs.items():
            audio = [f for f in fs if os.path.splitext(f.get("filename", ""))[1].lower() in AUDIO_EXT]
            if not audio:
                continue
            exts = {os.path.splitext(f["filename"])[1].lower() for f in audio}
            lossless = exts <= LOSSLESS_EXT
            if lossless_only and not lossless:
                continue
            bd = max((f.get("bitDepth") or 0) for f in audio)
            sr = max((f.get("sampleRate") or 0) for f in audio)
            br = max((f.get("bitRate") or 0) for f in audio)
            fmt = "/".join(sorted(e.lstrip(".").upper() for e in exts))
            if bd and sr:
                fmt += f" {bd}/{sr / 1000:g}"
            elif br:
                fmt += f" {br}k"
            images = [f for f in fs if os.path.splitext(f["filename"])[1].lower() in IMG_EXT]
            size = sum(int(f.get("size") or 0) for f in audio)
            free, speed, queue = bool(r.get("hasFreeUploadSlot")), int(r.get("uploadSpeed") or 0), \
                int(r.get("queueLength") or 0)
            score = (2 if lossless else 0) + (1 if bd >= 24 else 0) + (3 if free else 0) \
                + min(speed / 2_000_000, 2) - min(queue / 20, 2) + min(len(audio) / 10, 1)
            rows.append(dict(user=r.get("username", "?"), dir=d, name=remote_split(d)[1] or d, audio=audio,
                             images=images, fmt=fmt, size=size, free=free, speed=speed, queue=queue,
                             lossless=lossless, score=score,
                             length=sum(int(f.get("length") or 0) for f in audio)))
    rows.sort(key=lambda x: -x["score"])
    return rows


def human_size(n):
    for unit in ("B", "KB", "MB", "GB"):
        if n < 1024 or unit == "GB":
            return f"{n:.0f} {unit}" if unit in ("B", "KB") else f"{n:.1f} {unit}"
        n /= 1024


def safe_name(s, fallback="Unknown"):
    s = re.sub(r'[<>:"/\\|?*\x00-\x1f]', "_", str(s or "")).strip().rstrip(". ")
    return (s or fallback)[:120]


class Importer:
    """moves new music into the library as <root>/<Album Artist>/<Album>/<file> and triggers a rescan."""
    TEMP_EXT = (".crdownload", ".part", ".partial", ".tmp", ".download", ".opdownload")

    def __init__(self, win):
        self.win = win
        self.rescan_timer = QTimer(win)
        self.rescan_timer.setSingleShot(True)
        self.rescan_timer.setInterval(2500)
        self.rescan_timer.timeout.connect(lambda: win.rescan())
        self.log = collections.deque(maxlen=50)

    def root(self):
        cfg = self.win.cfg
        r = (cfg.get("import_root") or "").strip() or (cfg["library_folders"][0] if cfg["library_folders"] else "")
        r = Path(os.path.expanduser(r)) if r else Path.home() / "Music"
        r.mkdir(parents=True, exist_ok=True)
        if not any(str(r).startswith(str(Path(f))) for f in cfg["library_folders"]):
            cfg["library_folders"].append(str(r))
            cfg.save()
        return r

    def import_files(self, paths, move=True):
        root, done = self.root(), []
        for p in paths:
            p = Path(p)
            if not p.is_file() or p.suffix.lower() not in AUDIO_EXT:
                continue
            t = read_track(str(p))
            if not t:
                continue
            dest_dir = root / safe_name(t["albumartist"], "Unknown Artist") / safe_name(t["album"], "Unknown Album")
            dest_dir.mkdir(parents=True, exist_ok=True)
            dest = dest_dir / safe_name(p.name, "track" + p.suffix)
            try:
                if dest.exists() and dest.stat().st_size == p.stat().st_size:
                    if move and dest.resolve() != p.resolve():
                        p.unlink()
                elif move:
                    shutil.move(str(p), str(dest))
                else:
                    shutil.copy2(str(p), str(dest))
            except OSError as e:
                self.log.appendleft(f"couldn't import {p.name}: {e}")
                continue
            for img in p.parent.iterdir() if p.parent.exists() else []:   # bring album art along
                if img.suffix.lower() in IMG_EXT and img.stem.lower() in COVER_NAMES and not (dest_dir / img.name).exists():
                    try:
                        shutil.copy2(str(img), str(dest_dir / img.name))
                    except OSError:
                        pass
            done.append(str(dest))
            self.log.appendleft(f"imported {t['artist']} — {t['title']}")
        if done:
            self.rescan_timer.start()
        return done

    def import_zip(self, zpath):
        import tempfile
        import zipfile
        tmp = Path(tempfile.mkdtemp(prefix="nexus-import-"))
        files = []
        try:
            with zipfile.ZipFile(zpath) as z:
                for info in z.infolist():
                    name = info.filename.replace("\\", "/")
                    ext = os.path.splitext(name)[1].lower()
                    if info.is_dir() or ".." in name.split("/") or ext not in AUDIO_EXT | IMG_EXT:
                        continue
                    out = tmp / safe_name(os.path.dirname(name) or "_", "_") / safe_name(os.path.basename(name))
                    out.parent.mkdir(parents=True, exist_ok=True)
                    with z.open(info) as src, open(out, "wb") as dst:
                        shutil.copyfileobj(src, dst)
                    if ext in AUDIO_EXT:
                        files.append(out)
            return self.import_files(files, move=True)
        except Exception as e:
            self.log.appendleft(f"couldn't read {Path(zpath).name}: {e}")
            return []
        finally:
            shutil.rmtree(tmp, ignore_errors=True)


class WatchFolder:
    """auto-imports audio files / zips that land in a folder (e.g. Downloads). only new files, never old ones."""

    def __init__(self, win, importer):
        from PySide6.QtCore import QFileSystemWatcher
        self.win, self.importer = win, importer
        self.watcher = QFileSystemWatcher(win)
        self.watcher.directoryChanged.connect(lambda _p: self.timer.start())
        self.timer = QTimer(win)
        self.timer.setSingleShot(True)
        self.timer.setInterval(4000)
        self.timer.timeout.connect(self.process)
        self.seen_path = win.dir / "watch_seen.json"
        self.seen = set(load_json(self.seen_path, []))
        self.sizes = {}
        self.folder = None
        self.apply()

    def apply(self):
        cfg = self.win.cfg
        folder = os.path.expanduser((cfg.get("watch_folder") or "").strip())
        on = bool(cfg.get("watch_enabled")) and folder and os.path.isdir(folder)
        if self.watcher.directories():
            self.watcher.removePaths(self.watcher.directories())
        self.folder = folder if on else None
        if on:
            if folder != cfg.get("_watch_marked"):
                for e in os.scandir(folder):                         # everything already there counts as old
                    self.seen.add(self.key(e))
                cfg["_watch_marked"] = folder
                cfg.save()
                self.save()
            self.watcher.addPath(folder)

    @staticmethod
    def key(entry):
        try:
            st = entry.stat()
            return f"{entry.path}|{st.st_size}|{int(st.st_mtime)}"
        except OSError:
            return entry.path

    def save(self):
        try:
            save_json(self.seen_path, sorted(self.seen)[-5000:])
        except OSError:
            pass

    def process(self):
        if not self.folder or not os.path.isdir(self.folder):
            return
        pending, again = [], False
        for e in os.scandir(self.folder):
            if not e.is_file():
                continue
            name = e.name.lower()
            ext = os.path.splitext(name)[1]
            if name.endswith(Importer.TEMP_EXT) or (ext not in AUDIO_EXT and ext != ".zip"):
                continue
            k = self.key(e)
            if k in self.seen:
                continue
            size = e.stat().st_size
            if self.sizes.get(e.path) != size:        # still downloading? wait for the size to settle
                self.sizes[e.path] = size
                again = True
                continue
            pending.append((e.path, ext, k))
        for path, ext, k in pending:
            self.seen.add(k)
            self.sizes.pop(path, None)
            if ext == ".zip":
                self.importer.import_zip(path)
            else:
                self.importer.import_files([path], move=True)
        if pending:
            self.save()
            self.win.downloads_changed()
        if again:
            self.timer.start()


class SoulseekNative:
    """Nexus as its own Soulseek client (aioslsk on a background asyncio thread).
    the Download page talks to it through a small search / enqueue / downloads interface."""

    STATE_MAP = {"COMPLETE": "Completed, Succeeded", "FAILED": "Completed, Errored",
                 "ABORTED": "Completed, Cancelled", "DOWNLOADING": "InProgress", "INITIALIZING": "Initializing",
                 "PAUSED": "Paused", "VIRGIN": "Requested", "INCOMPLETE": "Queued, Locally"}

    def __init__(self, win):
        self.win = win
        self.loop = self.thread = self.client = self.stop_ev = None
        self.state, self.error = "off", ""
        self.base = "soulseek network"

    @staticmethod
    def available():
        try:
            import aioslsk  # noqa: F401
            return True, ""
        except Exception as e:
            return False, f"{type(e).__name__}: {e}"

    def creds(self):
        cfg = self.win.cfg
        return (cfg.get("slsk_user") or "").strip(), cfg.get("slsk_password") or ""

    def ready(self):
        u, p = self.creds()
        return bool(u and p)

    def label(self):
        u, _ = self.creds()
        return {"online": f"soulseek · {u} · online", "connecting": "soulseek · connecting…",
                "error": f"soulseek · {self.error}"[:90]}.get(self.state, f"soulseek · {u or 'no login set'}")

    # ---- lifecycle
    def ensure_started(self, timeout=25):
        if self.state == "online":
            return
        ok, why = self.available()
        if not ok:
            raise DownloadError(f"soulseek library missing ({why}) — restart Nexus to install it")
        if not self.ready():
            raise DownloadError("set a soulseek username + password in settings → downloads")
        if self.thread is None or not self.thread.is_alive():
            self.state, self.error = "connecting", ""
            self.thread = threading.Thread(target=self._run, daemon=True)
            self.thread.start()
        t0 = _time.monotonic()
        while self.state == "connecting" and _time.monotonic() - t0 < timeout:
            _time.sleep(0.1)
        if self.state != "online":
            raise DownloadError(self.error or "couldn't connect to soulseek")

    def restart(self):
        self.stop()
        if self.thread is not None:
            self.thread.join(5)
        self.thread, self.state = None, "off"

    def stop(self, wait=0):
        if self.loop is not None and self.stop_ev is not None:
            try:
                self.loop.call_soon_threadsafe(self.stop_ev.set)
            except RuntimeError:
                pass
        if wait and self.thread is not None and self.thread.is_alive():
            self.thread.join(wait)      # lets aioslsk save unfinished downloads so they resume next launch

    def log_path(self):
        return self.win.dir / "soulseek.log"

    def _setup_log(self):
        import logging
        from logging.handlers import RotatingFileHandler
        lg = logging.getLogger("aioslsk")
        if not any(isinstance(h, RotatingFileHandler) for h in lg.handlers):
            h = RotatingFileHandler(self.log_path(), maxBytes=1_500_000, backupCount=1, encoding="utf-8")
            h.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s"))
            lg.addHandler(h)
        lg.setLevel(logging.INFO)
        self.log = logging.getLogger("aioslsk.nexus")

    def _run(self):
        import asyncio
        self._setup_log()
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)
        try:
            self.loop.run_until_complete(self._main())
        except Exception as e:
            self.error, self.state = f"{type(e).__name__}: {e}", "error"
        finally:
            self.loop.close()
            self.loop = None
            if self.state == "online":
                self.state = "off"

    async def _main(self):
        import asyncio
        from aioslsk.client import SoulSeekClient
        from aioslsk.exceptions import AuthenticationError
        from aioslsk.settings import (CredentialsSettings, ListeningSettings, NetworkSettings, ReconnectSettings,
                                      ServerSettings, Settings, SharedDirectorySettingEntry, SharesSettings,
                                      TransferLimitSettings, TransfersSettings, UpnpSettings, UserInfoSettings)
        from aioslsk.shares.cache import SharesShelveCache
        from aioslsk.transfer.cache import TransferShelveCache
        cfg = self.win.cfg
        user, pw = self.creds()
        data = self.win.dir / "soulseek"
        dl = Path(os.path.expanduser(cfg.get("slsk_download_dir") or "")) if cfg.get("slsk_download_dir") \
            else data / "downloads"
        dl.mkdir(parents=True, exist_ok=True)
        self.download_dir = str(dl)
        shares = []
        if cfg.get("slsk_share_library", True):
            shares += list(cfg["library_folders"])
        shares += [s.strip() for s in (cfg.get("slsk_share_extra") or "").split(";") if s.strip()]
        shares = [SharedDirectorySettingEntry(path=os.path.expanduser(s)) for s in dict.fromkeys(shares)
                  if os.path.isdir(os.path.expanduser(s))]
        port = int(cfg.get("slsk_port", 2234))
        settings = Settings(
            credentials=CredentialsSettings(username=user, password=pw,
                                            info=UserInfoSettings(description=f"{APP_NAME} music player")),
            network=NetworkSettings(
                server=ServerSettings(hostname=cfg.get("slsk_server", "vps.slsknet.org"),
                                      port=int(cfg.get("slsk_server_port", 2271)),
                                      reconnect=ReconnectSettings(auto=True)),
                listening=ListeningSettings(port=port, obfuscated_port=port + 1),
                upnp=UpnpSettings(enabled=bool(cfg.get("slsk_upnp", True)))),
            shares=SharesSettings(scan_on_start=True, download=str(dl), directories=shares),
            transfers=TransfersSettings(limits=TransferLimitSettings(upload_slots=int(cfg.get("slsk_upload_slots", 2)))),
        )
        self.client = SoulSeekClient(settings, shares_cache=SharesShelveCache(str(data)),
                                     transfer_cache=TransferShelveCache(str(data)))
        self.stop_ev = asyncio.Event()
        try:
            await self.client.start()
            await self.client.login()
        except AuthenticationError as e:
            self.error = f"login refused ({getattr(e, 'reason', '') or e}) — wrong password, or the name is taken"
            self.state = "error"
            await self.client.stop()
            return
        except Exception as e:
            self.error, self.state = f"can't connect: {e}", "error"
            try:
                await self.client.stop()
            except Exception:
                pass
            return
        self.state = "online"
        try:
            self.log.info("logged in as %s, listening ports %s, sharing %d folders",
                          user, self.client.network.get_listening_ports(), len(shares))
        except Exception:
            pass
        await self.stop_ev.wait()
        await self.client.stop()

    def call(self, make_coro, timeout=30):
        import asyncio
        if self.loop is None:
            raise DownloadError("soulseek isn't running")
        return asyncio.run_coroutine_threadsafe(make_coro(), self.loop).result(timeout)

    # ---- interface the Download page uses
    def status(self):
        self.ensure_started()
        return {"state": f"logged in as {self.creds()[0]}", "isLoggedIn": True}

    @staticmethod
    def _file(f):
        from aioslsk.protocol.primitives import AttributeKey
        try:
            a = f.get_attribute_map()
        except Exception:
            a = {}
        return {"filename": f.filename, "size": int(f.filesize or 0), "bitRate": a.get(AttributeKey.BITRATE),
                "length": a.get(AttributeKey.DURATION), "sampleRate": a.get(AttributeKey.SAMPLE_RATE),
                "bitDepth": a.get(AttributeKey.BIT_DEPTH)}

    def search(self, text, timeout_ms=15000, progress=None, cancelled=lambda: False):
        self.ensure_started()

        async def start():
            return await self.client.searches.search(text)
        req = self.call(start)

        async def snap():
            return [(r.username, r.has_free_slots, r.avg_speed, r.queue_size, list(r.shared_items))
                    for r in list(req.results)]
        end = _time.monotonic() + timeout_ms / 1000
        try:
            while _time.monotonic() < end and not cancelled():
                _time.sleep(0.5)
                if progress:
                    rs = self.call(snap)
                    progress(len(rs), sum(len(x[4]) for x in rs))
            rs = self.call(snap)
            try:
                self.log.info("search %r: %d users answered, %d files", text, len(rs), sum(len(x[4]) for x in rs))
            except Exception:
                pass
        finally:
            async def drop():
                try:
                    self.client.searches.remove_request(req)
                except Exception:
                    pass
            try:
                self.call(drop, 5)
            except Exception:
                pass
        return [{"username": u, "hasFreeUploadSlot": bool(free), "uploadSpeed": int(speed or 0),
                 "queueLength": int(q or 0), "files": [self._file(f) for f in items]}
                for u, free, speed, q, items in rs]

    def enqueue(self, username, files):
        self.ensure_started()

        async def go():
            for f in files:
                await self.client.transfers.download(username, f["filename"])
        return self.call(go)

    def diagnose(self, query=""):
        """why might a search come back empty? → short human explanation."""
        if self.state != "online":
            return self.error or "not connected to soulseek"

        async def snap():
            c = self.client
            ports = c.network.get_listening_ports()
            phrases = list(getattr(c.searches, "excluded_search_phrases", []) or [])
            try:
                parent = c.distributed_network.parent is not None
            except Exception:
                parent = None
            return ports, phrases, parent
        ports, phrases, parent = self.call(snap, 10)
        q = query.lower()
        hit = next((p for p in phrases if p and p.lower() in q), None)
        port = int(self.win.cfg.get("slsk_port", 2234))
        self.log.info("diagnose %r: ports=%s excluded=%d hit=%r parent=%s", query, ports, len(phrases), hit, parent)
        if hit:
            return (f"the Soulseek server lists “{hit}” as an excluded search phrase, so most clients won't answer "
                    f"searches containing it — try the album or song name without it")
        if not ports[0]:
            return (f"Nexus couldn't open its listening port {port} (something else may be using it) — "
                    f"pick another port in settings → downloads")
        if sys.platform.startswith("win"):
            return (f"nobody answered. most likely Windows Firewall is blocking Python, so peers can't reach you: "
                    f"settings → downloads → “allow through firewall”, or forward port {port} on your router")
        return f"nobody answered — check that port {port} is reachable (firewall / router port forward)"

    def downloads(self):
        if self.state != "online":
            return []

        async def snap():
            out = collections.OrderedDict()
            for t in self.client.transfers.get_downloads():
                name = t.state.VALUE.name
                st = self.STATE_MAP.get(name, name.title())
                if name == "QUEUED":
                    st = "Queued, Remotely" if t.remotely_queued else "Queued, Locally"
                d, _base = remote_split(t.remote_path)
                out.setdefault(t.username, collections.OrderedDict()).setdefault(d.replace("/", "\\"), []).append({
                    "id": f"{t.username}\x00{t.remote_path}", "filename": t.remote_path,
                    "size": int(t.filesize or 0), "bytesTransferred": int(t.bytes_transfered or 0), "state": st,
                    "localPath": t.local_path if name == "COMPLETE" else None,
                    "placeInQueue": t.place_in_queue, "failReason": t.fail_reason})
            return [{"username": u, "directories": [{"directory": d, "files": fs} for d, fs in ds.items()]}
                    for u, ds in out.items()]
        return self.call(snap)

    def cancel(self, username, tid, remove=True):
        _u, remote = tid.split("\x00", 1)

        async def go():
            for t in list(self.client.transfers.get_downloads()):
                if t.username == username and t.remote_path == remote:
                    await self.client.transfers.remove(t)
        return self.call(go)


class DownloadPage(QWidget):
    """search soulseek, queue albums, watch them land in the library."""

    def __init__(self, win):
        super().__init__(objectName="downloadpage")
        self.win = win
        self.rows, self.busy = [], False
        st = win.__dict__.setdefault("dl_state", {"pending": {}, "imported": set()})
        self.pending = st["pending"]            # (user, remote filename) → {"size", "album"}
        self.imported = st["imported"]
        self.jobs = st.setdefault("jobs", {})  # "user\0folder" → {name, user, dir, alts, tried, started}
        self.load_pending()
        self.transfers = []
        v = QVBoxLayout(self)
        v.setContentsMargins(32, 28, 28, 12)
        v.setSpacing(12)
        head = QHBoxLayout()
        head.addWidget(QLabel("Download", objectName="h1"))
        head.addStretch()
        self.conn = QLabel(objectName="time")
        head.addWidget(self.conn, 0, Qt.AlignBottom)
        head.addSpacing(10)
        head.addWidget(win.tbtn("connection", lambda: win.go("settings", "downloads"), "pill"), 0, Qt.AlignBottom)
        v.addLayout(head)
        sr = QHBoxLayout()
        sr.setSpacing(10)
        self.q = QLineEdit(objectName="search")
        self.q.setPlaceholderText(win.L("Search Soulseek — artist, album or song"))
        self.q.setMaximumWidth(620)
        self.q.returnPressed.connect(self.search)
        sr.addWidget(self.q, 1)
        self.lossless = ToggleSwitch(win, "lossless only")
        self.lossless.setChecked(bool(win.cfg.get("dl_lossless_only", True)))
        self.lossless.toggled.connect(self.lossless_toggled)
        sr.addWidget(self.lossless)
        sr.addWidget(win.tbtn("search", self.search, "pillPrimary"))
        sr.addStretch()
        v.addLayout(sr)
        self.status = QLabel(objectName="dim")
        v.addWidget(self.status)
        from PySide6.QtWidgets import QTreeWidget
        self.tree = QTreeWidget(objectName="dltree")
        self.tree.setHeaderLabels(["", "ALBUM / FOLDER", "FORMAT", "TRACKS", "SIZE", "USER", "SPEED", ""])
        self.tree.setRootIsDecorated(True)
        self.tree.setUniformRowHeights(True)
        self.tree.setIndentation(18)
        self.tree.setAllColumnsShowFocus(True)
        self.tree.itemDoubleClicked.connect(self.item_activated)
        hh = self.tree.header()
        hh.setStretchLastSection(False)
        hh.setSectionResizeMode(1, QHeaderView.Stretch)
        for i, wdt in ((0, 26), (2, 130), (3, 70), (4, 90), (5, 150), (6, 100), (7, 120)):
            hh.setSectionResizeMode(i, QHeaderView.Fixed)
            hh.resizeSection(i, wdt)
        v.addWidget(self.tree, 3)
        dh = QHBoxLayout()
        dh.addWidget(QLabel("DOWNLOADS", objectName="eyebrow"))
        dh.addStretch()
        dh.addWidget(win.tbtn("clear finished", self.clear_finished, "pill"))
        v.addLayout(dh)
        self.fw = QWidget(objectName="onlinebox")
        self.fw.setAttribute(Qt.WA_StyledBackground, True)
        fwl = QHBoxLayout(self.fw)
        fwl.setContentsMargins(14, 6, 8, 6)
        fwt = QLabel("Windows Firewall isn't letting other Soulseek users connect to you, so some downloads never "
                     "start. One admin prompt fixes it (only Nexus's download ports are opened).", objectName="dim")
        fwt.setWordWrap(True)
        fwl.addWidget(fwt, 1)
        fwl.addWidget(win.tbtn("fix firewall", self.fix_firewall, "pillPrimary"))
        self.fw.hide()
        v.addWidget(self.fw)
        self.dl = QListWidget(objectName="dllist")
        self.dl.setContextMenuPolicy(Qt.CustomContextMenu)
        self.dl.customContextMenuRequested.connect(self.dl_menu)
        self.dl.setToolTip("right-click a download for more")
        self.dl.setMaximumHeight(210)
        v.addWidget(self.dl, 1)
        self.timer = QTimer(self)
        self.timer.setInterval(2000)
        self.timer.timeout.connect(self.poll)
        self.refresh_conn()

    # ---- unfinished downloads survive restarts / updates
    def pending_file(self):
        return self.win.dir / "downloads_pending.json"

    def load_pending(self):
        try:
            d = json.loads(self.pending_file().read_text("utf-8"))
        except Exception:
            return
        if isinstance(d, list):                       # older format: just the pending list
            d = {"pending": d}
        for user, fn, info in d.get("pending") or []:
            self.pending.setdefault((user, fn), info)
        for user, fn in d.get("imported") or []:
            self.imported.add((user, fn))
        for k, j in (d.get("jobs") or {}).items():
            j["started"] = _time.time()                 # fresh clock after a restart: peers need a moment again
            self.jobs.setdefault(k, j)

    def save_pending(self):
        try:
            imp = list(self.imported)[-4000:]
            self.pending_file().write_text(json.dumps({
                "pending": [[u, f, i] for (u, f), i in self.pending.items()],
                "imported": [[u, f] for u, f in imp], "jobs": self.jobs}), "utf-8")
        except Exception:
            pass

    def resume_on_start(self):
        """after a restart / update: reconnect and let unfinished downloads carry on (and import when done)."""
        c = self.client()
        first = not self.pending_file().exists()          # first launch of a version that remembers downloads
        if not c.ready() or not (self.pending or (first and c is self.win.soulseek)):
            return
        if first:
            self.save_pending()

        def go():
            if c is self.win.soulseek:
                c.ensure_started()

        def done(_r, err):
            if err:
                return
            self.timer.start()
            self.poll()
        self.win.bg(go, done)

    # ---- connection
    def client(self):
        return self.win.soulseek

    def refresh_conn(self):
        c = self.client()
        self.conn.setText(c.label() if c.ready() else self.win.L("set a soulseek login in settings → downloads"))

    def lossless_toggled(self, on):
        self.win.cfg["dl_lossless_only"] = on
        self.win.cfg.save()
        if getattr(self, "raw", None) is not None:
            self.show_rows(group_results(self.raw, on))

    # ---- search
    def search(self):
        text = self.q.text().strip()
        if not text or self.busy:
            return
        c = self.client()
        if not c.ready():
            self.status.setText(self.win.L("set up your soulseek login first: settings → downloads"))
            return
        self.busy = True
        self.tree.clear()
        self.status.setText(self.win.L(f"searching for “{text}”…"))
        prog = {"r": 0, "f": 0}

        def work():
            return c.search(text, int(self.win.cfg.get("dl_search_seconds", 15)) * 1000,
                            progress=lambda r, f: prog.update(r=r, f=f))

        def tick():
            if self.busy:
                self.status.setText(self.win.L(f"searching for “{text}”… {prog['r']} users, {prog['f']} files"))

        def done(res, err):
            self.busy = False
            if err:
                self.status.setText(str(err))
                return
            self.raw = res
            rows = group_results(res, self.lossless.isChecked())
            self.show_rows(rows)
            if not res and isinstance(c, SoulseekNative):
                self.win.bg(lambda: c.diagnose(text),
                            lambda why, e: self.status.setText(self.win.L("0 results — ") + (why or str(e))))
            elif res and not rows:
                self.status.setText(self.win.L(f"{len(res)} users answered, but none had lossless folders — "
                                               f"untick lossless only"))
        self.win.bg(work, done, tick)

    def show_rows(self, rows):
        from PySide6.QtWidgets import QTreeWidgetItem
        self.rows = rows[:120]
        self.tree.clear()
        n_all = len(rows)
        self.status.setText(self.win.L(f"{n_all} folders found" + (" (lossless)" if self.lossless.isChecked() else "")
                                       + ("" if n_all else " — try fewer words, or untick lossless only")))
        for i, r in enumerate(self.rows):
            it = QTreeWidgetItem(["●" if r["free"] else "○", r["name"], r["fmt"], str(len(r["audio"])),
                                  human_size(r["size"]), r["user"],
                                  f"{r['speed'] / 1_000_000:.1f} MB/s" if r["speed"] else "—", ""])
            it.setData(0, Qt.UserRole, i)
            it.setToolTip(0, "free upload slot" if r["free"] else f"queued: {r['queue']} ahead")
            it.setToolTip(1, r["dir"])
            for f in sorted(r["audio"], key=lambda f: f["filename"]):
                _d, base = remote_split(f["filename"])
                extra = f"{f.get('bitDepth')}/{(f.get('sampleRate') or 0) / 1000:g}" if f.get("bitDepth") else \
                    (f"{f.get('bitRate')}k" if f.get("bitRate") else "")
                ch = QTreeWidgetItem(["", base, extra, fmt_time(f.get("length") or 0), human_size(int(f.get("size") or 0)),
                                      "", "", ""])
                ch.setData(0, Qt.UserRole, (i, f["filename"]))
                it.addChild(ch)
            self.tree.addTopLevelItem(it)
            b = self.win.tbtn("download", lambda r=r: self.download(r), "rowpill")
            b.setFixedHeight(26)
            self.tree.setItemWidget(it, 7, b)

    def item_activated(self, it, _col):
        d = it.data(0, Qt.UserRole)
        if isinstance(d, int):
            self.download(self.rows[d])
        elif isinstance(d, tuple):
            r = self.rows[d[0]]
            f = next((f for f in r["audio"] if f["filename"] == d[1]), None)
            if f:
                self.download(r, [f])

    # ---- downloading
    @staticmethod
    def _tokens(name):
        name = re.sub(r"[\(\[][^)\]]*(flac|mp3|web|cd|vinyl|24|16|bit|khz|lossless|320|v0)[^)\]]*[\)\]]", " ",
                      str(name).lower())
        return {t for t in re.findall(r"[a-z0-9$]+", name) if len(t) > 1 and t not in ("the", "and", "flac", "mp3")}

    def similar(self, a, b):
        ta, tb = self._tokens(a["name"]), self._tokens(b["name"])
        if not ta or not tb:
            return False
        j = len(ta & tb) / len(ta | tb)
        na, nb = len(a["audio"]), len(b["audio"])
        return j >= 0.5 and abs(na - nb) <= max(2, int(na * 0.25))

    def alternatives(self, row, rows):
        return [r for r in rows if r["user"] != row["user"] and self.similar(row, r)][:6]

    @staticmethod
    def job_key(user, folder):
        return f"{user}\x00{folder.replace('/', chr(92))}"

    def download(self, row, files=None, job=None):
        files = files or (row["audio"] + row["images"])
        c = self.client()
        self.status.setText(self.win.L(f"queueing {len(files)} files from {row['user']}…"))
        key = self.job_key(row["user"], row["dir"])
        if job is None:
            job = {"name": row["name"], "tried": [], "alts": self.alternatives(row, self.rows)}
        job.update(user=row["user"], dir=row["dir"], started=_time.time(), auto=bool(files and len(files) > 1))
        if row["user"] not in job["tried"]:
            job["tried"].append(row["user"])
        job["alts"] = [a for a in job.get("alts", []) if a["user"] not in job["tried"]]
        self.jobs[key] = job

        def done(_res, err):
            if err:
                self.status.setText(str(err))
                return
            for f in files:
                self.pending[(row["user"], f["filename"])] = {"size": int(f.get("size") or 0), "album": row["name"]}
            self.save_pending()
            self.status.setText(self.win.L(f"queued “{row['name']}” from {row['user']}"))
            self.timer.start()
            self.poll()
        self.win.bg(lambda: c.enqueue(row["user"], files), done)

    def poll(self):
        c = self.client()
        if not c.ready() or getattr(self, "_polling", False):
            return
        self._polling = True

        def done(res, err):
            self._polling = False
            if err:
                self.conn.setText(str(err)[:80])
                return
            self.refresh_conn()
            self.transfers = res or []
            self.render_downloads()
            self.import_finished()
            self.check_stalls()
            live = {(u.get("username"), f.get("filename")): f.get("state") or ""
                    for u in self.transfers for d in u.get("directories", []) for f in d.get("files", [])}
            online = self.win.soulseek.state == "online"
            gone = [] if not online else [k for k in self.pending if k not in live or
                    ("Completed" in live[k] and "Succeeded" not in live[k])]      # failed, cancelled or removed
            if gone:
                for k in gone:
                    self.pending.pop(k, None)
                self.save_pending()
            for k in list(self.jobs):
                fs = self.folder_files(k)
                if online and fs and all("Succeeded" in (f.get("state") or "") for f in fs):
                    self.jobs.pop(k, None)
            active = any("Completed" not in (f.get("state") or "")
                         for u in self.transfers for d in u.get("directories", []) for f in d.get("files", []))
            if not active and not self.pending:
                self.timer.stop()
        self.win.bg(c.downloads, done)

    def render_downloads(self):
        self.dl.clear()
        for u in self.transfers:
            for d in u.get("directories", []):
                fs = d.get("files", [])
                if not fs:
                    continue
                total = sum(int(f.get("size") or 0) for f in fs) or 1
                got = sum(int(f.get("bytesTransferred") or 0) for f in fs)
                ok = sum(1 for f in fs if "Succeeded" in (f.get("state") or ""))
                bad = sum(1 for f in fs if "Completed" in (f.get("state") or "") and "Succeeded" not in f["state"])
                queued = sum(1 for f in fs if "Queued" in (f.get("state") or ""))
                imp = sum(1 for f in fs if (u.get("username"), f.get("filename")) in self.imported
                          or ("Succeeded" in (f.get("state") or "") and not (f.get("localPath") and
                                                                             os.path.exists(f["localPath"]))))
                pct = int(got * 100 / total)
                bar_n = 24
                filled = int(bar_n * pct / 100)
                bar = "●" * filled + "·" * (bar_n - filled)
                key = self.job_key(u.get("username", ""), d.get("directory", ""))
                job = self.jobs.get(key) or {}
                spots = [f.get("placeInQueue") for f in fs if f.get("placeInQueue")]
                remote = any("Remotely" in (f.get("state") or "") for f in fs)
                if imp and imp >= ok:
                    state = f"in your library {imp}/{len(fs)}" if imp == len(fs) else f"imported {imp}/{len(fs)}"
                elif ok == len(fs):
                    state = "done"
                elif bad and ok + bad == len(fs):
                    reason = next((f.get("failReason") for f in fs if f.get("failReason")), "")
                    state = f"{bad} failed" + (f" ({reason})" if reason else "") + " — right-click → try another user"
                elif queued and not got:
                    state = (f"waiting in their queue (#{min(spots)})" if spots else
                             "waiting for the user to start sending" if remote else f"queued ({queued})")
                    left = self.stall_left(job)
                    if left is not None:
                        state += f" · tries another user in {left} min" if left > 0 else " · trying another user…"
                else:
                    state = f"{pct}%" + (f" · {bad} failed" if bad else "")
                name = remote_split(d.get("directory", ""))[1] or d.get("directory", "")
                it = QListWidgetItem(f"{bar}   {name}  ·  {u.get('username')}  ·  {ok}/{len(fs)}  ·  {state}")
                it.setData(Qt.UserRole, key)
                self.dl.addItem(it)

    def import_finished(self):
        done = [(u.get("username"), f) for u in self.transfers for d in u.get("directories", [])
                for f in d.get("files", []) if "Succeeded" in (f.get("state") or "")]
        own = getattr(self.win.soulseek, "download_dir", "")

        def ours(f):
            lp = f.get("localPath") or ""
            return bool(own and lp and os.path.abspath(lp).startswith(os.path.abspath(own)) and os.path.exists(lp))
        todo = [(user, f) for user, f in done if ((user, f["filename"]) in self.pending or ours(f))
                and (user, f["filename"]) not in self.imported]
        if not todo:
            return
        direct = [(u, f) for u, f in todo if f.get("localPath") and os.path.exists(f["localPath"])]
        if direct:                                   # built-in soulseek: we know exactly where each file is
            for u, f in direct:
                self.imported.add((u, f["filename"]))
                self.pending.pop((u, f["filename"]), None)
            audio = [f["localPath"] for _u, f in direct if os.path.splitext(f["localPath"])[1].lower() in AUDIO_EXT]
            if audio:
                self.win.importer.import_files(audio, move=True)
                self.status.setText(self.win.L(f"imported {len(audio)} files into your library"))
                self.render_downloads()
            self.save_pending()
            return


    # ---- when a download won't start: move it to someone else
    def stall_left(self, job):
        if not job or not self.win.cfg.get("dl_auto_switch", True) or not job.get("auto", True):
            return None
        mins = max(1, int(self.win.cfg.get("dl_stall_minutes", 5)))
        return max(0, int(math.ceil((job.get("started", _time.time()) + mins * 60 - _time.time()) / 60)))

    def folder_files(self, key):
        user, folder = key.split("\x00", 1)
        for u in self.transfers:
            if u.get("username") != user:
                continue
            for d in u.get("directories", []):
                if d.get("directory", "").replace("/", "\\") == folder:
                    return d.get("files", [])
        return []

    def check_stalls(self):
        if not self.win.cfg.get("dl_auto_switch", True) or getattr(self, "_switching", False):
            return
        mins = max(1, int(self.win.cfg.get("dl_stall_minutes", 5)))
        for key, job in list(self.jobs.items()):
            fs = self.folder_files(key)
            if not fs:
                continue
            got = sum(int(f.get("bytesTransferred") or 0) for f in fs)
            states = [f.get("state") or "" for f in fs]
            all_failed = all("Completed" in s and "Succeeded" not in s for s in states)
            stalled = got == 0 and all(("Queued" in s or "Requested" in s or "Initializing" in s or
                                        ("Completed" in s and "Succeeded" not in s)) for s in states)
            if job.get("auto", True) and (all_failed or (stalled and _time.time() - job.get("started", 0) > mins * 60)):
                self.switch_source(key, auto=True)
                return                                      # one at a time

    def switch_source(self, key, auto=False):
        job = self.jobs.get(key)
        if not job:
            user, folder = key.split("\x00", 1)
            job = {"name": remote_split(folder)[1] or folder, "tried": [user], "alts": [], "user": user, "dir": folder}
            self.jobs[key] = job
        fs = self.folder_files(key)
        done_stems = {self._stem(f["filename"]) for f in fs if "Succeeded" in (f.get("state") or "")}
        leftover = [f for f in fs if "Succeeded" not in (f.get("state") or "")]
        self._switching = True
        c = self.client()
        self.status.setText(self.win.L(f"looking for another user with “{job['name']}”…"))

        def work():
            alts = [a for a in job.get("alts", []) if a["user"] not in job["tried"]]
            if not alts:                                        # nobody left from the old search: search again
                q = " ".join(sorted(self._tokens(job["name"]), key=job["name"].lower().find))[:80] or job["name"]
                res = c.search(q, int(self.win.cfg.get("dl_search_seconds", 15)) * 1000)
                ref = {"name": job["name"], "audio": [None] * max(1, len(fs) - sum(1 for f in fs if os.path.splitext(
                    f["filename"])[1].lower() in IMG_EXT))}
                rows = group_results(res, bool(self.win.cfg.get("dl_lossless_only", True)))
                alts = [r for r in rows if r["user"] not in job["tried"] and self.similar(ref, r)]
            return alts

        def done(alts, err):
            self._switching = False
            if err or not alts:
                job["auto"] = False                          # don't keep hammering
                msg = f"no one else has “{job['name']}” right now" if not err else str(err)
                self.status.setText(self.win.L(msg))
                if not auto:
                    self.win.toast(msg)
                self.render_downloads()
                return
            new = alts[0]
            want = [f for f in new["audio"] + new["images"] if self._stem(f["filename"]) not in done_stems]
            if done_stems and len(want) == len(new["audio"] + new["images"]):
                want = [f for f in want if os.path.splitext(f["filename"])[1].lower() in IMG_EXT] or want
            # drop the stuck ones from the old user
            for f in leftover:
                try:
                    c.cancel(job["user"], f["id"], remove=True)
                except Exception:
                    pass
                self.pending.pop((job["user"], f["filename"]), None)
            self.jobs.pop(key, None)
            job["alts"] = [a for a in alts[1:]]
            old = job["user"]
            self.download(new, want or None, job=job)
            if auto:
                self.win.toast(f"“{job['name']}” wasn't starting — switched from {old} to {new['user']}")
            else:
                self.win.toast(f"switched “{job['name']}” from {old} to {new['user']}")
        self.win.bg(work, done)

    @staticmethod
    def _stem(fn):
        base = os.path.splitext(remote_split(fn)[1])[0].lower()
        base = re.sub(r"^[\d\s._-]+", "", base)
        return re.sub(r"[^a-z0-9]+", "", base)

    def retry_failed(self, key):
        user, _folder = key.split("\x00", 1)
        failed = [f for f in self.folder_files(key)
                  if "Completed" in (f.get("state") or "") and "Succeeded" not in f["state"]]
        if not failed:
            self.win.toast("nothing failed in that one")
            return
        c = self.client()

        def work():
            for f in failed:
                try:
                    c.cancel(user, f["id"], remove=True)
                except Exception:
                    pass
            c.enqueue(user, failed)
        job = self.jobs.get(key)
        if job:
            job["started"] = _time.time()
        self.win.bg(work, lambda _r, e: (self.status.setText(str(e) if e else self.win.L(
            f"retrying {len(failed)} files from {user}")), self.poll()))

    def cancel_folder(self, key):
        user, _folder = key.split("\x00", 1)
        fs = [f for f in self.folder_files(key) if "Succeeded" not in (f.get("state") or "")]
        c = self.client()

        def work():
            for f in fs:
                try:
                    c.cancel(user, f["id"], remove=True)
                except Exception:
                    pass
        for f in fs:
            self.pending.pop((user, f["filename"]), None)
        self.jobs.pop(key, None)
        self.save_pending()
        self.win.bg(work, lambda _r, _e: self.poll())

    def dl_menu(self, pos):
        it = self.dl.itemAt(pos)
        key = it.data(Qt.UserRole) if it else None
        if not key:
            return
        L = self.win.L
        m = QMenu(self)
        m.addAction(L("try another user"), lambda: self.switch_source(key))
        m.addAction(L("retry failed files"), lambda: self.retry_failed(key))
        m.addAction(L("cancel"), lambda: self.cancel_folder(key))
        m.addSeparator()
        m.addAction(L("search for it again"), lambda: (self.q.setText((self.jobs.get(key) or {}).get(
            "name") or remote_split(key.split("\x00", 1)[1])[1]), self.search()))
        m.addAction(L("open soulseek log"), lambda: QDesktopServices.openUrl(
            QUrl.fromLocalFile(str(self.win.soulseek.log_path()))))
        m.exec(self.dl.viewport().mapToGlobal(pos))

    # ---- windows firewall
    def check_firewall(self):
        if not sys.platform.startswith("win"):
            return
        self.win.bg(firewall_ok, lambda ok, _e: self.fw.setVisible(ok is False))

    def fix_firewall(self):
        if firewall_fix(int(self.win.cfg.get("slsk_port", 2234))):
            self.fw.hide()
            self.win.toast("firewall fixed (if you clicked yes) — reconnecting to Soulseek")
            self.win.soulseek.restart()
            QTimer.singleShot(3000, self.poll)
        else:
            self.win.toast("firewall change cancelled")

    def clear_finished(self):
        c = self.client()

        def work():
            for u in self.transfers:
                for d in u.get("directories", []):
                    for f in d.get("files", []):
                        if "Completed" in (f.get("state") or ""):
                            try:
                                c.cancel(u["username"], f["id"], remove=True)
                            except Exception:
                                pass
        self.win.bg(work, lambda _r, _e: self.poll())

    def showEvent(self, e):
        super().showEvent(e)
        self.refresh_conn()
        if self.client().ready():
            self.timer.start()
            self.poll()
            self.check_firewall()


# ------------------------------------------------------------------ inspect (spek-style spectrum + true-lossless verdict)
SPEK_STOPS = [(0.00, (0, 0, 0)), (0.12, (0, 0, 70)), (0.30, (120, 0, 150)), (0.50, (230, 20, 40)),
              (0.68, (255, 130, 0)), (0.85, (255, 230, 40)), (1.00, (255, 255, 255))]
DB_FLOOR = -120.0


def _palette(kind):
    xs = np.linspace(0, 1, 256)
    if kind == "mono":
        g = (xs ** 0.9 * 255).astype(np.uint8)
        return np.stack([g, g, g], 1)
    pts = np.array([p for p, _ in SPEK_STOPS])
    cols = np.array([c for _, c in SPEK_STOPS], dtype=float)
    return np.stack([np.interp(xs, pts, cols[:, i]) for i in range(3)], 1).astype(np.uint8)


def analyze_audio(path, columns=1400, progress=None, cancelled=lambda: False):
    """decode once, take evenly spaced FFT windows across the file. → dict with spectrogram (dB) + verdict info."""
    c = av.open(path)
    try:
        s = c.streams.audio[0]
        cc = s.codec_context
        sr = int(s.rate or cc.sample_rate or 44100)
        ch = int(getattr(cc, "channels", 0) or (cc.layout.nb_channels if cc.layout else 2))
        raw_bits = int(getattr(cc, "bits_per_raw_sample", 0) or 0)
        dur = float(s.duration * s.time_base) if s.duration and s.time_base else (c.duration or 0) / 1e6
        n = 8192 if sr > 50000 else 4096
        total = max(n, int(dur * sr))
        pos = np.linspace(0, max(0, total - n), columns).astype(np.int64)
        win = np.hanning(n).astype(np.float32)
        rows, buf, base, k = [], np.zeros(0, np.float32), 0, 0
        lowbits_used, int_frames = False, 0
        peak = 0.0
        for frame in c.decode(s):
            if cancelled():
                return None
            fmt = frame.format.name
            a = frame.to_ndarray()
            if fmt.startswith("s32") and raw_bits > 16 and not lowbits_used:
                int_frames += 1
                if np.any(a.astype(np.int64) & 0xFFFF):        # 24-bit audio stored <<8: real 24-bit uses the low bits
                    lowbits_used = True
            if frame.format.is_planar:
                x = a.astype(np.float32).mean(axis=0)
            else:
                x = a.reshape(-1, max(1, len(frame.layout.channels))).astype(np.float32).mean(axis=1)
            if fmt.startswith("s16"):
                x /= 32768.0
            elif fmt.startswith("s32"):
                x /= 2147483648.0
            elif fmt.startswith("u8"):
                x = (x - 128) / 128.0
            peak = max(peak, float(np.abs(x).max()) if len(x) else 0.0)
            buf = np.concatenate([buf, x]) if len(buf) else x
            while k < len(pos) and pos[k] + n <= base + len(buf):
                st = pos[k] - base
                seg = buf[st:st + n]
                rows.append(np.abs(np.fft.rfft(seg * win)).astype(np.float32))
                k += 1
                if progress and k % 50 == 0:
                    progress(k / len(pos))
            if k < len(pos):
                drop = max(0, pos[k] - base)
                if drop:
                    buf, base = buf[drop:], base + drop
            else:
                break
        while k < len(pos) and len(buf):          # tail windows (zero-padded)
            st = max(0, pos[k] - base)
            seg = np.zeros(n, np.float32)
            part = buf[st:st + n]
            seg[:len(part)] = part
            rows.append(np.abs(np.fft.rfft(seg * win)).astype(np.float32))
            k += 1
    finally:
        c.close()
    if not rows:
        raise ValueError("no audio decoded")
    S = np.stack(rows, 1)                                   # bins × columns
    ref = (n * win.mean() / 2)                              # full-scale sine → 0 dB
    db = 20 * np.log10(np.maximum(S / ref, 1e-9))
    db = np.maximum(db, DB_FLOOR)
    freqs = np.fft.rfftfreq(n, 1.0 / sr)
    info = verdict(db, freqs, sr, raw_bits, lowbits_used if int_frames else None)
    info.update(db=db, freqs=freqs, sr=sr, channels=ch, bits=raw_bits, duration=dur, n=n, peak=peak)
    return info


def verdict(db, freqs, sr, bits, lowbits_used):
    """find the lossy-encoder lowpass cliff; judge true lossless / fake hi-res / padded 24-bit."""
    nyq = sr / 2
    loud = db.max(axis=0) > -60                     # ignore silent columns
    cols = db[:, loud] if loud.sum() > 10 else db
    L = np.percentile(cols, 97, axis=1)             # near-max level each bin reaches over time
    k = max(1, int(round(150 / (freqs[1] - freqs[0]))))
    Ls = np.convolve(np.pad(L, k, mode="edge"), np.ones(k) / k, mode="same")[k:-k]
    step = freqs[1] - freqs[0]
    span = max(2, int(round(800 / step)))
    cutoff = nyq
    lo = np.searchsorted(freqs, 11000)
    top_margin = np.searchsorted(freqs, nyq - 400)
    best_drop, best_i = 0.0, None
    for i in range(lo, max(lo + 1, top_margin - span)):
        below = Ls[max(0, i - span):i].mean()
        above = Ls[i:i + span].mean()
        drop = below - above
        if drop > best_drop:
            best_drop, best_i = drop, i
    above_lvl = Ls[best_i:best_i + span].mean() if best_i is not None else 0
    if best_i is not None and ((best_drop > 22 and above_lvl < -95) or (best_drop > 14 and above_lvl < -108)):
        # cliff: walk up to where it actually falls off
        i = best_i
        while i > lo and Ls[i - 1] < Ls[best_i - span // 2] - 10:
            i -= 1
        cutoff = float(freqs[best_i])
    else:
        # no cliff: highest frequency still carrying real content
        content = np.where(Ls > -100)[0]
        cutoff = float(freqs[content[-1]]) if len(content) else nyq
        best_drop = 0.0
    khz = cutoff / 1000
    if sr > 50000:                                      # hi-res file
        if khz <= 20.5 and best_drop:
            kind, text = "bad", f"lossy source — cuts off at {khz:.1f} kHz (upsampled from MP3/AAC)"
        elif khz <= 27.0:
            kind, text = "warn", f"not true hi-res — nothing above {khz:.1f} kHz (upsampled from 44.1/48 kHz)"
        else:
            kind, text = "good", f"true hi-res — content up to {khz:.1f} kHz"
    else:
        if best_drop and khz < 17.0:
            kind, text = "bad", f"lossy transcode — hard cutoff at {khz:.1f} kHz (≈128 kbps MP3 or lower)"
        elif best_drop and khz < 19.6:
            kind, text = "bad", f"lossy transcode — hard cutoff at {khz:.1f} kHz (≈160–256 kbps MP3/AAC)"
        elif best_drop and khz < 21.0:
            kind, text = "warn", f"probably lossy — cutoff at {khz:.1f} kHz (≈320 kbps MP3 / 256 kbps AAC)"
        else:
            kind, text = "good", f"looks lossless — content up to {khz:.1f} kHz"
    notes = []
    if not best_drop and khz < 19 and sr <= 50000:
        notes.append(f"little content above {khz:.0f} kHz but no encoder cliff — normal for older or lo-fi recordings")
    if bits > 16 and lowbits_used is False:
        notes.append("24-bit container but only 16 bits used (padded)")
        if kind == "good":
            kind = "warn"
    return {"cutoff": cutoff, "drop": float(best_drop), "kind": kind, "text": text, "notes": notes}


def spectrum_image(info, kind="spek", height=560):
    """dB spectrogram → QImage (columns × height), low frequencies at the bottom."""
    db = info["db"]
    bins = db.shape[0]
    edges = np.linspace(0, bins, height + 1).astype(int)
    pooled = np.stack([db[edges[i]:max(edges[i] + 1, edges[i + 1])].max(axis=0) for i in range(height)])
    norm = np.clip((pooled - DB_FLOOR) / -DB_FLOOR, 0, 1)
    idx = (norm * 255).astype(np.uint8)[::-1]
    rgb = np.ascontiguousarray(_palette(kind)[idx])
    h, w = idx.shape
    return QImage(rgb.data, w, h, 3 * w, QImage.Format_RGB888).copy()


class SpectrumView(QWidget):
    """the spectrogram with Spek-style axes: time along the bottom, kHz up the left, dB scale on the right."""

    def __init__(self, win):
        super().__init__()
        self.win, self.img, self.info, self.palette = win, None, None, "spek"
        self.setMinimumHeight(320)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        self.setMouseTracking(True)
        self.hover = None

    def set_result(self, info, palette):
        self.info, self.palette = info, palette
        self.img = spectrum_image(info, palette) if info else None
        self.update()

    def plot_rect(self):
        fm = self.fontMetrics()
        left = fm.horizontalAdvance("00 kHz") + 14
        return QRect(left, 8, self.width() - left - 70, self.height() - 8 - fm.height() - 16)

    def paintEvent(self, _e):
        p = QPainter(self)
        c = self.win.c
        r = self.plot_rect()
        p.fillRect(r, QColor("#000000"))
        fm = self.fontMetrics()
        dim = QColor(c["dim"])
        if self.img is None:
            p.setPen(dim)
            p.drawText(r, Qt.AlignCenter, self.win.L("pick a song: right-click → inspect, ctrl+i, or drop a file here"))
            return
        p.setRenderHint(QPainter.SmoothPixmapTransform)
        p.drawImage(r, self.img)
        info = self.info
        nyq = info["sr"] / 2
        p.setPen(QPen(QColor(c["border"]), 1))
        p.drawRect(r.adjusted(0, 0, -1, -1))
        # frequency axis
        step = 2000 if nyq <= 24000 else 5000 if nyq <= 50000 else 10000
        f = 0
        while f <= nyq:
            y = r.bottom() - f / nyq * r.height()
            p.setPen(dim)
            p.drawLine(QPointF(r.left() - 4, y), QPointF(r.left(), y))
            p.drawText(QRectF(0, y - fm.height() / 2, r.left() - 8, fm.height()), Qt.AlignRight | Qt.AlignVCenter,
                       f"{f / 1000:g} kHz" if f else "0")
            f += step
        # cutoff marker
        cut = info.get("cutoff", nyq)
        if info.get("drop") and cut < nyq - 200:
            y = r.bottom() - cut / nyq * r.height()
            pen = QPen(QColor(c["text"]), 1, Qt.DashLine)
            p.setPen(pen)
            p.drawLine(QPointF(r.left(), y), QPointF(r.right(), y))
            p.drawText(QPointF(r.right() - fm.horizontalAdvance("cutoff 00.0 kHz") - 6, y - 5), f"cutoff {cut / 1000:.1f} kHz")
        # time axis
        dur = max(1.0, info["duration"])
        tstep = next(t for t in (5, 10, 15, 30, 60, 120, 300, 600) if dur / t <= 12)
        t = 0
        while t <= dur:
            x = r.left() + t / dur * r.width()
            p.setPen(dim)
            p.drawLine(QPointF(x, r.bottom()), QPointF(x, r.bottom() + 4))
            lab = fmt_time(t)
            p.drawText(QRectF(x - 30, r.bottom() + 6, 60, fm.height()), Qt.AlignHCenter | Qt.AlignTop, lab)
            t += tstep
        # dB legend
        lx = r.right() + 14
        legend = QRect(lx, r.top(), 14, r.height())
        pal = _palette(self.palette)
        for i in range(legend.height()):
            v = 1 - i / max(1, legend.height() - 1)
            col = pal[int(v * 255)]
            p.setPen(QColor(int(col[0]), int(col[1]), int(col[2])))
            p.drawLine(legend.left(), legend.top() + i, legend.right(), legend.top() + i)
        p.setPen(dim)
        for d in (0, -20, -40, -60, -80, -100, -120):
            y = legend.top() + (-d / 120) * legend.height()
            p.drawText(QRectF(legend.right() + 4, y - fm.height() / 2, 40, fm.height()), Qt.AlignLeft | Qt.AlignVCenter, f"{d}")
        # hover readout
        if self.hover and r.contains(self.hover):
            hx, hy = self.hover.x(), self.hover.y()
            fr = (r.bottom() - hy) / r.height() * nyq
            tt = (hx - r.left()) / r.width() * dur
            text = f"{fmt_time(tt)}  ·  {fr / 1000:.1f} kHz"
            w = fm.horizontalAdvance(text) + 16
            box = QRectF(min(hx + 12, r.right() - w), max(r.top(), hy - 28), w, fm.height() + 8)
            p.fillRect(box, QColor(0, 0, 0, 200))
            p.setPen(QColor("#ffffff"))
            p.drawText(box, Qt.AlignCenter, text)

    def mouseMoveEvent(self, e):
        self.hover = e.position().toPoint()
        self.update()

    def leaveEvent(self, e):
        self.hover = None
        self.update()


class InspectPage(QWidget):
    """spek-style check: is this really lossless?"""

    def __init__(self, win):
        super().__init__(objectName="inspectpage")
        self.win = win
        self.path, self.info, self.cache, self.busy = None, None, OrderedDict(), False
        self.setAcceptDrops(True)
        v = QVBoxLayout(self)
        v.setContentsMargins(32, 28, 28, 18)
        v.setSpacing(10)
        head = QHBoxLayout()
        head.addWidget(QLabel("Inspect", objectName="h1"))
        head.addStretch()
        self.pal_btn = win.tbtn("mono", self.toggle_palette, "pill")
        head.addWidget(self.pal_btn, 0, Qt.AlignBottom)
        head.addSpacing(6)
        head.addWidget(win.tbtn("open file…", self.pick, "pill"), 0, Qt.AlignBottom)
        head.addSpacing(6)
        head.addWidget(win.tbtn("done", win.back, "pillPrimary"), 0, Qt.AlignBottom)
        v.addLayout(head)
        self.title = QLabel(objectName="npBig")
        self.meta = QLabel(objectName="dim")
        self.meta.setWordWrap(True)
        self.verdict = QLabel(objectName="verdict")
        self.verdict.setWordWrap(True)
        self.notes = QLabel(objectName="time")
        self.notes.setWordWrap(True)
        for wdg in (self.title, self.meta, self.verdict, self.notes):
            v.addWidget(wdg)
        self.view = SpectrumView(win)
        v.addWidget(self.view, 1)
        self.hint = QLabel(objectName="time")
        self.hint.setWordWrap(True)
        self.hint.setText(self.win.L("how to read it: real lossless fills up to the top (~22 kHz for CD, ~48 kHz for 96k). "
                                     "MP3/AAC leave a flat ceiling — the dashed line — at 16–20 kHz. "
                                     "it's a strong hint, not proof: some encoders keep full bandwidth."))
        v.addWidget(self.hint)
        self.set_empty()

    def set_empty(self):
        self.title.setText(self.win.L("nothing inspected yet"))
        self.meta.setText("")
        self.verdict.setText("")
        self.notes.setText("")
        self.view.set_result(None, self.palette())

    def palette(self):
        return self.win.cfg.get("inspect_palette", "spek")

    def toggle_palette(self):
        self.win.cfg["inspect_palette"] = "mono" if self.palette() == "spek" else "spek"
        self.win.cfg.save()
        self.pal_btn._raw = "color" if self.palette() == "mono" else "mono"
        self.pal_btn.setText(self.win.B(self.pal_btn._raw))
        if self.info:
            self.view.set_result(self.info, self.palette())

    def pick(self):
        p, _ = QFileDialog.getOpenFileName(self, "inspect audio file", str(Path.home() / "Music"),
                                           "audio (" + " ".join("*" + e for e in sorted(AUDIO_EXT)) + ")")
        if p:
            self.load(p)

    def dragEnterEvent(self, e):
        if e.mimeData().hasUrls():
            e.acceptProposedAction()

    def dropEvent(self, e):
        for u in e.mimeData().urls():
            if os.path.splitext(u.toLocalFile())[1].lower() in AUDIO_EXT:
                self.load(u.toLocalFile())
                return

    def load(self, path):
        path = str(path)
        self.path = path
        t = self.win.by_path.get(path) or read_track(path) or {}
        name = t.get("title") or Path(path).name
        self.title.setText(f"{name}  —  {t.get('artist', '')}".rstrip(" —"))
        try:
            size = os.path.getsize(path)
        except OSError:
            size = 0
        bits = [t.get("fmt", Path(path).suffix.lstrip(".").upper()),
                f"{t['sr'] / 1000:g} kHz" if t.get("sr") else "",
                f"{t['bits']}-bit" if t.get("bits") else "",
                f"{t['channels']} ch" if t.get("channels") else "",
                f"{(size * 8 / max(1, t.get('duration') or 1)) / 1000:.0f} kbps avg" if size and t.get("duration") else "",
                fmt_time(t.get("duration", 0)) if t.get("duration") else "", human_size(size) if size else ""]
        self.meta.setText("  ·  ".join(b for b in bits if b) + f"\n{path}")
        if path in self.cache:
            return self.show_info(self.cache[path])
        if np is None or av is None:
            self.verdict.setText(self.win.L("inspect needs numpy + av (the audio engine packages)"))
            return
        self.busy = True
        self.verdict.setProperty("kind", "")
        self.verdict.setText(self.win.L("analysing…"))
        self.notes.setText("")
        self.view.set_result(None, self.palette())
        prog = {"v": 0.0}

        def tick():
            if self.busy and self.path == path:
                self.verdict.setText(self.win.L(f"analysing… {int(prog['v'] * 100)}%"))

        def done(info, err):
            self.busy = False
            if self.path != path:
                return
            if err:
                self.verdict.setText(self.win.L(f"couldn't analyse this file: {err}"))
                return
            self.cache[path] = info
            if len(self.cache) > 12:
                self.cache.popitem(last=False)
            self.show_info(info)
        self.win.bg(lambda: analyze_audio(path, progress=lambda f: prog.update(v=f)), done, tick)

    def show_info(self, info):
        self.info = info
        mark = {"good": "●  ", "warn": "◐  ", "bad": "○  "}.get(info["kind"], "")
        self.verdict.setProperty("kind", info["kind"])
        self.verdict.style().unpolish(self.verdict)
        self.verdict.style().polish(self.verdict)
        self.verdict.setText(mark + info["text"])
        self.notes.setText("   ·   ".join(info.get("notes") or []))
        self.view.set_result(info, self.palette())


# ------------------------------------------------------------------ discord rich presence ("Listening to Nexus")
import queue as _queue
import struct as _struct


class DiscordPresence:
    """talks to the local Discord app over its IPC pipe/socket — no bot, no token, nothing leaves the pc
    except (optionally) an album-art lookup on iTunes so friends see the cover."""

    def __init__(self, win):
        self.win = win
        self.q = _queue.Queue()
        self.conn = None
        self.status = "off"
        self.user = ""
        self.sdt_ok = True             # does this discord accept status_display_type?
        self.art_cache = {}           # in-memory, incl. misses (retried next launch)
        self.art_file = win.dir / "discord_art.json"
        try:
            self.art_saved = json.loads(self.art_file.read_text("utf-8"))
        except Exception:
            self.art_saved = {}
        self.assets = None            # art asset names on your discord app
        self.last = None
        self.stop_ev = threading.Event()
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    # ---- ipc plumbing
    @staticmethod
    def _candidates():
        if sys.platform.startswith("win"):
            return [rf"\\?\pipe\discord-ipc-{i}" for i in range(10)]
        roots = [os.environ.get(k) for k in ("XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP")] + ["/tmp"]
        subs = ["", "app/com.discordapp.Discord", "snap.discord", "app/dev.vencord.Vesktop",
                ".flatpak/dev.vencord.Vesktop/xdg-run", "app/com.discordapp.DiscordCanary"]
        out = []
        for r in roots:
            if r:
                for s in subs:
                    for i in range(10):
                        out.append(os.path.join(r, s, f"discord-ipc-{i}"))
        return list(dict.fromkeys(out))

    def _open(self):
        for path in self._candidates():
            try:
                if sys.platform.startswith("win"):
                    f = open(path, "r+b", buffering=0)
                    return ("pipe", f)
                if os.path.exists(path):
                    import socket
                    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                    s.settimeout(5)
                    s.connect(path)
                    return ("sock", s)
            except OSError:
                continue
        return None

    def _send(self, op, payload):
        data = json.dumps(payload).encode("utf-8")
        frame = _struct.pack("<II", op, len(data)) + data
        kind, h = self.conn
        if kind == "pipe":
            h.write(frame)
        else:
            h.sendall(frame)

    def _recv(self):
        kind, h = self.conn

        def read(n):
            buf = b""
            while len(buf) < n:
                chunk = h.read(n - len(buf)) if kind == "pipe" else h.recv(n - len(buf))
                if not chunk:
                    raise OSError("discord closed the connection")
                buf += chunk
            return buf
        op, ln = _struct.unpack("<II", read(8))
        return op, json.loads(read(ln) or b"{}")

    def _close(self):
        if self.conn:
            try:
                self.conn[1].close()
            except Exception:
                pass
        self.conn = None

    def _connect(self):
        cid = (self.win.cfg.get("discord_client_id") or "").strip()
        if not cid.isdigit():
            self.status = "add your discord application id in settings → discord"
            return False
        c = self._open()
        if not c:
            self.status = "discord isn't running (or isn't the desktop app)"
            return False
        self.conn = c
        try:
            self._send(0, {"v": 1, "client_id": cid})
            op, msg = self._recv()
            if msg.get("evt") != "READY":
                self.status = f"discord refused: {msg.get('message') or msg.get('data', {}).get('message') or msg}"
                self._close()
                return False
            user = (msg.get("data") or {}).get("user") or {}
            self.user = user.get('global_name') or user.get('username') or 'you'
            self.status = f"connected as {self.user}"
            self.assets = self._asset_names(cid)
            return True
        except Exception as e:
            self.status = f"discord connection failed: {e}"
            self._close()
            return False

    def _set(self, activity):
        payload = {"cmd": "SET_ACTIVITY", "args": {"pid": os.getpid(), "activity": activity},
                   "nonce": str(uuid.uuid4())}
        self._send(1, payload)
        _op, msg = self._recv()
        if msg.get("evt") == "ERROR":
            err = (msg.get("data") or {}).get("message", "")
            if activity and "status_display_type" in activity and self.sdt_ok:
                self.sdt_ok = False                       # older discord: retry without the newer field
                activity = {k: v for k, v in activity.items() if k != "status_display_type"}
                return self._set(activity)
            raise OSError(f"discord rejected the status: {err}")

    def _run(self):
        pending, retry_at = None, 0.0
        while not self.stop_ev.is_set():
            try:
                item = self.q.get(timeout=1.0)
                while True:                              # only the latest update matters
                    try:
                        item = self.q.get_nowait()
                    except _queue.Empty:
                        break
                pending = item
            except _queue.Empty:
                pass
            if pending == "reset":
                self._close()
                pending, retry_at = self.last, 0.0
            if pending is None or _time.monotonic() < retry_at:
                continue
            if pending == "quit":
                break
            if self.conn is None and not self._connect():
                retry_at = _time.monotonic() + 15
                continue
            try:
                act = pending.get("activity") if isinstance(pending, dict) else None
                if act:
                    logo = self._asset_key(self.win.cfg.get("discord_large_image"))
                    if logo:
                        act["assets"]["large_image"] = logo
                    else:
                        act["assets"].pop("large_image", None)
                    art = None
                    if pending.get("art_lookup"):
                        if not self._art_known(pending):         # show the song right away, cover follows
                            self._set(json.loads(json.dumps(act)))
                            if not self.q.empty():
                                continue
                        art = self._art(pending)
                    if art:
                        act["assets"]["large_image"] = art
                        small = self._asset_key(self.win.cfg.get("discord_small_image"))
                        if small:
                            act["assets"]["small_image"] = small
                            act["assets"]["small_text"] = APP_NAME
                    if not self.sdt_ok:
                        act.pop("status_display_type", None)
                self._set(act)
                self.status = (f"connected as {self.user} · showing “{act['details'].strip()}” "
                               f"(discord accepted it)" if act else
                               f"connected as {self.user} · nothing showing (paused or stopped)")
                pending = None
            except Exception as e:
                self.status = f"lost discord: {e}"
                self._close()
                retry_at = _time.monotonic() + 10
        try:
            if self.conn:
                self._set(None)
        except Exception:
            pass
        self._close()

    # ---- cover art (discord can't read local files, so it needs a public https image)
    @staticmethod
    def _asset_names(cid):
        try:
            req = urllib.request.Request(f"https://discord.com/api/v9/oauth2/applications/{cid}/assets",
                                         headers={"User-Agent": f"Nexus/{VERSION}"})
            with urllib.request.urlopen(req, timeout=6) as r:
                return [a.get("name") for a in json.loads(r.read().decode("utf-8")) if a.get("name")]
        except Exception:
            return None

    def _asset_key(self, want):
        """the asset you set, else one called nexus, else whatever you uploaded first."""
        want = (want or "nexus").strip()
        names = self.assets
        if names is None:                       # couldn't list them: trust the setting
            return want
        for n in (want, "nexus"):
            if n in names:
                return n
        return names[0] if names else None

    @staticmethod
    def _norm(x):
        x = re.sub(r"\s*[\(\[][^)\]]*(edition|deluxe|remaster|version|explicit|expanded|anniversary)[^)\]]*[\)\]]", "",
                   str(x or ""), flags=re.I)
        x = re.split(r"\s+(?:feat\.?|ft\.?|featuring|with|&|x|,)\s+", x, maxsplit=1, flags=re.I)[0]
        return re.sub(r"[^\w]+", " ", x.lower()).strip()

    def _get_json(self, url):
        req = urllib.request.Request(url, headers={"User-Agent": f"Nexus/{VERSION}"})
        with urllib.request.urlopen(req, timeout=6) as r:
            return json.loads(r.read().decode("utf-8"))

    def _online(self, artist, album, title):
        a, al = self._norm(artist), self._norm(album)
        if not a or not al or al in ("music", "unknown album"):
            return None

        def pick(items):                    # [(album name, artist name, url)] → exact title first, then close
            raw = lambda x: re.sub(r"[^\w]+", " ", str(x or "").lower()).strip()
            items = [(self._norm(n), self._norm(w), u, raw(n)) for n, w, u in items if u]
            items = [x for x in items if x[1] and (a in x[1] or x[1] in a)]
            for n, _w, u, r in items:                          # same title, edition and all
                if r == raw(album):
                    return u
            for n, _w, u, _r in items:
                if n == al:
                    return u
            for n, _w, u, _r in items:
                if len(al) > 3 and (al in n or n in al):
                    return u
            return None
        try:                                                   # itunes
            q = urllib.parse.urlencode({"term": f"{a} {al}", "entity": "album", "limit": 10})
            res = self._get_json(f"https://itunes.apple.com/search?{q}").get("results") or []
            u = pick([(x.get("collectionName"), x.get("artistName"), x.get("artworkUrl100")) for x in res])
            if u:
                return u.replace("100x100bb", "600x600bb")
        except Exception:
            pass
        try:                                                   # deezer
            q = urllib.parse.urlencode({"q": f'artist:"{a}" album:"{al}"', "limit": 10})
            res = self._get_json(f"https://api.deezer.com/search/album?{q}").get("data") or []
            return pick([(x.get("title"), (x.get("artist") or {}).get("name"), x.get("cover_xl")) for x in res])
        except Exception:
            pass
        return None

    UPLOAD_URL = os.environ.get("NEXUS_ART_UPLOAD", "https://catbox.moe/user/api.php")

    def _upload(self, cover_file):
        """your exact cover → catbox.moe (free, no account). each album is uploaded once and remembered."""
        if not cover_file or not os.path.exists(cover_file):
            return None
        img = QImage(cover_file)
        if img.isNull():
            return None
        if max(img.width(), img.height()) > 512:
            img = img.scaled(512, 512, Qt.KeepAspectRatio, Qt.SmoothTransformation)
        ba = QByteArray()
        buf = QBuffer(ba)
        buf.open(QIODevice.WriteOnly)
        img.save(buf, "JPG", 90)
        buf.close()
        data = bytes(ba)
        b = uuid.uuid4().hex
        body = (f"--{b}\r\nContent-Disposition: form-data; name=\"reqtype\"\r\n\r\nfileupload\r\n"
                f"--{b}\r\nContent-Disposition: form-data; name=\"fileToUpload\"; filename=\"cover.jpg\"\r\n"
                f"Content-Type: image/jpeg\r\n\r\n").encode() + data + f"\r\n--{b}--\r\n".encode()
        req = urllib.request.Request(self.UPLOAD_URL, data=body, method="POST",
                                     headers={"Content-Type": f"multipart/form-data; boundary={b}",
                                              "User-Agent": f"Nexus/{VERSION}"})
        try:
            with urllib.request.urlopen(req, timeout=20) as r:
                url = r.read().decode("utf-8", "replace").strip()
            return url if url.startswith("https://") else None
        except Exception:
            return None

    def _art_key(self, p):
        key = p.get("art_key") or f"{p.get('artist')}|{p.get('album')}".lower()
        return f"{p.get('art_source') or 'upload'}:{key}"

    def _art_known(self, p):
        ck = self._art_key(p)
        return ck in self.art_cache or ck in self.art_saved

    def _art(self, p):
        src = p.get("art_source") or "upload"
        ck = self._art_key(p)
        if ck in self.art_cache:
            return self.art_cache[ck]
        url = self.art_saved.get(ck)
        if not url:
            if src == "upload":
                url = self._upload(p.get("cover_file")) or self._online(p["artist"], p["album"], p.get("title"))
            else:
                url = self._online(p["artist"], p["album"], p.get("title"))
            if url:
                self.art_saved[ck] = url
                try:
                    self.art_file.write_text(json.dumps(self.art_saved), "utf-8")
                except Exception:
                    pass
        self.art_cache[ck] = url
        return url

    # ---- called from the ui thread
    def update(self, track, playing, pos_ms=0, dur_ms=0):
        cfg = self.win.cfg
        if not cfg.get("discord_enabled"):
            return
        if not track or (not playing and cfg.get("discord_hide_paused", True)):
            self.last = {"activity": None}
            self.q.put({"activity": None})
            return
        title, artist, album = track.get("title", ""), track.get("artist", ""), track.get("album", "")

        def clip(s, n=128):
            s = str(s or "")
            return (s if len(s) >= 2 else s + "  ")[:n]
        act = {
            "type": 2,                                                # "Listening to …"
            "details": clip(title),
            "state": clip(f"by {artist}"),
            "assets": {"large_image": cfg.get("discord_large_image") or "nexus",
                       "large_text": clip(" · ".join(x for x in (album, quality(track)) if x))},
            "status_display_type": 2 if cfg.get("discord_song_in_status", True) else 0,
        }
        if playing and dur_ms:
            now = _time.time()
            act["timestamps"] = {"start": int(now - pos_ms / 1000), "end": int(now - pos_ms / 1000 + dur_ms / 1000)}
        elif not playing:
            act["state"] = clip(f"by {artist} · paused")
        cover = track.get("cover") or ""
        cover_file = str(self.win.dir / "covers" / f"{cover}.jpg") if cover else ""
        self.last = {"activity": act, "artist": track.get("albumartist") or artist, "album": album, "title": title,
                     "art_key": track.get("album_key") or "", "cover_file": cover_file,
                     "art_source": cfg.get("discord_cover_source", "upload"),
                     "art_lookup": bool(cfg.get("discord_cover_lookup", True))}
        self.q.put(dict(self.last, activity=dict(act, assets=dict(act["assets"]))))

    def reset(self):
        self.q.put("reset")

    def clear(self):
        self.q.put({"activity": None})

    def shutdown(self):
        self.q.put("quit")
        self.thread.join(2)


# ------------------------------------------------------------------ studio layout pieces
class CoverButton(QPushButton):
    """rail item: a playlist shown as its cover (spotify-style library rail)."""

    def __init__(self, win, name, cover, size=44):
        super().__init__()
        self.win, self.name, self.cover, self.active = win, name, cover, False
        self.setFixedSize(size, size)
        self.setCursor(Qt.PointingHandCursor)
        self.setFocusPolicy(Qt.NoFocus)
        self.setToolTip(name)
        self.setAttribute(Qt.WA_Hover, True)

    def paintEvent(self, _e):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        s = self.width()
        c = self.win.c
        rad = min(10.0, float(self.win.cfg["corner_radius"]))
        path_ = QPainterPath()
        path_.addRoundedRect(QRectF(0, 0, s, s), rad, rad)
        p.setClipPath(path_)
        if self.cover:
            p.drawPixmap(QRect(0, 0, s, s), self.win.covers.get(self.cover, s))
        else:
            p.fillPath(path_, QColor(c["select"]))
            f = QFont(self.font())
            f.setFamily(self.win.display_family or f.family())
            f.setPixelSize(int(s * 0.46))
            f.setBold(True)
            p.setFont(f)
            p.setPen(QColor(c["text"]))
            p.drawText(QRectF(0, 0, s, s), Qt.AlignCenter, (self.name[:1] or "?").upper())
        p.setClipping(False)
        if self.active or self.underMouse():
            col = QColor(c["text"])
            if not self.active:
                col.setAlphaF(0.35)
            p.setPen(QPen(col, 2))
            p.setBrush(Qt.NoBrush)
            p.drawRoundedRect(QRectF(1, 1, s - 2, s - 2), rad, rad)


class AlbumRowDelegate(QStyledItemDelegate):
    """album row in the column browser: thumb + title + year/quality."""

    def __init__(self, win, parent):
        super().__init__(parent)
        self.win = win

    def sizeHint(self, opt, idx):
        return QSize(200, 62)

    def paint(self, p, opt, idx):
        w, c = self.win, self.win.c
        a = w.albums.get(idx.data(Qt.UserRole))
        r = opt.rect.adjusted(4, 2, -4, -2)
        sel = bool(opt.state & QStyle.State_Selected)
        hov = bool(opt.state & QStyle.State_MouseOver)
        p.save()
        p.setRenderHint(QPainter.Antialiasing)
        rad = min(9.0, float(w.cfg["corner_radius"]))
        if sel or hov:
            bgp = QPainterPath()
            bgp.addRoundedRect(QRectF(r), rad, rad)
            p.fillPath(bgp, QColor(c["text"] if sel else c["select"]))
        tc = QColor(c["bg"] if sel else c["text"])
        sc = QColor(c["bg"] if sel else c["dim"])
        if sel:
            sc.setAlphaF(0.65)
        fm = opt.fontMetrics
        if a is None:
            p.setPen(tc)
            p.drawText(r.adjusted(12, 0, -8, 0), Qt.AlignVCenter | Qt.AlignLeft, idx.data(Qt.DisplayRole))
            p.restore()
            return
        ts = 46
        tx, ty = r.x() + 8, r.y() + (r.height() - ts) // 2
        p.drawPixmap(QRect(tx, ty, ts, ts), w.covers.get(a["cover"], ts))
        x = tx + ts + 12
        tw = r.right() - x - 8
        f = QFont(opt.font)
        f.setWeight(QFont.DemiBold)
        p.setFont(f)
        p.setPen(tc)
        p.drawText(QRect(x, r.y() + 10, tw, fm.height()), Qt.AlignLeft | Qt.AlignVCenter,
                   QFontMetrics(f).elidedText(a["album"], Qt.ElideRight, tw))
        p.setFont(opt.font)
        p.setPen(sc)
        sub = " · ".join(v for v in (a["artist"], a["year"], quality(a["tracks"][0]) if a["tracks"] else "") if v)
        p.drawText(QRect(x, r.y() + 10 + fm.height() + 2, tw, fm.height()), Qt.AlignLeft | Qt.AlignVCenter,
                   fm.elidedText(sub, Qt.ElideRight, tw))
        p.restore()


def vline():
    f = QFrame(objectName="vline")
    f.setFixedWidth(1)
    return f


def hline():
    f = QFrame(objectName="hline")
    f.setFixedHeight(1)
    return f


class ColumnBrowser(QWidget):
    """albums → tracks, side by side. arrow keys move between columns."""

    def __init__(self, win):
        super().__init__()
        self.win = win
        h = QHBoxLayout(self)
        h.setContentsMargins(0, 0, 0, 0)
        h.setSpacing(0)

        def column(title, widget, width=None):
            box = QWidget()
            v = QVBoxLayout(box)
            v.setContentsMargins(8, 16, 8, 0)
            v.setSpacing(8)
            head = QHBoxLayout()
            head.setContentsMargins(10, 0, 10, 0)
            lab = QLabel(title, objectName="eyebrow")
            cnt = QLabel(objectName="colcount")
            head.addWidget(lab)
            head.addStretch()
            head.addWidget(cnt, 0, Qt.AlignBottom)
            v.addLayout(head)
            v.addWidget(widget, 1)
            if width:
                box.setFixedWidth(width)
            return box, cnt

        self.albums = QListWidget(objectName="colalbums")
        self.albums.setItemDelegate(AlbumRowDelegate(win, self.albums))
        self.albums.setMouseTracking(True)
        self.albums.viewport().setAttribute(Qt.WA_Hover, True)
        self.albums.setVerticalScrollMode(QAbstractItemView.ScrollPerPixel)
        self.albums.setUniformItemSizes(True)
        self.tracks = SongTable(win, "browse", hide={"album", "year", "genre", "format"}, thumbs=False)
        self.tracks.setObjectName("coltracks")
        self.tracks.model_.plain = True
        b1, self.c_alb = column("ALBUMS", self.albums, 360)
        b2, self.c_trk = column("TRACKS", self.tracks)
        h.addWidget(b1)
        h.addWidget(vline())
        h.addWidget(b2, 1)
        self.albums.currentItemChanged.connect(self.on_album)
        self.albums.itemDoubleClicked.connect(lambda it: self.play_album())
        self.albums.setContextMenuPolicy(Qt.CustomContextMenu)
        self.albums.customContextMenuRequested.connect(self.album_menu)
        for wdg in (self.albums, self.tracks):
            wdg.installEventFilter(self)

    def current_album(self):
        it = self.albums.currentItem()
        return self.win.albums.get(it.data(Qt.UserRole)) if it else None

    def play_album(self):
        a = self.current_album()
        if a:
            self.win.play_list(a["tracks"], 0)

    def refresh(self, select=None, album=None):
        """select = artist name (jumps to their first album), album = album key."""
        w = self.win
        albums = w.sorted_albums(w.albums.values())
        keep = album or (self.albums.currentItem().data(Qt.UserRole) if self.albums.currentItem() else None)
        self.albums.blockSignals(True)
        self.albums.clear()
        target = None
        for a in albums:
            it = QListWidgetItem(f"{a['album']} {a['artist']}")
            it.setData(Qt.UserRole, a["key"])
            self.albums.addItem(it)
            if select and target is None and a["artist"] == select:
                target = it
            elif not select and a["key"] == keep:
                target = it
        if target is None and self.albums.count():
            target = self.albums.item(0)
        if target:
            self.albums.setCurrentItem(target)
            self.albums.scrollToItem(target, QAbstractItemView.PositionAtCenter)
        self.albums.blockSignals(False)
        self.c_alb.setText(f"{len(albums)}")
        self.on_album(target, None)

    def on_album(self, cur, _prev):
        a = self.win.albums.get(cur.data(Qt.UserRole)) if cur else None
        tracks = a["tracks"] if a else []
        multi = len({t["artist"] for t in tracks}) > 1
        self.tracks.hide = {"album", "year", "genre", "format"} | (set() if multi else {"artist"})
        self.tracks.set_tracks(tracks)
        total = sum(t["duration"] for t in tracks)
        self.c_trk.setText(f"{len(tracks)} · {fmt_time(total)}" if tracks else "")

    def album_menu(self, pos):
        it = self.albums.itemAt(pos)
        a = self.win.albums.get(it.data(Qt.UserRole)) if it else None
        if not a:
            return
        w, L = self.win, self.win.L
        m = QMenu(self)
        m.addAction(L("play"), lambda: w.play_list(a["tracks"], 0))
        m.addAction(L("shuffle"), lambda: w.play_shuffled(a["tracks"]))
        m.addAction(L("play next"), lambda: w.queue_next(a["tracks"]))
        m.addAction(L("add to queue"), lambda: w.queue_add(a["tracks"]))
        pm = m.addMenu(L("add to playlist"))
        for name in w.playlists:
            pm.addAction(name, lambda n=name: w.playlist_add(n, a["tracks"]))
        pm.addSeparator()
        pm.addAction(L("+ new playlist"), lambda: w.playlist_new(a["tracks"]))
        m.addAction(L("open album page"), lambda: w.go("album", a["key"]))
        m.addAction(L("edit album tags & cover"), lambda: w.go("tags", [t["path"] for t in a["tracks"]]))
        m.exec(self.albums.viewport().mapToGlobal(pos))

    def eventFilter(self, obj, e):
        if e.type() != QEvent.KeyPress or obj not in (self.albums, self.tracks):
            return False
        k = e.key()
        if k == Qt.Key_Right and obj is self.albums:
            self.tracks.setFocus()
            if not self.tracks.currentIndex().isValid() and self.tracks.proxy.rowCount():
                self.tracks.selectRow(0)
            return True
        if k == Qt.Key_Left and obj is self.tracks:
            self.albums.setFocus()
            return True
        if k in (Qt.Key_Return, Qt.Key_Enter):
            if obj is self.albums:
                self.play_album()
            elif self.tracks.currentIndex().isValid():
                self.tracks.activate(self.tracks.currentIndex())
            return True
        return False


class DialWidget(QWidget):
    """round record cover wrapped in a dot ring that fills with progress."""
    seeked = Signal(float)
    toggled = Signal()

    def __init__(self, win):
        super().__init__()
        self.win, self.v, self.key, self.angle = win, 0.0, "", 0.0
        self.timer = QTimer(self)
        self.timer.setInterval(33)
        self.timer.timeout.connect(self.tick)
        self.setCursor(Qt.PointingHandCursor)
        self.relayout()

    def relayout(self):
        s = int(self.win.cfg.get("dial_size", 290))
        self.setFixedSize(s, s)
        self.update()

    def set_spinning(self, on):
        if on and self.win.cfg.get("dial_spin", True) and self.isVisible():
            if not self.timer.isActive():
                self.timer.start()
        else:
            self.timer.stop()

    def tick(self):
        secs = max(2.0, float(self.win.cfg.get("dial_spin_seconds", 12)))
        self.angle = (self.angle + 360 / (secs * 30)) % 360
        self.update()

    def hideEvent(self, e):
        self.timer.stop()
        self.hideEvent_vis()
        super().hideEvent(e)

    def dancing(self):
        return bool(self.win.cfg.get("dial_visualizer", True)) and self.win.analyzer.available()

    def showEvent(self, e):
        super().showEvent(e)
        if self.dancing():
            self.win.analyzer.listen(self)

    def hideEvent_vis(self):
        self.win.analyzer.unlisten(self)

    def geometry_(self):
        s = min(self.width(), self.height())
        R = s / 2 - (21 if self.win.cfg.get("dial_visualizer", True) else 7)
        return s / 2, s / 2, R, R - 20

    def paintEvent(self, _e):
        c = self.win.c
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        p.setRenderHint(QPainter.SmoothPixmapTransform)
        cx, cy, R, rc = self.geometry_()
        n = max(40, int(2 * math.pi * R / 9.5))
        filled = int(round(n * self.v))
        fg, dim = QColor(c["text"]), QColor(c["dim"])
        dim.setAlphaF(0.3)
        p.setPen(Qt.NoPen)
        an = self.win.analyzer
        dance = self.dancing() and an.timer.isActive() and len(an.bands)
        if dance:
            half = Visualizer.resample(an.bands, n // 2 + 1)
            lv = [half[min(i, n - i)] for i in range(n)]      # bass at the top, mirrored down both sides
        for i in range(n):
            a = -math.pi / 2 + i / n * 2 * math.pi
            head = filled > 0 and i == filled - 1
            d = 9.0 if head else 4.2
            ca, sa = math.cos(a), math.sin(a)
            p.setBrush(fg if i < filled else dim)
            p.drawEllipse(QRectF(cx + R * ca - d / 2, cy + R * sa - d / 2, d, d))
            if dance:
                k = int(lv[i] * 3.2)
                for j in range(1, min(3, k) + 1):
                    rr = R + j * 6.5
                    q = QColor(fg)
                    q.setAlphaF(0.85 - j * 0.2)
                    p.setBrush(q)
                    dd = 3.6 - j * 0.4
                    p.drawEllipse(QRectF(cx + rr * ca - dd / 2, cy + rr * sa - dd / 2, dd, dd))
        pm = self.win.covers.get(self.key, int(rc * 2))
        p.save()
        p.translate(cx, cy)
        p.rotate(self.angle)
        clip = QPainterPath()
        clip.addEllipse(QRectF(-rc, -rc, rc * 2, rc * 2))
        p.setClipPath(clip)
        p.drawPixmap(QRectF(-rc, -rc, rc * 2, rc * 2), pm, QRectF(0, 0, pm.width(), pm.height()))
        p.restore()
        hole = rc * 0.16
        p.setBrush(QColor(c["panel"]))
        p.drawEllipse(QRectF(cx - hole, cy - hole, hole * 2, hole * 2))
        ring = QColor(c["text"])
        ring.setAlphaF(0.12)
        p.setPen(QPen(ring, 1))
        p.setBrush(Qt.NoBrush)
        p.drawEllipse(QRectF(cx - rc, cy - rc, rc * 2, rc * 2))
        p.setPen(Qt.NoPen)
        p.setBrush(dim)
        d = rc * 0.05
        p.drawEllipse(QRectF(cx - d, cy - d, d * 2, d * 2))

    def mousePressEvent(self, e):
        if e.button() != Qt.LeftButton:
            return
        cx, cy, R, rc = self.geometry_()
        x, y = e.position().x() - cx, e.position().y() - cy
        dist = math.hypot(x, y)
        if dist >= R - 16:
            frac = ((math.atan2(y, x) + math.pi / 2) / (2 * math.pi)) % 1.0
            self.v = frac
            self.update()
            self.seeked.emit(frac)
        elif dist <= rc:
            self.toggled.emit()


class NextRow(QWidget):
    clicked = Signal()

    def __init__(self):
        super().__init__()
        self.qidx = -1
        h = QHBoxLayout(self)
        h.setContentsMargins(4, 7, 4, 7)
        h.setSpacing(12)
        self.num = QLabel(objectName="nextnum")
        self.num.setFixedWidth(26)
        self.title = QLabel()
        self.title.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
        self.time = QLabel(objectName="time")
        h.addWidget(self.num)
        h.addWidget(self.title, 1)
        h.addWidget(self.time)
        self.setCursor(Qt.PointingHandCursor)

    def mousePressEvent(self, e):
        if e.button() == Qt.LeftButton:
            self.clicked.emit()


class NowPanel(QWidget):
    def __init__(self, win):
        super().__init__(objectName="nowpanel")
        self.win = win
        self.setAttribute(Qt.WA_StyledBackground, True)
        self.setFixedWidth(int(win.cfg.get("dial_size", 290)) + 44)
        v = QVBoxLayout(self)
        v.setContentsMargins(22, 18, 22, 16)
        v.setSpacing(3)
        v.addWidget(QLabel("NOW PLAYING", objectName="eyebrow"))
        v.addSpacing(12)
        self.dial = DialWidget(win)
        self.dial.seeked.connect(lambda f: win.player.setPosition(int(f * win.dur)))
        self.dial.toggled.connect(win.toggle_play)
        v.addWidget(self.dial, 0, Qt.AlignHCenter)
        v.addSpacing(16)
        self.meta = QLabel(objectName="eyebrow")
        self.title = ClickLabel(objectName="npBig")
        self.title.setCursor(Qt.PointingHandCursor)
        self.title.clicked.connect(win.goto_playing_album)
        self.artist = ClickLabel(objectName="dim")
        self.artist.setCursor(Qt.PointingHandCursor)
        self.artist.clicked.connect(win.goto_playing_artist)
        for wdg in (self.title, self.artist):
            wdg.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
        v.addWidget(self.meta)
        v.addSpacing(2)
        v.addWidget(self.title)
        v.addWidget(self.artist)
        v.addSpacing(8)
        tr = QHBoxLayout()
        tr.setSpacing(10)
        self.time = QLabel("0:00", objectName="bigtime")
        self.total = QLabel("", objectName="bigtotal")
        tr.addWidget(self.time, 0, Qt.AlignBottom)
        tr.addWidget(self.total, 0, Qt.AlignBottom)
        tr.addStretch()
        v.addLayout(tr)
        v.addSpacing(14)
        self.next_label = QLabel("UP NEXT", objectName="eyebrow")
        v.addWidget(self.next_label)
        v.addSpacing(2)
        self.rows = []
        for _ in range(4):
            r = NextRow()
            r.clicked.connect(lambda r=r: win.play_queue_index(r.qidx))
            self.rows.append(r)
            v.addWidget(r)
        v.addStretch()
        self.set_track(None)

    def relayout(self):
        self.dial.relayout()
        self.setFixedWidth(int(self.win.cfg.get("dial_size", 290)) + 44)

    def set_track(self, t):
        w = self.win
        width = self.width() - 44
        if not t:
            self.meta.setText("IDLE")
            self.title.setText(w.L("nothing playing"))
            self.artist.setText(w.L("pick something on the left"))
            self.dial.key = ""
            self.time.setText("0:00")
            self.total.setText("")
            self.dial.v = 0
            self.dial.update()
            self.refresh_next()
            return
        a = w.albums.get(t["album_key"])
        n = len(a["tracks"]) if a else 0
        bits = [f"TRACK {t['track']}" + (f" / {n}" if n else "") if t["track"] else "", quality(t)]
        self.meta.setText("  ·  ".join(b for b in bits if b))
        self.title.setText(self.title.fontMetrics().elidedText(t["title"], Qt.ElideRight, width))
        self.title.setToolTip(t["title"])
        sub = f"{t['artist']} — {t['album']}"
        self.artist.setText(self.artist.fontMetrics().elidedText(sub, Qt.ElideRight, width))
        self.dial.key = t["cover"]
        self.dial.update()
        self.refresh_next()

    def set_pos(self, ms, dur):
        self.time.setText(fmt_time(ms / 1000))
        self.total.setText("/ " + fmt_time(dur / 1000) if dur else "")
        self.dial.v = (ms / dur) if dur else 0
        self.dial.update()

    def refresh_next(self):
        w = self.win
        start = max(0, w.qi + 1)
        q = w.queue[start: start + len(self.rows)]
        self.next_label.setVisible(bool(q))
        width = self.width() - 140
        for i, r in enumerate(self.rows):
            if i < len(q):
                t = q[i]
                r.qidx = start + i
                r.num.setText(f"{i + 1:02}")
                r.title.setText(r.title.fontMetrics().elidedText(t["title"], Qt.ElideRight, max(60, width)))
                r.time.setText(fmt_time(t["duration"]))
                r.show()
            else:
                r.hide()


def blurred_cover(win, key, w, h):
    """soft colour wash made from the album art (cheap blur: tiny downscale, smooth upscale in steps)."""
    path = win.dir / "covers" / f"{key}.jpg"
    img = QImage(str(path)) if key and path.exists() else QImage()
    if img.isNull():
        return None
    img = img.scaled(14, 14, Qt.IgnoreAspectRatio, Qt.SmoothTransformation)
    for s in (56, 224):
        img = img.scaled(s, s, Qt.IgnoreAspectRatio, Qt.SmoothTransformation)
    side = max(w, h)
    return QPixmap.fromImage(img.scaled(side // 2 + 1, side // 2 + 1, Qt.IgnoreAspectRatio, Qt.SmoothTransformation))


class AmbientRoot(QWidget):
    """window background: theme colour, plus (optionally) a blurred wash of the current album cover."""

    def __init__(self, win):
        super().__init__(objectName="root")
        self.win = win
        self.setAttribute(Qt.WA_StyledBackground, True)
        self.key, self.pm, self.old, self.fade = "", None, None, 1.0
        self.anim = QVariantAnimation(self)
        self.anim.setDuration(900)
        self.anim.setStartValue(0.0)
        self.anim.setEndValue(1.0)
        self.anim.valueChanged.connect(self._fade)
        self.skin_bg, self.skin_bg_key = None, None
        self.load_skin_bg()

    def load_skin_bg(self):
        sk = active_skin(self.win)
        path = (Path(sk["path"]) / sk["background"]) if sk and sk.get("background") else None
        blur = bool(self.win.cfg.get("skin_bg_blur"))
        key = (str(path), blur)
        if key == self.skin_bg_key:
            return
        self.skin_bg_key = key
        self.skin_bg = None
        if path and path.exists():
            img = QImage(str(path))
            if not img.isNull():
                if blur:
                    small = img.scaled(48, 48, Qt.KeepAspectRatioByExpanding, Qt.SmoothTransformation)
                    for sz in (192, 768):
                        small = small.scaled(sz, sz, Qt.KeepAspectRatioByExpanding, Qt.SmoothTransformation)
                    img = small
                self.skin_bg = QPixmap.fromImage(img)
        self.update()

    def _fade(self, v):
        self.fade = float(v)
        self.update()

    def set_cover(self, key):
        if not self.win.cfg.get("ambient_background"):
            self.key, self.pm = key, None
            return
        if key == self.key and self.pm is not None:
            return
        self.old = self.pm
        self.key = key
        self.pm = blurred_cover(self.win, key, self.width(), self.height())
        self.fade = 0.0
        self.anim.stop()
        self.anim.start()

    def resizeEvent(self, e):
        super().resizeEvent(e)
        if self.pm is not None and max(self.width(), self.height()) // 2 > self.pm.width() + 40:
            self.pm = blurred_cover(self.win, self.key, self.width(), self.height())

    def paintEvent(self, e):
        opt = QStyleOption()
        opt.initFrom(self)
        p = QPainter(self)
        self.style().drawPrimitive(QStyle.PE_Widget, opt, p, self)
        if self.skin_bg is not None:
            p.setRenderHint(QPainter.SmoothPixmapTransform)
            pm = self.skin_bg
            sc = max(self.width() / pm.width(), self.height() / pm.height())
            w, h = pm.width() * sc, pm.height() * sc
            p.setOpacity(max(5, min(100, int(self.win.cfg.get("skin_bg_opacity", 60)))) / 100)
            p.drawPixmap(QRectF((self.width() - w) / 2, (self.height() - h) / 2, w, h), pm, QRectF(pm.rect()))
            p.setOpacity(1)
        if not self.win.cfg.get("ambient_background"):
            return
        strength = max(0, min(100, int(self.win.cfg.get("ambient_strength", 45)))) / 100
        p.setRenderHint(QPainter.SmoothPixmapTransform)
        side = max(self.width(), self.height())
        r = QRectF((self.width() - side) / 2, (self.height() - side) / 2, side, side)
        if self.old is not None and self.fade < 1:
            p.setOpacity(strength * (1 - self.fade))
            p.drawPixmap(r, self.old, QRectF(self.old.rect()))
        if self.pm is not None:
            p.setOpacity(strength * self.fade)
            p.drawPixmap(r, self.pm, QRectF(self.pm.rect()))
        p.setOpacity(1)                      # keep the top bar + player bar text readable over bright covers
        bg = QColor(self.win.c["bg"])
        light = bg.lightnessF() > 0.5
        H = self.height()
        for y0, y1, a0, a1 in ((0, 70, 0.55 if light else 0.35, 0), (H - 130, H, 0, 0.75 if light else 0.5)):
            g = QLinearGradient(0, y0, 0, y1)
            c0, c1 = QColor(bg), QColor(bg)
            c0.setAlphaF(a0)
            c1.setAlphaF(a1)
            g.setColorAt(0, c0)
            g.setColorAt(1, c1)
            p.fillRect(QRectF(0, y0, self.width(), y1 - y0), g)


class VisualizerPage(QWidget):
    """full-window visualizer over a moving blur of the cover. double-click (or ⤢) for immersive."""

    def __init__(self, win):
        super().__init__(objectName="vispage")
        self.win = win
        self.pm, self.key, self.t0 = None, None, _time.monotonic()
        v = QVBoxLayout(self)
        v.setContentsMargins(32, 24, 28, 22)
        v.setSpacing(10)
        top = QHBoxLayout()
        top.setSpacing(6)
        self.eyebrow = QLabel("VISUALIZER", objectName="eyebrow")
        top.addWidget(self.eyebrow)
        top.addStretch()
        self.chips = {}
        for s in Visualizer.STYLES:
            b = QPushButton(win.L(s), objectName="chip")
            b.setCheckable(True)
            b.setCursor(Qt.PointingHandCursor)
            b.clicked.connect(lambda _=False, s=s: self.set_style(s))
            self.chips[s] = b
            top.addWidget(b)
        top.addSpacing(10)
        self.colbtn = QPushButton(objectName="chip")
        self.colbtn.setCheckable(True)
        self.colbtn.setCursor(Qt.PointingHandCursor)
        self.colbtn.clicked.connect(self.toggle_color)
        top.addWidget(self.colbtn)
        top.addSpacing(10)
        self.imm = win.tbtn("immersive", lambda: win.set_immersive(not win.immersive), "pill")
        top.addWidget(self.imm)
        top.addWidget(win.tbtn("done", win.back, "pillPrimary"))
        self.topw = QWidget()
        self.topw.setLayout(top)
        v.addWidget(self.topw)
        self.vis = Visualizer(win, pitch=14)
        self.vis.center_cover = True
        self.vis.mouseDoubleClickEvent = lambda _e: win.set_immersive(not win.immersive)
        v.addWidget(self.vis, 1)
        bot = QHBoxLayout()
        bot.setSpacing(16)
        self.cover = QLabel()
        self.cover.setFixedSize(72, 72)
        bot.addWidget(self.cover)
        tv = QVBoxLayout()
        tv.setSpacing(2)
        tv.addStretch()
        self.title = QLabel(objectName="npBig")
        self.artist = QLabel(objectName="dim")
        for wdg in (self.title, self.artist):
            wdg.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
        tv.addWidget(self.title)
        tv.addWidget(self.artist)
        tv.addStretch()
        bot.addLayout(tv, 1)
        self.time = QLabel("", objectName="bigtime")
        bot.addWidget(self.time, 0, Qt.AlignBottom)
        v.addLayout(bot)
        self.timer = QTimer(self)
        self.timer.setInterval(40)
        self.timer.timeout.connect(self.update)
        self.refresh()

    def set_style(self, s):
        self.win.cfg["vis_style"] = s
        self.win.cfg.save()
        self.refresh()

    def toggle_color(self):
        self.win.cfg["vis_color"] = "mono" if self.win.cfg.get("vis_color") == "cover" else "cover"
        self.win.cfg.save()
        self.refresh()

    def refresh(self):
        st = self.win.cfg.get("vis_style", "dots")
        for s, b in self.chips.items():
            b.setChecked(s == st)
        cov = self.win.cfg.get("vis_color", "mono") == "cover"
        self.colbtn.setText(self.win.L("cover colour" if cov else "mono"))
        self.colbtn.setChecked(cov)
        self.imm.setText(self.win.L("exit immersive" if self.win.immersive else "immersive"))
        self.vis.update()

    def set_track(self, t):
        w = self.win
        if not t:
            self.title.setText(w.L("nothing playing"))
            self.artist.setText("")
            self.cover.clear()
            self.key = None
            self.pm = None
            return
        self.title.setText(t["title"])
        self.artist.setText(f"{t['artist']} — {t['album']}")
        pm = w.covers.get(t["cover"], 72)
        self.cover.setPixmap(pm)
        if t["cover"] != self.key:
            self.key = t["cover"]
            self.pm = blurred_cover(w, self.key, max(800, self.width()), max(600, self.height()))
        self.update()

    def set_pos(self, ms):
        self.time.setText(fmt_time(ms / 1000))

    def showEvent(self, e):
        super().showEvent(e)
        self.timer.start()
        self.refresh()
        w = self.win
        self.set_track(w.queue[w.qi] if 0 <= w.qi < len(w.queue) else None)

    def hideEvent(self, e):
        self.timer.stop()
        super().hideEvent(e)

    def paintEvent(self, _e):
        p = QPainter(self)
        p.setRenderHint(QPainter.SmoothPixmapTransform)
        if self.pm is None:
            return
        t = _time.monotonic() - self.t0
        lvl = min(1.0, self.win.analyzer.level) if self.win.analyzer.available() else 0.0
        side = max(self.width(), self.height()) * (1.25 + 0.05 * math.sin(t / 7) + 0.04 * lvl)
        cx = self.width() / 2 + math.sin(t / 11) * self.width() * 0.06
        cy = self.height() / 2 + math.cos(t / 13) * self.height() * 0.06
        p.translate(cx, cy)
        p.rotate(math.sin(t / 17) * 12)
        p.setOpacity(0.55 + 0.15 * lvl)
        p.drawPixmap(QRectF(-side / 2, -side / 2, side, side), self.pm, QRectF(self.pm.rect()))
        p.resetTransform()
        p.setOpacity(1)
        g = QLinearGradient(0, 0, 0, self.height())
        bg = QColor(self.win.c["panel"])
        a, b = QColor(bg), QColor(bg)
        a.setAlphaF(0.15)
        b.setAlphaF(0.75)
        g.setColorAt(0, a)
        g.setColorAt(1, b)
        p.fillRect(self.rect(), g)


class SideTabs(QWidget):
    """settings sections: a list on the left, the section on the right (macOS-settings style).
    keeps the QTabWidget calls the rest of the app uses."""
    currentChanged = Signal(int)

    def __init__(self, win):
        super().__init__()
        self.win = win
        self.names = []
        h = QHBoxLayout(self)
        h.setContentsMargins(0, 0, 0, 0)
        h.setSpacing(28)
        self.nav = QListWidget(objectName="settingsnav")
        self.nav.setFixedWidth(170)
        self.nav.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.nav.setCursor(Qt.PointingHandCursor)
        self.stack = QStackedWidget()
        h.addWidget(self.nav)
        h.addWidget(self.stack, 1)
        self.nav.currentRowChanged.connect(self._row)

    def _row(self, i):
        if 0 <= i < self.stack.count():
            self.stack.setCurrentIndex(i)
            self.currentChanged.emit(i)

    def addTab(self, w, name):
        self.names.append(name)
        it = QListWidgetItem(self.win.L(name))
        self.nav.addItem(it)
        page = QWidget()
        v = QVBoxLayout(page)
        v.setContentsMargins(0, 0, 0, 0)
        v.setSpacing(14)
        v.addWidget(QLabel(name[:1].upper() + name[1:], objectName="h2"))
        if not isinstance(w, QScrollArea):
            sa = QScrollArea()
            sa.setWidgetResizable(True)
            sa.setFrameShape(QScrollArea.NoFrame)
            sa.setWidget(w)
            w = sa
        w.setHorizontalScrollBarPolicy(Qt.ScrollBarAsNeeded)
        v.addWidget(w, 1)
        self.stack.addWidget(page)
        if self.nav.currentRow() < 0:
            self.nav.setCurrentRow(0)
        return len(self.names) - 1

    def count(self):
        return len(self.names)

    def tabText(self, i):
        return self.names[i]

    def currentIndex(self):
        return self.nav.currentRow()

    def setCurrentIndex(self, i):
        self.nav.setCurrentRow(i)

    def widget(self, i):
        return self.stack.widget(i)


# ------------------------------------------------------------------ tag editing (read / write every format)
TAG_FIELDS = [("title", "title"), ("artist", "artist"), ("album", "album"), ("albumartist", "album artist"),
              ("tracknumber", "track"), ("discnumber", "disc"), ("date", "year / date"), ("genre", "genre"),
              ("composer", "composer"), ("comment", "comment"), ("lyrics", "lyrics")]

_VORBIS = {"title": ["TITLE"], "artist": ["ARTIST"], "album": ["ALBUM"],
           "albumartist": ["ALBUMARTIST", "ALBUM ARTIST", "ALBUM_ARTIST"], "tracknumber": ["TRACKNUMBER"],
           "discnumber": ["DISCNUMBER"], "date": ["DATE", "YEAR"], "genre": ["GENRE"], "composer": ["COMPOSER"],
           "comment": ["COMMENT", "DESCRIPTION"], "lyrics": ["LYRICS", "UNSYNCEDLYRICS"]}
_ID3 = {"title": "TIT2", "artist": "TPE1", "album": "TALB", "albumartist": "TPE2", "tracknumber": "TRCK",
        "discnumber": "TPOS", "date": "TDRC", "genre": "TCON", "composer": "TCOM"}
_MP4 = {"title": "\xa9nam", "artist": "\xa9ART", "album": "\xa9alb", "albumartist": "aART", "date": "\xa9day",
        "genre": "\xa9gen", "composer": "\xa9wrt", "comment": "\xa9cmt", "lyrics": "\xa9lyr"}
_APE = {"title": ["Title"], "artist": ["Artist"], "album": ["Album"], "albumartist": ["Album Artist", "AlbumArtist"],
        "tracknumber": ["Track"], "discnumber": ["Disc"], "date": ["Year", "Date"], "genre": ["Genre"],
        "composer": ["Composer"], "comment": ["Comment"], "lyrics": ["Lyrics"]}


def tag_family(f):
    n = type(f).__name__
    if n in ("FLAC", "OggVorbis", "OggOpus", "OggFLAC", "OggSpeex", "OggTheora"):
        return "vorbis"
    if n in ("MP4", "M4A", "EasyMP4"):
        return "mp4"
    if n in ("WavPack", "MonkeysAudio", "Musepack", "OptimFROG", "APEv2File", "TAK"):
        return "ape"
    return "id3"          # MP3, AIFF, WAVE, DSF, TrueAudio, …


def image_info(data):
    img = QImage()
    if not data or not img.loadFromData(data):
        return None
    kind = "PNG" if data[:8] == b"\x89PNG\r\n\x1a\n" else "JPEG" if data[:2] == b"\xff\xd8" else "image"
    return {"w": img.width(), "h": img.height(), "kind": kind, "bytes": len(data)}


def prepare_cover(data, max_side=0):
    """→ (bytes, mime). keeps JPEG/PNG as they are; converts anything else (webp, bmp…) to JPEG."""
    is_png = data[:8] == b"\x89PNG\r\n\x1a\n"
    is_jpg = data[:2] == b"\xff\xd8"
    img = QImage()
    if not img.loadFromData(data):
        raise ValueError("that isn't an image Nexus can read")
    if max_side and max(img.width(), img.height()) > max_side:
        img = img.scaled(max_side, max_side, Qt.KeepAspectRatio, Qt.SmoothTransformation)
    elif is_png or is_jpg:
        return data, ("image/png" if is_png else "image/jpeg")
    ba = QByteArray()
    buf = QBuffer(ba)
    buf.open(QIODevice.WriteOnly)
    img.save(buf, "JPG", 93)
    buf.close()
    return bytes(ba), "image/jpeg"


def read_tags(path, folder=True):
    """→ {field: text, …, "_embedded": bytes|None, "_folder": bytes|None, "_family": str}"""
    out = {k: "" for k, _ in TAG_FIELDS}
    out["_embedded"], out["_folder"], out["_family"] = None, None, ""
    f = mutagen.File(path)
    if f is None:
        raise ValueError("not an audio file mutagen can read")
    fam = out["_family"] = tag_family(f)
    t = f.tags
    if t is not None:
        if fam == "vorbis":
            low = {k.lower(): k for k in t.keys()}
            for field, keys in _VORBIS.items():
                for k in keys:
                    if k.lower() in low:
                        v = t[low[k.lower()]]
                        out[field] = "; ".join(str(x) for x in v) if isinstance(v, list) else str(v)
                        break
            tt = low.get("tracktotal") or low.get("totaltracks")
            if tt and out["tracknumber"] and "/" not in out["tracknumber"]:
                out["tracknumber"] += "/" + str(t[tt][0])
            dt = low.get("disctotal") or low.get("totaldiscs")
            if dt and out["discnumber"] and "/" not in out["discnumber"]:
                out["discnumber"] += "/" + str(t[dt][0])
            pics = list(getattr(f, "pictures", []) or [])
            if not pics and "metadata_block_picture" in low:
                import base64
                from mutagen.flac import Picture
                try:
                    pics = [Picture(base64.b64decode(x)) for x in t[low["metadata_block_picture"]]]
                except Exception:
                    pics = []
            if pics:
                front = [p for p in pics if p.type == 3]
                out["_embedded"] = (front or pics)[0].data
        elif fam == "mp4":
            for field, k in _MP4.items():
                if k in t:
                    out[field] = "; ".join(str(x) for x in t[k])
            for field, k in (("tracknumber", "trkn"), ("discnumber", "disk")):
                if k in t and t[k]:
                    n, tot = (tuple(t[k][0]) + (0, 0))[:2]
                    out[field] = f"{n}/{tot}" if tot else (str(n) if n else "")
            if "covr" in t and t["covr"]:
                out["_embedded"] = bytes(t["covr"][0])
        elif fam == "ape":
            low = {k.lower(): k for k in t.keys()}
            for field, keys in _APE.items():
                for k in keys:
                    if k.lower() in low:
                        out[field] = str(t[low[k.lower()]])
                        break
            ck = low.get("cover art (front)")
            if ck:
                raw = bytes(t[ck].value)
                out["_embedded"] = raw.split(b"\x00", 1)[1] if b"\x00" in raw[:256] else raw
        else:
            for field, fid in _ID3.items():
                fr = t.get(fid)
                if fr is not None and getattr(fr, "text", None):
                    out[field] = "; ".join(str(x) for x in fr.text)
            comms = t.getall("COMM") if hasattr(t, "getall") else []
            comm = next((c for c in comms if not c.desc), comms[0] if comms else None)
            if comm is not None and comm.text:
                out["comment"] = str(comm.text[0])
            us = t.getall("USLT") if hasattr(t, "getall") else []
            if us:
                out["lyrics"] = str(us[0].text)
            ap = t.getall("APIC") if hasattr(t, "getall") else []
            if ap:
                front = [a for a in ap if a.type == 3]
                out["_embedded"] = (front or ap)[0].data
    if folder:
        try:
            for p in Path(path).parent.iterdir():
                if p.suffix.lower() in IMG_EXT and p.stem.lower() in COVER_NAMES:
                    out["_folder"] = p.read_bytes()
                    break
        except OSError:
            pass
    return out


def write_tags(path, changes, cover=None):
    """changes: {field: text} ("" deletes it). cover: None = leave, b"" = remove, (bytes, mime) = set."""
    f = mutagen.File(path)
    if f is None:
        raise ValueError("not an audio file mutagen can read")
    fam = tag_family(f)
    if f.tags is None:
        f.add_tags()
    t = f.tags
    if fam == "vorbis":
        for field, val in changes.items():
            keys = _VORBIS[field]
            for k in list(t.keys()):
                if k.upper() in keys:
                    del t[k]
            if field in ("tracknumber", "discnumber"):
                tot_keys = ("TRACKTOTAL", "TOTALTRACKS") if field == "tracknumber" else ("DISCTOTAL", "TOTALDISCS")
                for k in list(t.keys()):
                    if k.upper() in tot_keys:
                        del t[k]
                if val and "/" in val:
                    n, tot = [x.strip() for x in val.split("/", 1)]
                    val = n
                    if tot:
                        t[tot_keys[0]] = [tot]
            if val:
                t[keys[0]] = [x.strip() for x in val.split(";")] if field in ("artist", "genre") and ";" in val else [val]
        if cover is not None:
            from mutagen.flac import Picture
            if hasattr(f, "clear_pictures"):
                f.clear_pictures()
            for k in list(t.keys()):
                if k.upper() in ("METADATA_BLOCK_PICTURE", "COVERART", "COVERARTMIME"):
                    del t[k]
            if cover:
                data, mime = cover
                pic = Picture()
                pic.type, pic.mime, pic.desc, pic.data = 3, mime, "", data
                info = image_info(data) or {}
                pic.width, pic.height, pic.depth = info.get("w", 0), info.get("h", 0), 24
                if hasattr(f, "add_picture"):
                    f.add_picture(pic)
                else:
                    import base64
                    t["METADATA_BLOCK_PICTURE"] = [base64.b64encode(pic.write()).decode("ascii")]
    elif fam == "mp4":
        from mutagen.mp4 import MP4Cover
        for field, val in changes.items():
            if field in ("tracknumber", "discnumber"):
                k = "trkn" if field == "tracknumber" else "disk"
                if not val:
                    t.pop(k, None)
                else:
                    n, _, tot = val.partition("/")
                    t[k] = [(_int(n), _int(tot))]
                continue
            k = _MP4[field]
            if val:
                t[k] = [val]
            else:
                t.pop(k, None)
        if cover is not None:
            if cover:
                data, mime = cover
                t["covr"] = [MP4Cover(data, MP4Cover.FORMAT_PNG if mime == "image/png" else MP4Cover.FORMAT_JPEG)]
            else:
                t.pop("covr", None)
    elif fam == "ape":
        from mutagen.apev2 import APEValue, BINARY
        for field, val in changes.items():
            for k in list(t.keys()):
                if k.lower() in [x.lower() for x in _APE[field]]:
                    del t[k]
            if val:
                t[_APE[field][0]] = val
        if cover is not None:
            for k in list(t.keys()):
                if k.lower() == "cover art (front)":
                    del t[k]
            if cover:
                data, mime = cover
                ext = "png" if mime == "image/png" else "jpg"
                t["Cover Art (Front)"] = APEValue(f"cover.{ext}".encode() + b"\x00" + data, BINARY)
    else:
        from mutagen import id3
        frames = {"TIT2": id3.TIT2, "TPE1": id3.TPE1, "TALB": id3.TALB, "TPE2": id3.TPE2, "TRCK": id3.TRCK,
                  "TPOS": id3.TPOS, "TDRC": id3.TDRC, "TCON": id3.TCON, "TCOM": id3.TCOM}
        for field, val in changes.items():
            if field in _ID3:
                fid = _ID3[field]
                t.delall(fid)
                if val:
                    t.add(frames[fid](encoding=3, text=[val]))
            elif field == "comment":
                t.delall("COMM")
                if val:
                    t.add(id3.COMM(encoding=3, lang="eng", desc="", text=[val]))
            elif field == "lyrics":
                t.delall("USLT")
                if val:
                    t.add(id3.USLT(encoding=3, lang="eng", desc="", text=val))
        if cover is not None:
            t.delall("APIC")
            if cover:
                data, mime = cover
                t.add(id3.APIC(encoding=3, mime=mime, type=3, desc="Cover", data=data))
    f.save()


def zip_has(path, name):
    import zipfile
    try:
        with zipfile.ZipFile(path) as z:
            return any(n.split("/")[-1] == name and n.count("/") <= 1 for n in z.namelist())
    except Exception:
        return False


def safe_filename(s):
    s = re.sub(r'[<>:"/\\|?*\x00-\x1f]', "", str(s)).strip().rstrip(".")
    return re.sub(r"\s+", " ", s)[:150] or "untitled"


def online_album_lookup(artist, album):
    """best match on iTunes (then Deezer) → {name, artist, year, genre, cover_url, source} or None."""
    def norm(x):
        return DiscordPresence._norm(x)
    a, al = norm(artist), norm(album)
    if not al:
        return None

    def get(url):
        req = urllib.request.Request(url, headers={"User-Agent": f"Nexus/{VERSION}"})
        with urllib.request.urlopen(req, timeout=8) as r:
            return json.loads(r.read().decode("utf-8"))

    def score(name, who):
        n, w = norm(name), norm(who)
        s = 0
        if n == al:
            s += 3
        elif len(al) > 3 and (al in n or n in al):
            s += 1
        if a and w and (a in w or w in a):
            s += 2
        return s
    try:
        q = urllib.parse.urlencode({"term": f"{artist} {album}", "entity": "album", "limit": 15})
        res = get(f"https://itunes.apple.com/search?{q}").get("results") or []
        best = max(res, key=lambda x: score(x.get("collectionName"), x.get("artistName")), default=None)
        if best and score(best.get("collectionName"), best.get("artistName")) >= 3:
            return {"name": best.get("collectionName", ""), "artist": best.get("artistName", ""),
                    "year": (best.get("releaseDate") or "")[:4], "genre": best.get("primaryGenreName", ""),
                    "cover_url": (best.get("artworkUrl100") or "").replace("100x100bb", "1200x1200bb"),
                    "source": "iTunes"}
    except Exception:
        pass
    try:
        q = urllib.parse.urlencode({"q": f'artist:"{artist}" album:"{album}"', "limit": 15})
        res = get(f"https://api.deezer.com/search/album?{q}").get("data") or []
        best = max(res, key=lambda x: score(x.get("title"), (x.get("artist") or {}).get("name")), default=None)
        if best and score(best.get("title"), (best.get("artist") or {}).get("name")) >= 3:
            out = {"name": best.get("title", ""), "artist": (best.get("artist") or {}).get("name", ""),
                   "year": "", "genre": "", "cover_url": best.get("cover_xl") or "", "source": "Deezer"}
            try:
                d = get(f"https://api.deezer.com/album/{best['id']}")
                out["year"] = (d.get("release_date") or "")[:4]
                g = (d.get("genres") or {}).get("data") or []
                out["genre"] = g[0]["name"] if g else ""
            except Exception:
                pass
            return out
    except Exception:
        pass
    return None


def fetch_bytes(url, limit=15_000_000):
    req = urllib.request.Request(url, headers={"User-Agent": f"Nexus/{VERSION}"})
    with urllib.request.urlopen(req, timeout=15) as r:
        return r.read(limit)


class CoverDrop(QLabel):
    """cover preview that takes dropped / pasted images."""
    dropped = Signal(bytes)

    def __init__(self, win, size=250):
        super().__init__()
        self.win, self.size_ = win, size
        self.setFixedSize(size, size)
        self.setAlignment(Qt.AlignCenter)
        self.setAcceptDrops(True)
        self.setObjectName("coverdrop")
        self.setToolTip("drop an image here")
        self.hover = False

    def show_bytes(self, data, text=""):
        if data:
            img = QImage()
            if img.loadFromData(data):
                pm = QPixmap.fromImage(img.scaled(self.size_ * 2, self.size_ * 2, Qt.KeepAspectRatio,
                                                  Qt.SmoothTransformation))
                pm.setDevicePixelRatio(2)
                self.setPixmap(pm)
                return
        self.clear()
        self.setText(text)

    def dragEnterEvent(self, e):
        md = e.mimeData()
        if md.hasImage() or any(u.toLocalFile().lower().endswith(tuple(IMG_EXT) + (".webp", ".bmp"))
                                for u in md.urls()):
            e.acceptProposedAction()
            self.hover = True
            self.update()

    def dragLeaveEvent(self, _e):
        self.hover = False
        self.update()

    def dropEvent(self, e):
        self.hover = False
        md = e.mimeData()
        data = None
        for u in md.urls():
            p = u.toLocalFile()
            if p and os.path.isfile(p):
                try:
                    data = Path(p).read_bytes()
                    break
                except OSError:
                    pass
        if data is None and md.hasImage():
            img = md.imageData()
            ba = QByteArray()
            buf = QBuffer(ba)
            buf.open(QIODevice.WriteOnly)
            QImage(img).save(buf, "PNG")
            data = bytes(ba)
        if data:
            self.dropped.emit(data)
        e.acceptProposedAction()
        self.update()

    def paintEvent(self, e):
        super().paintEvent(e)
        if self.hover:
            p = QPainter(self)
            p.setRenderHint(QPainter.Antialiasing)
            p.setPen(QPen(QColor(self.win.c["text"]), 2, Qt.DashLine))
            p.setBrush(Qt.NoBrush)
            p.drawRoundedRect(QRectF(self.rect()).adjusted(2, 2, -2, -2), 12, 12)


class TagEditorPage(QWidget):
    """edit tags + cover art for one song, a selection, or a whole album. nothing is written until Save."""
    MULTI = "‹multiple values›"
    COLS = [("tracknumber", "#", 54), ("discnumber", "disc", 50), ("title", "title", 0), ("artist", "artist", 0),
            ("album", "album", 0), ("date", "year", 64), ("_file", "file", 0)]

    def __init__(self, win):
        super().__init__(objectName="tagpage")
        self.win = win
        self.paths, self.orig, self.pending, self.covers_new = [], {}, {}, {}
        self.loading = False
        self._filling = False
        v = QVBoxLayout(self)
        v.setContentsMargins(32, 28, 28, 18)
        v.setSpacing(14)
        # header
        head = QHBoxLayout()
        tv = QVBoxLayout()
        tv.setSpacing(2)
        tv.addWidget(QLabel("EDIT TAGS", objectName="eyebrow"))
        self.h1 = QLabel(objectName="h1")
        self.h1.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
        tv.addWidget(self.h1)
        head.addLayout(tv, 1)
        self.status = QLabel(objectName="dim")
        head.addWidget(self.status, 0, Qt.AlignBottom)
        head.addSpacing(10)
        self.b_revert = win.tbtn("revert", self.revert, "pill")
        self.b_save = win.tbtn("save", self.save, "pillPrimary")
        head.addWidget(self.b_revert, 0, Qt.AlignBottom)
        head.addWidget(self.b_save, 0, Qt.AlignBottom)
        head.addWidget(win.tbtn("done", win.back, "pill"), 0, Qt.AlignBottom)
        v.addLayout(head)
        # body: cover column | fields
        body = QHBoxLayout()
        body.setSpacing(30)
        left = QVBoxLayout()
        left.setSpacing(8)
        self.cover = CoverDrop(win, 190)
        self.cover.dropped.connect(self.set_cover_bytes)
        left.addWidget(self.cover)
        self.cover_info = QLabel(objectName="time")
        self.cover_info.setWordWrap(True)
        self.cover_info.setFixedWidth(190)
        self.cover_info.setMinimumHeight(34)
        left.addWidget(self.cover_info)
        cb = QGridLayout()
        cb.setSpacing(6)
        for i, (label, fn) in enumerate((("choose image", self.choose_image), ("paste", self.paste_image),
                                          ("find online", self.find_online), ("remove", self.remove_cover))):
            b = win.tbtn(label, fn, "pill")
            cb.addWidget(b, i // 2, i % 2)
        self.b_folder = win.tbtn("use folder image", self.use_folder_image, "pill")
        cb.addWidget(self.b_folder, 2, 0, 1, 2)
        left.addLayout(cb)
        self.shrink = ToggleSwitch(win, "shrink to 1200 px")
        self.shrink.setToolTip("big covers make every file bigger; 1200 px still looks sharp everywhere")
        left.addWidget(self.shrink)
        self.online = QWidget(objectName="onlinebox")
        self.online.setAttribute(Qt.WA_StyledBackground, True)
        ol = QHBoxLayout(self.online)
        ol.setContentsMargins(14, 6, 8, 6)
        ol.setSpacing(8)
        self.online_l = QLabel(objectName="dim")
        self.online_l.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
        ol.addWidget(self.online_l, 1)
        self.b_on_cover = win.tbtn("use cover", lambda: self.use_online("cover"), "pill")
        self.b_on_info = win.tbtn("use year + genre", lambda: self.use_online("info"), "pill")
        ol.addWidget(self.b_on_cover)
        ol.addWidget(self.b_on_info)
        ol.addWidget(win.tbtn("✕", self.online_hide, "pill"))
        self.online.hide()
        self.online_res = None
        left.addStretch()
        body.addLayout(left)
        right = QVBoxLayout()
        right.setSpacing(6)
        self.applies = QLabel(objectName="eyebrow")
        right.addWidget(self.applies)
        form = QGridLayout()
        form.setHorizontalSpacing(14)
        form.setVerticalSpacing(8)
        self.fields = {}
        short = {"tracknumber", "discnumber", "date"}
        r = c = 0
        for key, label in TAG_FIELDS:
            if key == "lyrics":
                continue
            lab = QLabel(win.L(label), objectName="formlabel")
            ed = QLineEdit()
            ed.textEdited.connect(lambda txt, k=key: self.field_edited(k, txt))
            self.fields[key] = ed
            if key in short:
                ed.setMaximumWidth(140)
            form.addWidget(lab, r, c * 2)
            form.addWidget(ed, r, c * 2 + 1)
            c += 1
            if c == 2:
                c, r = 0, r + 1
        form.setColumnStretch(1, 1)
        form.setColumnStretch(3, 1)
        right.addLayout(form)
        right.addSpacing(4)
        lh = QHBoxLayout()
        lh.addWidget(QLabel(win.L("lyrics"), objectName="formlabel"))
        lh.addStretch()
        self.lyr_hint = QLabel("plain text, or [mm:ss.xx] timed lines for synced lyrics", objectName="time")
        lh.addWidget(self.lyr_hint)
        right.addLayout(lh)
        self.lyrics = QPlainTextEdit()
        self.lyrics.setFixedHeight(64)
        self.lyrics.textChanged.connect(self.lyrics_edited)
        self.fields["lyrics"] = self.lyrics
        right.addWidget(self.lyrics)
        body.addLayout(right, 1)
        v.addLayout(body)
        # tools
        tools = QHBoxLayout()
        tools.setSpacing(6)
        for label, fn, tip in (("number 1…n", self.auto_number, "number the selected songs in the order shown"),
                               ("titles from file names", self.titles_from_files, "“03 - Song name.flac” → Song name"),
                               ("Title Case", self.title_case, "capitalise titles in the selection"),
                               ("find / replace", lambda: self.toggle_bar(self.replace_bar), ""),
                               ("rename files", lambda: self.toggle_bar(self.rename_bar), "")):
            b = win.tbtn(label, fn, "pill")
            if tip:
                b.setToolTip(tip)
            tools.addWidget(b)
        tools.addStretch()
        self.sel_all = win.tbtn("select all", lambda: self.table.selectAll(), "pill")
        tools.addWidget(self.sel_all)
        v.addLayout(tools)
        v.addWidget(self.online)
        # find & replace bar
        self.replace_bar = QWidget()
        rb = QHBoxLayout(self.replace_bar)
        rb.setContentsMargins(0, 0, 0, 0)
        rb.setSpacing(8)
        self.rep_field = QComboBox()
        for key, label in TAG_FIELDS:
            if key != "lyrics":
                self.rep_field.addItem(win.L(label), key)
        self.rep_find = QLineEdit()
        self.rep_find.setPlaceholderText(win.L("find"))
        self.rep_with = QLineEdit()
        self.rep_with.setPlaceholderText(win.L("replace with"))
        rb.addWidget(QLabel(win.L("in"), objectName="dim"))
        rb.addWidget(self.rep_field)
        rb.addWidget(self.rep_find, 1)
        rb.addWidget(self.rep_with, 1)
        rb.addWidget(win.tbtn("replace in selection", self.do_replace, "pill"))
        self.replace_bar.hide()
        v.addWidget(self.replace_bar)
        # rename bar
        self.rename_bar = QWidget()
        nb = QHBoxLayout(self.rename_bar)
        nb.setContentsMargins(0, 0, 0, 0)
        nb.setSpacing(8)
        self.pattern = QLineEdit(win.cfg.get("rename_pattern", "{track:02} {title}"))
        self.pattern.setToolTip("{track} {disc} {title} {artist} {album} {albumartist} {year} — e.g. "
                                "{disc}-{track:02} {artist} - {title}")
        self.pattern.textChanged.connect(self.preview_rename)
        nb.addWidget(QLabel(win.L("file name"), objectName="dim"))
        nb.addWidget(self.pattern, 1)
        self.rename_prev = QLabel(objectName="time")
        nb.addWidget(self.rename_prev, 1)
        nb.addWidget(win.tbtn("rename selected files", self.rename_files, "pill"))
        self.rename_bar.hide()
        v.addWidget(self.rename_bar)
        # table
        self.table = QTableWidget(0, len(self.COLS), objectName="tagtable")
        self.table.setHorizontalHeaderLabels([win.L(h) for _k, h, _w in self.COLS])
        self.table.verticalHeader().hide()
        self.table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.table.setEditTriggers(QAbstractItemView.DoubleClicked | QAbstractItemView.EditKeyPressed |
                                   QAbstractItemView.AnyKeyPressed)
        self.table.setShowGrid(False)
        self.table.setWordWrap(False)
        self.table.verticalHeader().setDefaultSectionSize(30)
        hh = self.table.horizontalHeader()
        for i, (_k, _h, wdt) in enumerate(self.COLS):
            if wdt:
                hh.setSectionResizeMode(i, QHeaderView.Fixed)
                self.table.setColumnWidth(i, wdt)
            else:
                hh.setSectionResizeMode(i, QHeaderView.Stretch)
        hh.setHighlightSections(False)
        self.table.itemSelectionChanged.connect(self.selection_changed)
        self.table.itemChanged.connect(self.cell_edited)
        self.table.setMinimumHeight(120)
        v.addWidget(self.table, 1)
        self.refresh_buttons()

    # ------------------------------------------------------------ loading
    def dirty(self):
        return bool(any(self.pending.values()) or self.covers_new)

    def load(self, paths):
        paths = [p for p in (paths or []) if os.path.exists(p)]
        if paths == self.paths and self.orig:
            return
        if self.dirty():
            self.win.toast("unsaved tag edits were discarded")
        self.paths, self.orig, self.pending, self.covers_new = paths, {}, {}, {}
        self.online.hide()
        self.loading = True
        self.status.setText(self.win.L(f"reading {len(paths)} files…"))
        self.table.setRowCount(0)

        def work():
            out, bad = {}, []
            for p in paths:
                try:
                    out[p] = read_tags(p)
                except Exception as e:
                    bad.append(f"{os.path.basename(p)}: {e}")
            return out, bad

        def done(res, err):
            self.loading = False
            if err or res is None:
                self.status.setText(str(err))
                return
            self.orig, bad = res
            self.paths = [p for p in self.paths if p in self.orig]
            self.status.setText(self.win.L(f"couldn't read {len(bad)} files") if bad else "")
            self.fill_table()
            self.table.selectAll()
            self.update_header()
        self.win.bg(work, done)

    def value(self, p, k):
        return self.pending.get(p, {}).get(k, self.orig.get(p, {}).get(k, ""))

    def update_header(self):
        albums = {self.value(p, "album") for p in self.paths}
        if len(self.paths) == 1:
            self.h1.setText(self.value(self.paths[0], "title") or os.path.basename(self.paths[0]))
        elif len(albums) == 1 and next(iter(albums)):
            self.h1.setText(next(iter(albums)))
        else:
            self.h1.setText(self.win.L(f"{len(self.paths)} songs"))
        self.refresh_buttons()

    def refresh_buttons(self):
        d = self.dirty()
        n = sum(1 for p in self.paths if self.pending.get(p) or p in self.covers_new)
        self.b_save.setEnabled(d)
        self.b_revert.setEnabled(d)
        self.b_save.setText(self.win.L(f"save {n} file{'s' if n != 1 else ''}") if d else self.win.L("save"))

    def fill_table(self):
        self._filling = True
        t = self.table
        t.setRowCount(len(self.paths))
        bold = QFont(t.font())
        bold.setBold(True)
        for r, p in enumerate(self.paths):
            for c, (k, _h, _w) in enumerate(self.COLS):
                text = os.path.basename(p) if k == "_file" else self.value(p, k)
                it = t.item(r, c)
                if it is None:
                    it = QTableWidgetItem()
                    t.setItem(r, c, it)
                it.setText(text)
                changed = k in self.pending.get(p, {}) and self.pending[p][k] != self.orig[p].get(k, "")
                it.setFont(bold if changed else t.font())
                it.setToolTip(p if k == "_file" else (f"was: {self.orig[p].get(k, '') or '—'}" if changed else ""))
                flags = Qt.ItemIsSelectable | Qt.ItemIsEnabled
                if k not in ("_file", "album"):
                    flags |= Qt.ItemIsEditable
                it.setFlags(flags)
                it.setData(Qt.UserRole, p)
        self._filling = False

    def selected_paths(self):
        rows = sorted({i.row() for i in self.table.selectedIndexes()})
        return [self.paths[r] for r in rows if r < len(self.paths)]

    # ------------------------------------------------------------ form <-> selection
    def selection_changed(self):
        sel = self.selected_paths()
        self._filling = True
        n = len(sel)
        if not n:
            txt = "select songs below"
        elif len(self.paths) == 1:
            txt = "editing this song"
        elif n == len(self.paths):
            txt = f"editing all {n} songs"
        else:
            txt = f"editing {n} of {len(self.paths)} songs"
        self.applies.setText(self.win.L(txt).upper())
        for key, ed in self.fields.items():
            vals = {self.value(p, key) for p in sel}
            if isinstance(ed, QPlainTextEdit):
                ed.setPlainText(next(iter(vals)) if len(vals) == 1 else "")
                ed.setPlaceholderText(self.MULTI if len(vals) > 1 else "")
            else:
                ed.setText(next(iter(vals)) if len(vals) == 1 else "")
                ed.setPlaceholderText(self.MULTI if len(vals) > 1 else "")
            ed.setEnabled(bool(n))
        self._filling = False
        self.show_cover(sel)
        self.preview_rename()

    def field_edited(self, key, text):
        if self._filling:
            return
        for p in self.selected_paths():
            self.pending.setdefault(p, {})[key] = text.strip() if key != "comment" else text
        self.after_edit()

    def lyrics_edited(self):
        if self._filling:
            return
        text = self.lyrics.toPlainText()
        for p in self.selected_paths():
            self.pending.setdefault(p, {})["lyrics"] = text
        self.after_edit()

    def cell_edited(self, it):
        if self._filling:
            return
        k = self.COLS[it.column()][0]
        p = it.data(Qt.UserRole)
        if not p or k == "_file":
            return
        self.pending.setdefault(p, {})[k] = it.text().strip()
        self.after_edit(refill_form=True)

    def after_edit(self, refill_form=False):
        for p in list(self.pending):                     # drop edits that put things back how they were
            self.pending[p] = {k: v for k, v in self.pending[p].items() if v != self.orig[p].get(k, "")}
        self.fill_table()                                  # same rows → the selection stays put
        if refill_form:
            self.selection_changed()
        self.update_header()

    def set_for_selection(self, fn):
        """fn(path, index_in_selection) → {field: value}"""
        sel = self.selected_paths()
        if not sel:
            self.win.toast("select some songs first")
            return
        for i, p in enumerate(sel):
            for k, val in fn(p, i).items():
                self.pending.setdefault(p, {})[k] = val
        self.after_edit(refill_form=True)

    # ------------------------------------------------------------ tools
    def auto_number(self):
        n = len(self.selected_paths())
        self.set_for_selection(lambda p, i: {"tracknumber": f"{i + 1}/{n}" if "/" in self.value(p, "tracknumber")
                                             else str(i + 1)})

    def titles_from_files(self):
        def fn(p, _i):
            stem = Path(p).stem
            stem = re.sub(r"^\s*(\d+[-.]?)?\d*\s*[-._)]*\s*", "", stem) or stem
            return {"title": stem.replace("_", " ").strip()}
        self.set_for_selection(fn)

    def title_case(self):
        small = {"a", "an", "the", "and", "but", "or", "for", "nor", "on", "at", "to", "of", "in", "by", "vs", "feat."}

        def tc(s):
            words = s.split(" ")
            out = []
            for i, w in enumerate(words):
                lw = w.lower()
                if i and lw in small:
                    out.append(lw)
                elif w.isupper() and len(w) > 1:
                    out.append(w)                 # keep acronyms / stylised caps
                else:
                    out.append(w[:1].upper() + w[1:])
            return " ".join(out)
        self.set_for_selection(lambda p, _i: {"title": tc(self.value(p, "title"))})

    def toggle_bar(self, bar):
        bar.setVisible(not bar.isVisible())
        if bar is self.rename_bar:
            self.preview_rename()

    def do_replace(self):
        k, a, b = self.rep_field.currentData(), self.rep_find.text(), self.rep_with.text()
        if not a:
            return
        hits = [p for p in self.selected_paths() if a in self.value(p, k)]
        self.set_for_selection(lambda p, _i: {k: self.value(p, k).replace(a, b)} if a in self.value(p, k) else {})
        self.status.setText(self.win.L(f"replaced in {len(hits)} songs"))

    def new_name(self, p):
        def num(x):
            return _int(x)
        vals = {"track": num(self.value(p, "tracknumber")), "disc": num(self.value(p, "discnumber")) or 1,
                "title": self.value(p, "title") or Path(p).stem, "artist": self.value(p, "artist"),
                "album": self.value(p, "album"), "albumartist": self.value(p, "albumartist") or self.value(p, "artist"),
                "year": self.value(p, "date")[:4]}
        try:
            name = self.pattern.text().format(**vals)
        except (KeyError, ValueError, IndexError) as e:
            raise ValueError(f"pattern problem: {e}")
        return safe_filename(name) + Path(p).suffix.lower()

    def preview_rename(self):
        if not self.rename_bar.isVisible():
            return
        sel = self.selected_paths()
        try:
            self.rename_prev.setText(("→ " + self.new_name(sel[0])) if sel else "")
        except ValueError as e:
            self.rename_prev.setText(str(e))

    def rename_files(self):
        sel = self.selected_paths()
        if not sel:
            return
        if self.dirty():
            self.win.toast("save your tag edits first, then rename")
            return
        try:
            plan = [(p, str(Path(p).with_name(self.new_name(p)))) for p in sel]
        except ValueError as e:
            self.status.setText(str(e))
            return
        self.win.cfg["rename_pattern"] = self.pattern.text()
        self.win.cfg.save()
        plan = [(a, b) for a, b in plan if a != b]
        if not plan:
            self.status.setText(self.win.L("names already match"))
            return
        moved, failed = {}, []
        for a, b in plan:
            if os.path.exists(b) and os.path.normcase(a) != os.path.normcase(b):
                failed.append(f"{os.path.basename(b)} already exists")
                continue
            try:
                os.replace(a, b) if os.path.normcase(a) == os.path.normcase(b) else os.rename(a, b)
                moved[a] = b
                lrc = Path(a).with_suffix(".lrc")
                if lrc.exists():
                    try:
                        lrc.rename(Path(b).with_suffix(".lrc"))
                    except OSError:
                        pass
            except OSError as e:
                failed.append(f"{os.path.basename(a)}: {'in use (playing?)' if getattr(e, 'winerror', 0) == 32 else e}")
        if moved:
            self.win.paths_moved(moved)
            self.paths = [moved.get(p, p) for p in self.paths]
            self.orig = {moved.get(p, p): v for p, v in self.orig.items()}
            self.fill_table()
        self.status.setText(self.win.L(f"renamed {len(moved)} files") +
                            (f" · {len(failed)} skipped: {failed[0]}" if failed else ""))

    # ------------------------------------------------------------ cover
    def current_cover(self, p):
        if p in self.covers_new:
            c = self.covers_new[p]
            return (c[0] if c else None), ("new" if c else "removed")
        o = self.orig.get(p, {})
        if o.get("_embedded"):
            return o["_embedded"], "embedded"
        if o.get("_folder"):
            return o["_folder"], "folder"
        return None, "none"

    def show_cover(self, sel=None):
        sel = self.selected_paths() if sel is None else sel
        if not sel:
            self.cover.show_bytes(None, "")
            self.cover_info.setText("")
            self.b_folder.hide()
            return
        covers = [self.current_cover(p) for p in sel]
        datas = {hashlib.md5(d).hexdigest() if d else None for d, _ in covers}
        kinds = {k for _, k in covers}
        if len(datas) > 1:
            self.cover.show_bytes(None, self.win.L("different covers"))
            self.cover_info.setText(self.win.L("drop an image to give them all the same cover"))
        else:
            d, k = covers[0]
            self.cover.show_bytes(d, self.win.L("no cover — drop an image here"))
            info = image_info(d) if d else None
            where = {"embedded": "embedded in the file", "folder": "folder image only (not in the file)",
                     "new": "new — not saved yet", "removed": "will be removed", "none": ""}[k]
            self.cover_info.setText("\n".join(x for x in (
                f"{info['w']}×{info['h']} {info['kind']} · {info['bytes'] // 1024} KB" if info else "", where) if x))
        has_folder = any(self.orig.get(p, {}).get("_folder") and not self.orig[p].get("_embedded") for p in sel)
        self.b_folder.setVisible(has_folder and not kinds & {"new"})

    def set_cover_bytes(self, data):
        sel = self.selected_paths()
        if not sel:
            self.win.toast("select some songs first")
            return
        try:
            prepped = prepare_cover(data, 1200 if self.shrink.isChecked() else 0)
        except ValueError as e:
            self.win.toast(str(e))
            return
        for p in sel:
            self.covers_new[p] = prepped
        self.show_cover()
        self.refresh_buttons()

    def choose_image(self):
        start = str(Path(self.selected_paths()[0]).parent) if self.selected_paths() else str(Path.home())
        fn, _ = QFileDialog.getOpenFileName(self, "cover image", start, "Images (*.jpg *.jpeg *.png *.webp *.bmp)")
        if fn:
            self.set_cover_bytes(Path(fn).read_bytes())

    def paste_image(self):
        md = QApplication.clipboard().mimeData()
        if md.hasImage():
            ba = QByteArray()
            buf = QBuffer(ba)
            buf.open(QIODevice.WriteOnly)
            QImage(md.imageData()).save(buf, "PNG")
            self.set_cover_bytes(bytes(ba))
        elif md.hasUrls() and md.urls()[0].isLocalFile():
            self.set_cover_bytes(Path(md.urls()[0].toLocalFile()).read_bytes())
        else:
            self.win.toast("no image on the clipboard")

    def remove_cover(self):
        for p in self.selected_paths():
            if self.orig.get(p, {}).get("_embedded"):
                self.covers_new[p] = b""
            else:
                self.covers_new.pop(p, None)
        self.show_cover()
        self.refresh_buttons()

    def use_folder_image(self):
        sel = self.selected_paths()
        data = next((self.orig[p]["_folder"] for p in sel if self.orig[p].get("_folder")), None)
        if data:
            self.set_cover_bytes(data)

    def find_online(self):
        sel = self.selected_paths() or self.paths
        if not sel:
            return
        p = sel[0]
        artist = self.value(p, "albumartist") or self.value(p, "artist")
        album = self.value(p, "album")
        if not album:
            self.win.toast("needs an album name to search for")
            return
        self.online.show()
        self.online_l.setText(self.win.L(f"searching for “{album}”…"))
        self.b_on_cover.hide()
        self.b_on_info.hide()

        def work():
            res = online_album_lookup(artist, album)
            if res and res.get("cover_url"):
                try:
                    res["cover"] = fetch_bytes(res["cover_url"])
                except Exception:
                    res["cover"] = None
            return res

        def done(res, err):
            self.online_res = res if not err else None
            if not self.online_res:
                self.online_l.setText(self.win.L("nothing matching found on iTunes or Deezer"))
                return
            r = self.online_res
            info = image_info(r.get("cover")) if r.get("cover") else None
            bits = [x for x in (r["year"], r["genre"], f"{info['w']}×{info['h']} cover" if info else "") if x]
            self.online_l.setText(f"{r['source']}:  {r['name']} — {r['artist']}   ·   " + "  ·  ".join(bits))
            self.b_on_cover.setVisible(bool(info))
            self.b_on_info.setVisible(bool(r["year"] or r["genre"]))
        self.win.bg(work, done)

    def online_hide(self):
        self.online.hide()

    def use_online(self, what):
        r = self.online_res
        if not r:
            return
        if what == "cover" and r.get("cover"):
            self.set_cover_bytes(r["cover"])
        elif what == "info":
            def fn(p, _i):
                out = {}
                if r["year"] and not self.value(p, "date"):
                    out["date"] = r["year"]
                if r["genre"]:
                    out["genre"] = r["genre"]
                return out
            self.set_for_selection(fn)

    # ------------------------------------------------------------ save / revert
    def revert(self):
        self.pending, self.covers_new = {}, {}
        self.fill_table()
        self.selection_changed()
        self.update_header()
        self.status.setText(self.win.L("changes undone"))

    def save(self):
        jobs = [(p, dict(self.pending.get(p, {})), self.covers_new.get(p)) for p in self.paths
                if self.pending.get(p) or p in self.covers_new]
        if not jobs:
            return
        self.b_save.setEnabled(False)
        self.status.setText(self.win.L(f"saving {len(jobs)} files…"))

        def work():
            ok, bad = [], []
            for p, ch, cov in jobs:
                try:
                    write_tags(p, ch, cov)
                    ok.append(p)
                except Exception as e:
                    msg = "in use — try again when it isn't playing" if "denied" in str(e).lower() or \
                        getattr(e, "winerror", 0) in (5, 32) else str(e)
                    bad.append(f"{os.path.basename(p)}: {msg}")
            return ok, bad

        def done(res, err):
            if err:
                self.status.setText(str(err))
                self.refresh_buttons()
                return
            ok, bad = res
            covers_changed = {p for p in ok if p in self.covers_new}
            for p in ok:
                self.pending.pop(p, None)
                self.covers_new.pop(p, None)
            self.win.tags_saved(ok, covers_changed)
            try:
                for p in ok:
                    self.orig[p] = read_tags(p)
            except Exception:
                pass
            self.fill_table()
            self.selection_changed()
            self.update_header()
            self.status.setText(self.win.L(f"saved {len(ok)} files") + (f" · {len(bad)} failed: {bad[0]}" if bad else ""))
        self.win.bg(work, done)


# ------------------------------------------------------------------ skins (.nexusskin files)
# a skin is a zip: skin.json (name, author, colours, look settings) + optional style.qss, assets/ (images)
# and fonts/ (.ttf/.otf). no code — only whitelisted settings, a stylesheet and files, so imported skins are safe.
SKIN_EXT = ".nexusskin"
SKIN_FORMAT = 1
SKIN_KEYS = {          # look settings a skin may set → expected type
    "layout": str, "now_panel": bool, "dial_size": int, "dial_spin": bool, "dial_spin_seconds": int,
    "theme": str, "font_family": str, "display_font": str, "font_size": (int, float), "uppercase": bool,
    "brackets": str, "bracket_padding": bool, "nav_marker": str, "header_case": str, "header_text": str,
    "corner_radius": int, "cover_size": int, "list_cover_size": int, "placeholder": str, "ui_case": str,
    "control_style": str, "pill_buttons": bool, "progress_style": str, "volume_style": str,
    "progress_chars": str, "glyphs": dict, "sidebar_width": int, "ambient_background": bool,
    "ambient_strength": int, "dial_visualizer": bool, "vis_style": str, "vis_color": str, "vis_sensitivity": int,
    "background_alpha": int, "skin_bg_opacity": int, "skin_bg_blur": bool,
}
SKIN_LIMIT = 60_000_000
_SKIN_FILE_OK = {".json", ".qss", ".css", ".txt", ".md", ".png", ".jpg", ".jpeg", ".webp", ".gif", ".svg",
                 ".ttf", ".otf"}


def skins_dir(win):
    d = win.dir / "skins"
    d.mkdir(parents=True, exist_ok=True)
    return d


def skin_slug(name):
    s = re.sub(r"[^a-z0-9]+", "-", str(name).lower()).strip("-")
    return s[:48] or "skin"


def clean_skin_meta(meta):
    """keep only known keys with the right types; colours must look like colours."""
    out = {"name": str(meta.get("name") or "Untitled skin")[:60], "author": str(meta.get("author") or "")[:60],
           "description": str(meta.get("description") or "")[:300], "version": str(meta.get("version") or "1.0")[:20],
           "format": SKIN_FORMAT, "background": "", "settings": {}, "colors": {}}
    bg = str(meta.get("background") or "")
    if bg and not bg.startswith("/") and ".." not in bg.replace("\\", "/").split("/"):
        out["background"] = bg.replace("\\", "/")
    for k, v in (meta.get("colors") or {}).items():
        if k in COLOR_KEYS and isinstance(v, str) and QColor(v).isValid():
            out["colors"][k] = QColor(v).name()
    for k, v in (meta.get("settings") or {}).items():
        t = SKIN_KEYS.get(k)
        if t is None:
            continue
        if t is bool and isinstance(v, bool) or t is not bool and isinstance(v, t) and not isinstance(v, bool):
            if k == "glyphs":
                v = {str(a)[:12]: str(b)[:12] for a, b in v.items()}
            if isinstance(v, str):
                v = v[:200]
            out["settings"][k] = v
    return out


def read_skin_dir(path):
    try:
        meta = json.loads((path / "skin.json").read_text("utf-8"))
    except Exception:
        return None
    m = clean_skin_meta(meta)
    m["slug"], m["path"] = path.name, path
    qss = path / "style.qss"
    m["qss"] = qss.read_text("utf-8", errors="replace") if qss.exists() else ""
    return m


def list_skins(win):
    out = []
    for d in sorted(skins_dir(win).iterdir()):
        if d.is_dir():
            m = read_skin_dir(d)
            if m:
                out.append(m)
    return out


def import_skin_file(win, file):
    """unzip a .nexusskin into the skins folder (safely). returns the skin's slug."""
    import zipfile
    if os.path.getsize(file) > SKIN_LIMIT:
        raise ValueError("that skin is too big (60 MB max)")
    with zipfile.ZipFile(file) as z:
        names = [i for i in z.infolist() if not i.is_dir()]
        meta_name = next((i.filename for i in names if i.filename.split("/")[-1] == "skin.json"
                          and i.filename.count("/") <= 1), None)
        if not meta_name:
            raise ValueError("not a Nexus skin (no skin.json inside)")
        prefix = meta_name[: -len("skin.json")]
        meta = clean_skin_meta(json.loads(z.read(meta_name).decode("utf-8")))
        if sum(i.file_size for i in names) > SKIN_LIMIT:
            raise ValueError("that skin unpacks too big")
        dest = skins_dir(win) / skin_slug(meta["name"])
        tmp = dest.with_name(dest.name + ".tmp")
        shutil.rmtree(tmp, ignore_errors=True)
        tmp.mkdir(parents=True)
        for i in names:
            if not i.filename.startswith(prefix):
                continue
            rel = i.filename[len(prefix):]
            parts = rel.replace("\\", "/").split("/")
            if ".." in parts or rel.startswith("/") or ":" in rel or not rel:
                continue
            if os.path.splitext(rel)[1].lower() not in _SKIN_FILE_OK:
                continue                                   # skins carry no code: anything else is dropped
            out = tmp.joinpath(*parts)
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_bytes(z.read(i))
        (tmp / "skin.json").write_text(json.dumps({k: v for k, v in meta.items()}, indent=2), "utf-8")
        shutil.rmtree(dest, ignore_errors=True)
        tmp.rename(dest)
    return dest.name


def export_skin(skin, file):
    import zipfile
    root = Path(skin["path"])
    with zipfile.ZipFile(file, "w", zipfile.ZIP_DEFLATED) as z:
        for p in sorted(root.rglob("*")):
            if p.is_file() and p.suffix.lower() in _SKIN_FILE_OK:
                z.write(p, p.relative_to(root).as_posix())


def skin_qss(win, skin):
    """the skin's stylesheet with @bg / @text… colour tokens and skin: urls filled in."""
    q = skin.get("qss") or ""
    if not q:
        return ""
    c = theme_colors(win.cfg)
    for k in sorted(COLOR_KEYS, key=len, reverse=True):
        q = q.replace("@" + k, c[k])
    base = Path(skin["path"]).as_posix()
    return re.sub(r"url\(\s*['\"]?skin:([^)'\"]+)['\"]?\s*\)", lambda m: f'url("{base}/{m.group(1).strip()}")', q)


def active_skin(win):
    slug = win.cfg.get("skin") or ""
    if not slug:
        return None
    cache = win.__dict__.get("_skin")
    if cache and cache.get("slug") == slug:
        return cache
    d = skins_dir(win) / slug
    m = read_skin_dir(d) if d.is_dir() else None
    win._skin = m
    if m:
        fd = d / "fonts"
        if fd.is_dir():
            for f in sorted(fd.glob("*.[ot]tf")):
                QFontDatabase.addApplicationFont(str(f))
    return m


def apply_skin(win, slug):
    """skin settings on top of the defaults (so nothing from the old look leaks through), colours, stylesheet."""
    d = skins_dir(win) / slug
    m = read_skin_dir(d)
    if not m:
        raise ValueError("skin not found")
    cfg = win.cfg
    for k in SKIN_KEYS:
        if k in DEFAULTS:
            v = DEFAULTS[k]
            cfg[k] = json.loads(json.dumps(v)) if isinstance(v, (dict, list)) else v
    cfg["skin_bg_opacity"], cfg["skin_bg_blur"] = 60, False
    for k, v in m["settings"].items():
        if k == "glyphs":
            g = dict(DEFAULTS["glyphs"])
            g.update(v)
            v = g
        cfg[k] = v
    if cfg.get("theme") not in THEMES:
        cfg["theme"] = "nothing"
    cfg["color_overrides"] = dict(m["colors"])
    cfg["skin"] = slug
    cfg["preset"] = ""
    cfg.save()
    win.__dict__.pop("_skin", None)


def current_look_as_skin(win, name, author="", description="", qss="", background=None):
    """saves the current look (settings + colours) as a skin folder; returns its slug."""
    cfg = win.cfg
    settings = {k: cfg[k] for k in SKIN_KEYS if k in cfg}
    old = active_skin(win)
    slug = skin_slug(name)
    d = skins_dir(win) / slug
    d.mkdir(parents=True, exist_ok=True)
    if old and Path(old["path"]) != d:                 # building on another skin: bring its files along
        for sub in ("assets", "fonts"):
            src = Path(old["path"]) / sub
            if src.is_dir():
                shutil.copytree(src, d / sub, dirs_exist_ok=True)
    bg = old["background"] if old else ""
    if background is not None:
        if background == b"":
            bg = ""
        else:
            data, mime = background
            (d / "assets").mkdir(exist_ok=True)
            for f in (d / "assets").glob("background.*"):
                f.unlink()
            bg = "assets/background." + ("png" if mime == "image/png" else "jpg")
            (d / bg).write_bytes(data)
    meta = {"name": name, "author": author, "description": description, "version": "1.0", "format": SKIN_FORMAT,
            "made_with": f"Nexus {VERSION}", "background": bg,
            "colors": theme_colors(cfg), "settings": settings}
    (d / "skin.json").write_text(json.dumps(clean_skin_meta(meta) | {"made_with": meta["made_with"]}, indent=2),
                                 "utf-8")
    if qss.strip():
        (d / "style.qss").write_text(qss, "utf-8")
    elif (d / "style.qss").exists():
        (d / "style.qss").unlink()
    cfg["skin"] = slug
    cfg["preset"] = ""
    cfg.save()
    win.__dict__.pop("_skin", None)
    return slug


def install_bundled_skins(win):
    """first run: copy the example skins that ship with Nexus into the skins folder."""
    src = resource_dir() / "skins"
    if not src.is_dir():
        return
    have = {d.name for d in skins_dir(win).iterdir() if d.is_dir()}
    seen = set(win.cfg.get("skins_installed") or [])
    for f in sorted(src.glob("*" + SKIN_EXT)):
        if f.stem in seen:
            continue
        try:
            if skin_slug(f.stem) not in have:
                import_skin_file(win, f)
        except Exception as e:
            print("bundled skin failed:", f.name, e)
        seen.add(f.stem)
    win.cfg["skins_installed"] = sorted(seen)


class SkinCard(QWidget):
    """little preview of a skin: its colours drawn as a mini Nexus window."""
    clicked = Signal()

    def __init__(self, win, name, sub, colors, bg_path="", active=False, radius=12):
        super().__init__(objectName="skincard")
        self.win, self.name, self.sub, self.col, self.active, self.r = win, name, sub, colors, active, radius
        self.bg = QPixmap(str(bg_path)) if bg_path and Path(bg_path).exists() else None
        self.setFixedSize(196, 168)
        self.setCursor(Qt.PointingHandCursor)
        self.setToolTip("click to apply")

    def mousePressEvent(self, e):
        if e.button() == Qt.LeftButton:
            self.clicked.emit()

    def paintEvent(self, _e):
        c, k = self.win.c, self.col
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        prev = QRectF(4, 4, self.width() - 8, 112)
        path = QPainterPath()
        path.addRoundedRect(prev, 10, 10)
        p.setClipPath(path)
        p.fillRect(prev, QColor(k.get("bg", "#000")))
        if self.bg is not None:
            p.setOpacity(0.7)
            p.drawPixmap(prev, self.bg, QRectF(self.bg.rect()))
            p.setOpacity(1)
        rr = min(self.r, 8) / 2
        p.setPen(Qt.NoPen)
        p.setBrush(QColor(k.get("panel", "#111")))
        p.drawRoundedRect(QRectF(10, 10, 18, 92), rr, rr)                       # rail
        p.drawRoundedRect(QRectF(32, 10, 104, 92), rr, rr)                      # card
        p.drawRoundedRect(QRectF(140, 10, prev.width() - 140, 92), rr, rr)      # now playing
        for i in range(3):                                                       # album tiles
            p.setBrush(QColor(k.get("select", "#222")))
            p.drawRoundedRect(QRectF(38 + i * 32, 18, 26, 26), rr, rr)
        p.setBrush(QColor(k.get("text", "#fff")))
        p.drawRoundedRect(QRectF(38, 52, 60, 5), 2.5, 2.5)
        p.setBrush(QColor(k.get("dim", "#888")))
        p.drawRoundedRect(QRectF(38, 62, 42, 4), 2, 2)
        p.drawRoundedRect(QRectF(38, 72, 80, 4), 2, 2)
        cx, cy = 140 + (prev.width() - 140) / 2 + 2, 42
        p.setBrush(QColor(k.get("accent", "#fff")))
        p.drawEllipse(QPointF(cx, cy), 17, 17)
        p.setBrush(QColor(k.get("panel", "#111")))
        p.drawEllipse(QPointF(cx, cy), 4, 4)
        p.setBrush(QColor(k.get("text", "#fff")))
        p.drawEllipse(QPointF(cx, 86), 6, 6)
        p.setClipping(False)
        if self.active:
            p.setPen(QPen(QColor(c["text"]), 2))
            p.setBrush(Qt.NoBrush)
            p.drawRoundedRect(prev.adjusted(-2, -2, 2, 2), 12, 12)
        p.setPen(QColor(c["text"]))
        f = QFont(self.font())
        f.setBold(True)
        p.setFont(f)
        fm = QFontMetrics(f)
        p.drawText(QRectF(4, 122, self.width() - 8, 20), Qt.AlignLeft | Qt.AlignVCenter,
                   fm.elidedText(self.name, Qt.ElideRight, self.width() - 8))
        p.setFont(self.font())
        p.setPen(QColor(c["dim"]))
        p.drawText(QRectF(4, 142, self.width() - 8, 20), Qt.AlignLeft | Qt.AlignVCenter,
                   QFontMetrics(self.font()).elidedText(self.sub, Qt.ElideRight, self.width() - 8))


class FlowLayout(QLayout):
    """wraps widgets onto new lines like text."""

    def __init__(self, parent=None, spacing=14):
        super().__init__(parent)
        self.items, self.sp = [], spacing

    def addItem(self, item):
        self.items.append(item)

    def count(self):
        return len(self.items)

    def itemAt(self, i):
        return self.items[i] if 0 <= i < len(self.items) else None

    def takeAt(self, i):
        return self.items.pop(i) if 0 <= i < len(self.items) else None

    def hasHeightForWidth(self):
        return True

    def heightForWidth(self, w):
        return self._do(QRect(0, 0, w, 0), True)

    def setGeometry(self, r):
        super().setGeometry(r)
        self._do(r, False)

    def sizeHint(self):
        return self.minimumSize()

    def minimumSize(self):
        s = QSize()
        for it in self.items:
            s = s.expandedTo(it.minimumSize())
        return s

    def _do(self, r, test):
        x, y, line = r.x(), r.y(), 0
        for it in self.items:
            hs = it.sizeHint()
            if x + hs.width() > r.right() and line > 0:
                x, y, line = r.x(), y + line + self.sp, 0
            if not test:
                it.setGeometry(QRect(QPoint(x, y), hs))
            x += hs.width() + self.sp
            line = max(line, hs.height())
        return y + line - r.y()


class SkinsTab(QWidget):
    """settings → skins: gallery, import / export, and a small skin maker."""

    def __init__(self, settings):
        super().__init__()
        self.s, self.win = settings, settings.win
        win = self.win
        v = QVBoxLayout(self)
        v.setContentsMargins(0, 0, 0, 0)
        v.setSpacing(12)
        intro = QLabel("Skins change Nexus's whole look: colours, fonts, shapes, layout, glyphs, a background image and "
                       "their own stylesheet. Share them as .nexusskin files — drop one on the window to install it. "
                       "Skins can't contain code, so they're safe to import.", objectName="dim")
        intro.setWordWrap(True)
        v.addWidget(intro)
        row = QHBoxLayout()
        row.setSpacing(8)
        row.addWidget(win.tbtn("import skin file", self.import_dialog, "pill"))
        row.addWidget(win.tbtn("open skins folder", lambda: QDesktopServices.openUrl(
            QUrl.fromLocalFile(str(skins_dir(win)))), "pill"))
        row.addStretch()
        self.msg = QLabel(objectName="dim")
        row.addWidget(self.msg)
        v.addLayout(row)
        self.gallery_w = QWidget()
        self.flow = FlowLayout(self.gallery_w)
        v.addWidget(QLabel("YOUR SKINS", objectName="eyebrow"))
        v.addWidget(self.gallery_w)
        # maker
        v.addSpacing(10)
        v.addWidget(QLabel("MAKE A SKIN", objectName="eyebrow"))
        how = QLabel("Set things up how you like in Look (colours, fonts, corner radius, layout, visualizer…), add "
                     "a background image and some stylesheet here if you want, then save it as a skin.", objectName="dim")
        how.setWordWrap(True)
        v.addWidget(how)
        f = QFormLayout()
        sk = active_skin(win)
        self.name = QLineEdit(sk["name"] if sk else "")
        self.name.setPlaceholderText("My skin")
        self.author = QLineEdit(sk["author"] if sk else "")
        self.author.setPlaceholderText("your name")
        self.desc = QLineEdit(sk["description"] if sk else "")
        f.addRow("name", self.name)
        f.addRow("made by", self.author)
        f.addRow("description", self.desc)
        bgr = QHBoxLayout()
        self.bg_new = None
        self.bg_label = QLabel(objectName="time")
        self.bg_label.setText((sk["background"] if sk and sk["background"] else "none"))
        bgr.addWidget(win.tbtn("choose image", self.pick_bg, "pill"))
        bgr.addWidget(win.tbtn("remove", self.clear_bg, "pill"))
        bgr.addWidget(self.bg_label, 1)
        f.addRow("background image", bgr)
        f.addRow("background opacity %", settings.spin("skin_bg_opacity", 5, 100, step=5, rebuild=False))
        f.addRow("blur the background", settings.check("skin_bg_blur", rebuild=False))
        v.addLayout(f)
        qh = QHBoxLayout()
        qh.addWidget(QLabel("stylesheet (Qt CSS)", objectName="formlabel"))
        qh.addStretch()
        self.ref_btn = win.tbtn("reference", lambda: self.ref.setVisible(not self.ref.isVisible()), "pill")
        qh.addWidget(self.ref_btn)
        v.addLayout(qh)
        self.ref = QLabel(
            "colour tokens: @bg @panel @text @dim @accent @select @border  ·  images: url(skin:assets/file.png)\n"
            "parts: #root #topbar #rail #card #nowpanel #npbar #sidebar #search #brand #h1 #h2 #eyebrow #dim #time "
            "#badge #pill #pillPrimary #chip #nav #col #coltracks #settingsnav #eqpage #vispage #tagpage #lyric "
            "#bigtime #npBig #toast #coverdrop\nexample:  QWidget#card { border: 1px solid @accent; }   "
            "QLabel#h1 { color: @accent; }   QPushButton#pillPrimary { background: @accent; }", objectName="time")
        self.ref.setWordWrap(True)
        self.ref.setTextInteractionFlags(Qt.TextSelectableByMouse)
        self.ref.hide()
        v.addWidget(self.ref)
        self.qss = QPlainTextEdit(sk["qss"] if sk else "")
        self.qss.setPlaceholderText("/* optional — e.g.  QWidget#rail { background: @accent; } */")
        self.qss.setFixedHeight(130)
        v.addWidget(self.qss)
        brow = QHBoxLayout()
        brow.setSpacing(8)
        brow.addWidget(win.tbtn("preview stylesheet", self.preview, "pill"))
        brow.addWidget(win.tbtn("save as skin", self.save, "pillPrimary"))
        brow.addWidget(win.tbtn("save + export file", lambda: self.save(export=True), "pill"))
        brow.addStretch()
        v.addLayout(brow)
        v.addStretch()
        self.fill()

    def fill(self):
        while self.flow.count():
            it = self.flow.takeAt(0)
            if it.widget():
                it.widget().deleteLater()
        win = self.win
        cur = win.cfg.get("skin") or ""
        for name in PRESETS:
            th = PRESETS[name].get("theme", "nothing")
            cols = dict(THEMES.get(th, THEMES["nothing"]))
            card = SkinCard(win, name[:1].upper() + name[1:], "built in", cols,
                            active=(not cur and win.cfg.get("preset") == name))
            card.clicked.connect(lambda n=name: self.use_builtin(n))
            self.flow.addWidget(self.wrap(card, None))
        for m in list_skins(win):
            cols = dict(THEMES.get(m["settings"].get("theme", "nothing"), THEMES["nothing"]))
            cols.update(m["colors"])
            bgp = Path(m["path"]) / m["background"] if m["background"] else ""
            card = SkinCard(win, m["name"], ("by " + m["author"]) if m["author"] else m["description"] or "skin",
                            cols, bgp, active=(m["slug"] == cur))
            card.clicked.connect(lambda s=m["slug"]: self.use(s))
            self.flow.addWidget(self.wrap(card, m))
        self.gallery_w.updateGeometry()

    def wrap(self, card, m):
        w = QWidget()
        lay = QVBoxLayout(w)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(4)
        lay.addWidget(card)
        if m is not None:
            h = QHBoxLayout()
            h.setSpacing(4)
            b1 = self.win.tbtn("export", lambda: self.export(m), "chip")
            b2 = self.win.tbtn("delete", lambda: self.delete(m), "chip")
            h.addWidget(b1)
            h.addWidget(b2)
            h.addStretch()
            lay.addLayout(h)
        else:
            lay.addSpacing(30)
        return w

    def rebuild_soon(self):
        self.s.ready = False
        QTimer.singleShot(0, lambda: (self.win.rebuild_ui(), self.win.go("settings", "skins", push=False)))

    def use_builtin(self, name):
        self.win.cfg["skin"] = ""
        self.win.__dict__.pop("_skin", None)
        self.s.apply_preset(name)

    def use(self, slug):
        try:
            apply_skin(self.win, slug)
        except Exception as e:
            self.msg.setText(str(e))
            return
        self.rebuild_soon()

    def import_dialog(self):
        fn, _ = QFileDialog.getOpenFileName(self, "import skin", str(Path.home() / "Downloads"),
                                            f"Nexus skins (*{SKIN_EXT} *.zip)")
        if fn:
            self.win.import_skin(fn)

    def export(self, m):
        fn, _ = QFileDialog.getSaveFileName(self, "export skin", str(Path.home() / (skin_slug(m["name"]) + SKIN_EXT)),
                                            f"Nexus skin (*{SKIN_EXT})")
        if not fn:
            return
        if not fn.lower().endswith(SKIN_EXT):
            fn += SKIN_EXT
        try:
            export_skin(m, fn)
            self.win.toast(f"exported “{m['name']}”", "show", lambda: QDesktopServices.openUrl(
                QUrl.fromLocalFile(str(Path(fn).parent))))
        except Exception as e:
            self.msg.setText(f"export failed: {e}")

    def delete(self, m):
        tmp = Path(m["path"]).with_name(m["slug"] + ".deleted")
        shutil.rmtree(tmp, ignore_errors=True)
        try:
            Path(m["path"]).rename(tmp)
        except OSError as e:
            self.msg.setText(str(e))
            return
        was_active = self.win.cfg.get("skin") == m["slug"]
        if was_active:
            self.win.cfg["skin"] = ""
            self.win.__dict__.pop("_skin", None)
            self.win.cfg.save()
            self.win.apply_settings(rebuild=False)
        self.fill()

        def undo():
            if tmp.exists() and not Path(m["path"]).exists():
                tmp.rename(m["path"])
                if was_active:
                    self.use(m["slug"])
                else:
                    self.fill()
        self.win.toast(f"deleted “{m['name']}”", "undo", undo)
        QTimer.singleShot(15000, lambda: shutil.rmtree(tmp, ignore_errors=True))

    def pick_bg(self):
        fn, _ = QFileDialog.getOpenFileName(self, "background image", str(Path.home()),
                                            "Images (*.jpg *.jpeg *.png *.webp)")
        if fn:
            try:
                self.bg_new = prepare_cover(Path(fn).read_bytes(), 2560)
                self.bg_label.setText(os.path.basename(fn) + " (saved with the skin)")
            except Exception as e:
                self.msg.setText(str(e))

    def clear_bg(self):
        self.bg_new = b""
        self.bg_label.setText("none (saved with the skin)")

    def preview(self):
        sk = active_skin(self.win) or {"path": str(skins_dir(self.win)), "qss": ""}
        self.win._skin_preview = skin_qss(self.win, dict(sk, qss=self.qss.toPlainText()))
        self.win.apply_settings(rebuild=False)
        self.msg.setText("previewing — save the skin to keep it")

    def save(self, export=False):
        name = self.name.text().strip()
        if not name:
            self.msg.setText("give the skin a name first")
            self.name.setFocus()
            return
        try:
            slug = current_look_as_skin(self.win, name, self.author.text().strip(), self.desc.text().strip(),
                                        self.qss.toPlainText(), self.bg_new)
        except Exception as e:
            self.msg.setText(f"couldn't save: {e}")
            return
        self.win.__dict__.pop("_skin_preview", None)
        self.win.toast(f"saved skin “{name}”")
        if export:
            m = read_skin_dir(skins_dir(self.win) / slug)
            QTimer.singleShot(0, lambda: self.export(m))
        self.win.root.load_skin_bg()
        self.win.apply_settings(rebuild=False)
        self.fill()


# ------------------------------------------------------------------ discover (recommendations)
# looks at who's in your library (and what you actually play), asks Deezer's free public API who's similar,
# and shows artists you don't have yet, albums you're missing from artists you do have, and stuff to rediscover.
DEEZER = "https://api.deezer.com"
_SKIP_ARTISTS = {"unknown artist", "various artists", "various", "va", ""}


def deezer_get(path, tries=3):
    url = path if path.startswith("http") else DEEZER + path
    for i in range(tries):
        req = urllib.request.Request(url, headers={"User-Agent": f"Nexus/{VERSION}"})
        with urllib.request.urlopen(req, timeout=10) as r:
            d = json.loads(r.read().decode("utf-8"))
        err = d.get("error") if isinstance(d, dict) else None
        if not err:
            return d
        if err.get("code") == 4 and i + 1 < tries:            # quota: 50 requests / 5 s
            _time.sleep(1.5 * (i + 1))
            continue
        raise RuntimeError(err.get("message") or "deezer error")
    return {}


def _norm_name(s):
    return re.sub(r"[^\w]+", " ", str(s or "").lower().replace("$", "s")).strip()


class Discover:
    """the recommendation engine (runs on a worker thread)."""

    def __init__(self, win):
        self.win = win
        self.cache_file = win.dir / "discover.json"

    def load(self):
        try:
            return json.loads(self.cache_file.read_text("utf-8"))
        except Exception:
            return None

    def library_artists(self):
        """[(name, weight, albums, tracks, plays)] best first — plays count most, then how much you own."""
        stats = self.win.stats
        by = {}
        for t in self.win.tracks:
            a = t["albumartist"] or t["artist"]
            if _norm_name(a) in _SKIP_ARTISTS:
                continue
            e = by.setdefault(a, {"albums": set(), "tracks": 0, "plays": 0})
            e["albums"].add(t["album_key"])
            e["tracks"] += 1
            e["plays"] += int(stats.get(t["path"], {}).get("plays", 0))
        out = [(a, e["plays"] * 2 + len(e["albums"]) * 3 + e["tracks"] * 0.3, len(e["albums"]), e["tracks"], e["plays"])
               for a, e in by.items()]
        out.sort(key=lambda x: -x[1])
        return out

    def find_artist(self, name):
        d = deezer_get("/search/artist?" + urllib.parse.urlencode({"q": name, "limit": 6}))
        want = _norm_name(name)
        res = d.get("data") or []
        for a in res:
            if _norm_name(a.get("name")) == want:
                return a
        for a in res:
            n = _norm_name(a.get("name"))
            if len(want) >= 4 and (want in n or n in want):
                return a
        return None

    def run(self, progress=lambda *_: None, seeds_n=15):
        lib = self.library_artists()
        have = {_norm_name(a) for a, *_ in lib}
        for t in self.win.tracks:
            have.add(_norm_name(t["artist"]))
        lib_albums = {}
        for t in self.win.tracks:
            lib_albums.setdefault(_norm_name(t["albumartist"]), set()).add(DiscordPresence._norm(t["album"]))
        seeds = []
        for i, (name, w, albums, tracks, plays) in enumerate(lib[:seeds_n]):
            progress(f"looking up {name}…", i, seeds_n * 2)
            try:
                a = self.find_artist(name)
            except Exception:
                a = None
            if a:
                seeds.append({"name": name, "id": a["id"], "picture": a.get("picture_medium", ""),
                              "albums": albums, "tracks": tracks, "plays": plays, "weight": w})
        if not seeds:
            raise RuntimeError("couldn't find any of your artists on Deezer (or no internet)")
        scores, because, info = {}, {}, {}
        top_w = max(s["weight"] for s in seeds) or 1
        for i, s in enumerate(seeds):
            progress(f"who sounds like {s['name']}…", seeds_n + i, seeds_n * 2)
            try:
                rel = deezer_get(f"/artist/{s['id']}/related?limit=25").get("data") or []
            except Exception:
                rel = []
            sw = 0.5 + s["weight"] / top_w
            for rank, r in enumerate(rel):
                if _norm_name(r.get("name")) in have:
                    continue
                rid = r["id"]
                scores[rid] = scores.get(rid, 0) + sw / (1 + rank * 0.12)
                because.setdefault(rid, [])
                if s["name"] not in because[rid]:
                    because[rid].append(s["name"])
                info[rid] = r
        ranked = sorted(scores, key=lambda k: -(scores[k] * (1 + 0.15 * math.log10(1 + (info[k].get("nb_fan") or 0)))))
        recs = []
        for rid in ranked[:18]:
            r = info[rid]
            try:
                top = deezer_get(f"/artist/{rid}/top?limit=5").get("data") or []
            except Exception:
                top = []
            recs.append({"id": rid, "name": r.get("name", ""), "picture": r.get("picture_big") or r.get("picture_medium", ""),
                         "fans": r.get("nb_fan") or 0, "because": because[rid][:3],
                         "top": [{"title": x.get("title", ""), "preview": x.get("preview", ""), "tid": x.get("id"),
                                  "album": (x.get("album") or {}).get("title", ""),
                                  "cover": (x.get("album") or {}).get("cover_medium", ""),
                                  "duration": x.get("duration") or 0} for x in top]})
            progress(f"getting {r.get('name')}'s top songs…", len(recs), 18)
        missing = []
        for s in seeds[:10]:
            try:
                albs = deezer_get(f"/artist/{s['id']}/albums?limit=60").get("data") or []
            except Exception:
                continue
            mine = lib_albums.get(_norm_name(s["name"]), set())
            seen, got = set(), 0
            for a in sorted(albs, key=lambda x: x.get("release_date") or "", reverse=True):
                if a.get("record_type") not in ("album", "ep"):
                    continue
                n = DiscordPresence._norm(a.get("title"))
                if not n or n in seen or n in mine or any(n in m or m in n for m in mine if len(m) > 3):
                    continue
                seen.add(n)
                missing.append({"artist": s["name"], "title": a.get("title", ""), "year": (a.get("release_date") or "")[:4],
                                "type": a.get("record_type", "album"), "cover": a.get("cover_medium", ""),
                                "have": s["albums"]})
                got += 1
                if got >= 3:
                    break
        missing.sort(key=lambda x: x["year"], reverse=True)
        out = {"time": _time.time(), "seeds": [{k: s[k] for k in ("name", "picture", "albums", "tracks", "plays")}
                                               for s in seeds],
               "recs": recs, "missing": missing[:24]}
        try:
            self.cache_file.write_text(json.dumps(out), "utf-8")
        except OSError:
            pass
        return out


class NetImages(QObject):
    """downloads + disk-caches pictures off the ui thread."""
    ready = Signal(str, QImage)

    def __init__(self, win):
        super().__init__(win)
        self.dir = win.dir / "discover_img"
        self.dir.mkdir(parents=True, exist_ok=True)
        self.mem = {}
        from concurrent.futures import ThreadPoolExecutor
        self.pool = ThreadPoolExecutor(max_workers=4)

    def get(self, url):
        """→ QImage now if we have it, else None (and `ready` fires when it arrives)."""
        if not url:
            return None
        if url in self.mem:
            return self.mem[url]
        f = self.dir / (hashlib.md5(url.encode()).hexdigest() + ".jpg")
        if f.exists():
            img = QImage(str(f))
            if not img.isNull():
                self.mem[url] = img
                return img
        self.mem[url] = None
        self.pool.submit(self._fetch, url, f)
        return None

    def _fetch(self, url, f):
        try:
            data = fetch_bytes(url, 4_000_000)
            f.write_bytes(data)
            img = QImage()
            if img.loadFromData(data):
                self.mem[url] = img
                self.ready.emit(url, img)
        except Exception:
            pass


class PreviewPlayer(QObject):
    """30-second previews from Deezer; pauses your music while one plays and picks it back up after."""
    changed = Signal()

    def __init__(self, win):
        super().__init__(win)
        self.win, self.player, self.url, self.key, self.resume = win, None, "", "", False

    def _ensure(self):
        if self.player is None:
            self.player = QMediaPlayer(self)
            self.out = QAudioOutput(self)
            self.out.setVolume(0.8)
            self.player.setAudioOutput(self.out)
            self.player.mediaStatusChanged.connect(self._status)
        return self.player

    def _status(self, st):
        if st in (QMediaPlayer.EndOfMedia, QMediaPlayer.InvalidMedia):
            self.stop()

    def playing(self, key=None):
        return bool(self.url) and (key is None or key == self.key)

    def toggle(self, url, label="", key=None):
        key = key or url
        if self.playing(key):
            self.stop()
            return
        if not url:
            self.win.toast("no preview for that one")
            return
        p = self._ensure()
        if not self.url:
            self.resume = self.win.player.playbackState() == QMediaPlayer.PlayingState
            if self.resume:
                self.win.player.pause()
        self.url, self.key = url, key
        p.setSource(QUrl(url))
        p.play()
        self.win.toast(f"previewing {label}" if label else "previewing", "stop", self.stop, 31000)
        self.changed.emit()

    def stop(self):
        if self.player is not None:
            self.player.stop()
        was = bool(self.url)
        self.url = self.key = ""
        t = getattr(self.win, "_toast", None)
        try:
            if t is not None and t.action == self.stop:
                t.hide()
        except RuntimeError:
            pass
        if was and self.resume:
            self.win.player.play()
        self.resume = False
        self.changed.emit()


class RoundPic(QLabel):
    def __init__(self, size, round_=True, color="#222222"):
        super().__init__()
        self.s, self.round, self.color = size, round_, QColor(color)
        self.setFixedSize(size, size)
        self.img = None

    def set_image(self, img):
        self.img = img
        self.update()

    def paintEvent(self, _e):
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        p.setRenderHint(QPainter.SmoothPixmapTransform)
        path = QPainterPath()
        r = QRectF(0, 0, self.s, self.s)
        if self.round:
            path.addEllipse(r)
        else:
            path.addRoundedRect(r, 10, 10)
        p.setClipPath(path)
        if self.img is not None and not self.img.isNull():
            p.drawImage(r, self.img)
        else:
            p.fillRect(r, self.color)


class DiscoverPage(QWidget):
    def __init__(self, win):
        super().__init__(objectName="discoverpage")
        self.win = win
        self.engine = Discover(win)
        self.imgs = NetImages(win)
        self.imgs.ready.connect(self.image_ready)
        self.preview = PreviewPlayer(win)
        self.preview.changed.connect(self.refresh_play_buttons)
        self.pics = {}                 # url → [RoundPic]
        self.play_btns = []            # (button, url)
        self.data, self.busy, self.sel = None, False, None
        outer = QVBoxLayout(self)
        outer.setContentsMargins(0, 0, 0, 0)
        sa = QScrollArea()
        sa.setWidgetResizable(True)
        sa.setFrameShape(QFrame.NoFrame)
        sa.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.scroll = sa
        outer.addWidget(sa)
        body = QWidget()
        sa.setWidget(body)
        v = QVBoxLayout(body)
        v.setContentsMargins(32, 28, 28, 28)
        v.setSpacing(10)
        head = QHBoxLayout()
        tv = QVBoxLayout()
        tv.addWidget(QLabel("FOR YOU", objectName="eyebrow"))
        tv.addWidget(QLabel("Discover", objectName="h1"))
        head.addLayout(tv, 1)
        self.status = QLabel(objectName="dim")
        head.addWidget(self.status, 0, Qt.AlignBottom)
        head.addSpacing(8)
        self.b_refresh = win.tbtn("refresh", lambda: self.refresh(force=True), "pill")
        head.addWidget(self.b_refresh, 0, Qt.AlignBottom)
        v.addLayout(head)
        note = QLabel("Based on the artists in your library and what you play most. Similar artists come from Deezer "
                      "(only artist names are sent). Hit ▶ for a 30-second preview, ⤓ to search Soulseek for it.",
                      objectName="time")
        note.setWordWrap(True)
        v.addWidget(note)
        self.sections = {}
        for key, title in (("seeds", "your top artists"), ("detail", ""), ("recs", "artists you might like"),
                           ("missing", "albums you don't have yet — from artists you do"),
                           ("rediscover", "rediscover in your library")):
            lab = QLabel(win.L(title).upper(), objectName="eyebrow") if title else None
            if lab:
                lab.setContentsMargins(0, 18, 0, 4)
                v.addWidget(lab)
            box = QWidget()
            if key == "detail":
                box.setObjectName("detailbox")
                box.setAttribute(Qt.WA_StyledBackground, True)
                QVBoxLayout(box)
                box.hide()
            else:
                FlowLayout(box, spacing=14 if key != "seeds" else 18)
            v.addWidget(box)
            self.sections[key] = (lab, box)
        v.addStretch()

    # ------------------------------------------------------------ data
    def showEvent(self, e):
        super().showEvent(e)
        if self.data is None:
            self.data = self.engine.load()
            self.render()
        stale = not self.data or _time.time() - self.data.get("time", 0) > 3 * 86400
        if stale and not self.busy and self.win.tracks:
            self.refresh()
        self.render_rediscover()

    def hideEvent(self, e):
        self.preview.stop()
        super().hideEvent(e)

    def refresh(self, force=False):
        if self.busy:
            return
        if not self.win.tracks:
            self.status.setText(self.win.L("add some music first"))
            return
        self.busy = True
        self.b_refresh.setEnabled(False)
        self.status.setText(self.win.L("finding music for you…"))
        box = {}

        def prog(text, i, n):
            box["p"] = (text, i, n)

        def tick():
            if "p" in box:
                self.status.setText(box.pop("p")[0])

        def done(res, err):
            self.busy = False
            self.b_refresh.setEnabled(True)
            if err:
                self.status.setText(self.win.L(f"couldn't refresh: {err}")[:120])
                return
            self.data = res
            self.render()
        self.win.bg(lambda: self.engine.run(prog), done, tick)

    # ------------------------------------------------------------ render helpers
    def clear(self, key):
        box = self.sections[key][1]
        lay = box.layout()
        while lay.count():
            it = lay.takeAt(0)
            w = it.widget()
            if w is not None:
                w.deleteLater()
            elif it.layout() is not None:
                QWidget().setLayout(it.layout())

    def pic(self, url, size, round_=True):
        rp = RoundPic(size, round_, self.win.c["select"])
        img = self.imgs.get(url)
        if img is not None:
            rp.set_image(img)
        else:
            self.pics.setdefault(url, []).append(rp)
        return rp

    def image_ready(self, url, img):
        for rp in self.pics.pop(url, []):
            try:
                rp.set_image(img)
            except RuntimeError:
                pass

    def play_btn(self, url, label, tid=None):
        key = str(tid or url)
        b = QPushButton("▶", objectName="chip")
        b.setCursor(Qt.PointingHandCursor)
        b.setToolTip("30-second preview")
        b.setEnabled(bool(url or tid))
        b.clicked.connect(lambda: self.start_preview(url, label, tid, key))
        self.play_btns.append((b, key))
        return b

    def start_preview(self, url, label, tid, key):
        if self.preview.playing(key):
            self.preview.stop()
            return
        if not tid:
            self.preview.toggle(url, label, key)
            return

        def fresh():                                 # preview links expire, so ask for a current one
            return (deezer_get(f"/track/{tid}") or {}).get("preview") or url
        self.win.bg(fresh, lambda res, err: self.preview.toggle(url if err else res, label, key))

    def refresh_play_buttons(self):
        alive = []
        for b, url in self.play_btns:
            try:
                b.setText("■" if self.preview.playing(url) else "▶")
                alive.append((b, url))
            except RuntimeError:
                pass
        self.play_btns = alive

    def find_btn(self, query, tip="search Soulseek"):
        b = QPushButton("⤓", objectName="chip")
        b.setCursor(Qt.PointingHandCursor)
        b.setToolTip(f"{tip}: {query}")
        b.clicked.connect(lambda: self.win.search_soulseek(query))
        return b

    def render(self):
        d = self.data
        for k in ("seeds", "recs", "missing"):
            self.clear(k)
        self.play_btns = []
        if not d:
            self.status.setText(self.win.L("hit refresh to get recommendations") if not self.busy else self.status.text())
            for k in ("seeds", "recs", "missing"):
                self.sections[k][0].hide()
            return
        for k in ("seeds", "recs", "missing"):
            self.sections[k][0].show()
        age = int((_time.time() - d.get("time", 0)) / 3600)
        self.status.setText(self.win.L(f"updated {age} h ago" if age else "just updated"))
        # top artists
        for s in d.get("seeds", [])[:12]:
            w = QWidget()
            l = QVBoxLayout(w)
            l.setContentsMargins(0, 0, 0, 0)
            l.setSpacing(4)
            l.addWidget(self.pic(s.get("picture"), 76), 0, Qt.AlignHCenter)
            n = QLabel(s["name"])
            n.setAlignment(Qt.AlignCenter)
            n.setFixedWidth(96)
            n.setText(n.fontMetrics().elidedText(s["name"], Qt.ElideRight, 94))
            l.addWidget(n)
            sub = QLabel((f"{s['plays']} plays" if s.get("plays") else f"{s['albums']} album" + ("s" if s["albums"] != 1 else "")),
                         objectName="time")
            sub.setAlignment(Qt.AlignCenter)
            l.addWidget(sub)
            w.setCursor(Qt.PointingHandCursor)
            w.mousePressEvent = lambda _e, a=s["name"]: self.win.go("browse", a)
            self.sections["seeds"][1].layout().addWidget(w)
        # recommended artists
        for r in d.get("recs", []):
            card = QWidget(objectName="reccard")
            card.setAttribute(Qt.WA_StyledBackground, True)
            card.setFixedWidth(190)
            l = QVBoxLayout(card)
            l.setContentsMargins(12, 12, 12, 10)
            l.setSpacing(4)
            l.addWidget(self.pic(r.get("picture"), 166, round_=False))
            nm = QLabel(objectName="npTitle")
            nm.setText(nm.fontMetrics().elidedText(r["name"], Qt.ElideRight, 166))
            l.addWidget(nm)
            bc = QLabel(objectName="time")
            bc.setText(bc.fontMetrics().elidedText("like " + ", ".join(r.get("because") or []), Qt.ElideRight, 166))
            bc.setToolTip("because you have " + ", ".join(r.get("because") or []))
            l.addWidget(bc)
            row = QHBoxLayout()
            row.setSpacing(6)
            top = (r.get("top") or [{}])[0]
            row.addWidget(self.play_btn(top.get("preview", ""), f"{r['name']} — {top.get('title', '')}", top.get("tid")))
            row.addWidget(self.find_btn(r["name"]))
            more = QPushButton(self.win.L("songs"), objectName="chip")
            more.setCursor(Qt.PointingHandCursor)
            more.clicked.connect(lambda _=False, rr=r: self.show_detail(rr))
            row.addWidget(more)
            row.addStretch()
            l.addLayout(row)
            self.sections["recs"][1].layout().addWidget(card)
        # missing albums
        for a in d.get("missing", []):
            card = QWidget(objectName="reccard")
            card.setAttribute(Qt.WA_StyledBackground, True)
            card.setFixedWidth(170)
            l = QVBoxLayout(card)
            l.setContentsMargins(10, 10, 10, 8)
            l.setSpacing(3)
            l.addWidget(self.pic(a.get("cover"), 150, round_=False))
            t = QLabel(objectName="npTitle")
            t.setText(t.fontMetrics().elidedText(a["title"], Qt.ElideRight, 150))
            t.setToolTip(a["title"])
            l.addWidget(t)
            sub = QLabel(objectName="time")
            sub.setText(sub.fontMetrics().elidedText(
                " · ".join(x for x in (a["artist"], a["year"], a["type"] if a["type"] != "album" else "") if x),
                Qt.ElideRight, 150))
            l.addWidget(sub)
            row = QHBoxLayout()
            row.addWidget(self.find_btn(f"{a['artist']} {a['title']}", "find this album on Soulseek"))
            row.addStretch()
            l.addLayout(row)
            self.sections["missing"][1].layout().addWidget(card)
        if not d.get("missing"):
            self.sections["missing"][0].hide()
        if self.sel:
            r = next((x for x in d.get("recs", []) if x["id"] == self.sel), None)
            self.show_detail(r, toggle=False) if r else self.hide_detail()

    def hide_detail(self):
        self.sel = None
        self.clear("detail")
        self.sections["detail"][1].hide()

    def show_detail(self, r, toggle=True):
        box = self.sections["detail"][1]
        if r is None or (toggle and self.sel == r["id"] and box.isVisible()):
            self.hide_detail()
            return
        self.clear("detail")
        lay = box.layout()
        self.sel = r["id"]
        lay.setContentsMargins(18, 16, 18, 16)
        lay.setSpacing(6)
        top = QHBoxLayout()
        top.setSpacing(16)
        top.addWidget(self.pic(r.get("picture"), 92))
        tv = QVBoxLayout()
        tv.addStretch()
        nm = QLabel(r["name"], objectName="h2")
        nm.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
        tv.addWidget(nm)
        fans = r.get("fans") or 0
        sub = QLabel(f"{fans:,} fans on Deezer  ·  because you have {', '.join(r.get('because') or [])}",
                     objectName="dim")
        sub.setWordWrap(True)
        sub.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
        tv.addWidget(sub)
        tv.addStretch()
        top.addLayout(tv, 1)
        top.addWidget(self.win.tbtn("search Soulseek", lambda: self.win.search_soulseek(r["name"]), "pillPrimary"),
                      0, Qt.AlignVCenter)
        top.addWidget(self.win.tbtn("close", self.hide_detail, "pill"), 0, Qt.AlignVCenter)
        lay.addLayout(top)
        for i, t in enumerate(r.get("top") or []):
            row = QHBoxLayout()
            row.setSpacing(10)
            num = QLabel(f"{i + 1:02}", objectName="nextnum")
            num.setFixedWidth(26)
            row.addWidget(num)
            row.addWidget(self.pic(t.get("cover"), 36, round_=False))
            tl = QLabel(f"{t['title']}")
            tl.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
            row.addWidget(tl, 2)
            al = QLabel(t.get("album", ""), objectName="dim")
            al.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
            row.addWidget(al, 2)
            row.addWidget(QLabel(fmt_time(t.get("duration") or 0), objectName="time"))
            row.addWidget(self.play_btn(t.get("preview", ""), f"{r['name']} — {t['title']}", t.get("tid")))
            row.addWidget(self.find_btn(f"{r['name']} {t.get('album') or t['title']}"))
            lay.addLayout(row)
        box.show()
        self.refresh_play_buttons()
        QTimer.singleShot(0, lambda: self.scroll.ensureWidgetVisible(box, 0, 40))

    def render_rediscover(self):
        """local: albums you haven't played in a while (or ever), a different handful each visit."""
        self.clear("rediscover")
        stats, now = self.win.stats, _time.time()
        cands = []
        for a in self.win.albums.values():
            last = max((stats.get(t["path"], {}).get("last", 0) for t in a["tracks"]), default=0)
            plays = sum(stats.get(t["path"], {}).get("plays", 0) for t in a["tracks"])
            if now - last > 30 * 86400:
                cands.append((plays, a))
        random.shuffle(cands)
        cands.sort(key=lambda x: -min(x[0], 5))          # favourites you've drifted from first, then the rest
        lab, box = self.sections["rediscover"]
        lab.setVisible(bool(cands))
        for plays, a in cands[:8]:
            card = QWidget(objectName="reccard")
            card.setAttribute(Qt.WA_StyledBackground, True)
            card.setFixedWidth(150)
            card.setCursor(Qt.PointingHandCursor)
            l = QVBoxLayout(card)
            l.setContentsMargins(8, 8, 8, 8)
            l.setSpacing(3)
            c = QLabel()
            c.setPixmap(self.win.covers.get(a["cover"], 134))
            l.addWidget(c)
            t = QLabel(objectName="npTitle")
            t.setText(t.fontMetrics().elidedText(a["album"], Qt.ElideRight, 134))
            l.addWidget(t)
            s = QLabel(objectName="time")
            s.setText(s.fontMetrics().elidedText(f"{a['artist']} · " + (f"{plays} plays" if plays else "never played"),
                                                 Qt.ElideRight, 134))
            l.addWidget(s)
            card.mousePressEvent = lambda e, k=a["key"], tr=a["tracks"]: (
                self.win.play_list(tr, 0) if e.button() == Qt.LeftButton and e.position().y() < 142
                else self.win.go("album", k))
            card.setToolTip("click the cover to play · click the name to open")
            box.layout().addWidget(card)


# ------------------------------------------------------------------ settings
class Settings(QWidget):
    def __init__(self, win):
        super().__init__(objectName="settingspage")
        self.win, self.ready = win, False
        cfg = win.cfg
        outer = QVBoxLayout(self)
        outer.setContentsMargins(32, 28, 28, 16)
        outer.setSpacing(16)
        head = QHBoxLayout()
        head.addWidget(QLabel("Settings", objectName="h1"))
        head.addStretch()
        head.addWidget(win.tbtn("done", win.back, "pillPrimary"))
        outer.addLayout(head)
        tabs = SideTabs(win)
        self.tabs = tabs
        outer.addWidget(tabs, 1)

        # ---- look
        look = QWidget()
        f = QFormLayout(look)
        f.setLabelAlignment(Qt.AlignRight)
        sk = active_skin(win)
        skrow = QHBoxLayout()
        skl = QLabel((sk["name"] + (f"  ·  by {sk['author']}" if sk["author"] else "")) if sk else
                     (cfg.get("preset") or "custom").capitalize() + " (built in)")
        skrow.addWidget(skl)
        skrow.addWidget(win.tbtn("change skin", lambda: self.tabs.setCurrentIndex(self.tabs.names.index("skins")),
                                 "pill"))
        skrow.addStretch()
        f.addRow("skin", skrow)
        lay = QComboBox()
        lay.addItems(["studio", "classic"])
        lay.setCurrentText(cfg.get("layout", "studio"))
        lay.currentTextChanged.connect(lambda v: self.set("layout", v, True))
        f.addRow("layout", lay)
        f.addRow("now playing panel", self.check("now_panel", rebuild=False))
        f.addRow("spinning record", self.check("dial_spin", rebuild=False))
        f.addRow("record spin (sec/turn)", self.spin("dial_spin_seconds", 2, 60, rebuild=False))
        f.addRow("record size", self.spin("dial_size", 200, 420, step=10, rebuild=False))
        self.theme = QComboBox()
        self.theme.addItems(list(THEMES))
        self.theme.setCurrentText(cfg["theme"])
        self.theme.currentTextChanged.connect(lambda v: self.set("theme", v, False))
        f.addRow("theme", self.theme)

        grid = QGridLayout()
        self.color_btns = {}
        for i, k in enumerate(COLOR_KEYS):
            b = QPushButton()
            b.setFixedHeight(26)
            b.clicked.connect(lambda _=False, key=k: self.pick_color(key))
            self.color_btns[k] = b
            grid.addWidget(QLabel(k), i // 4 * 2, i % 4)
            grid.addWidget(b, i // 4 * 2 + 1, i % 4)
        reset = QPushButton("reset colors to theme")
        reset.clicked.connect(self.reset_colors)
        cw = QWidget()
        cv = QVBoxLayout(cw)
        cv.setContentsMargins(0, 0, 0, 0)
        cv.addLayout(grid)
        cv.addWidget(reset, 0, Qt.AlignLeft)
        f.addRow("colors", cw)

        frow = QHBoxLayout()
        self.font = QFontComboBox()
        self.mono_only = ToggleSwitch(win, "mono only")
        self.mono_only.toggled.connect(lambda on: self.font.setFontFilters(
            QFontComboBox.MonospacedFonts if on else QFontComboBox.AllFonts))
        self.font.setCurrentFont(QApplication.font())
        self.font.currentFontChanged.connect(lambda fo: self.set("font_family", fo.family(), False))
        frow.addWidget(self.font, 1)
        frow.addWidget(self.mono_only)
        f.addRow("font", frow)
        self.dfont = QFontComboBox()
        self.dfont.setCurrentFont(QFont(win.display_family or "Doto"))
        self.dfont.currentFontChanged.connect(lambda fo: self.set("display_font", fo.family(), False))
        f.addRow("heading font", self.dfont)
        f.addRow("font size", self.spin("font_size", 7, 32, rebuild=False))
        hc = QComboBox()
        hc.addItems(["upper", "title", "lower"])
        hc.setCurrentText(cfg.get("header_case", "upper"))
        hc.currentTextChanged.connect(lambda v: self.set("header_case", v, True))
        f.addRow("column header case", hc)
        uc = QComboBox()
        uc.addItems(["sentence", "lower"])
        uc.setCurrentText(cfg.get("ui_case", "sentence"))
        uc.currentTextChanged.connect(lambda v: self.set("ui_case", v, True))
        f.addRow("ui text case", uc)
        f.addRow("pill buttons", self.check("pill_buttons", rebuild=False))
        f.addRow("window opacity % (restart)", self.spin("background_alpha", 20, 100, step=5, rebuild=False))
        f.addRow("uppercase ui", self.check("uppercase", rebuild=True))
        self.brackets = QComboBox()
        self.brackets.setEditable(True)
        self.brackets.addItems(["[]", "()", "<>", "{}", "||", "«»", "none"])
        self.brackets.setCurrentText(cfg["brackets"] or "none")
        self.brackets.currentTextChanged.connect(
            lambda v: self.set("brackets", "" if v == "none" else v, False))
        f.addRow("button brackets", self.brackets)
        f.addRow("bracket padding", self.check("bracket_padding", rebuild=False))
        f.addRow("nav marker", self.line("nav_marker"))
        f.addRow("header text", self.line("header_text"))
        f.addRow("corner radius", self.spin("corner_radius", 0, 40, rebuild=True))
        f.addRow("album cover size", self.spin("cover_size", 64, 400, step=8, rebuild=True))
        f.addRow("list thumb size (0=off)", self.spin("list_cover_size", 0, 96, step=2, rebuild=True))
        f.addRow("no-art placeholder", self.line("placeholder", rebuild=True))
        f.addRow("sidebar width", self.spin("sidebar_width", 120, 500, step=10, rebuild=False))
        f.addRow(self.section("background & visualizer"))
        f.addRow("album-art glow behind the app", self.check("ambient_background", rebuild=False))
        f.addRow("glow strength %", self.spin("ambient_strength", 10, 90, step=5, rebuild=False))
        f.addRow("record ring dances to the music", self.check("dial_visualizer", rebuild=False))
        vst = QComboBox()
        vst.addItems(list(Visualizer.STYLES))
        vst.setCurrentText(cfg.get("vis_style", "dots"))
        vst.currentTextChanged.connect(lambda v: self.set("vis_style", v, False))
        f.addRow("visualizer style", vst)
        vcol = QComboBox()
        vcol.addItem("mono (theme colour)", "mono")
        vcol.addItem("colour from the album cover", "cover")
        vcol.setCurrentIndex(max(0, vcol.findData(cfg.get("vis_color", "mono"))))
        vcol.currentIndexChanged.connect(lambda _i: self.set("vis_color", vcol.currentData(), False))
        f.addRow("visualizer colour", vcol)
        f.addRow("visualizer sensitivity %", self.spin("vis_sensitivity", 40, 250, step=10, rebuild=False))
        tabs.addTab(self.scroll(look), "look")
        self.skins_tab = SkinsTab(self)
        tabs.addTab(self.scroll(self.skins_tab), "skins")

        # ---- player
        pl = QWidget()
        f2 = QFormLayout(pl)
        f2.setLabelAlignment(Qt.AlignRight)
        ps = QComboBox()
        for key, label in (("progress_style", "progress bar style"), ("volume_style", "volume bar style")):
            cb = QComboBox()
            cb.addItems(["dots", "line", "text"])
            cb.setCurrentText(cfg[key])
            cb.currentTextChanged.connect(lambda v, k=key: self.set(k, v, False))
            f2.addRow(label, cb)
        cs = QComboBox()
        cs.addItems(["icons", "text"])
        cs.setCurrentText(cfg["control_style"])
        cs.currentTextChanged.connect(lambda v: self.set("control_style", v, False))
        f2.addRow("player controls", cs)
        f2.addRow("progress chars (text)", self.line("progress_chars"))
        f2.addRow(QLabel("  filled, empty, optional head — e.g.  ━─   █░   =-   ■□   ━─●"))
        for g in ("play", "pause", "prev", "next", "shuffle", "repeat", "repeat_one", "now"):
            le = QLineEdit(cfg["glyphs"].get(g, ""))
            le.editingFinished.connect(lambda key=g, e=le: self.set_glyph(key, e.text()))
            f2.addRow(f"text glyph: {g}", le)
        f2.addRow("show remaining time", self.check("show_remaining", rebuild=False))
        f2.addRow("remember queue on exit", self.check("remember_session", rebuild=False))
        npf = QHBoxLayout()
        self.np_file = QLineEdit(cfg["now_playing_file"])
        self.np_file.setPlaceholderText("off — e.g. ~/nowplaying.txt (for OBS text sources)")
        self.np_file.editingFinished.connect(lambda: self.set("now_playing_file", self.np_file.text().strip(), False))
        browse = QPushButton("...")
        browse.clicked.connect(self.pick_np_file)
        npf.addWidget(self.np_file, 1)
        npf.addWidget(browse)
        f2.addRow("now playing file", npf)
        f2.addRow("now playing format", self.line("now_playing_format"))
        f2.addRow(QLabel("  keys: {title} {artist} {album} {year} {quality} {genre}"))
        tabs.addTab(self.scroll(pl), "player")

        # ---- library
        lib = QWidget()
        lv = QVBoxLayout(lib)
        lv.addWidget(QLabel("music folders"))
        self.folders = QListWidget()
        self.folders.addItems(cfg["library_folders"])
        lv.addWidget(self.folders, 1)
        fb = QHBoxLayout()
        for text, fn in (("add folder", self.add_folder), ("remove", self.remove_folder),
                         ("rescan", lambda: win.rescan()), ("full rescan", lambda: win.rescan(full=True))):
            b = QPushButton(text)
            b.clicked.connect(fn)
            fb.addWidget(b)
        fb.addStretch()
        lv.addLayout(fb)
        f3 = QFormLayout()
        f3.addRow("scan on startup", self.check("scan_on_startup", rebuild=False))
        f3.addRow("single click plays songs", self.check("single_click_play", rebuild=False))
        sort = QComboBox()
        sort.addItems(["artist", "title", "year", "recent"])
        sort.setCurrentText(cfg["album_sort"])
        sort.currentTextChanged.connect(lambda v: self.set("album_sort", v, True))
        f3.addRow("album sort", sort)
        sv = QComboBox()
        sv.addItems(["albums", "browse", "artists", "songs", "queue"])
        sv.setCurrentText(cfg["start_view"])
        sv.currentTextChanged.connect(lambda v: self.set("start_view", v, False))
        f3.addRow("start view", sv)
        colrow = QHBoxLayout()
        self.col_checks = {}
        for c in COLS:
            cb = ToggleSwitch(win, c)
            cb.setChecked(c in cfg["song_columns"])
            cb.toggled.connect(self.set_columns)
            self.col_checks[c] = cb
            colrow.addWidget(cb)
        colrow.addStretch()
        f3.addRow("song columns", colrow)
        lv.addLayout(f3)
        tabs.addTab(lib, "library")

        # ---- sound
        snd = QWidget()
        fs_ = QFormLayout(snd)
        fs_.setLabelAlignment(Qt.AlignRight)
        eng_on = isinstance(win.player, NexusEngine)
        status = QLabel(win.player.out_info if eng_on and win.player.out_info else
                        ("Nexus engine ready — starts with the first song" if eng_on else
                         (win.engine_note or "basic Qt player")), objectName="dim")
        status.setWordWrap(True)
        fs_.addRow("status", status)
        ec = QComboBox()
        ec.addItems(["nexus", "qt"])
        ec.setCurrentText(cfg.get("audio_engine", "nexus"))
        ec.currentTextChanged.connect(lambda v: (self.set("audio_engine", v, False),
                                                 status.setText("restart Nexus to switch engine")))
        fs_.addRow("audio engine (restart)", ec)
        fs_.addRow(QLabel("  nexus = EQ, volume boost, gapless, ReplayGain, exclusive mode.  qt = basic fallback."))
        dev = QComboBox()
        dev.addItem("system default", "")
        for name in NexusEngine.output_devices():
            dev.addItem(name, name)
        i = dev.findData(cfg.get("audio_device", ""))
        dev.setCurrentIndex(max(0, i))
        dev.currentIndexChanged.connect(lambda _i: self.set_audio("audio_device", dev.currentData()))
        fs_.addRow("output device", dev)
        if sys.platform.startswith("win"):
            ex = ToggleSwitch(win)
            ex.setChecked(bool(cfg.get("exclusive_mode")))
            ex.toggled.connect(lambda on: self.set_audio("exclusive_mode", on))
            fs_.addRow("exclusive mode (bit-perfect)", ex)
            fs_.addRow(QLabel("  takes over the device: no windows mixer / resampling. other apps go silent while it plays."))
        vm = QComboBox()
        for pct in (100, 150, 200, 300, 400):
            vm.addItem(f"{pct}%", pct)
        vm.setCurrentIndex(max(0, vm.findData(int(cfg.get("volume_max", 200)))))
        vm.currentIndexChanged.connect(lambda _i: (self.set("volume_max", vm.currentData(), False),
                                                   win.set_volume(win.cfg["volume"])))
        fs_.addRow("max volume", vm)
        rg = QComboBox()
        rg.addItems(["off", "track", "album"])
        rg.setCurrentText(cfg.get("replaygain", "off"))
        rg.currentTextChanged.connect(lambda v: self.set("replaygain", v, False))
        fs_.addRow("ReplayGain (even loudness)", rg)
        fs_.addRow("ReplayGain preamp dB", self.spin("replaygain_preamp", -12, 12, rebuild=False))
        fs_.addRow("gapless playback", self.check("gapless", rebuild=False))
        eqb = QPushButton("open equalizer  (ctrl+e)")
        eqb.clicked.connect(win.open_eq)
        fs_.addRow("equalizer", eqb)
        tabs.addTab(self.scroll(snd), "sound")

        # ---- downloads
        dlw = QWidget()
        fd = QFormLayout(dlw)
        fd.setLabelAlignment(Qt.AlignRight)
        fd.addRow(QLabel("SOULSEEK", objectName="eyebrow"))
        sx = QLabel("Any username + password works — the first login creates the account. Keep the password: it's how "
                    "Soulseek knows the name is yours.", objectName="dim")
        sx.setWordWrap(True)
        fd.addRow(sx)
        fd.addRow("username", self.line("slsk_user"))
        spw = self.line("slsk_password")
        spw.setEchoMode(QLineEdit.Password)
        fd.addRow("password", spw)
        shares = list(cfg["library_folders"]) if cfg.get("slsk_share_library", True) else []
        shares += [x for x in (cfg.get("slsk_share_extra") or "").split(";") if x.strip()]
        fd.addRow("share my music folders", self.check("slsk_share_library", rebuild=False))
        ex = self.line("slsk_share_extra")
        ex.setPlaceholderText("more folders to share, separated by ;")
        fd.addRow("extra shared folders", ex)
        sh = QLabel(("sharing: " + ", ".join(shares)) if shares else
                    "sharing nothing — many peers refuse to upload to people who don't share", objectName="time")
        sh.setWordWrap(True)
        fd.addRow("", sh)
        fd.addRow("upload slots", self.spin("slsk_upload_slots", 1, 20, rebuild=False))
        fd.addRow("listening port", self.spin("slsk_port", 1024, 65000, rebuild=False))
        fd.addRow("open port with UPnP", self.check("slsk_upnp", rebuild=False))
        trow = QHBoxLayout()
        tb = QPushButton("test connection")
        self.test_result = QLabel(objectName="dim")
        self.test_result.setWordWrap(True)
        tb.clicked.connect(self.test_soulseek)
        trow.addWidget(tb)
        trow.addWidget(self.test_result, 1)
        fd.addRow("", trow)
        hrow = QHBoxLayout()
        if sys.platform.startswith("win"):
            fw = QPushButton("allow through firewall")
            fw.setToolTip("adds a Windows Firewall rule for Python so other Soulseek users can reach you (asks for admin)")
            fw.clicked.connect(self.allow_firewall)
            hrow.addWidget(fw)
        lg = QPushButton("open soulseek log")
        lg.clicked.connect(lambda: QDesktopServices.openUrl(QUrl.fromLocalFile(str(win.soulseek.log_path())))
                           if win.soulseek.log_path().exists() else self.test_result.setText("no log yet — connect first"))
        hrow.addWidget(lg)
        hrow.addStretch()
        fd.addRow("", hrow)
        fd.addRow(QLabel(""))
        fd.addRow(QLabel(""))
        fd.addRow(QLabel("IMPORTING", objectName="eyebrow"))
        fd.addRow("import into", self.folder_row("import_root", "default: your first music folder"))
        fd.addRow("move (not copy) on import", self.check("import_move", rebuild=False))
        fd.addRow("search time (sec)", self.spin("dl_search_seconds", 5, 60, rebuild=False))
        fd.addRow("switch to another user if a download\nhasn't started", self.check("dl_auto_switch", rebuild=False))
        fd.addRow("…after (minutes)", self.spin("dl_stall_minutes", 1, 60, rebuild=False))
        fd.addRow(QLabel(""))
        fd.addRow(QLabel("WATCH FOLDER", objectName="eyebrow"))
        wexp = QLabel("New music files or zips that land here (e.g. your Downloads folder) get sorted into "
                      "Artist/Album in your library automatically. Files already there are left alone.", objectName="dim")
        wexp.setWordWrap(True)
        fd.addRow(wexp)
        fd.addRow("folder", self.folder_row("watch_folder", str(Path.home() / "Downloads")))
        fd.addRow("watch it", self.check("watch_enabled", rebuild=False))
        tabs.addTab(self.scroll(dlw), "downloads")

        # ---- discord
        dw = QWidget()
        fdc = QFormLayout(dw)
        fdc.setLabelAlignment(Qt.AlignRight)
        how = QLabel(
            "Show friends what you're playing (“Listening to Nexus”). One-time setup:\n"
            "1. go to discord.com/developers/applications → New Application → name it Nexus (that name is what friends see)\n"
            "2. copy its Application ID and paste it below\n"
            "3. optional: Rich Presence → Art Assets → upload icons/nexus-1024.png, and put its asset name in "
            "“fallback image asset” below\n"
            "Not showing on your profile while it says “discord accepted it”? Discord → User Settings → "
            "Activity Privacy → turn on “Share my activity”.\n"
            "Needs the Discord desktop app running on this PC. Nothing is sent anywhere except to Discord — and, if the "
            "cover is on: either the album cover image to catbox.moe (so Discord has a link to show; once per album), or "
            "the artist + album to iTunes/Deezer to find a matching cover.", objectName="dim")
        how.setWordWrap(True)
        fdc.addRow(how)
        fdc.addRow("show on discord", self.check("discord_enabled", rebuild=False))
        cid = self.line("discord_client_id")
        cid.setPlaceholderText("application id, e.g. 1234567890123456789")
        fdc.addRow("application id", cid)
        fdc.addRow("song title in status", self.check("discord_song_in_status", rebuild=False))
        fdc.addRow("hide when paused", self.check("discord_hide_paused", rebuild=False))
        fdc.addRow("show album cover", self.check("discord_cover_lookup", rebuild=False))
        csrc = QComboBox()
        srcs = [("upload", "your own cover (exact, uploaded once to catbox.moe)"),
                ("online", "match it online (itunes / deezer, nothing uploaded)")]
        for k, label in srcs:
            csrc.addItem(label, k)
        csrc.setCurrentIndex(max(0, [k for k, _ in srcs].index(win.cfg.get("discord_cover_source", "upload"))
                                 if win.cfg.get("discord_cover_source") in dict(srcs) else 0))
        csrc.currentIndexChanged.connect(lambda i: (self.set("discord_cover_source", csrc.itemData(i), False),
                                                    win.push_presence()))
        fdc.addRow("cover from", csrc)
        fdc.addRow("fallback image asset", self.line("discord_large_image"))
        drow = QHBoxLayout()
        dt = QPushButton("check connection")
        self.discord_status = QLabel(win.discord.status, objectName="dim")
        self.discord_status.setWordWrap(True)

        def refresh_status():
            win.push_presence()
            for ms in (1500, 4000, 8000):
                QTimer.singleShot(ms, lambda: self.discord_status.setText(win.discord.status))
        dt.clicked.connect(refresh_status)
        drow.addWidget(dt)
        drow.addWidget(self.discord_status, 1)
        fdc.addRow("", drow)
        tabs.addTab(self.scroll(dw), "discord")

        # ---- update
        up = QWidget()
        uv = QVBoxLayout(up)
        uv.addWidget(QLabel(f"{APP_NAME} {VERSION}", objectName="h2"))
        info = QLabel("Got a new nexus.zip? Install it here, or just drag the zip onto the Nexus window. "
                      "It swaps the app files in place, keeps your library, playlists and settings, and restarts.",
                      objectName="dim")
        info.setWordWrap(True)
        uv.addWidget(info)
        ub = QPushButton("install update from zip…")
        ub.clicked.connect(lambda: win.pick_update())
        uv.addWidget(ub, 0, Qt.AlignLeft)
        uv.addSpacing(16)
        uf = QFormLayout()
        uf.addRow("check for updates on launch", self.check("update_check", rebuild=False))
        repo_ed = self.line("update_repo")
        repo_ed.setPlaceholderText(GITHUB_REPO or "owner/repo on GitHub")
        uf.addRow("GitHub releases from", repo_ed)
        feed = self.line("update_url")
        feed.setPlaceholderText('or a version.json url: {"version": "1.3.0", "zip": "https://…/nexus.zip"}')
        uf.addRow("custom feed (optional)", feed)
        chk = QPushButton("check now")
        chk.clicked.connect(lambda: win.check_updates(manual=True))
        uf.addRow("", chk)
        uv.addLayout(uf)
        uv.addStretch()
        tabs.addTab(up, "update")

        # ---- advanced
        adv = QWidget()
        av = QVBoxLayout(adv)
        av.addWidget(QLabel("custom Qt stylesheet (QSS) — appended after the theme. object names:\n"
                            "#sidebar #topbar #npbar #nav #tb #h1 #dim #brand #npTitle"))
        self.qss = QPlainTextEdit(cfg["custom_qss"])
        self.qss.setPlaceholderText("QPushButton#tb:hover { text-decoration: underline; }\n"
                                    "QTableView::item:selected { background: #ff0066; }")
        av.addWidget(self.qss, 1)
        ab = QHBoxLayout()
        apply_b = QPushButton("apply qss")
        apply_b.clicked.connect(lambda: self.set("custom_qss", self.qss.toPlainText(), False))
        open_b = QPushButton("open config folder")
        open_b.clicked.connect(lambda: QDesktopServices.openUrl(QUrl.fromLocalFile(str(win.dir))))
        ab.addWidget(apply_b)
        ab.addWidget(open_b)
        ab.addStretch()
        av.addLayout(ab)
        av.addWidget(QLabel(f"config: {cfg.path}"))
        tabs.addTab(adv, "advanced")

        self.tidy()
        self.paint_swatches()
        self.ready = True

    # -- builders
    def section(self, text):
        lab = QLabel(self.win.L(text).upper(), objectName="eyebrow")
        lab.setContentsMargins(0, 18, 0, 2)
        return lab

    def tidy(self):
        """consistent rows: labels in one left column, fields at a sensible width instead of stretched."""
        for form in self.findChildren(QFormLayout):
            form.setLabelAlignment(Qt.AlignLeft | Qt.AlignVCenter)
            form.setFormAlignment(Qt.AlignLeft | Qt.AlignTop)
            form.setHorizontalSpacing(28)
            form.setVerticalSpacing(12)
            form.setRowWrapPolicy(QFormLayout.DontWrapRows)
            for r in range(form.rowCount()):
                it = form.itemAt(r, QFormLayout.LabelRole)
                if it and it.widget():
                    it.widget().setMinimumWidth(200)
                    it.widget().setObjectName("formlabel")
        for form in self.findChildren(QFormLayout):
            for r in range(form.rowCount()):
                it = form.itemAt(r, QFormLayout.FieldRole)
                if it and isinstance(it.widget(), QPushButton):
                    it.widget().setSizePolicy(QSizePolicy.Maximum, QSizePolicy.Fixed)
        for w in self.findChildren(QWidget):
            if isinstance(w, QSpinBox):
                w.setMaximumWidth(130)
            elif isinstance(w, (QComboBox, QFontComboBox)) and w.maximumWidth() > 440:
                w.setMaximumWidth(440)
            elif isinstance(w, QLineEdit) and not isinstance(w.parent(), (QComboBox, QSpinBox)) \
                    and w.maximumWidth() > 520:
                w.setMaximumWidth(520)

    def scroll(self, w):
        sa = QScrollArea()
        sa.setWidgetResizable(True)
        sa.setFrameShape(QScrollArea.NoFrame)
        sa.setWidget(w)
        return sa

    def spin(self, key, lo, hi, step=1, rebuild=True):
        s = QSpinBox()
        s.setButtonSymbols(QSpinBox.NoButtons)
        s.setRange(lo, hi)
        s.setSingleStep(step)
        s.setValue(int(self.win.cfg[key]))
        s.valueChanged.connect(lambda v: self.set(key, v, rebuild))
        return s

    def check(self, key, rebuild):
        c = ToggleSwitch(self.win)
        c.setChecked(bool(self.win.cfg[key]))
        c.toggled.connect(lambda v: self.set(key, v, rebuild))
        return c

    def line(self, key, rebuild=False):
        le = QLineEdit(str(self.win.cfg[key]))
        le.editingFinished.connect(lambda: self.set(key, le.text(), rebuild))
        return le

    # -- setters
    def set(self, key, val, rebuild):
        if not self.ready:
            return
        if self.win.cfg.get(key) == val:
            return
        self.win.cfg[key] = val
        self.win.cfg.save()
        if key == "layout":
            self.ready = False
            QTimer.singleShot(0, self.win.rebuild_ui)      # this page gets rebuilt too
            return
        self.win.apply_settings(rebuild=rebuild)
        if key in ("watch_folder", "watch_enabled"):
            self.win.watch.apply()
        if key.startswith("skin_bg"):
            self.win.root.load_skin_bg()
            self.win.root.update()
        if key.startswith("ambient_"):
            r, w = self.win.root, self.win
            r.key, r.pm, r.old = "", None, None
            cur = w.queue[w.qi] if 0 <= w.qi < len(w.queue) else None
            r.set_cover(cur["cover"] if cur else "")
            r.update()
        if key == "dial_visualizer":
            d = self.win.now_panel.dial
            d.hideEvent_vis()
            if val and d.isVisible() and d.dancing():
                self.win.analyzer.listen(d)
            d.relayout()
        if key.startswith("discord_"):
            if key == "discord_client_id":
                self.win.discord.reset()
            if key == "discord_enabled" and not val:
                self.win.discord.clear()
            self.win.push_presence()
        if key.startswith("slsk_"):
            if key != "slsk_download_dir":
                self.win.soulseek.restart()
            self.win.download_page.refresh_conn()
        self.paint_swatches()

    def apply_preset(self, name):
        cfg = self.win.cfg
        for k, v in PRESETS.get(name, {}).items():
            if isinstance(v, dict) and isinstance(cfg.get(k), dict):
                cfg[k].update(v)
            else:
                cfg[k] = v
        cfg["preset"] = name
        cfg["color_overrides"] = {}
        cfg["skin"] = ""
        self.win.__dict__.pop("_skin", None)
        cfg.save()
        self.ready = False
        QTimer.singleShot(0, self.win.rebuild_ui)          # rebuilds this page with the new values

    def folder_row(self, key, placeholder=""):
        row = QHBoxLayout()
        le = self.line(key)
        le.setPlaceholderText(placeholder)
        b = QPushButton("…")
        b.setFixedWidth(44)

        def pick():
            d = QFileDialog.getExistingDirectory(self, key.replace("_", " "), le.text() or str(Path.home()))
            if d:
                le.setText(d)
                self.set(key, d, False)
        b.clicked.connect(pick)
        row.addWidget(le, 1)
        row.addWidget(b)
        return row

    def allow_firewall(self):
        """inbound allow rule for python on the soulseek ports only, via an elevated netsh. windows asks first."""
        if firewall_fix(int(self.win.cfg.get("slsk_port", 2234))):
            self.test_result.setText("firewall rules added (if you clicked yes) — restart Nexus, then search again")
            self.win.soulseek.restart()
        else:
            self.test_result.setText("firewall change cancelled")

    def test_soulseek(self):
        self.test_result.setText("connecting…")
        c = self.win.soulseek

        def done(res, err):
            if err:
                self.test_result.setText(str(err))
            else:
                self.test_result.setText(f"ok — {(res or {}).get('state') or 'online'}")
            self.win.download_page.refresh_conn()
        self.win.bg(c.status, done)

    def set_audio(self, key, val):
        if not self.ready or self.win.cfg.get(key) == val:
            return
        self.win.cfg[key] = val
        self.win.cfg.save()
        if isinstance(self.win.player, NexusEngine):
            self.win.player.restart_output()

    def set_glyph(self, key, val):
        self.win.cfg["glyphs"][key] = val
        self.win.cfg.save()
        self.win.apply_settings(rebuild=False)

    def set_columns(self):
        cols = [c for c, cb in self.col_checks.items() if cb.isChecked()]
        self.set("song_columns", cols, True)

    def pick_color(self, key):
        cur = QColor(self.win.c[key])
        col = QColorDialog.getColor(cur, self, f"color: {key}")
        if col.isValid():
            ov = dict(self.win.cfg.get("color_overrides", {}))
            ov[key] = col.name()
            self.set("color_overrides", ov, False)

    def reset_colors(self):
        self.set("color_overrides", {}, False)

    def paint_swatches(self):
        for k, b in self.color_btns.items():
            col = self.win.c[k]
            fg = "#000" if QColor(col).lightness() > 128 else "#fff"
            b.setText(col)
            b.setStyleSheet(f"background:{col}; color:{fg}; border:1px solid {self.win.c['border']};"
                            f"text-align:center;")

    def pick_np_file(self):
        p, _ = QFileDialog.getSaveFileName(self, "now playing file", str(Path.home() / "nowplaying.txt"))
        if p:
            self.np_file.setText(p)
            self.set("now_playing_file", p, False)

    def add_folder(self):
        d = QFileDialog.getExistingDirectory(self, "music folder", str(Path.home()))
        if d and d not in self.win.cfg["library_folders"]:
            self.win.cfg["library_folders"].append(d)
            self.win.cfg.save()
            self.folders.addItem(d)
            self.win.rescan()

    def remove_folder(self):
        it = self.folders.currentItem()
        if it:
            self.win.cfg["library_folders"].remove(it.text())
            self.win.cfg.save()
            self.folders.takeItem(self.folders.row(it))
            self.win.rescan()


# ------------------------------------------------------------------ main window
def build_qss(c, cfg, family, display, skin_css="", skin_bg=False):
    r = int(cfg["corner_radius"])
    fs = float(cfg["font_size"])
    sr = min(r, 5)
    alpha = max(0, min(100, int(cfg.get("background_alpha", 100)))) / 100

    def rgba(hexc, a=alpha):
        q = QColor(hexc)
        return f"rgba({q.red()},{q.green()},{q.blue()},{a:.2f})"

    bg, panel = rgba(c["bg"]), rgba(c["panel"])
    if cfg.get("ambient_background"):
        glow = max(0, min(100, int(cfg.get("ambient_strength", 45)))) / 100
        panel = rgba(c["panel"], min(alpha, 1 - glow * 0.5))
    if skin_bg:
        panel = rgba(c["panel"], min(alpha, 0.72))
    pills = cfg.get("pill_buttons", True)
    if pills:
        pill = f"""
QPushButton#pill, QPushButton#pillPrimary {{ border-radius: 16px; padding: 7px 18px; font-weight: 600; }}
QPushButton#pill {{ border: 1px solid {c['border']}; background: {c['panel']}; color: {c['text']}; }}
QPushButton#rowpill {{ border-radius: 13px; padding: 0 14px; font-weight: 600; border: 1px solid {c['border']};
  background: {c['panel']}; color: {c['text']}; text-align: center; }}
QPushButton#rowpill:hover {{ background: {c['text']}; color: {c['bg']}; border-color: {c['text']}; }}
QPushButton#pill:hover {{ border-color: {c['dim']}; color: {c['text']}; }}
QPushButton#pillPrimary {{ background: {c['text']}; color: {c['bg']}; border: 1px solid {c['text']}; }}
QPushButton#pillPrimary:hover {{ background: {c['dim']}; border-color: {c['dim']}; color: {c['bg']}; }}
QDialog QPushButton, QWidget#settingspage QStackedWidget QPushButton, QWidget#eqpage QWidget QPushButton
  {{ border: 1px solid {c['border']}; border-radius: 14px; padding: 6px 14px; background: {c['panel']}; }}
QDialog QPushButton:hover, QWidget#settingspage QStackedWidget QPushButton:hover, QWidget#eqpage QWidget QPushButton:hover
  {{ border-color: {c['dim']}; }}
"""
    else:
        pill = f"""
QPushButton#pill, QPushButton#pillPrimary {{ padding: 3px 4px; }}
QPushButton#pillPrimary, QPushButton#rowpill {{ color: {c['accent']}; }}
QDialog QPushButton, QWidget#settingspage QStackedWidget QPushButton, QWidget#eqpage QWidget QPushButton
  {{ border: 1px solid {c['border']}; border-radius: {r}px; padding: 4px 8px; }}
"""
    return f"""
* {{ font-family: "{family}"; font-size: {fs:g}pt; }}
QMainWindow {{ background: {"transparent" if alpha < 1 else c['bg']}; color: {c['text']}; }}
QDialog {{ background: {c['bg']}; color: {c['text']}; }}
QWidget#root {{ background: {bg}; color: {c['text']}; }}
QWidget {{ color: {c['text']}; }}
QWidget#sidebar {{ background: {panel}; border-right: 1px solid {rgba(c['border'], max(alpha, .4))}; }}
QWidget#topbar {{ background: transparent; }}
QWidget#npbar {{ background: {panel}; border-top: 1px solid {rgba(c['border'], max(alpha, .4))}; }}
QWidget#npbar[studio="true"] {{ background: transparent; border: none; }}
QLabel#verdict {{ font-size: {fs + 3:g}pt; font-weight: 700; color: {c['text']}; }}
QLabel#lyric {{ font-size: {fs + 13:g}pt; font-weight: 700; color: {c['dim']}; }}
QLabel#lyric[state="current"] {{ color: {c['text']}; }}
QLabel#lyric[state="past"] {{ color: {rgba(c['dim'], .6)}; }}
QLabel#lyric[state="plain"] {{ font-size: {fs + 5:g}pt; font-weight: 500; color: {c['text']}; }}
QLabel#lyric:hover {{ color: {c['text']}; }}
QWidget#rail, QWidget#card, QWidget#nowpanel {{ background: {panel}; border-radius: {max(r, 6)}px; }}
QFrame#vline, QFrame#hline {{ background: {c['border']}; border: none; }}
QLabel#colcount {{ font-family: "{display}"; font-size: {fs + 3:g}pt; font-weight: bold; color: {c['dim']}; }}
QLabel#npBig {{ font-size: {fs + 6:g}pt; font-weight: 700; }}
QLabel#bigtime {{ font-family: "{display}"; font-size: {fs + 29:g}pt; font-weight: bold; }}
QLabel#bigtotal {{ font-family: "{display}"; font-size: {fs + 9:g}pt; font-weight: bold; color: {c['dim']}; }}
QLabel#nextnum {{ font-family: "{display}"; font-size: {fs + 2:g}pt; font-weight: bold; color: {c['dim']}; }}
QListWidget#col::item {{ padding: 8px 10px; border-radius: {min(r, 8)}px; color: {c['dim']}; }}
QListWidget#col::item:hover {{ color: {c['text']}; background: {c['select']}; }}
QListWidget#col::item:selected {{ background: {c['text']}; color: {c['bg']}; font-weight: 600; }}
QListWidget#colalbums::item {{ border: none; }}
QTableView#coltracks::item:selected {{ background: {c['text']}; color: {c['bg']}; }}
QLabel {{ background: transparent; }}
QLabel#dim {{ color: {c['dim']}; }}
QLabel#section, QLabel#eyebrow {{ color: {c['dim']}; font-size: {fs - 1.5:g}pt; font-weight: 600; }}
QLabel#h1 {{ font-family: "{display}"; font-size: {fs + 27:g}pt; font-weight: bold; }}
QLabel#h2 {{ font-family: "{display}"; font-size: {fs + 11:g}pt; font-weight: bold; }}
QLabel#brand {{ font-family: "{display}"; font-size: {fs + 10:g}pt; font-weight: bold; color: {c['text']}; }}
QLabel#npTitle {{ font-weight: 600; }}
QLabel#time {{ color: {c['dim']}; font-size: {fs - 1.5:g}pt; }}
QLabel#badge {{ color: {c['dim']}; border: 1px solid {c['border']}; border-radius: 9px;
  padding: 2px 8px; font-size: {fs - 2.5:g}pt; font-weight: 600; }}
QPushButton {{ background: transparent; border: none; padding: 3px 4px; color: {c['text']}; text-align: left; }}
QPushButton:hover {{ color: {c['accent']}; }}
QPushButton#tb {{ color: {c['dim']}; }}
QPushButton#tb:hover {{ color: {c['text']}; }}
QPushButton#nav {{ padding: 7px 8px; color: {c['dim']}; border-radius: {min(r, 9)}px; font-weight: 500; }}
QPushButton#nav:hover {{ color: {c['text']}; }}
QPushButton#nav:checked {{ color: {c['text']}; background: {c['select']}; }}
QPushButton[active="true"] {{ color: {c['accent']}; }}
QPushButton:disabled {{ color: {c['border']}; }}
{pill}
QLineEdit, QSpinBox, QComboBox, QPlainTextEdit, QFontComboBox {{
  background: {c['panel']}; border: 1px solid {c['border']}; border-radius: {min(r, 9)}px; padding: 5px 8px;
  selection-background-color: {c['text']}; selection-color: {c['bg']}; }}
QSpinBox QLineEdit, QComboBox QLineEdit, QFontComboBox QLineEdit {{ padding: 0; margin: 0; border: none; background: transparent; }}
QSpinBox QLineEdit {{ padding-left: 8px; }}
QSpinBox::up-button, QSpinBox::down-button {{ width: 0; height: 0; border: none; subcontrol-origin: border; }}
QLineEdit#search {{ border-radius: {17 if pills else r}px; padding: 7px 14px; background: {c['panel']}; }}
QLineEdit:focus, QPlainTextEdit:focus, QSpinBox:focus {{ border-color: {c['dim']}; }}
QComboBox::drop-down {{ border: none; width: 18px; }}
QComboBox QAbstractItemView {{ background: {c['panel']}; border: 1px solid {c['border']};
  selection-background-color: {c['select']}; selection-color: {c['text']}; outline: 0; }}
QTableView, QListWidget, QListView {{ background: transparent; border: none; outline: 0;
  selection-background-color: {c['select']}; selection-color: {c['text']}; }}
QTableView::item {{ padding: 0 8px; border: none; }}
QTableView::item:hover {{ background: {c['select']}; }}
QTableView::item:selected {{ background: {c['select']}; color: {c['text']}; }}
QListWidget#artists::item, QListWidget#playlists::item {{ padding: 6px 6px; border-radius: {min(r, 8)}px; color: {c['dim']}; }}
QListWidget#artists::item:hover, QListWidget#playlists::item:hover {{ color: {c['text']}; }}
QListWidget#artists::item:selected, QListWidget#playlists::item:selected {{ background: {c['select']}; color: {c['text']}; }}
QTreeView {{ background: transparent; border: none; outline: 0; }}
QTreeView::item {{ padding: 6px 4px; border: none; }}
QTreeView::item:hover {{ background: {c['select']}; }}
QTreeView::item:selected {{ background: {c['select']}; color: {c['text']}; }}
QListWidget#dllist::item {{ padding: 6px 4px; color: {c['dim']}; }}
QHeaderView, QHeaderView::section {{ background: transparent; color: {c['dim']}; border: none; }}
QHeaderView::section {{ border-bottom: 1px solid {c['border']}; padding: 6px 8px;
  font-size: {fs - 1.5:g}pt; font-weight: 600; }}
QTableCornerButton::section {{ background: transparent; border: none; }}
QScrollArea {{ background: transparent; border: none; }}
QScrollArea > QWidget > QWidget {{ background: transparent; }}
QScrollBar:vertical {{ background: transparent; width: 8px; margin: 2px; }}
QScrollBar:horizontal {{ background: transparent; height: 8px; margin: 2px; }}
QScrollBar::handle {{ background: {c['border']}; border-radius: {3 if pills else sr}px; min-height: 30px; min-width: 30px; }}
QScrollBar::handle:hover {{ background: {c['dim']}; }}
QScrollBar::add-line, QScrollBar::sub-line {{ height: 0; width: 0; }}
QScrollBar::add-page, QScrollBar::sub-page {{ background: transparent; }}
QMenu {{ background: {c['panel']}; border: 1px solid {c['border']}; border-radius: {min(r, 10)}px; padding: 6px; }}
QMenu::item {{ padding: 6px 24px 6px 12px; background: transparent; border-radius: {min(r, 6)}px; }}
QMenu::item:selected {{ background: {c['select']}; color: {c['text']}; }}
QMenu::separator {{ height: 1px; background: {c['border']}; margin: 5px 8px; }}
QTabWidget::pane {{ border: 1px solid {c['border']}; border-radius: {min(r, 10)}px; top: -1px; }}
QTabBar::tab {{ background: transparent; padding: 7px 16px; color: {c['dim']}; font-weight: 600; }}
QTabBar::tab:selected {{ color: {c['text']}; }}
QSplitter::handle {{ background: {c['border']}; }}
QPushButton#chip {{ border: 1px solid {c['border']}; border-radius: {15 if pills else min(r, 8)}px; padding: 6px 14px;
  background: transparent; color: {c['dim']}; font-weight: 600; text-align: center; }}
QPushButton#chip:hover {{ color: {c['text']}; border-color: {c['dim']}; }}
QPushButton#chip:checked, QPushButton#chip:checked:disabled {{ background: {c['text']}; color: {c['bg']};
  border-color: {c['text']}; }}
QPushButton#chip:disabled {{ color: {c['border']}; }}
QLabel#eqval {{ font-family: "{display}"; font-size: {fs + 4:g}pt; font-weight: bold; color: {c['text']}; }}
QListWidget#settingsnav {{ background: transparent; border: none; }}
QListWidget#settingsnav::item {{ padding: 9px 14px; margin: 1px 0; border-radius: {min(r, 10)}px; color: {c['dim']};
  font-weight: 600; }}
QListWidget#settingsnav::item:hover {{ color: {c['text']}; }}
QListWidget#settingsnav::item:selected {{ background: {c['select']}; color: {c['text']}; }}
QLabel#formlabel {{ color: {c['text']}; }}
QWidget#reccard {{ background: {c['select']}; border-radius: {min(r, 14)}px; }}
QWidget#detailbox {{ background: {c['select']}; border-radius: {min(r, 14)}px; }}
QLabel#coverdrop {{ background: {c['select']}; border-radius: {min(r, 12)}px; color: {c['dim']}; }}
QWidget#onlinebox {{ background: {c['select']}; border-radius: {min(r, 12)}px; }}
QTableWidget#tagtable::item {{ padding: 0 8px; }}
QTableWidget#tagtable QLineEdit {{ padding: 0 6px; border-radius: 4px; }}
QLineEdit#titleedit {{ font-family: "{display}"; font-size: {fs + 27:g}pt; font-weight: bold; background: transparent;
  border: none; border-bottom: 2px solid {c['text']}; border-radius: 0; padding: 0 0 2px 0; }}
QWidget#toast {{ background: {c['text']}; border-radius: {18 if pills else min(r, 10)}px; }}
QLabel#toasttext {{ color: {c['bg']}; font-weight: 600; }}
QPushButton#toastbtn {{ color: {c['bg']}; font-weight: 800; padding: 4px 10px; border-radius: 12px;
  background: transparent; border: 1px solid {c['bg']}; }}
QPushButton#toastbtn:hover {{ background: {c['bg']}; color: {c['text']}; }}
QToolTip {{ background: {c['panel']}; color: {c['text']}; border: 1px solid {c['border']}; padding: 5px; border-radius: 6px; }}
""" + "\n" + (skin_css or "") + "\n" + (cfg.get("custom_qss") or "")


class Main(QMainWindow):
    def __init__(self):
        super().__init__()
        load_bundled_fonts()
        self.dir = app_dir()
        self.cfg = Config(self.dir / "config.json")
        if not self.cfg.fresh and "cfg_version" not in self.cfg.raw_keys:   # monotone → nexus defaults
            if self.cfg.get("header_text") in ("monotone", "", None):
                self.cfg["header_text"] = "nexus"
            if self.cfg.get("start_view") == "browse":
                self.cfg["start_view"] = "albums"
            self.cfg["single_click_play"] = True
            self.cfg["cfg_version"] = 2
            self.cfg.save()
        if not self.cfg["library_folders"]:
            music = Path.home() / "Music"
            if music.is_dir():
                self.cfg["library_folders"] = [str(music)]
        try:
            install_bundled_skins(self)
        except Exception as e:
            print("skins:", e)
        self.c = theme_colors(self.cfg)
        self.covers = Covers(self)
        self.tracks, self.by_path, self.albums, self.artists = [], {}, {}, {}
        self.queue, self.unshuffled, self.qi = [], [], -1
        self.current_path, self.dur, self.errors, self.pending_pos = None, 0, 0, 0
        self.playlists = load_json(self.dir / "playlists.json", {})
        self.stats = load_json(self.dir / "stats.json", {})          # path → {"plays", "last"}
        self._counted = False
        self.stats_timer = QTimer(self)
        self.stats_timer.setSingleShot(True)
        self.stats_timer.setInterval(5000)
        self.stats_timer.timeout.connect(lambda: save_json(self.dir / "stats.json", self.stats))
        self.history, self.page, self.page_arg = [], None, None
        self.tbuttons, self.tables, self.scanner = [], [], None
        self.icon_buttons, self.display_family = [], ""
        self.soulseek = SoulseekNative(self)
        self.discord = DiscordPresence(self)
        self.presence_timer = QTimer(self)
        self.presence_timer.setSingleShot(True)
        self.presence_timer.setInterval(400)
        self.presence_timer.timeout.connect(self._push_presence)
        self._pres_pos = (0, 0.0)
        if int(self.cfg.get("background_alpha", 100)) < 100:
            self.setAttribute(Qt.WA_TranslucentBackground, True)

        ok, why = engine_available()
        self.engine_note = ""
        if self.cfg.get("audio_engine", "nexus") == "nexus" and ok:
            self.player = NexusEngine(self)
            self.audio = self.player
            self.player.advanced.connect(self.on_gapless)
            self.player.set_eq(self.cfg.get("eq") or {})
        else:
            if self.cfg.get("audio_engine", "nexus") == "nexus":
                self.engine_note = f"Nexus engine unavailable ({why}) — using the basic Qt player"
            self.player = QMediaPlayer(self)
            self.audio = QAudioOutput(self)
            self.player.setAudioOutput(self.audio)
        self.player.positionChanged.connect(self.on_position)
        self.player.durationChanged.connect(self.on_duration)
        self.player.mediaStatusChanged.connect(self.on_status)
        self.player.playbackStateChanged.connect(lambda _s: (self.update_controls(), self.push_presence()))
        self.player.errorOccurred.connect(self.on_error)

        self.analyzer = Analyzer(self)
        self.immersive, self._imm_restore = False, None
        self.build_ui()
        self.setWindowTitle(APP_NAME)
        self.shortcuts()
        self.apply_settings(rebuild=False)
        self.set_tracks(load_json(self.dir / "library.json", []))
        self.set_volume(float(self.cfg["volume"]))
        geo = self.cfg.get("geometry")
        if not (geo and self.restoreGeometry(QByteArray.fromBase64(geo.encode()))):
            self.resize(1280, 820)
        if self.cfg["remember_session"]:
            self.restore_session()
        self.setAcceptDrops(True)
        self.importer = Importer(self)
        self.watch = WatchFolder(self, self.importer)
        if self.cfg.get("update_check", True) and (self.cfg.get("update_url") or self.update_repo()):
            QTimer.singleShot(4000, self.check_updates)
        self.go(self.cfg["start_view"], push=False)
        if self.cfg["scan_on_startup"] or not self.tracks:
            QTimer.singleShot(400, self.rescan)

    # ---------------------------------------------------------- text helpers
    def L(self, s):
        if self.cfg["uppercase"]:
            return s.upper()
        if self.cfg.get("ui_case", "sentence") == "sentence":
            for i, ch in enumerate(s):
                if ch.isalpha():
                    return s[:i] + ch.upper() + s[i + 1:]
                if ch.isalnum() or ch in "“\"'":      # starts with a number or a quote: leave as is
                    return s
        return s

    def B(self, s):
        s = self.L(s)
        br = self.cfg.get("brackets") or ""
        if not br:
            return s
        if " " in br.strip() and len(br.split()) == 2:
            l, r = br.split()
        elif len(br) >= 2:
            l, r = br[: len(br) // 2], br[len(br) // 2:]
        else:
            l = r = br
        pad = " " if self.cfg.get("bracket_padding", True) else ""
        return f"{l}{pad}{s}{pad}{r}"

    def tbtn(self, text, slot=None, obj="tb"):
        b = QPushButton()
        b.setObjectName(obj)
        b.setCursor(Qt.PointingHandCursor)
        b.setFocusPolicy(Qt.NoFocus)
        b._raw = text
        if slot:
            b.clicked.connect(lambda _=False: slot())
        b.setText(self.B(text))
        self.tbuttons.append(b)
        return b

    @staticmethod
    def set_active(b, on):
        b.setProperty("active", bool(on))
        b.style().unpolish(b)
        b.style().polish(b)

    # ---------------------------------------------------------- ui
    def build_ui(self):
        self.studio = self.cfg.get("layout", "studio") == "studio"
        root = AmbientRoot(self)
        self.root = root
        self.setCentralWidget(root)
        outer = QVBoxLayout(root)
        outer.setContentsMargins(0, 0, 0, 0)
        outer.setSpacing(0)
        self.nav = {}
        self.rail_items = []
        self.build_pages()
        self.now_panel = NowPanel(self)
        if self.studio:
            self.build_studio(outer)
        else:
            self.build_classic(outer)
        self.npbar_w = self.build_npbar()
        outer.addWidget(self.npbar_w)

    def set_immersive(self, on):
        """visualizer only: hide the chrome and go full screen."""
        on = bool(on) and self.page == "visualizer"
        if on == self.immersive:
            return
        self.immersive = on
        parts = [getattr(self, n, None) for n in ("topbar", "sidebar", "npbar_w")]
        if on:
            self._imm_restore = (self.isMaximized(), self.isFullScreen(), self.now_panel.isVisible())
            for w in parts + [self.now_panel]:
                if w is not None:
                    w.hide()
            self.showFullScreen()
        else:
            was_max, was_full, panel = self._imm_restore or (False, False, True)
            for w in parts:
                if w is not None:
                    w.show()
            self.now_panel.setVisible(panel)
            if not was_full:
                self.showMaximized() if was_max else self.showNormal()
        self.vis_page.refresh()

    def set_immersive_off(self):
        cur = self.page
        self.page = "visualizer"
        self.set_immersive(False)
        self.page = cur

    def toggle_visualizer(self):
        if self.page == "visualizer":
            self.set_immersive(False)
            self.back()
        else:
            self.go("visualizer")

    def build_pages(self):
        self.stack = QStackedWidget()
        self.album_grid = AlbumGrid(self)
        albums_page = QWidget()
        ap = QVBoxLayout(albums_page)
        ap.setContentsMargins(22, 10, 8, 0)
        self.empty = QWidget()
        ev = QVBoxLayout(self.empty)
        ev.addStretch()
        self.empty_label = QLabel(objectName="dim")
        self.empty_label.setAlignment(Qt.AlignCenter)
        ev.addWidget(self.empty_label)
        ev.addSpacing(10)
        ev.addWidget(self.tbtn("add music folder", self.add_folder, "pillPrimary"), 0, Qt.AlignCenter)
        ev.addStretch()
        ap.addWidget(self.empty, 1)
        ap.addWidget(self.album_grid, 1)
        self.artists_page = ArtistsPage(self)
        self.browser = ColumnBrowser(self)
        songs_page = QWidget()
        spl = QVBoxLayout(songs_page)
        spl.setContentsMargins(28, 14, 28, 0)
        self.songs_table = SongTable(self, "list")
        spl.addWidget(self.songs_table)
        self.collection = CollectionPage(self)
        self.queue_page = QueuePage(self)
        self.lyrics_page = LyricsPage(self)
        self.download_page = DownloadPage(self)
        QTimer.singleShot(3000, self.download_page.resume_on_start)
        self.inspect_page = InspectPage(self)
        self.vis_page = VisualizerPage(self)
        self.tag_page = TagEditorPage(self)
        self.discover_page = DiscoverPage(self)
        self.pages = {"visualizer": self.vis_page, "tags": self.tag_page, "discover": self.discover_page, "browse": self.browser, "albums": albums_page, "artists": self.artists_page,
                      "songs": songs_page, "queue": self.queue_page, "album": self.collection,
                      "playlist": self.collection, "lyrics": self.lyrics_page, "download": self.download_page,
                      "inspect": self.inspect_page}
        for w in (self.browser, albums_page, self.artists_page, songs_page, self.queue_page, self.collection,
                  self.lyrics_page, self.download_page, self.inspect_page, self.vis_page, self.tag_page,
                  self.discover_page):
            self.stack.addWidget(w)

    def make_search(self):
        self.search = QLineEdit(objectName="search")
        self.search.setMaximumWidth(520)
        self.search.setClearButtonEnabled(True)
        self.search.textChanged.connect(self.on_search)
        return self.search

    def build_studio(self, outer):
        outer.setContentsMargins(8, 8, 8, 0)
        outer.setSpacing(8)
        top = QWidget(objectName="topbar")
        top.setAttribute(Qt.WA_StyledBackground, True)
        self.topbar = top
        tl = QHBoxLayout(top)
        tl.setContentsMargins(10, 0, 6, 0)
        tl.setSpacing(10)
        self.brand = QLabel(objectName="brand")
        self.brand.setFixedWidth(190)
        tl.addWidget(self.brand)
        self.back_btn = IconButton(self, "back", 34)
        self.back_btn.round = True
        self.back_btn.clicked.connect(self.back)
        self.icon_buttons.append(self.back_btn)
        tl.addWidget(self.back_btn)
        home = IconButton(self, "home", 44)
        home.round = True
        home.setToolTip("home")
        home.clicked.connect(lambda: self.go("albums"))
        self.icon_buttons.append(home)
        tl.addWidget(home)
        tl.addWidget(self.make_search(), 1)
        tl.addStretch()
        self.scan_label = QLabel(objectName="time")
        tl.addWidget(self.scan_label)
        self.count_label = QLabel(objectName="time")
        tl.addWidget(self.count_label)
        tl.addSpacing(6)
        gear = IconButton(self, "gear", 40)
        gear.round = True
        gear.setToolTip("settings  (ctrl+,)")
        gear.clicked.connect(self.open_settings)
        self.icon_buttons.append(gear)
        tl.addWidget(gear)
        outer.addWidget(top)

        body = QHBoxLayout()
        body.setSpacing(8)
        rail = QWidget(objectName="rail")
        rail.setAttribute(Qt.WA_StyledBackground, True)
        rail.setFixedWidth(68)
        self.sidebar = rail
        rl = QVBoxLayout(rail)
        rl.setContentsMargins(12, 12, 12, 12)
        rl.setSpacing(8)
        for key, kind, tip in (("albums", "grid", "albums  (ctrl+1)"), ("browse", "columns", "browse  (ctrl+2)"),
                               ("songs", "list", "songs  (ctrl+3)"), ("queue", "queue", "queue  (ctrl+4)"),
                               ("discover", "discover", "discover  (ctrl+5)"),
                               ("download", "download", "download  (ctrl+d)")):
            b = IconButton(self, kind, 44)
            b.rail = True
            b.setToolTip(tip)
            b.clicked.connect(lambda _=False, k=key: self.go(k))
            self.nav[key] = b
            self.icon_buttons.append(b)
            rl.addWidget(b, 0, Qt.AlignHCenter)
        rl.addSpacing(4)
        rl.addWidget(hline())
        rl.addSpacing(4)
        plus = IconButton(self, "plus", 44)
        plus.rail = True
        plus.setToolTip("new playlist")
        plus.clicked.connect(lambda: self.playlist_new([]))
        self.icon_buttons.append(plus)
        rl.addWidget(plus, 0, Qt.AlignHCenter)
        sa = QScrollArea()
        sa.setWidgetResizable(True)
        sa.setFrameShape(QFrame.NoFrame)
        sa.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        sa.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        inner = QWidget()
        self.rail_pl = QVBoxLayout(inner)
        self.rail_pl.setContentsMargins(0, 0, 0, 0)
        self.rail_pl.setSpacing(8)
        self.rail_pl.addStretch()
        sa.setWidget(inner)
        rl.addWidget(sa, 1)
        refresh = IconButton(self, "refresh", 44)
        refresh.rail = True
        refresh.setToolTip("rescan library")
        refresh.clicked.connect(self.rescan)
        self.icon_buttons.append(refresh)
        rl.addWidget(refresh, 0, Qt.AlignHCenter)
        body.addWidget(rail)

        card = QWidget(objectName="card")
        card.setAttribute(Qt.WA_StyledBackground, True)
        cv = QVBoxLayout(card)
        cv.setContentsMargins(0, 0, 0, 0)
        cv.addWidget(self.stack)
        body.addWidget(card, 1)
        body.addWidget(self.now_panel)
        outer.addLayout(body, 1)

    def build_classic(self, outer):
        body = QHBoxLayout()
        body.setSpacing(0)
        sb = QWidget(objectName="sidebar")
        sb.setAttribute(Qt.WA_StyledBackground, True)
        self.sidebar = sb
        sl = QVBoxLayout(sb)
        sl.setContentsMargins(14, 22, 14, 14)
        sl.setSpacing(2)
        self.brand = QLabel(objectName="brand")
        sl.addWidget(self.brand)
        sl.addSpacing(22)
        for key in ("albums", "browse", "artists", "songs", "queue", "discover", "download"):
            b = QPushButton(objectName="nav")
            b.setCheckable(True)
            b.setCursor(Qt.PointingHandCursor)
            b.setFocusPolicy(Qt.NoFocus)
            b.clicked.connect(lambda _=False, k=key: self.go(k))
            self.nav[key] = b
            sl.addWidget(b)
        sl.addSpacing(22)
        self.pl_label = QLabel(objectName="section")
        self.pl_label.setContentsMargins(8, 0, 0, 4)
        sl.addWidget(self.pl_label)
        self.pl_list = QListWidget(objectName="playlists")
        self.pl_list.setFocusPolicy(Qt.NoFocus)
        self.pl_list.itemClicked.connect(lambda it: self.go("playlist", it.data(Qt.UserRole)))
        self.pl_list.setContextMenuPolicy(Qt.CustomContextMenu)
        self.pl_list.customContextMenuRequested.connect(self.playlist_menu)
        sl.addWidget(self.pl_list, 1)
        sl.addWidget(self.tbtn("+ new playlist", lambda: self.playlist_new([])))
        sl.addSpacing(8)
        self.scan_label = QLabel(objectName="dim")
        self.scan_label.setWordWrap(True)
        sl.addWidget(self.scan_label)
        sl.addWidget(self.tbtn("rescan", self.rescan))
        sl.addWidget(self.tbtn("settings", self.open_settings))
        body.addWidget(sb)

        main = QVBoxLayout()
        main.setSpacing(0)
        top = QWidget(objectName="topbar")
        top.setAttribute(Qt.WA_StyledBackground, True)
        self.topbar = top
        tl = QHBoxLayout(top)
        tl.setContentsMargins(24, 14, 28, 6)
        tl.setSpacing(12)
        self.back_btn = IconButton(self, "back", 30)
        self.back_btn.clicked.connect(self.back)
        self.icon_buttons.append(self.back_btn)
        tl.addWidget(self.back_btn)
        tl.addWidget(self.make_search(), 1)
        tl.addStretch()
        self.count_label = QLabel(objectName="time")
        tl.addWidget(self.count_label)
        main.addWidget(top)
        main.addWidget(self.stack, 1)
        body.addLayout(main, 1)
        body.addWidget(self.now_panel)
        outer.addLayout(body, 1)

    def rebuild_ui(self):
        page, arg = self.page, self.page_arg
        text = self.search.text() if hasattr(self, "search") else ""
        self.tbuttons, self.tables, self.icon_buttons = [], [], []
        self.build_ui()
        self.page = None
        self.apply_settings(rebuild=True)
        self.go(page or self.cfg["start_view"], arg, push=False)
        if text:
            self.search.setText(text)
        if self.queue and 0 <= self.qi < len(self.queue):
            self.update_now_playing(self.queue[self.qi])
        self.set_volume(self.cfg["volume"])
        self.on_position(self.player.position())
        self.update_controls()

    def build_npbar(self):
        bar = QWidget(objectName="npbar")
        bar.setAttribute(Qt.WA_StyledBackground, True)
        bar.setProperty("studio", self.studio)
        h = QHBoxLayout(bar)
        h.setContentsMargins(16, 12, 20, 12)
        h.setSpacing(14)
        self.np_cover = ClickLabel()
        self.np_cover.setFixedSize(56, 56)
        self.np_cover.setCursor(Qt.PointingHandCursor)
        self.np_cover.clicked.connect(self.goto_playing_album)
        h.addWidget(self.np_cover)
        left = QWidget()
        left.setFixedWidth(270)
        lv = QVBoxLayout(left)
        lv.setContentsMargins(0, 0, 0, 0)
        lv.setSpacing(3)
        self.np_title = ClickLabel(objectName="npTitle")
        self.np_title.setCursor(Qt.PointingHandCursor)
        self.np_title.clicked.connect(self.goto_playing_album)
        self.np_artist = ClickLabel(objectName="dim")
        self.np_artist.setCursor(Qt.PointingHandCursor)
        self.np_artist.clicked.connect(self.goto_playing_artist)
        lv.addStretch()
        lv.addWidget(self.np_title)
        lv.addWidget(self.np_artist)
        lv.addStretch()
        h.addWidget(left)

        center = QVBoxLayout()
        center.setSpacing(4)
        ctr = QHBoxLayout()
        ctr.setSpacing(14)
        ctr.addStretch()

        def ib(kind, size, slot):
            b = IconButton(self, kind, size)
            b.clicked.connect(lambda _=False: slot())
            self.icon_buttons.append(b)
            ctr.addWidget(b, 0, Qt.AlignVCenter)
            return b
        self.b_shuffle = ib("shuffle", 30, self.toggle_shuffle)
        self.b_prev = ib("prev", 32, self.prev)
        self.b_play = ib("play", 40, self.toggle_play)
        self.b_next = ib("next", 32, self.next)
        self.b_repeat = ib("repeat", 30, self.cycle_repeat)
        ctr.addStretch()
        prog = QHBoxLayout()
        prog.setSpacing(12)
        self.t_pos = QLabel("0:00", objectName="time")
        self.t_pos.setMinimumWidth(40)
        self.t_pos.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        self.seek = TextBar(key="progress_style")
        self.seek.setMaximumWidth(620)
        self.seek.changed.connect(lambda v: self.t_pos.setText(fmt_time(v * self.dur / 1000)))
        self.seek.released.connect(lambda v: self.player.setPosition(int(v * self.dur)))
        self.t_dur = ClickLabel("0:00", objectName="time")
        self.t_dur.setMinimumWidth(40)
        self.t_dur.setCursor(Qt.PointingHandCursor)
        self.t_dur.clicked.connect(self.toggle_remaining)
        prog.addStretch()
        prog.addWidget(self.t_pos)
        prog.addWidget(self.seek, 1)
        prog.addWidget(self.t_dur)
        prog.addStretch()
        center.addLayout(ctr)
        center.addLayout(prog)
        h.addLayout(center, 1)

        right = QWidget()
        right.setFixedWidth(420)
        rl = QHBoxLayout(right)
        rl.setContentsMargins(0, 0, 0, 0)
        rl.setSpacing(8)
        rl.addStretch()
        self.np_quality = QLabel(objectName="badge")
        self.np_quality.hide()
        self.np_quality.setSizePolicy(QSizePolicy.Fixed, QSizePolicy.Fixed)
        rl.addWidget(self.np_quality, 0, Qt.AlignVCenter)
        rl.addSpacing(4)
        self.b_lyrics = IconButton(self, "lyrics", 30)
        self.b_lyrics.setToolTip("lyrics  (ctrl+l)")
        self.b_lyrics.clicked.connect(self.toggle_lyrics)
        self.icon_buttons.append(self.b_lyrics)
        rl.addWidget(self.b_lyrics)
        self.b_vis = IconButton(self, "vis", 30)
        self.b_vis.setToolTip("visualizer  (ctrl+b)")
        self.b_vis.clicked.connect(self.toggle_visualizer)
        self.icon_buttons.append(self.b_vis)
        rl.addWidget(self.b_vis)
        self.b_eq = IconButton(self, "eq", 30)
        self.b_eq.setToolTip("equalizer  (ctrl+e)")
        self.b_eq.clicked.connect(self.open_eq)
        self.icon_buttons.append(self.b_eq)
        rl.addWidget(self.b_eq)
        self.b_queue = IconButton(self, "queue", 30)
        self.b_queue.setToolTip("queue")
        self.b_queue.clicked.connect(lambda: self.go("queue"))
        self.b_panel = IconButton(self, "panel", 30)
        self.b_panel.setToolTip("now playing panel")
        self.b_panel.clicked.connect(self.toggle_panel)
        for b in (self.b_queue, self.b_panel):
            self.icon_buttons.append(b)
            rl.addWidget(b)
        rl.addSpacing(2)
        self.b_vol = IconButton(self, "volume", 26)
        self.b_vol.setToolTip("mute")
        self.b_vol.clicked.connect(self.toggle_mute)
        self.icon_buttons.append(self.b_vol)
        rl.addWidget(self.b_vol)
        self.vol = TextBar(wheel=True, key="volume_style")
        self.vol.setFixedWidth(96)
        self.vol.changed.connect(lambda f: self.set_volume(f * self.vol_max()))
        rl.addWidget(self.vol)
        self.vol_label = QLabel(objectName="time")
        self.vol_label.setFixedWidth(38)
        rl.addWidget(self.vol_label)
        h.addWidget(right)
        return bar

    def shortcuts(self):
        def sc(seq, fn):
            s = QShortcut(QKeySequence(seq), self)
            s.activated.connect(fn)
        sc("Space", self.toggle_play)
        sc("Ctrl+Right", self.next)
        sc("Ctrl+Left", self.prev)
        sc("Ctrl+F", lambda: (self.search.setFocus(), self.search.selectAll()))
        sc("/", lambda: (self.search.setFocus(), self.search.selectAll()))
        sc("Ctrl+,", self.open_settings)
        sc("Ctrl+S", self.toggle_shuffle)
        sc("Ctrl+R", self.cycle_repeat)
        sc("Ctrl+Up", lambda: self.set_volume(self.cfg["volume"] + 0.05))
        sc("Ctrl+Down", lambda: self.set_volume(self.cfg["volume"] - 0.05))
        sc("Alt+Left", self.back)
        sc("Escape", self.on_escape)
        sc("Ctrl+B", self.toggle_visualizer)
        sc("Ctrl+T", lambda: self.go("tags", [self.queue[self.qi]["path"]]) if 0 <= self.qi < len(self.queue) else None)
        sc("Ctrl+1", lambda: self.go("albums"))
        sc("Ctrl+2", lambda: self.go("browse"))
        sc("Ctrl+3", lambda: self.go("songs"))
        sc("Ctrl+4", lambda: self.go("queue"))
        sc("Ctrl+5", lambda: self.go("discover"))
        sc("Ctrl+E", self.open_eq)
        sc("Ctrl+L", self.toggle_lyrics)
        sc("Ctrl+D", lambda: self.go("download"))
        sc("Ctrl+I", self.inspect_current)
        sc(Qt.Key_MediaTogglePlayPause, self.toggle_play)
        sc(Qt.Key_MediaPlay, self.toggle_play)
        sc(Qt.Key_MediaNext, self.next)
        sc(Qt.Key_MediaPrevious, self.prev)

    # ---------------------------------------------------------- settings / style
    def apply_settings(self, rebuild=True):
        cfg = self.cfg
        active_skin(self)                      # loads the skin's fonts before they're looked up
        self.c = theme_colors(cfg)
        app = QApplication.instance()
        font, display = resolve_font(cfg)
        app.setFont(font)
        sk = active_skin(self)
        css = self.__dict__.get("_skin_preview")
        if css is None:
            css = skin_qss(self, sk) if sk else ""
        app.setStyleSheet(build_qss(self.c, cfg, font.family(), display, css, bool(sk and sk.get("background"))))
        self.root.load_skin_bg()
        self.display_family = display
        self.covers.clear()
        if not self.studio:
            self.sidebar.setFixedWidth(int(cfg["sidebar_width"]))
        self.now_panel.setVisible(bool(cfg.get("now_panel", True)))
        self.now_panel.relayout()
        self.brand.setText(cfg.get("header_text") or APP)
        if not self.studio:
            self.pl_label.setText("PLAYLISTS")
        self.search.setPlaceholderText(self.L("Search songs, artists, albums, formats"))
        self.empty_label.setText(self.L("No music yet.\nPoint me at the folder with your FLAC files."))
        alive = []
        for b in self.tbuttons:                # pages that get rebuilt leave dead buttons behind
            try:
                b.setText(self.B(b._raw))
                alive.append(b)
            except RuntimeError:
                pass
        self.tbuttons = alive
        self.seek.restyle(cfg, self.c)
        self.vol.restyle(cfg, self.c)
        for b in self.icon_buttons:
            b.relayout()
        self.update_nav()
        self.update_controls()
        self.refresh_playlists_list()
        if rebuild:
            self.refresh_views()
        else:
            for t in self.tables:
                t.fit()
                t.horizontalHeader().viewport().update()
            self.album_grid.relayout()
            self.artists_page.grid.relayout()
            if self.page in ("album", "playlist"):
                self.collection.rerender()
        if self.queue and 0 <= self.qi < len(self.queue):
            self.update_now_playing(self.queue[self.qi])
        else:
            self.np_cover.setPixmap(self.covers.get("", 56))

    def open_settings(self):
        if self.page == "settings":
            self.back()
        else:
            self.go("settings")

    # ---------------------------------------------------------- library
    def set_tracks(self, tracks):
        self.tracks = tracks
        self.rebuild_index()
        self.refresh_views()

    def rebuild_index(self):
        self.by_path = {t["path"]: t for t in self.tracks}
        albums = {}
        for t in self.tracks:
            a = albums.get(t["album_key"])
            if a is None:
                a = albums[t["album_key"]] = dict(key=t["album_key"], album=t["album"],
                                                  artist=t["albumartist"], year=t["year"],
                                                  cover=t["cover"], tracks=[], added=0)
            a["tracks"].append(t)
            a["added"] = max(a["added"], t.get("mtime", 0))
            a["year"] = a["year"] or t["year"]
            a["cover"] = a["cover"] or t["cover"]
        for a in albums.values():
            a["tracks"].sort(key=lambda t: (t["disc"], t["track"], t["title"].lower()))
        self.albums = albums
        artists = {}
        for a in albums.values():
            artists.setdefault(a["artist"], []).append(a)
        self.artists = artists

    def sorted_albums(self, albums):
        s, al = self.cfg["album_sort"], list(albums)
        if s == "title":
            al.sort(key=lambda a: a["album"].lower())
        elif s == "year":
            al.sort(key=lambda a: (a["year"] or "9999", a["artist"].lower(), a["album"].lower()))
        elif s == "recent":
            al.sort(key=lambda a: -a["added"])
        else:
            al.sort(key=lambda a: (a["artist"].lower(), a["year"], a["album"].lower()))
        return al

    def refresh_views(self):
        self.album_grid.set_albums(self.sorted_albums(self.albums.values()))
        self.artists_page.refresh()
        self.browser.refresh()
        self.songs_table.set_tracks(self.tracks)
        self.queue_page.refresh()
        if self.page in ("album", "playlist") and not self.collection.rerender():
            self.go("albums", push=False)
        has = bool(self.tracks)
        self.empty.setVisible(not has)
        self.album_grid.setVisible(has)
        self.count_label.setText(f"{len(self.tracks):,} tracks · {len(self.albums):,} albums")
        self.refresh_playlists_list()

    def add_folder(self):
        d = QFileDialog.getExistingDirectory(self, "music folder", str(Path.home()))
        if d and d not in self.cfg["library_folders"]:
            self.cfg["library_folders"].append(d)
            self.cfg.save()
            self.rescan()

    def rescan(self, full=False):
        if self.scanner and self.scanner.isRunning():
            return
        folders = [f for f in self.cfg["library_folders"] if os.path.isdir(f)]
        if not folders:
            self.scan_label.setText(self.L("no music folders — add one in settings"))
            return
        self.scan_label.setText(self.L("scanning…"))
        self.scanner = Scanner(folders, list(self.tracks), self.dir / "covers", full)
        self.scanner.progress.connect(lambda i, n: self.scan_label.setText(self.L(f"scanning {i}/{n}")))
        self.scanner.done.connect(self.on_scan_done)
        self.scanner.start()

    def on_scan_done(self, tracks):
        self.scan_label.setText("")
        self.covers.clear()
        self.set_tracks(tracks)
        self.save_library()

    def import_skin(self, f):
        try:
            slug = import_skin_file(self, f)
            apply_skin(self, slug)
        except Exception as e:
            self.toast(f"couldn't import that skin: {e}")
            return
        name = (read_skin_dir(skins_dir(self) / slug) or {}).get("name", slug)
        back = self.page if self.page != "settings" else None

        def done():
            self.rebuild_ui()
            if back is None:
                self.go("settings", "skins", push=False)
            self.toast(f"skin “{name}” installed")
        QTimer.singleShot(0, done)

    def search_soulseek(self, query):
        self.go("download")
        dp = self.download_page
        dp.q.setText(query)
        dp.search()

    def save_library(self):
        try:
            save_json(self.dir / "library.json",
                      [{k: v for k, v in t.items() if not k.startswith("_")} for t in self.tracks])
        except OSError as e:
            print("library save failed:", e)

    def tags_saved(self, paths, covers_changed=()):
        """after the tag editor wrote files: re-read them, refresh covers, views, discord art."""
        cover_dir = self.dir / "covers"
        keys = set()
        for p in paths:
            old = self.by_path.get(p)
            new = read_track(p)
            if not new:
                continue
            try:
                new["mtime"] = os.path.getmtime(p)
            except OSError:
                pass
            if old is not None:
                keys.add(old["album_key"])
                old.clear()
                old.update(new)
            else:
                self.tracks.append(new)
            keys.add(new["album_key"])
        touched = {self.by_path[p]["album_key"] for p in paths if p in self.by_path}
        redo = {self.by_path[p]["album_key"] for p in covers_changed if p in self.by_path}
        for key in touched:
            cp = cover_dir / f"{key}.jpg"
            if key in redo or not cp.exists():
                src = next((t["path"] for t in self.tracks if t["album_key"] == key), None)
                if src and not save_cover(src, cp) and cp.exists():
                    try:
                        cp.unlink()
                    except OSError:
                        pass
        for t in self.tracks:
            if t["album_key"] in keys or t["album_key"] in touched:
                t["cover"] = t["album_key"] if (cover_dir / f"{t['album_key']}.jpg").exists() else ""
        self.tracks.sort(key=track_sort_key)
        self.covers.clear()
        self.__dict__.pop("_cover_colors", None)
        self.__dict__.pop("_lyrics_cache", None)
        d = getattr(self, "discord", None)
        if d is not None and (keys | touched):
            for store in (d.art_cache, d.art_saved):
                for k in [k for k in store if k.split(":", 1)[-1] in keys | touched]:
                    store.pop(k, None)
            try:
                d.art_file.write_text(json.dumps(d.art_saved), "utf-8")
            except Exception:
                pass
        self.set_tracks(self.tracks)
        self.save_library()
        cur = self.queue[self.qi] if 0 <= self.qi < len(self.queue) else None
        if cur is not None and cur.get("path") in paths:
            self.root.key = ""
            self.update_now_playing(cur)
        if self.page in ("album", "playlist"):
            self.collection.rerender()

    def paths_moved(self, moved):
        """files were renamed: keep library, playlists, queue and lyrics pointing at them."""
        for t in self.tracks:
            if t["path"] in moved:
                t["path"] = moved[t["path"]]
                try:
                    t["mtime"] = os.path.getmtime(t["path"])
                except OSError:
                    pass
        for name, lst in self.playlists.items():
            self.playlists[name] = [moved.get(p, p) for p in lst]
        for a, b in moved.items():
            if a in self.stats:
                self.stats[b] = self.stats.pop(a)
        self.stats_timer.start()
        if self.current_path in moved:
            self.current_path = moved[self.current_path]
        self.__dict__.pop("_lyrics_cache", None)
        self.save_playlists()
        self.set_tracks(self.tracks)
        self.save_library()

    # ---------------------------------------------------------- navigation
    def go(self, page, arg=None, push=True):
        if page not in PAGES:
            page = "browse"
        if page == "artists" and self.studio:
            page = "browse"
        if self.immersive and page != "visualizer":
            self.set_immersive_off()
        if self.page == "tags" and page != "tags" and self.tag_page.dirty():
            self.toast("unsaved tag edits", "save", self.tag_page.save, 9000)
        if page == self.page and page in ("inspect", "lyrics", "settings", "eq", "visualizer", "tags"):
            push = False                     # re-opening the same tool page: replace, don't stack history
        if push and self.page and (self.page, self.page_arg) != (page, arg):
            self.history = (self.history + [(self.page, self.page_arg)])[-60:]
        if page == "album" and not self.collection.show_album(arg):
            return
        if page == "playlist" and not self.collection.show_playlist(arg):
            return
        if page == "artists" and arg:
            self.artists_page.refresh(select=arg)
        if page == "browse" and arg:
            self.browser.refresh(select=arg)
        if page == "queue":
            self.queue_page.refresh()
        if page in ("settings", "eq"):
            self.fresh_page(page, arg)
        if page == "lyrics":
            self.lyrics_page.load(self.queue[self.qi] if 0 <= self.qi < len(self.queue) else None)
        if page == "inspect" and arg:
            self.inspect_page.load(arg)
        if page == "tags":
            self.tag_page.load(arg or ([self.queue[self.qi]["path"]] if 0 <= self.qi < len(self.queue) else []))
        self.page, self.page_arg = page, arg
        self.stack.setCurrentWidget(self.pages[page])
        self.update_nav()
        self.refresh_playlists_list()

    def back(self):
        if self.history:
            page, arg = self.history.pop()
            self.go(page, arg, push=False)
        elif self.page in ("settings", "eq", "lyrics", "inspect", "visualizer", "tags"):
            self.go(self.cfg.get("start_view", "albums"), push=False)

    def update_nav(self):
        mk = self.cfg.get("nav_marker") or ""
        cur = "albums" if self.page == "album" else self.page
        for k, b in self.nav.items():
            on = k == cur
            if isinstance(b, IconButton):
                b.active = on
                b.update()
                continue
            b.setChecked(on)
            prefix = (mk if on else " " * len(mk)) + " " if mk else ""
            b.setText(prefix + self.L(k))
        self.back_btn.setEnabled(bool(self.history))
        if hasattr(self, "b_vis"):
            self.b_vis.active = self.page == "visualizer"
            self.b_vis.update()
        if hasattr(self, "b_queue"):
            self.b_queue.active = self.page == "queue"
            self.b_queue.update()

    def on_search(self, text):
        if text and self.page != "songs":
            self.go("songs")
        self.songs_table.proxy.set_query(text)

    # ---------------------------------------------------------- playlists
    def save_playlists(self):
        save_json(self.dir / "playlists.json", self.playlists)
        self.refresh_playlists_list()

    def refresh_playlists_list(self):
        if self.studio:
            while self.rail_pl.count() > 1:
                it = self.rail_pl.takeAt(0)
                if it.widget():
                    it.widget().deleteLater()
            for i, name in enumerate(self.playlists):
                bp = self.by_path
                cover = next((bp[p]["cover"] for p in self.playlists[name] if p in bp and bp[p]["cover"]), "")
                b = CoverButton(self, name, cover)
                b.active = self.page == "playlist" and self.page_arg == name
                b.clicked.connect(lambda _=False, n=name: self.go("playlist", n))
                b.setContextMenuPolicy(Qt.CustomContextMenu)
                b.customContextMenuRequested.connect(lambda _pos, n=name, w=b: self.playlist_menu_for(n, w))
                self.rail_pl.insertWidget(i, b, 0, Qt.AlignHCenter)
            return
        mk = self.cfg.get("nav_marker") or ""
        self.pl_list.clear()
        for name in self.playlists:
            on = self.page == "playlist" and self.page_arg == name
            prefix = (mk if on else " " * len(mk)) + " " if mk else ""
            it = QListWidgetItem(prefix + name)
            it.setData(Qt.UserRole, name)
            if on:
                it.setForeground(QColor(self.c["accent"]))
            self.pl_list.addItem(it)

    def playlist_menu_for(self, name, widget):
        m = QMenu(self)
        m.addAction(self.L("play"), lambda: self.play_list(
            [self.by_path[p] for p in self.playlists.get(name, []) if p in self.by_path], 0))
        m.addAction(self.L("rename"), lambda: self.playlist_rename(name))
        m.addAction(self.L("delete"), lambda: self.playlist_delete(name))
        m.exec(widget.mapToGlobal(widget.rect().bottomRight()))

    def playlist_new(self, tracks):
        """makes it straight away and opens it with the name ready to type over — no pop-up."""
        base = self.L("new playlist")
        name, i = base, 2
        while name in self.playlists:
            name, i = f"{base} {i}", i + 1
        self.playlists[name] = [t["path"] for t in tracks]
        self.save_playlists()
        self.go("playlist", name)
        self.collection.start_rename()

    def playlist_add(self, name, tracks):
        self.playlists.setdefault(name, []).extend(t["path"] for t in tracks)
        self.save_playlists()
        if self.page == "playlist" and self.page_arg == name:
            self.collection.rerender()

    def playlist_remove_rows(self, name, rows):
        # rows index into the resolved track list shown on the page
        shown = self.collection.tracks
        drop = {id(shown[r]) for r in rows if r < len(shown)}
        keep, bp = [], self.by_path
        for p in self.playlists.get(name, []):
            t = bp.get(p)
            if t is not None and id(t) in drop:
                drop.discard(id(t))
                continue
            keep.append(p)
        self.playlists[name] = keep
        self.save_playlists()
        self.collection.rerender()

    def playlist_rename(self, name):
        if self.page != "playlist" or self.page_arg != name:
            self.go("playlist", name)
        self.collection.start_rename()

    def playlist_rename_to(self, name, new):
        if not new or new == name or new in self.playlists or name not in self.playlists:
            return
        self.playlists = {(new if k == name else k): v for k, v in self.playlists.items()}
        self.save_playlists()
        self.history = [("playlist", new) if h == ("playlist", name) else h for h in self.history]
        self.go("playlist", new, push=False)

    def playlist_delete(self, name):
        if name not in self.playlists:
            return
        order = list(self.playlists)
        idx, paths = order.index(name), self.playlists.pop(name)
        self.save_playlists()
        self.history = [h for h in self.history if h != ("playlist", name)]
        if self.page == "playlist" and self.page_arg == name:
            self.go("albums", push=False)
        else:
            self.refresh_playlists_list()

        def undo():
            if name in self.playlists:
                return
            items = list(self.playlists.items())
            items.insert(min(idx, len(items)), (name, paths))
            self.playlists = dict(items)
            self.save_playlists()
            self.go("playlist", name)
        self.toast(f"deleted “{name}”", "undo", undo)

    def toast(self, text, action_text=None, action=None, ms=6000):
        """small message at the bottom of the window (instead of pop-up boxes)."""
        t = getattr(self, "_toast", None)
        try:
            stale = t is None or t.parent() is not self.root
        except RuntimeError:                   # the ui was rebuilt (skin / layout change)
            stale = True
        if stale:
            t = QWidget(self.root, objectName="toast")
            t.setAttribute(Qt.WA_StyledBackground, True)
            h = QHBoxLayout(t)
            h.setContentsMargins(18, 8, 10, 8)
            h.setSpacing(12)
            t.label = QLabel(objectName="toasttext")
            t.btn = QPushButton(objectName="toastbtn")
            t.btn.setCursor(Qt.PointingHandCursor)
            h.addWidget(t.label)
            h.addWidget(t.btn)
            t.timer = QTimer(t)
            t.timer.setSingleShot(True)
            t.timer.timeout.connect(t.hide)
            t.action = None

            def clicked():
                t.hide()
                if t.action:
                    t.action()
            t.btn.clicked.connect(clicked)
            self._toast = t
        t.label.setText(self.L(text) if text[:1].islower() else text)
        t.action = action
        t.btn.setVisible(bool(action_text))
        t.btn.setText(self.L(action_text or ""))
        t.adjustSize()
        bar = getattr(self, "npbar_w", None)
        bottom = self.root.height() - (bar.height() if bar is not None and bar.isVisible() else 0) - 18
        t.move(max(8, (self.root.width() - t.width()) // 2), max(8, bottom - t.height()))
        t.show()
        t.raise_()
        t.timer.start(ms)

    def playlist_menu(self, pos):
        it = self.pl_list.itemAt(pos)
        if not it:
            return
        name = it.data(Qt.UserRole)
        m = QMenu(self)
        m.addAction(self.L("play"), lambda: self.play_list(
            [self.by_path[p] for p in self.playlists.get(name, []) if p in self.by_path], 0))
        m.addAction(self.L("rename"), lambda: self.playlist_rename(name))
        m.addAction(self.L("delete"), lambda: self.playlist_delete(name))
        m.exec(self.pl_list.viewport().mapToGlobal(pos))

    # ---------------------------------------------------------- queue / playback
    def play_list(self, tracks, start=0):
        tracks = list(tracks)
        if not tracks:
            return
        start = min(max(0, start), len(tracks) - 1)
        self.unshuffled = tracks[:]
        if self.cfg["shuffle"]:
            first = tracks.pop(start)
            random.shuffle(tracks)
            tracks.insert(0, first)
            start = 0
        self.queue, self.qi = tracks, start
        self.play_current()

    def play_shuffled(self, tracks):
        if not tracks:
            return
        if not self.cfg["shuffle"]:
            self.cfg["shuffle"] = True
            self.update_controls()
        self.play_list(tracks, random.randrange(len(tracks)))

    def play_queue_index(self, i):
        if 0 <= i < len(self.queue):
            self.qi = i
            self.play_current()

    def play_current(self, autoplay=True):
        if not (0 <= self.qi < len(self.queue)):
            self.player.stop()
            return
        t = self.queue[self.qi]
        self.current_path = t["path"]
        self._counted = False
        self.player.setSource(QUrl.fromLocalFile(t["path"]))
        if autoplay:
            self.player.play()
        self.update_now_playing(t)
        self.write_now_playing(t)
        self.refresh_playing()

    def update_now_playing(self, t):
        self.np_cover.setPixmap(self.covers.get(t["cover"], 56))
        fm = self.np_title.fontMetrics()
        self.np_title.setText(fm.elidedText(t["title"], Qt.ElideRight, 250))
        self.np_title.setToolTip(t["title"])
        sub = f"{t['artist']} — {t['album']}"
        self.np_artist.setText(self.np_artist.fontMetrics().elidedText(sub, Qt.ElideRight, 250))
        self.np_artist.setToolTip(sub)
        self.np_quality.setText(quality(t))
        self.np_quality.setVisible(bool(quality(t)) and not self.cfg.get("now_panel", True))
        self.now_panel.set_track(t)
        self.root.set_cover(t["cover"])
        if self.page == "visualizer":
            self.vis_page.set_track(t)
        self.setWindowTitle(f"{t['title']} — {t['artist']}")
        if self.page == "lyrics":
            self.lyrics_page.load(t)
        if hasattr(self, "b_lyrics"):
            self.update_controls()
        self.push_presence()

    def write_now_playing(self, t):
        path = (self.cfg.get("now_playing_file") or "").strip()
        if not path:
            return

        class Safe(dict):
            def __missing__(self, k):
                return ""
        try:
            text = (self.cfg.get("now_playing_format") or "{artist} - {title}").format_map(
                Safe(t, quality=quality(t)))
            Path(os.path.expanduser(path)).write_text(text, "utf-8")
        except Exception as e:
            print("now playing file:", e)

    def refresh_playing(self):
        self.update_gapless_next()
        self.now_panel.refresh_next()
        for t in self.tables:
            t.viewport().update()
        self.album_grid.viewport().update()
        self.artists_page.grid.viewport().update()
        if self.page == "queue":
            self.queue_page.refresh()

    def playing_album(self):
        if 0 <= self.qi < len(self.queue):
            return self.queue[self.qi]["album_key"]
        return None

    def goto_playing_album(self):
        if self.playing_album():
            self.go("album", self.playing_album())

    def goto_playing_artist(self):
        if 0 <= self.qi < len(self.queue):
            self.go("artists", self.queue[self.qi]["albumartist"])

    def toggle_play(self):
        st = self.player.playbackState()
        if st == QMediaPlayer.PlayingState:
            self.player.pause()
        elif self.queue and 0 <= self.qi < len(self.queue):
            if self.player.source().isEmpty():
                self.play_current()
            else:
                self.player.play()
        elif self.tracks:
            self.play_list(self.songs_table.visible_tracks() or self.tracks, 0)

    def next(self, auto=False):
        if not self.queue:
            return
        if auto and self.cfg["repeat"] == "one":
            self.player.setPosition(0)
            self.player.play()
            return
        if self.qi + 1 < len(self.queue):
            self.qi += 1
        elif self.cfg["repeat"] == "all":
            self.qi = 0
        else:
            if auto:
                self.player.stop()
                self.qi = len(self.queue) - 1
                self.refresh_playing()
            return
        self.play_current()

    def prev(self):
        if not self.queue:
            return
        if self.player.position() > 3000 or self.qi <= 0:
            self.player.setPosition(0)
            return
        self.qi -= 1
        self.play_current()

    def toggle_shuffle(self):
        on = not self.cfg["shuffle"]
        self.cfg["shuffle"] = on
        if self.queue and 0 <= self.qi < len(self.queue):
            cur = self.queue[self.qi]
            if on:
                self.unshuffled = self.queue[:]
                rest = self.queue[self.qi + 1:]
                random.shuffle(rest)
                self.queue = self.queue[: self.qi + 1] + rest
            elif self.unshuffled:
                self.queue = self.unshuffled[:]
                self.qi = next((i for i, t in enumerate(self.queue) if t is cur), 0)
        self.update_controls()
        self.update_gapless_next()
        if self.page == "queue":
            self.queue_page.refresh()

    def cycle_repeat(self):
        order = ["off", "all", "one"]
        self.cfg["repeat"] = order[(order.index(self.cfg["repeat"]) + 1) % 3] \
            if self.cfg["repeat"] in order else "off"
        self.update_controls()
        self.update_gapless_next()

    def toggle_remaining(self):
        self.cfg["show_remaining"] = not self.cfg["show_remaining"]
        self.on_position(self.player.position())

    def queue_next(self, tracks):
        tracks = list(tracks)
        if not self.queue:
            return self.play_list(tracks, 0)
        self.queue[self.qi + 1: self.qi + 1] = tracks
        if self.unshuffled:
            cur = self.queue[self.qi]
            j = next((i for i, t in enumerate(self.unshuffled) if t is cur), len(self.unshuffled) - 1)
            self.unshuffled[j + 1: j + 1] = tracks
        self.refresh_playing()

    def queue_add(self, tracks):
        tracks = list(tracks)
        if not self.queue:
            return self.play_list(tracks, 0)
        self.queue.extend(tracks)
        self.unshuffled.extend(tracks)
        self.refresh_playing()

    def queue_remove(self, rows):
        rows = set(rows)
        removed = [self.queue[r] for r in rows if r < len(self.queue)]
        before = sum(1 for r in rows if r < self.qi)
        cur_gone = self.qi in rows
        self.queue = [t for i, t in enumerate(self.queue) if i not in rows]
        ids = {id(t) for t in removed}
        self.unshuffled = [t for t in self.unshuffled if id(t) not in ids]
        self.qi -= before + (1 if cur_gone else 0)
        self.refresh_playing()
        self.queue_page.refresh()

    def queue_clear(self):
        if 0 <= self.qi < len(self.queue):
            self.queue = [self.queue[self.qi]]
            self.qi = 0
        else:
            self.queue, self.qi = [], -1
        self.unshuffled = self.queue[:]
        self.queue_page.refresh()
        self.update_gapless_next()

    def vol_max(self):
        if not isinstance(self.player, NexusEngine):
            return 1.0
        return max(1.0, int(self.cfg.get("volume_max", 200)) / 100)

    def set_volume(self, v):
        vmax = self.vol_max()
        v = min(vmax, max(0.0, float(v)))
        self.cfg["volume"] = v
        gain = v * v if v <= 1 else v          # perceptual up to 100%, then straight boost
        self.audio.setVolume(gain if isinstance(self.player, NexusEngine) else min(1.0, gain))
        self.vol.set_value(v / vmax)
        pct = f"{int(round(v * 100))}%"
        self.vol.setToolTip(pct + ("  (boost)" if v > 1 else ""))
        self.vol_label.setText(pct)
        if hasattr(self, "b_vol"):
            self.update_controls()

    def update_controls(self):
        if not hasattr(self, "b_play"):
            return
        playing = self.player.playbackState() == QMediaPlayer.PlayingState
        self.b_play.set_kind("pause" if playing else "play")
        self.b_repeat.set_kind("repeat_one" if self.cfg["repeat"] == "one" else "repeat")
        self.b_shuffle.active = bool(self.cfg["shuffle"])
        self.b_repeat.active = self.cfg["repeat"] != "off"
        self.b_vol.set_kind("mute" if self.audio.isMuted() or self.cfg["volume"] <= 0 else "volume")
        self.b_panel.active = bool(self.cfg.get("now_panel", True))
        self.b_eq.active = bool((self.cfg.get("eq") or {}).get("on")) or self.page == "eq"
        self.b_lyrics.active = self.page == "lyrics"
        if hasattr(self, "b_vis"):
            self.b_vis.active = self.page == "visualizer"
        cur = self.queue[self.qi] if 0 <= self.qi < len(self.queue) else None
        self.b_lyrics.setEnabled(bool(cur and self.lyrics_for(cur)) or self.page == "lyrics")
        self.b_lyrics.setToolTip("lyrics  (ctrl+l)" if self.b_lyrics.isEnabled() else "no lyrics for this song")
        self.now_panel.dial.set_spinning(playing)
        for b in self.icon_buttons:
            b.relayout()

    def toggle_panel(self):
        self.cfg["now_panel"] = not self.cfg.get("now_panel", True)
        self.cfg.save()
        self.now_panel.setVisible(self.cfg["now_panel"])
        if self.queue and 0 <= self.qi < len(self.queue):
            self.update_now_playing(self.queue[self.qi])
        self.update_controls()

    def toggle_mute(self):
        self.audio.setMuted(not self.audio.isMuted())
        self.update_controls()

    def on_position(self, ms):
        if ms > 0:
            self.errors = 0
        if not self._counted and self.dur and self.current_path and ms >= min(30000, self.dur * 0.5):
            self._counted = True                       # counts as a play: 30 s in, or half of a short song
            e = self.stats.setdefault(self.current_path, {"plays": 0, "last": 0})
            e["plays"] = int(e.get("plays", 0)) + 1
            e["last"] = int(_time.time())
            self.stats_timer.start()
        last_ms, last_t = self._pres_pos
        now = _time.monotonic()
        if abs(ms - (last_ms + (now - last_t) * 1000)) > 2500 and self.player.playbackState() == QMediaPlayer.PlayingState:
            self.push_presence()               # seek → move discord's progress bar
        self._pres_pos = (ms, now)
        if self.dur:
            self.seek.set_value(ms / self.dur)
        if not self.seek.dragging:
            self.t_pos.setText(fmt_time(ms / 1000))
        self.now_panel.set_pos(ms, self.dur)
        if self.page == "visualizer":
            self.vis_page.set_pos(ms)
        if self.page == "lyrics":
            self.lyrics_page.set_pos(ms)
        if self.cfg["show_remaining"]:
            self.t_dur.setText("-" + fmt_time((self.dur - ms) / 1000))
        else:
            self.t_dur.setText(fmt_time(self.dur / 1000))

    def on_duration(self, ms):
        self.dur = ms
        self.on_position(self.player.position())

    def on_status(self, status):
        if status == QMediaPlayer.LoadedMedia and self.pending_pos:
            self.player.setPosition(self.pending_pos)
            self.pending_pos = 0
        elif status == QMediaPlayer.EndOfMedia:
            self.next(auto=True)

    def on_error(self, _err, msg=""):
        self.scan_label.setText(self.L(f"can't play: {msg}"[:120]))
        self.errors += 1
        if self.errors < 5:
            QTimer.singleShot(400, lambda: self.next(auto=True))

    # ---------------------------------------------------------- session
    def restore_session(self):
        s = self.cfg.get("session") or {}
        q = [self.by_path[p] for p in s.get("queue", []) if p in self.by_path]
        if not q:
            return
        self.queue, self.unshuffled = q, q[:]
        self.qi = min(max(0, int(s.get("qi", 0))), len(q) - 1)
        self.pending_pos = int(s.get("pos", 0))
        self.play_current(autoplay=False)

    # ---------------------------------------------------------- updates
    UPDATE_FILES = ("nexus.py", "Nexus.pyw", "Install Nexus.pyw", "requirements.txt", "README.md",
                    "make_icon.py", "make_skins.py", "build_exe.py", "flake.nix", "nexus.desktop", "run.sh",
                    "LICENSE", "install.ps1")

    def pick_update(self):
        p, _ = QFileDialog.getOpenFileName(self, "Nexus update", str(Path.home() / "Downloads"), "zip (*.zip)")
        if p:
            self.install_update(p)

    def install_update(self, zpath, confirmed=False):
        import re
        import zipfile
        dest = resource_dir()
        if getattr(sys, "frozen", False):
            self.toast("this is the .exe build — install the new version over it instead")
            return
        if not os.access(dest, os.W_OK):
            self.toast(f"{dest} is read-only (NixOS?) — update through nix instead")
            return
        try:
            z = zipfile.ZipFile(zpath)
        except Exception as e:
            self.toast(f"not a valid zip: {e}")
            return
        with z:
            main = next((n for n in z.namelist() if n.split("/")[-1] == "nexus.py" and n.count("/") <= 1), None)
            if not main:
                self.toast("that zip doesn't contain nexus.py")
                return
            prefix = main[: -len("nexus.py")]
            m = re.search(r'^VERSION = "([^"]+)"', z.read(main).decode("utf-8", "replace"), re.M)
            newv = m.group(1) if m else "?"
            if not confirmed:
                self.toast(f"{APP_NAME} {newv} is ready (you have {VERSION})", "update + restart",
                           lambda: self.install_update(zpath, confirmed=True), 30000)
                return
            backup = dest / "_previous"
            backup.mkdir(exist_ok=True)
            if (dest / "nexus.py").exists():
                shutil.copy2(dest / "nexus.py", backup / "nexus.py")
            for info in z.infolist():
                if not info.filename.startswith(prefix) or info.is_dir():
                    continue
                rel = info.filename[len(prefix):]
                parts = rel.split("/")
                if ".." in parts or rel.startswith("/") or not rel:
                    continue
                if not (rel in self.UPDATE_FILES or parts[0] in ("fonts", "icons", "skins")):
                    continue
                target = dest / rel
                target.parent.mkdir(parents=True, exist_ok=True)
                with z.open(info) as src, open(target, "wb") as out:
                    shutil.copyfileobj(src, out)
        self.restart_app()

    def restart_app(self):
        from PySide6.QtCore import QProcess
        dest = resource_dir()
        launcher = dest / "Nexus.pyw" if sys.platform.startswith("win") and (dest / "Nexus.pyw").exists() \
            else dest / "nexus.py"
        self.close()
        QProcess.startDetached(sys.executable, [str(launcher)], str(dest))
        QApplication.instance().quit()

    def update_repo(self):
        return (self.cfg.get("update_repo") or GITHUB_REPO or "").strip().strip("/")

    def check_updates(self, manual=False):
        url = (self.cfg.get("update_url") or "").strip()
        repo = self.update_repo()
        if not url and not repo:
            if manual:
                self.toast("no update source set (settings → update)")
            return
        if manual:
            self.toast("checking for updates…")

        def vtuple(v):
            return [int(x) for x in re.findall(r"\d+", v)[:4]]

        def work():
            import tempfile
            hdr = {"User-Agent": f"Nexus/{VERSION}", "Accept": "application/vnd.github+json"}
            if url:
                with urllib.request.urlopen(urllib.request.Request(url, headers=hdr), timeout=10) as r:
                    meta = json.loads(r.read().decode("utf-8"))
                ver, zurl = str(meta.get("version", "")), meta.get("zip", "")
            else:
                api = f"https://api.github.com/repos/{repo}/releases/latest"
                with urllib.request.urlopen(urllib.request.Request(api, headers=hdr), timeout=10) as r:
                    rel = json.loads(r.read().decode("utf-8"))
                ver = str(rel.get("tag_name", "")).lstrip("vV")
                assets = rel.get("assets") or []
                a = next((x for x in assets if x.get("name", "").lower() == "nexus.zip"), None) or \
                    next((x for x in assets if x.get("name", "").lower().endswith(".zip")), None)
                zurl = a.get("browser_download_url", "") if a else ""
            if not ver or vtuple(ver) <= vtuple(VERSION) or not zurl:
                return ("none", ver)
            tmp = Path(tempfile.gettempdir()) / f"nexus-{ver}.zip"
            with urllib.request.urlopen(urllib.request.Request(zurl, headers={"User-Agent": f"Nexus/{VERSION}"}),
                                        timeout=60) as r, open(tmp, "wb") as out:
                shutil.copyfileobj(r, out)
            return ("got", str(tmp))

        def done(res, err):
            if err:
                if manual:
                    self.toast(f"update check failed: {err}")
                return
            kind, val = res
            if kind == "got":
                self.install_update(val)
            elif manual:
                self.toast(f"you're up to date ({VERSION})")
        self.bg(work, done)

    def dragEnterEvent(self, e):
        if e.mimeData().hasUrls() and any(
                os.path.splitext(u.toLocalFile())[1].lower() in AUDIO_EXT | {".zip", SKIN_EXT}
                for u in e.mimeData().urls()):
            e.acceptProposedAction()

    def dropEvent(self, e):
        for u in e.mimeData().urls():
            f = u.toLocalFile()
            if f.lower().endswith(SKIN_EXT) or f.lower().endswith(".zip") and zip_has(f, "skin.json"):
                self.import_skin(f)
                return
            if f.lower().endswith(".zip"):
                self.install_update(f)
                return
            if os.path.splitext(f)[1].lower() in AUDIO_EXT:
                self.go("inspect", f)
                return

    # ---------------------------------------------------------- gapless + eq
    def peek_next(self):
        if not self.queue or not (0 <= self.qi < len(self.queue)):
            return None
        if self.cfg["repeat"] == "one":
            return self.queue[self.qi]
        if self.qi + 1 < len(self.queue):
            return self.queue[self.qi + 1]
        if self.cfg["repeat"] == "all":
            return self.queue[0]
        return None

    def update_gapless_next(self):
        if isinstance(self.player, NexusEngine):
            t = self.peek_next()
            self.player.queue_next(t["path"] if t else None, t["sr"] if t else 0)

    def on_gapless(self, path):
        """the engine already rolled into the next track without a gap: just move the ui along."""
        nxt = self.peek_next()
        if nxt is not None and nxt["path"] == path:
            if self.cfg["repeat"] != "one":
                self.qi = self.qi + 1 if self.qi + 1 < len(self.queue) else 0
        else:
            self.qi = next((i for i, t in enumerate(self.queue) if t["path"] == path), self.qi)
        if 0 <= self.qi < len(self.queue):
            t = self.queue[self.qi]
            self.current_path = t["path"]
            self._counted = False
            self.update_now_playing(t)
            self.write_now_playing(t)
        self.refresh_playing()

    def lyrics_for(self, t):
        cache = self.__dict__.setdefault("_lyrics_cache", OrderedDict())
        p = t["path"]
        if p not in cache:
            try:
                cache[p] = read_lyrics(p)
            except Exception:
                cache[p] = None
            if len(cache) > 300:
                cache.popitem(last=False)
        return cache[p]

    def inspect_current(self):
        if 0 <= self.qi < len(self.queue):
            self.go("inspect", self.queue[self.qi]["path"])
        else:
            self.go("inspect")

    def push_presence(self):
        if self.cfg.get("discord_enabled"):
            self.presence_timer.start()

    def _push_presence(self):
        t = self.queue[self.qi] if 0 <= self.qi < len(self.queue) else None
        playing = self.player.playbackState() == QMediaPlayer.PlayingState
        self.discord.update(t, playing, self.player.position(), self.dur or int((t or {}).get("duration", 0) * 1000))

    def on_escape(self):
        if self.page == "playlist" and self.collection.renaming():
            self.collection.cancel_rename()
        elif self.immersive:
            self.set_immersive(False)
        elif self.page == "visualizer":
            self.back()
        else:
            self.search.clear()
            self.songs_table.setFocus()

    def toggle_lyrics(self):
        if self.page == "lyrics":
            self.back()
        else:
            self.go("lyrics")
        self.update_controls()

    def open_eq(self):
        if self.page == "eq":
            self.back()
        else:
            self.go("eq")

    def fresh_page(self, page, arg=None):
        """settings / eq pages are rebuilt each time they open so every control shows current values."""
        old = self.pages.get(page)
        if old is not None:
            self.stack.removeWidget(old)
            old.deleteLater()
        new = Settings(self) if page == "settings" else EqualizerPage(self)
        if page == "settings" and arg:
            names = [new.tabs.tabText(i) for i in range(new.tabs.count())]
            if arg in names:
                new.tabs.setCurrentIndex(names.index(arg))
        self.stack.addWidget(new)
        self.pages[page] = new
        return new

    def bg(self, fn, done, tick=None):
        """run fn() on a thread, call done(result, error) back on the ui thread."""
        box = {}

        def run():
            try:
                box["r"] = fn()
            except Exception as e:
                box["e"] = e
            box["done"] = True
        threading.Thread(target=run, daemon=True).start()
        t = QTimer(self)
        t.setInterval(150)

        def check():
            try:
                if tick:
                    tick()
                if box.get("done"):
                    t.stop()
                    t.deleteLater()
                    done(box.get("r"), box.get("e"))
            except RuntimeError:          # the page that asked was rebuilt meanwhile
                t.stop()
        t.timeout.connect(check)
        t.start()

    def downloads_changed(self):
        if self.page == "download":
            self.download_page.status.setText(" · ".join(list(self.importer.log)[:3]))

    def closeEvent(self, e):
        if self.scanner and self.scanner.isRunning():
            self.scanner.requestInterruption()
            self.scanner.wait(3000)
        self.cfg["geometry"] = bytes(self.saveGeometry().toBase64()).decode()
        if self.cfg["remember_session"]:
            self.cfg["session"] = {"queue": [t["path"] for t in self.queue[:5000]],
                                   "qi": self.qi, "pos": self.player.position()}
        self.cfg.save()
        try:
            save_json(self.dir / "stats.json", self.stats)
        except OSError:
            pass
        if isinstance(self.player, NexusEngine):
            self.player.shutdown()
        self.soulseek.stop(wait=8)
        if self.cfg.get("discord_enabled"):
            self.discord.clear()
        self.discord.shutdown()
        super().closeEvent(e)


def _log_uncaught(kind, value, tb):
    """errors inside the running app (no console with pythonw): write them down instead of losing them."""
    import traceback
    try:
        with open(app_dir() / "crash.log", "a", encoding="utf-8") as f:
            f.write(f"\n==== {_time.strftime('%Y-%m-%d %H:%M:%S')}  nexus {VERSION}\n"
                    + "".join(traceback.format_exception(kind, value, tb)))
    except Exception:
        pass
    sys.__excepthook__(kind, value, tb) if sys.stderr else None


def main():
    sys.excepthook = _log_uncaught
    QGuiApplication.setHighDpiScaleFactorRoundingPolicy(Qt.HighDpiScaleFactorRoundingPolicy.PassThrough)
    app = QApplication(sys.argv)
    if sys.platform.startswith("win") and not (resource_dir() / "no_aumid").exists():
        try:  # own taskbar identity + icon instead of python's (matches the installer's shortcut)
            import ctypes
            ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID("Nexus.MusicPlayer")
        except Exception:
            pass
    app.setApplicationName(APP_NAME)
    app.setDesktopFileName(APP)       # wayland app_id → usable in hyprland window rules
    for ic in (("nexus.ico",) if sys.platform.startswith("win") else ()) + ("nexus-256.png",):
        icp = resource_dir() / "icons" / ic
        if icp.exists():
            from PySide6.QtGui import QIcon
            app.setWindowIcon(QIcon(str(icp)))
            break
    load_bundled_fonts()
    w = Main()
    w.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
