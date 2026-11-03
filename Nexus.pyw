# double-click to run Nexus on windows (no console window).
# runs through pythonw.exe, so smart app control doesn't need to approve anything.
# on every start it installs anything new listed in requirements.txt (so updates can add packages).
import importlib.util
import os
import re
import subprocess
import sys

here = os.path.dirname(os.path.abspath(__file__))
MODULE = {"pyside6": "PySide6", "mutagen": "mutagen", "numpy": "numpy", "av": "av", "sounddevice": "sounddevice",
          "aioslsk": "aioslsk"}
CORE = {"PySide6", "mutagen"}


def wanted():
    try:
        lines = open(os.path.join(here, "requirements.txt"), encoding="utf-8").read().splitlines()
    except OSError:
        lines = ["PySide6", "mutagen"]
    out = []
    for ln in lines:
        name = re.split(r"[<>=!~ ;\[]", ln.strip(), 1)[0]
        if name and not name.startswith("#"):
            out.append((name, MODULE.get(name.lower(), name)))
    return out


missing = [(pkg, mod) for pkg, mod in wanted() if importlib.util.find_spec(mod) is None]
if missing:
    py = os.path.join(os.path.dirname(sys.executable), "python.exe")
    try:
        subprocess.check_call([py, "-m", "pip", "install", "--user", *[p for p, _ in missing]])
        importlib.invalidate_caches()
    except Exception as e:
        if any(m in CORE for _, m in missing):
            import ctypes
            ctypes.windll.user32.MessageBoxW(
                0, f"couldn't install {', '.join(p for p, _ in missing)}:\n{e}\n\nrun in a terminal:\n"
                   f"py -m pip install -r requirements.txt", "Nexus", 0x10)
            sys.exit(1)
        # optional audio-engine packages failed: nexus falls back to the basic player

os.chdir(here)
sys.path.insert(0, here)
import nexus  # noqa: E402

nexus.main()
