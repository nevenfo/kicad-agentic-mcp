"""
Konnect -- Action Plugin launcher.

This thin Python plugin registers a menu item in KiCAD's Tools menu.
Clicking it opens a settings dialog where users can configure paths
and start/stop the compiled Rust MCP server binary.

Installation (KiCAD PCM):
  The plugin appears in KiCAD's Plugin and Content Manager.
  After installation the binary is placed in the plugin directory.
  KiCAD loads this __init__.py automatically.
"""

import os
import shutil
import subprocess
import sys
import threading

import pcbnew  # Available inside KiCAD

# Import the settings dialog (same directory)
_plugin_dir = os.path.dirname(os.path.abspath(__file__))
if _plugin_dir not in sys.path:
    sys.path.insert(0, _plugin_dir)

from settings_dialog import KonnectSettingsDialog, load_settings
import wx

PLUGIN_DIR = _plugin_dir
BINARY_NAME = "konnect.exe" if sys.platform == "win32" else "konnect"
BINARY_PATH = os.path.join(PLUGIN_DIR, "bin", BINARY_NAME)
SETTINGS_PATH = os.path.join(PLUGIN_DIR, "settings.json")

_server_process = None
_server_thread = None

# Windows GUI parents (pcbnew) attach a new console to their subprocesses.
# That console makes stdin look like a TTY, which triggers konnect's
# double-click install wizard instead of MCP server mode. CREATE_NO_WINDOW
# suppresses the console; stdin=PIPE stays intact.
_POPEN_FLAGS = subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0

_CACHE_DIR = os.path.join(
    os.environ.get("LOCALAPPDATA", os.path.expanduser("~/.cache")),
    "konnect", "cache",
)
_PID_FILE = os.path.join(_CACHE_DIR, "server.pid")


def _stage(source_path):
    """Copy file to LOCALAPPDATA cache, return cached path.

    OneDrive-redirected Documents (common in enterprise Windows) tag both
    the plugin's konnect.exe AND settings.json with IO_REPARSE_TAG_CLOUD,
    which trips ERROR_ACCESS_DENIED on execute and on serde config read.
    Staging to %LOCALAPPDATA% (never OneDrive-synced) sidesteps it.
    """
    if sys.platform != "win32":
        return source_path
    os.makedirs(_CACHE_DIR, exist_ok=True)
    dst = os.path.join(_CACHE_DIR, os.path.basename(source_path))
    shutil.copy2(source_path, dst)
    return dst


def _kill_tracked():
    """Terminate the PID recorded in the PID file (best-effort), clear the file.

    os.kill(pid, 0) is unsafe on Windows -- it invokes TerminateProcess for any
    signal value, so it would actually kill the process instead of probing.
    Use taskkill/kill by PID; if the PID is stale, the kill is a harmless no-op.
    """
    try:
        with open(_PID_FILE) as f:
            pid = int(f.read().strip())
    except (OSError, ValueError):
        return
    try:
        if sys.platform == "win32":
            subprocess.run(
                ["taskkill", "/F", "/PID", str(pid)],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                creationflags=_POPEN_FLAGS,
            )
        else:
            os.kill(pid, 9)
    except OSError:
        pass
    try:
        os.remove(_PID_FILE)
    except OSError:
        pass


def _run_server():
    """Run the MCP server subprocess. Exit means exit -- no retry loop.

    A retry loop respawns konnect after Stop Server kills it (wait() returns
    normally on external kill, so nothing distinguishes "crashed" from
    "stopped"). If konnect crashes on startup, the user clicks Start again.
    """
    global _server_process
    try:
        # This SWIG launcher exists only inside pcbnew, so carry that context
        # explicitly instead of making the server guess a document type.
        args = [_stage(BINARY_PATH), "--document-type", "pcb"]
        if os.path.exists(SETTINGS_PATH):
            args += ["--config", _stage(SETTINGS_PATH)]
        # pcbnew has no valid stderr; inheriting gives konnect a broken
        # handle and tracing_subscriber errors on init. DEVNULL is a
        # valid sink.
        _server_process = subprocess.Popen(
            args,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            creationflags=_POPEN_FLAGS,
        )
        os.makedirs(_CACHE_DIR, exist_ok=True)
        with open(_PID_FILE, "w") as f:
            f.write(str(_server_process.pid))
        _server_process.wait()
    except Exception as e:
        print(f"[Konnect] Server error: {e}", file=sys.stderr)
    finally:
        _server_process = None
        try:
            os.remove(_PID_FILE)
        except OSError:
            pass


def start_server():
    """Start the MCP server in a background thread."""
    global _server_thread

    if not os.path.exists(BINARY_PATH):
        wx.MessageBox(
            f"Konnect binary not found at:\n{BINARY_PATH}\n\n"
            "Please reinstall the plugin.",
            "Konnect",
            wx.OK | wx.ICON_ERROR,
        )
        return False

    # Orphan from a previous KiCAD session may still be holding the port.
    _kill_tracked()

    if _server_thread and _server_thread.is_alive():
        return True

    _server_thread = threading.Thread(target=_run_server, daemon=True)
    _server_thread.start()
    return True


def stop_server():
    """Stop the MCP server subprocess."""
    _kill_tracked()


def is_server_running():
    """Report whether we've launched a server we haven't stopped.

    PID file existence as a proxy -- survives module re-imports. If the
    server crashed on its own, the next start_server preflight cleans up.
    """
    return os.path.exists(_PID_FILE)


# ─── KiCAD Action Plugin entry point ─────────────────────────────────────────

class KonnectPlugin(pcbnew.ActionPlugin):
    def defaults(self):
        self.name = "Konnect"
        self.category = "AI Tools"
        self.description = (
            "Configure and control the Konnect -- enables AI assistants "
            "like Claude to design PCBs and schematics via the Model Context Protocol."
        )
        self.show_toolbar_button = True
        self.icon_file_name = os.path.join(PLUGIN_DIR, "resources", "icon.png")

    def Run(self):
        """Open the settings dialog. Server start/stop is handled via dialog buttons."""
        # Get the KiCAD main window as parent for the dialog
        parent = wx.GetTopLevelWindows()[0] if wx.GetTopLevelWindows() else None

        dlg = KonnectSettingsDialog(
            parent=parent,
            plugin_dir=PLUGIN_DIR,
            binary_path=BINARY_PATH,
            server_running=is_server_running(),
        )

        result = dlg.ShowModal()

        if result == wx.ID_YES:
            # User clicked "Start Server"
            start_server()
        elif result == wx.ID_NO:
            # User clicked "Stop Server"
            stop_server()

        dlg.Destroy()


# Register the plugin with KiCAD
KonnectPlugin().register()
