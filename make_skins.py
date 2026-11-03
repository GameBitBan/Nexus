"""builds the example .nexusskin files in skins/ (python make_skins.py)."""
import json
import math
import sys
import zipfile
from pathlib import Path

from PySide6.QtCore import QBuffer, QByteArray, QIODevice, QPointF, QRectF, Qt
from PySide6.QtGui import QColor, QImage, QLinearGradient, QPainter, QPen, QRadialGradient
from PySide6.QtWidgets import QApplication

HERE = Path(__file__).resolve().parent


def png(img, fmt="JPG"):
    ba = QByteArray()
    b = QBuffer(ba)
    b.open(QIODevice.WriteOnly)
    img.save(b, fmt, 88)
    b.close()
    return bytes(ba)


def vapor_bg():
    W, H = 1920, 1080
    img = QImage(W, H, QImage.Format_RGB32)
    p = QPainter(img)
    p.setRenderHint(QPainter.Antialiasing)
    g = QLinearGradient(0, 0, 0, H)
    g.setColorAt(0, QColor("#120428"))
    g.setColorAt(0.55, QColor("#3b0a5c"))
    g.setColorAt(1, QColor("#0b0218"))
    p.fillRect(img.rect(), g)
    sun = QLinearGradient(0, 220, 0, 640)
    sun.setColorAt(0, QColor("#ffd36e"))
    sun.setColorAt(1, QColor("#ff2fa0"))
    p.setPen(Qt.NoPen)
    p.setBrush(sun)
    p.drawEllipse(QPointF(W / 2, 470), 230, 230)
    p.setBrush(QColor("#2a0745"))
    for i in range(7):                                  # sun stripes
        y = 520 + i * 26
        p.drawRect(QRectF(W / 2 - 240, y, 480, 4 + i * 2))
    hz = 600
    p.fillRect(QRectF(0, hz, W, H - hz), QColor("#0b0218"))
    pen = QPen(QColor(255, 60, 200, 150), 2)
    p.setPen(pen)
    for i in range(-30, 31):                            # perspective grid
        p.drawLine(QPointF(W / 2 + i * 18, hz), QPointF(W / 2 + i * 160, H))
    y, step = hz, 6.0
    while y < H:
        p.drawLine(QPointF(0, y), QPointF(W, y))
        step *= 1.28
        y += step
    p.end()
    return png(img)


def glass_bg():
    W, H = 1920, 1080
    img = QImage(W, H, QImage.Format_RGB32)
    img.fill(QColor("#0a0f1f"))
    p = QPainter(img)
    p.setRenderHint(QPainter.Antialiasing)
    for x, y, r, c in ((380, 300, 620, "#2f6bff"), (1500, 260, 560, "#00c2a8"), (1100, 900, 700, "#8a3dff"),
                       (200, 950, 420, "#ff4d6d")):
        g = QRadialGradient(x, y, r)
        q = QColor(c)
        g.setColorAt(0, q)
        q2 = QColor(c)
        q2.setAlpha(0)
        g.setColorAt(1, q2)
        p.fillRect(img.rect(), g)
    p.end()
    return png(img)


