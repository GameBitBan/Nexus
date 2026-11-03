# double-click to run Nexus on windows (no console window).
# runs through pythonw.exe, so smart app control doesn't need to approve anything.
# on every start it installs anything new listed in requirements.txt (so updates can add packages).
# if Nexus can't start, it says why in a message box and writes %APPDATA%\nexus\crash.log
#   py Nexus.pyw --check   → checks every package loads and prints versions (used by the installer)
import sys

if hasattr(sys, "set_lazy_imports_filter"):      # python 3.15: lazy imports + PySide6 don't mix (see nexus.py)
    sys.set_lazy_imports_filter(lambda *_a: False)

import importlib.util  # noqa: E402
import os  # noqa: E402
import re  # noqa: E402
import subprocess  # noqa: E402
import traceback  # noqa: E402

here = os.path.dirname(os.path.abspath(__file__))
MODULE = {"pyside6": "PySide6", "mutagen": "mutagen", "numpy": "numpy", "av": "av", "sounddevice": "sounddevice",
          "aioslsk": "aioslsk", "aiortc": "aiortc"}
CORE = {"PySide6", "mutagen"}
CHECK = "--check" in sys.argv
LOG_DIR = os.path.join(os.environ.get("APPDATA") or os.path.expanduser("~"), "nexus")
LOG = os.path.join(LOG_DIR, "crash.log")


def log(text):
    try:
        os.makedirs(LOG_DIR, exist_ok=True)
        with open(LOG, "a", encoding="utf-8") as f:
            import datetime
            f.write(f"\n==== {datetime.datetime.now():%Y-%m-%d %H:%M:%S}  python {sys.version.split()[0]} "
                    f"{sys.executable}\n{text}\n")
    except OSError:
        pass


def tell(text, error=True):
    if CHECK or sys.stdout is not None and not sys.executable.lower().endswith("pythonw.exe"):
        print(text)
        return
    try:
        import ctypes
        ctypes.windll.user32.MessageBoxW(0, text, "Nexus", 0x10 if error else 0x40)
    except Exception:
        pass


def wanted():
    try:
        lines = open(os.path.join(here, "requirements.txt"), encoding="utf-8").read().splitlines()
    except OSError:
        lines = ["PySide6", "mutagen"]
    out = []
    for ln in lines:
        name = re.split(r"[<>=!~ ;\[]", ln.strip(), maxsplit=1)[0]
        if name and not name.startswith("#"):
            out.append((name, MODULE.get(name.lower(), name)))
    return out


missing = [(pkg, mod) for pkg, mod in wanted() if importlib.util.find_spec(mod) is None]
if missing:
    py = os.path.join(os.path.dirname(sys.executable), "python.exe")
    r = subprocess.run([py, "-m", "pip", "install", "--user", *[p for p, _ in missing]],
                       capture_output=True, text=True, creationflags=0x08000000 if os.name == "nt" else 0)
    importlib.invalidate_caches()
    if r.returncode != 0:
        log("pip install failed:\n" + (r.stdout or "")[-4000:] + (r.stderr or "")[-4000:])
        if any(m in CORE for _, m in missing):
            tell(f"Nexus couldn't install {', '.join(p for p, _ in missing)}.\n\n"
                 f"{(r.stderr or r.stdout or '').strip()[-600:]}\n\n"
                 f"Details: {LOG}\nTry in PowerShell:  py -m pip install --user -r \"{here}\\requirements.txt\"")
            sys.exit(1)
        # optional audio-engine packages failed: nexus falls back to the basic player

os.chdir(here)
sys.path.insert(0, here)
try:
    import nexus  # noqa: E402
    if CHECK:
        import PySide6
        from PySide6 import QtWidgets  # noqa: F401  (loads the Qt DLLs)
        print(f"nexus {nexus.VERSION} · python {sys.version.split()[0]} · PySide6 {PySide6.__version__}")
        for _pkg, mod in wanted():
            try:
                m = importlib.import_module(mod)
                print(f"  ok    {mod} {getattr(m, '__version__', '')}")
            except Exception as e:
                print(f"  FAIL  {mod}: {type(e).__name__}: {e}")
        ok, why = nexus.engine_available()
        print("  audio engine: " + ("ok" if ok else f"basic player ({why})"))
        sys.exit(0)
    nexus.main()
except SystemExit:
    raise
except BaseException as e:
    tb = traceback.format_exc()
    log(tb)
    tell(f"Nexus couldn't start:\n\n{type(e).__name__}: {e}\n\nThe full error is in\n{LOG}\n"
         f"(send that file to whoever gave you Nexus).")
    sys.exit(1)
