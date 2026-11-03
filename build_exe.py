"""builds the Windows app with PyInstaller:  py build_exe.py
output: dist\\Nexus\\Nexus.exe (keep the whole dist\\Nexus folder together). the release workflow then wraps it
into NexusSetup.exe with Inno Setup (installer.iss) and signs both when a certificate is set up.

heads up: Smart App Control blocks unsigned .exe files it hasn't seen before, and a fresh build always counts
as unseen. until the exe is signed, "Install Nexus.pyw" / install.ps1 is the way to install with SAC on."""
import os
import re
import subprocess
import sys
from pathlib import Path

here = Path(__file__).resolve().parent
os.chdir(here)
version = re.search(r'^VERSION = "([^"]+)"', (here / "nexus.py").read_text("utf-8"), re.M).group(1)
nums = [int(x) for x in re.findall(r"\d+", version)[:4]] + [0, 0, 0, 0]
vt = tuple(nums[:4])

if "--no-install" not in sys.argv:
    subprocess.check_call([sys.executable, "-m", "pip", "install", "pyinstaller>=6.10", "-r", "requirements.txt"])

# windows file properties (right-click → properties → details), which signing services also check
(here / "build").mkdir(exist_ok=True)
vfile = here / "build" / "version_info.txt"
vfile.write_text(f"""VSVersionInfo(
  ffi=FixedFileInfo(filevers={vt}, prodvers={vt}, mask=0x3f, flags=0x0, OS=0x40004, fileType=0x1, subtype=0x0,
                    date=(0, 0)),
  kids=[StringFileInfo([StringTable('040904B0', [
      StringStruct('CompanyName', 'GameBitBan'),
      StringStruct('FileDescription', 'Nexus music player'),
      StringStruct('FileVersion', '{version}'),
      StringStruct('InternalName', 'Nexus'),
      StringStruct('LegalCopyright', 'GPL-3.0 - GameBitBan'),
      StringStruct('OriginalFilename', 'Nexus.exe'),
      StringStruct('ProductName', 'Nexus'),
      StringStruct('ProductVersion', '{version}')])]),
    VarFileInfo([VarStruct('Translation', [1033, 1200])])])
""", "utf-8")

sep = os.pathsep
args = [
    sys.executable, "-m", "PyInstaller", "--noconfirm", "--clean", "--windowed",
    "--name", "Nexus",
    "--icon", str(here / "icons" / ("nexus.ico" if os.name == "nt" else "nexus-256.png")),
    "--add-data", f"fonts{sep}fonts",
    "--add-data", f"icons{sep}icons",
    "--add-data", f"skins{sep}skins",
    "--add-data", f"LICENSE{sep}.",
    "--add-data", f"PRIVACY.md{sep}.",
    "--add-data", f"README.md{sep}.",
    "--hidden-import", "PySide6.QtMultimedia",
    "--hidden-import", "PySide6.QtNetwork",
    "--collect-all", "aioslsk",
    "--collect-all", "aiortc",
    "--collect-all", "aioice",
    "--collect-all", "pylibsrtp",
    "--collect-submodules", "av",
    "--collect-data", "_sounddevice_data",
    "--exclude-module", "tkinter",
]
if os.name == "nt":
    args += ["--version-file", str(vfile)]
args.append("nexus.py")
subprocess.check_call(args)
print("\nbuilt:", here / "dist" / "Nexus", "· version", version)