def video(name):
    """renders a seamless looping colour-drift video with numpy + ffmpeg. → (mp4 bytes, poster jpg bytes)"""
    import subprocess
    import tempfile
    import numpy as np
    W, H, fps, secs = 640, 360, 30, 16
    P = {
        "aurora": dict(base=(6, 10, 30), seed=7, bands=True,
                       blobs=[((10, 163, 127), .55), ((91, 42, 134), .6), ((20, 90, 200), .5), ((0, 200, 160), .35),
                              ((140, 60, 200), .4)]),
        "lava": dict(base=(18, 2, 4), seed=11, bands=False,
                     blobs=[((255, 77, 0), .5), ((179, 0, 27), .6), ((255, 140, 20), .35), ((120, 0, 60), .55),
                            ((255, 40, 40), .4)]),
    }[name]
    y, x = np.mgrid[0:H, 0:W].astype(np.float32)
    x /= W
    y /= H
    x *= W / H
    rng = np.random.default_rng(P["seed"])
    spec = [(np.array(c, np.float32) / 255, r, rng.uniform(0, 2 * np.pi, 4), rng.integers(1, 3, 2))
            for c, r in P["blobs"]]
    tmp = Path(tempfile.mkdtemp())
    out, poster = tmp / f"{name}.mp4", tmp / f"{name}.jpg"
    ff = subprocess.Popen(["ffmpeg", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24",
                           "-s", f"{W}x{H}", "-r", str(fps), "-i", "-",
                           "-vf", "scale=1280:720:flags=bicubic,format=yuv420p", "-c:v", "libx264",
                           "-preset", "slow", "-crf", "26", "-tune", "film", "-movflags", "+faststart", "-an",
                           str(out)], stdin=subprocess.PIPE)
    base = np.array(P["base"], np.float32) / 255
    n = fps * secs
    for i in range(n):
        t = 2 * np.pi * i / n                       # one full loop → seamless
        img = np.broadcast_to(base, (H, W, 3)).copy()
        for col, r, ph, k in spec:
            cx = (W / H) * (0.5 + 0.38 * math.sin(k[0] * t + ph[0]) + 0.08 * math.sin(3 * t + ph[2]))
            cy = 0.5 + 0.36 * math.cos(k[1] * t + ph[1]) + 0.06 * math.cos(2 * t + ph[3])
            rr = r * (1 + 0.15 * math.sin(t + ph[2]))
            img += col[None, None, :] * np.exp(-((x - cx) ** 2 + (y - cy) ** 2) / (rr * rr * 0.35))[..., None] * 0.9
        if P["bands"]:                                # soft aurora curtains
            wave = 0.5 + 0.12 * np.sin(x * 3.1 + t) + 0.07 * np.sin(x * 7.3 - 2 * t)
            band = np.exp(-((y - wave) ** 2) / 0.006) * (0.5 + 0.5 * np.sin(x * 5 + t))
            img += band[..., None] * np.array([0.1, 0.9, 0.6], np.float32) * 0.35
        img = 1 - np.exp(-img * 1.6)
        ff.stdin.write((np.clip(img, 0, 1) * 255).astype(np.uint8).tobytes())
    ff.stdin.close()
    ff.wait()
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-ss", "4", "-i", str(out), "-frames:v", "1",
                    "-vf", "scale=640:-1", str(poster)], check=True)
    return out.read_bytes(), poster.read_bytes()


def stars_bg():
    import random
    rnd = random.Random(3)
    W, H = 1920, 1080
    img = QImage(W, H, QImage.Format_RGB32)
    p = QPainter(img)
    p.setRenderHint(QPainter.Antialiasing)
    g = QLinearGradient(0, 0, 0, H)
    g.setColorAt(0, QColor("#03050f"))
    g.setColorAt(1, QColor("#0d1430"))
    p.fillRect(img.rect(), g)
    neb = QRadialGradient(W * 0.7, H * 0.35, 700)
    neb.setColorAt(0, QColor(60, 70, 160, 90))
    neb.setColorAt(1, QColor(0, 0, 0, 0))
    p.fillRect(img.rect(), neb)
    p.setPen(Qt.NoPen)
    for _ in range(900):
        a = rnd.random()
        c = QColor(255, 240 if rnd.random() < .2 else 255, 220 if rnd.random() < .15 else 255, int(60 + 195 * a * a))
        p.setBrush(c)
        p.drawEllipse(QPointF(rnd.uniform(0, W), rnd.uniform(0, H)), 0.6 + 1.6 * a ** 3, 0.6 + 1.6 * a ** 3)
    p.end()
    return png(img)


def forest_bg():
    import random
    rnd = random.Random(5)
    W, H = 1920, 1080
    img = QImage(W, H, QImage.Format_RGB32)
    p = QPainter(img)
    p.setRenderHint(QPainter.Antialiasing)
    g = QLinearGradient(0, 0, 0, H)
    g.setColorAt(0, QColor("#22382d"))
    g.setColorAt(0.6, QColor("#0f1d16"))
    g.setColorAt(1, QColor("#070d0a"))
    p.fillRect(img.rect(), g)
    p.setPen(Qt.NoPen)
    for layer, (col, base, hmin, hmax) in enumerate((("#1a2e24", 640, 260, 420), ("#13241b", 760, 300, 520),
                                                     ("#0b1610", 880, 360, 640))):
        p.setBrush(QColor(col))
        x = -60.0
        while x < W + 60:
            h = rnd.uniform(hmin, hmax)
            w = h * rnd.uniform(0.28, 0.4)
            pts = [QPointF(x, base + 40), QPointF(x + w / 2, base - h), QPointF(x + w, base + 40)]
            p.drawPolygon(pts)
            x += w * rnd.uniform(0.45, 0.8)
        p.drawRect(QRectF(0, base + 30, W, H))
        fog = QLinearGradient(0, base - 160, 0, base + 60)
        fog.setColorAt(0, QColor(160, 200, 180, 0))
        fog.setColorAt(1, QColor(160, 200, 180, 26 - layer * 6))
        p.fillRect(QRectF(0, base - 160, W, 220), fog)
        p.setPen(Qt.NoPen)
    p.end()
    return png(img)


