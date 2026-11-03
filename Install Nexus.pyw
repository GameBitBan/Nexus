# double-click to install Nexus on windows.
#  - installs PySide6 + mutagen (+ comtypes for the shortcut) for your user
#  - copies the app to %LOCALAPPDATA%\Programs\Nexus
#  - makes Desktop + Start Menu shortcuts with the Nexus icon (right-click → pin to taskbar)
# runs through pythonw.exe (signed), so Smart App Control has nothing to block.
#   --yes        no questions (used by the one-line install.ps1)
#   --uninstall  removes the app + shortcuts (your library and settings stay)
import ctypes
import importlib.util
import os
import shutil
import subprocess
import sys
from pathlib import Path

APP_ID = "Nexus.MusicPlayer"
SRC = Path(__file__).resolve().parent
DEST = Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local")) / "Programs" / "Nexus"


QUIET = "--yes" in sys.argv


def msg(text, title="Nexus", flags=0x40):
    if QUIET:
        print(text)
        return 1 if flags & 0x0F in (0x01,) else 6
    return ctypes.windll.user32.MessageBoxW(0, text, title, flags)


def known_folder(csidl):
    buf = ctypes.create_unicode_buffer(260)
    ctypes.windll.shell32.SHGetFolderPathW(0, csidl, 0, 0, buf)
    return Path(buf.value)


def pythonw():
    exe = Path(sys.executable)
    w = exe.with_name("pythonw.exe")
    return w if w.exists() else exe


def install_deps():
    need = [m for m, mod in (("PySide6", "PySide6"), ("mutagen", "mutagen"), ("comtypes", "comtypes"),
                             ("numpy", "numpy"), ("av", "av"), ("sounddevice", "sounddevice"),
                             ("aioslsk", "aioslsk"))
            if importlib.util.find_spec(mod) is None]
    if not need:
        return
    py = Path(sys.executable).with_name("python.exe")
    subprocess.check_call([str(py if py.exists() else sys.executable), "-m", "pip", "install", "--user", *need])
    importlib.invalidate_caches()
    import site
    up = site.getusersitepackages()
    if up not in sys.path:
        sys.path.append(up)


def copy_app():
    DEST.mkdir(parents=True, exist_ok=True)
    for name in ("nexus.py", "Nexus.pyw", "README.md", "requirements.txt", "Install Nexus.pyw", "LICENSE"):
        if (SRC / name).exists():
            shutil.copy2(SRC / name, DEST / name)
    for folder in ("fonts", "icons", "skins"):
        if (SRC / folder).is_dir():
            shutil.copytree(SRC / folder, DEST / folder, dirs_exist_ok=True)


def make_link_comtypes(path, target, args, workdir, icon):
    import comtypes
    import comtypes.client
    from comtypes import COMMETHOD, GUID, HRESULT, IUnknown
    from comtypes.persist import IPersistFile
    from comtypes.shelllink import IShellLinkW, ShellLink
    from ctypes import POINTER, byref, wintypes

    class PROPERTYKEY(ctypes.Structure):
        _fields_ = [("fmtid", GUID), ("pid", wintypes.DWORD)]

    class PROPVARIANT(ctypes.Structure):
        _fields_ = [("vt", ctypes.c_ushort), ("r1", ctypes.c_ushort), ("r2", ctypes.c_ushort),
                    ("r3", ctypes.c_ushort), ("pwszVal", ctypes.c_wchar_p), ("pad", ctypes.c_void_p)]

    class IPropertyStore(IUnknown):
        _iid_ = GUID("{886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99}")
        _methods_ = [
            COMMETHOD([], HRESULT, "GetCount", (["out"], POINTER(wintypes.DWORD), "c")),
            COMMETHOD([], HRESULT, "GetAt", (["in"], wintypes.DWORD, "i"), (["out"], POINTER(PROPERTYKEY), "k")),
            COMMETHOD([], HRESULT, "GetValue", (["in"], POINTER(PROPERTYKEY), "k"), (["out"], POINTER(PROPVARIANT), "v")),
            COMMETHOD([], HRESULT, "SetValue", (["in"], POINTER(PROPERTYKEY), "k"), (["in"], POINTER(PROPVARIANT), "v")),
            COMMETHOD([], HRESULT, "Commit"),
        ]

    link = comtypes.client.CreateObject(ShellLink, interface=IShellLinkW)
    link.SetPath(str(target))
    link.SetArguments(args)
    link.SetWorkingDirectory(str(workdir))
    link.SetIconLocation(str(icon), 0)
    link.SetDescription("Nexus — local lossless music player")
    aumid_ok = True
    try:
        store = link.QueryInterface(IPropertyStore)
        key = PROPERTYKEY(GUID("{9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}"), 5)   # PKEY_AppUserModel_ID
        pv = PROPVARIANT()
        pv.vt = 31                                                             # VT_LPWSTR
        pv.pwszVal = APP_ID
        store.SetValue(byref(key), byref(pv))
        store.Commit()
    except Exception:
        aumid_ok = False
    link.QueryInterface(IPersistFile).Save(str(path), True)
    return aumid_ok


def make_link_powershell(path, target, args, workdir, icon):
    esc = lambda s: str(s).replace("'", "''")
    cmd = (f"$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{esc(path)}');"
           f"$s.TargetPath='{esc(target)}';$s.Arguments='{esc(args)}';"
           f"$s.WorkingDirectory='{esc(workdir)}';$s.IconLocation='{esc(icon)},0';$s.Save()")
    subprocess.check_call(["powershell", "-NoProfile", "-Command", cmd], creationflags=0x08000000)
    return False


