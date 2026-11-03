"""draws the Nexus icon at every size + builds nexus.ico. run: python make_icon.py"""
import math
import os
import sys
from pathlib import Path

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import QRectF, Qt
from PySide6.QtGui import QColor, QGuiApplication, QImage, QPainter, QPainterPath, QPen

OUT = Path(__file__).resolve().parent / "icons"
SIZES = [16, 24, 32, 48, 64, 128, 256, 512]


def draw(S):
    img = QImage(S, S, QImage.Format_ARGB32)
    img.fill(Qt.transparent)
    p = QPainter(img)
    p.setRenderHint(QPainter.Antialiasing)
    # tile
    tile = QPainterPath()
    pad = S * 0.02
    tile.addRoundedRect(QRectF(pad, pad, S - 2 * pad, S - 2 * pad), S * 0.23, S * 0.23)
    p.fillPath(tile, QColor("#000000"))
    p.setPen(QPen(QColor(255, 255, 255, 40), max(1.0, S / 128)))
    p.drawPath(tile)
    c = S / 2
    white, grey = QColor("#ffffff"), QColor("#3d3d3d")
    frac = 0.72
    R = S * 0.335
    if S <= 32:
        # simplified: solid arcs
        w = S * 0.085
        rect = QRectF(c - R, c - R, 2 * R, 2 * R)
        p.setPen(QPen(grey, w, Qt.SolidLine, Qt.RoundCap))
        p.drawArc(rect, int((90 - 360 * frac) * 16), int(-(360 * (1 - frac)) * 16))
        p.setPen(QPen(white, w, Qt.SolidLine, Qt.RoundCap))
        p.drawArc(rect, 90 * 16, int(-360 * frac * 16))
        rd = S * 0.17
    else:
        n = 30
        filled = int(round(n * frac))
        p.setPen(Qt.NoPen)
        for i in range(n):
            a = -math.pi / 2 + i / n * 2 * math.pi
            head = i == filled - 1
            d = S * (0.082 if head else 0.046)
            p.setBrush(white if i < filled else grey)
            p.drawEllipse(QRectF(c + R * math.cos(a) - d / 2, c + R * math.sin(a) - d / 2, d, d))
        rd = S * 0.205
    # record
    p.setPen(Qt.NoPen)
    p.setBrush(white)
    p.drawEllipse(QRectF(c - rd, c - rd, 2 * rd, 2 * rd))
    if S > 32:   # faint groove
        p.setPen(QPen(QColor(0, 0, 0, 38), max(1.0, S / 170)))
        p.setBrush(Qt.NoBrush)
        g = rd * 0.68
        p.drawEllipse(QRectF(c - g, c - g, 2 * g, 2 * g))
        p.setPen(Qt.NoPen)
    h = rd * (0.3 if S > 32 else 0.36)
    p.setBrush(QColor("#000000"))
    p.drawEllipse(QRectF(c - h, c - h, 2 * h, 2 * h))
    p.end()
    return img


def main():
    QGuiApplication.instance() or QGuiApplication(sys.argv)
    OUT.mkdir(exist_ok=True)
    paths = {}
    for s in SIZES:
        pth = OUT / f"nexus-{s}.png"
        draw(s).save(str(pth))
        paths[s] = pth
    try:
        from PIL import Image
        imgs = [Image.open(paths[s]).convert("RGBA") for s in (256, 128, 64, 48, 32, 24, 16)]
        imgs[0].save(OUT / "nexus.ico", sizes=[(s, s) for s in (256, 128, 64, 48, 32, 24, 16)],
                     append_images=imgs[1:])
    except ImportError:
        print("pillow missing: skipped nexus.ico (pip install pillow)")
    print("icons written to", OUT)


if __name__ == "__main__":
    main()