def sakura_bg():
    import random
    rnd = random.Random(9)
    W, H = 1920, 1080
    img = QImage(W, H, QImage.Format_RGB32)
    p = QPainter(img)
    p.setRenderHint(QPainter.Antialiasing)
    g = QLinearGradient(0, 0, W, H)
    g.setColorAt(0, QColor("#fff6f8"))
    g.setColorAt(1, QColor("#f7dde6"))
    p.fillRect(img.rect(), g)
    p.setPen(Qt.NoPen)
    for _ in range(160):
        c = QColor("#f4a7bf")
        c.setAlpha(rnd.randint(40, 140))
        p.setBrush(c)
        p.save()
        p.translate(rnd.uniform(0, W), rnd.uniform(0, H))
        p.rotate(rnd.uniform(0, 360))
        s = rnd.uniform(6, 18)
        p.drawEllipse(QRectF(-s, -s / 2, s * 2, s))
        p.restore()
    p.end()
    return png(img)


SKINS = [
    {"file": "Vapor", "background": ("assets/background.jpg", vapor_bg),
     "meta": {"name": "Vapor", "author": "Nexus", "description": "synthwave sunset, neon pink + cyan",
              "colors": {"bg": "#0b0218", "panel": "#160630", "text": "#f6e9ff", "dim": "#a98bc9",
                         "accent": "#ff3fb4", "select": "#2a0d4d", "border": "#3a1566"},
              "settings": {"theme": "nothing", "corner_radius": 16, "progress_style": "line", "volume_style": "line",
                           "vis_style": "mirror", "vis_color": "cover", "ambient_background": False,
                           "skin_bg_opacity": 85, "header_text": "nexus"}},
     "qss": "QLabel#h1, QLabel#brand { color: @accent; }\n"
            "QPushButton#pillPrimary { background: @accent; border-color: @accent; color: #ffffff; }\n"
            "QWidget#card, QWidget#nowpanel, QWidget#rail { border: 1px solid rgba(255, 63, 180, 0.25); }\n"
            "QLabel#bigtime { color: #6ff3ff; }\n"},
    {"file": "Paper", "meta": {
        "name": "Paper", "author": "Nexus", "description": "warm off-white, ink and a rust accent",
        "colors": {"bg": "#efe9dc", "panel": "#f8f4ea", "text": "#2a241c", "dim": "#8b8170",
                   "accent": "#b7410e", "select": "#e6dccb", "border": "#d9cfbd"},
        "settings": {"theme": "nothing light", "display_font": "Inter", "corner_radius": 6, "pill_buttons": True,
                     "progress_style": "line", "volume_style": "line", "header_case": "title",
                     "ambient_background": True, "ambient_strength": 25, "vis_style": "bars"}},
     "qss": "QLabel#h1 { font-weight: 800; letter-spacing: -1px; }\n"
            "QPushButton#pillPrimary { background: @accent; border-color: @accent; color: @panel; }\n"},
    {"file": "Amber CRT", "meta": {
        "name": "Amber CRT", "author": "Nexus", "description": "monochrome amber terminal",
        "colors": {"bg": "#0c0700", "panel": "#120b00", "text": "#ffb000", "dim": "#8a5f00",
                   "accent": "#ffcc4d", "select": "#2a1a00", "border": "#3a2600"},
        "settings": {"theme": "nothing", "layout": "classic", "now_panel": False, "font_family": "Consolas",
                     "display_font": "Consolas", "corner_radius": 0,
                     "control_style": "text", "progress_style": "text", "volume_style": "text",
                     "progress_chars": "█░", "brackets": "[]", "pill_buttons": False, "ui_case": "lower",
                     "header_case": "lower", "ambient_background": False, "vis_style": "dots",
                     "glyphs": {"play": ">", "pause": "||", "prev": "<<", "next": ">>", "now": "▶"}}},
     "qss": "QWidget#card, QWidget#nowpanel, QWidget#rail { border: 1px solid @border; }\n"},
    {"file": "Glass", "background": ("assets/background.jpg", glass_bg),
     "meta": {"name": "Glass", "author": "Nexus", "description": "frosted panels over soft colour",
              "colors": {"bg": "#0a0f1f", "panel": "#0f1630", "text": "#eef2ff", "dim": "#93a0c4",
                         "accent": "#7aa2ff", "select": "#1b2547", "border": "#26325c"},
              "settings": {"theme": "nothing", "corner_radius": 18, "ambient_background": True,
                           "ambient_strength": 35, "skin_bg_opacity": 100, "skin_bg_blur": True,
                           "vis_style": "ring", "vis_color": "cover"}},
     "qss": "QWidget#card, QWidget#nowpanel, QWidget#rail { border: 1px solid rgba(255, 255, 255, 0.08); }\n"},
    {"file": "Aurora", "video": "aurora",
     "meta": {"name": "Aurora", "author": "Nexus", "description": "slow-moving northern lights (video background)",
              "colors": {"bg": "#060a1e", "panel": "#0b1230", "text": "#eafff8", "dim": "#8fb3b0",
                         "accent": "#3ef0c0", "select": "#13204a", "border": "#1d2d5c"},
              "settings": {"theme": "nothing", "corner_radius": 18, "ambient_background": False,
                           "skin_bg_opacity": 100, "skin_bg_blur": False, "vis_style": "ring", "vis_color": "cover",
                           "progress_style": "line", "volume_style": "line"}},
     "qss": "QWidget#card, QWidget#nowpanel, QWidget#rail { border: 1px solid rgba(62, 240, 192, 0.18); }\n"
            "QPushButton#pillPrimary { background: @accent; border-color: @accent; color: @bg; }\n"},
    {"file": "Lava", "video": "lava",
     "meta": {"name": "Lava", "author": "Nexus", "description": "warm drifting lava-lamp glow (video background)",
              "colors": {"bg": "#120204", "panel": "#1c0508", "text": "#fff1e8", "dim": "#c08a7a",
                         "accent": "#ff7a1a", "select": "#3a0c10", "border": "#4a1418"},
              "settings": {"theme": "nothing", "corner_radius": 14, "ambient_background": False,
                           "skin_bg_opacity": 90, "vis_style": "bars", "vis_color": "mono",
                           "progress_style": "line", "volume_style": "line"}},
     "qss": "QLabel#h1, QLabel#bigtime { color: @accent; }\n"
            "QPushButton#pillPrimary { background: @accent; border-color: @accent; color: #1a0400; }\n"},
    {"file": "Midnight Gold", "background": ("assets/background.jpg", stars_bg),
     "meta": {"name": "Midnight Gold", "author": "Nexus", "description": "deep night sky, gold details",
              "colors": {"bg": "#04060f", "panel": "#0a0f22", "text": "#f4efe1", "dim": "#8e8a7c",
                         "accent": "#e3b341", "select": "#161d38", "border": "#222a4a"},
              "settings": {"theme": "nothing", "corner_radius": 10, "ambient_background": False,
                           "skin_bg_opacity": 100, "vis_style": "dots", "vis_color": "mono", "header_case": "upper"}},
     "qss": "QLabel#h1, QLabel#brand, QLabel#bigtime { color: @accent; }\n"
            "QPushButton#pillPrimary { background: @accent; border-color: @accent; color: #1a1300; }\n"
            "QWidget#card, QWidget#nowpanel, QWidget#rail { border: 1px solid rgba(227, 179, 65, 0.22); }\n"},
    {"file": "Forest", "background": ("assets/background.jpg", forest_bg),
     "meta": {"name": "Forest", "author": "Nexus", "description": "misty pines, moss green",
              "colors": {"bg": "#08100c", "panel": "#0f1a14", "text": "#e6f2e8", "dim": "#87a090",
                         "accent": "#8fd694", "select": "#18291f", "border": "#22382b"},
              "settings": {"theme": "nothing", "corner_radius": 12, "ambient_background": False,
                           "skin_bg_opacity": 100, "vis_style": "wave", "vis_color": "mono",
                           "progress_style": "line", "volume_style": "line"}},
     "qss": "QPushButton#pillPrimary { background: @accent; border-color: @accent; color: #08100c; }\n"},
    {"file": "Sakura", "background": ("assets/background.jpg", sakura_bg),
     "meta": {"name": "Sakura", "author": "Nexus", "description": "light pink with falling petals",
              "colors": {"bg": "#fbeef2", "panel": "#fff8fa", "text": "#3a2530", "dim": "#9c7f8b",
                         "accent": "#e05a8a", "select": "#f6dbe5", "border": "#efcfdb"},
              "settings": {"theme": "nothing light", "display_font": "Inter", "corner_radius": 20,
                           "ambient_background": False, "skin_bg_opacity": 100, "header_case": "title",
                           "vis_style": "mirror", "vis_color": "mono", "progress_style": "line",
                           "volume_style": "line"}},
     "qss": "QLabel#h1 { color: @accent; font-weight: 800; }\n"
            "QPushButton#pillPrimary { background: @accent; border-color: @accent; color: #ffffff; }\n"},
    {"file": "Green Screen", "meta": {
        "name": "Green Screen", "author": "Nexus", "description": "green phosphor terminal",
        "colors": {"bg": "#000a02", "panel": "#001205", "text": "#33ff66", "dim": "#1d8a3c",
                   "accent": "#9dffb6", "select": "#00240b", "border": "#00381a"},
        "settings": {"theme": "nothing", "layout": "classic", "now_panel": False, "font_family": "Consolas",
                     "display_font": "Consolas", "corner_radius": 0,
                     "control_style": "text", "progress_style": "text", "volume_style": "text",
                     "progress_chars": "#.", "brackets": "[]", "pill_buttons": False, "ui_case": "lower",
                     "header_case": "lower", "ambient_background": False, "vis_style": "bars",
                     "glyphs": {"play": ">", "pause": "||", "prev": "<<", "next": ">>", "now": "*"}}},
     "qss": "QWidget#card, QWidget#nowpanel, QWidget#rail { border: 1px solid @border; }\n"},
    {"file": "Crimson", "meta": {
        "name": "Crimson", "author": "Nexus", "description": "pure black, one red accent, dot matrix",
        "colors": {"bg": "#000000", "panel": "#0b0b0b", "text": "#ffffff", "dim": "#7a7a7a",
                   "accent": "#ff2a2a", "select": "#1a0606", "border": "#262626"},
        "settings": {"theme": "nothing", "corner_radius": 14, "ambient_background": False,
                     "vis_style": "dots", "vis_color": "mono", "progress_style": "dots", "volume_style": "dots"}},
     "qss": "QLabel#bigtime, QLabel#brand { color: @accent; }\n"
            "QPushButton#pillPrimary { background: @accent; border-color: @accent; color: #ffffff; }\n"},
]


def main():
    only = set(sys.argv[1:])
    app = QApplication.instance() or QApplication(sys.argv[:1])
    out = HERE / "skins"
    out.mkdir(exist_ok=True)
    for sk in SKINS:
        if only and sk["file"] not in only:
            continue
        meta = dict(sk["meta"], version="1.0", format=1)
        files = {}
        if "background" in sk:
            name, fn = sk["background"]
            meta["background"] = name
            files[name] = fn()
        if "video" in sk:
            mp4, poster = video(sk["video"])
            meta["background"] = "assets/background.mp4"
            files["assets/background.mp4"] = mp4
            files["assets/poster.jpg"] = poster
        with zipfile.ZipFile(out / f"{sk['file']}.nexusskin", "w", zipfile.ZIP_DEFLATED) as z:
            z.writestr("skin.json", json.dumps(meta, indent=2))
            if sk.get("qss"):
                z.writestr("style.qss", sk["qss"])
            for n, data in files.items():
                z.writestr(n, data)
        print("wrote", sk["file"])
    _ = app, math


if __name__ == "__main__":
    main()
