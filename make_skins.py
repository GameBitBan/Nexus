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
]


def main():
    app = QApplication.instance() or QApplication(sys.argv[:1])
    out = HERE / "skins"
    out.mkdir(exist_ok=True)
    for sk in SKINS:
        meta = dict(sk["meta"], version="1.0", format=1)
        files = {}
        if "background" in sk:
            name, fn = sk["background"]
            meta["background"] = name
            files[name] = fn()
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
