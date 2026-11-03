"""builds a real Nexus.exe with PyInstaller.   run:  py build_exe.py
output: dist\\Nexus\\Nexus.exe  (keep the whole dist\\Nexus folder together)

heads up: Smart App Control blocks unsigned .exe files it hasn't seen before, and a fresh
build always counts as unseen. if Windows blocks Nexus.exe, use "Install Nexus.pyw" instead —
it gives the same desktop/taskbar icon without needing an exe."""
import os
import subprocess
import sys
from pathlib import Path

here = Path(__file__).resolve().parent
os.chdir(here)
subprocess.check_call([sys.executable, "-m", "pip", "install", "--user", "pyinstaller", "PySide6", "mutagen"])
sep = os.pathsep
args = [
    sys.executable, "-m", "PyInstaller", "--noconfirm", "--clean", "--windowed",
    "--name", "Nexus",
    "--icon", str(here / "icons" / ("nexus.ico" if os.name == "nt" else "nexus-256.png")),
    "--add-data", f"fonts{sep}fonts",
    "--add-data", f"icons{sep}icons",
    "--add-data", f"skins{sep}skins",
    "nexus.py",
]
subprocess.check_call(args)
print("\nbuilt:", here / "dist" / "Nexus")