def make_shortcuts():
    target = pythonw()
    args = f'"{DEST / "Nexus.pyw"}"'
    icon = DEST / "icons" / "nexus.ico"
    places = [known_folder(0x10) / "Nexus.lnk",            # desktop
              known_folder(0x02) / "Nexus.lnk"]            # start menu → programs
    aumid_everywhere, made = True, []
    for lnk in places:
        lnk.parent.mkdir(parents=True, exist_ok=True)
        try:
            ok = make_link_comtypes(lnk, target, args, DEST, icon)
        except Exception:
            ok = make_link_powershell(lnk, target, args, DEST, icon)
        aumid_everywhere &= ok
        made.append(lnk)
    flag = DEST / "no_aumid"
    if aumid_everywhere:
        flag.unlink(missing_ok=True)
    else:
        flag.write_text("shortcuts have no app id; app keeps python's so pinning groups correctly\n")
    return made


def app_version():
    import re
    try:
        m = re.search(r'^VERSION = "([^"]+)"', (DEST / "nexus.py").read_text("utf-8"), re.M)
        return m.group(1) if m else ""
    except OSError:
        return ""


UNINSTALL_KEY = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Nexus"


def register_uninstall():
    """shows Nexus in Settings → Apps → Installed apps, with a working Uninstall button."""
    import winreg
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY) as k:
        vals = {"DisplayName": "Nexus", "DisplayVersion": app_version(), "Publisher": "Nexus",
                "DisplayIcon": str(DEST / "icons" / "nexus.ico"), "InstallLocation": str(DEST),
                "UninstallString": f'"{pythonw()}" "{DEST / "Install Nexus.pyw"}" --uninstall',
                "QuietUninstallString": f'"{pythonw()}" "{DEST / "Install Nexus.pyw"}" --uninstall --yes'}
        for n, v in vals.items():
            winreg.SetValueEx(k, n, 0, winreg.REG_SZ, v)
        winreg.SetValueEx(k, "NoModify", 0, winreg.REG_DWORD, 1)
        winreg.SetValueEx(k, "NoRepair", 0, winreg.REG_DWORD, 1)


def uninstall():
    if msg("Remove Nexus from this PC?\n\nYour library, playlists, skins and settings stay "
           "(in %APPDATA%\\nexus) in case you reinstall.", "Uninstall Nexus", 0x21) != 1:
        return
    import winreg
    for lnk in (known_folder(0x10) / "Nexus.lnk", known_folder(0x02) / "Nexus.lnk"):
        try:
            lnk.unlink()
        except OSError:
            pass
    try:
        winreg.DeleteKey(winreg.HKEY_CURRENT_USER, UNINSTALL_KEY)
    except OSError:
        pass
    # the firewall rule needs admin to remove; leave it (it only covers python on Nexus's ports)
    # the folder holds this very script: delete it a moment after we exit
    subprocess.Popen(["cmd", "/c", "timeout", "/t", "2", ">nul", "&", "rmdir", "/s", "/q", str(DEST)],
                     creationflags=0x08000000, cwd=str(Path.home()))
    msg("Nexus was removed.")


def allow_firewall(port=2234):
    """one admin prompt: lets other Soulseek users connect to Nexus (only its two download ports)."""
    exe_dir = Path(sys.executable).parent
    progs = [str(p) for p in dict.fromkeys((exe_dir / "pythonw.exe", exe_dir / "python.exe")) if Path(p).exists()]
    name = "Nexus Soulseek"
    cmds = [f'netsh advfirewall firewall delete rule name="{name}"']
    cmds += [f'netsh advfirewall firewall add rule name="{name}" dir=in action=allow program="{p}" protocol=TCP '
             f'localport={port},{port + 1} enable=yes profile=any' for p in progs]
    return ctypes.windll.shell32.ShellExecuteW(None, "runas", "cmd.exe", "/c " + " & ".join(cmds), None, 0) > 32


def firewall_rule_exists():
    try:
        r = subprocess.run(["netsh", "advfirewall", "firewall", "show", "rule", "name=Nexus Soulseek"],
                           capture_output=True, text=True, timeout=8, creationflags=0x08000000)
        return r.returncode == 0
    except Exception:
        return True


def main():
    if "--uninstall" in sys.argv:
        return uninstall()
    if msg(f"Install Nexus to\n{DEST}\n\nand add Desktop + Start Menu shortcuts?", "Install Nexus", 0x21) != 1:
        return
    try:
        install_deps()
    except Exception as e:
        msg(f"couldn't install python packages:\n{e}\n\nopen a terminal and run:\npy -m pip install PySide6 mutagen comtypes",
            "Nexus", 0x10)
        return
    try:
        copy_app()
        links = make_shortcuts()
        try:
            register_uninstall()
        except Exception:
            pass
        if not firewall_rule_exists():
            print("Windows will ask to let Nexus through the firewall (for Soulseek downloads) - click Yes.")
            try:
                allow_firewall()
            except Exception:
                pass
    except Exception as e:
        msg(f"install failed:\n{e}", "Nexus", 0x10)
        return
    if QUIET:
        print("Nexus installed:", *links, sep="\n  ")
        subprocess.Popen([str(pythonw()), str(DEST / "Nexus.pyw")], cwd=str(DEST))
    elif msg("Nexus is installed.\n\n" + "\n".join(str(l) for l in links) +
             "\n\nTo pin it: right-click the desktop icon → Show more options → Pin to taskbar.\n\nOpen Nexus now?",
             "Nexus", 0x44) == 6:
        subprocess.Popen([str(pythonw()), str(DEST / "Nexus.pyw")], cwd=str(DEST))


if __name__ == "__main__":
    main()
