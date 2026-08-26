"""
Konnect — Settings Dialog.

wxPython dialog for configuring the MCP server: paths and
server control. Launched from the KiCAD Tools menu via the action plugin.

Settings persist to settings.json in the plugin directory.
"""

import json
import os
import subprocess
import sys

import wx


# ─── Default settings ────────────────────────────────────────────────────────

DEFAULT_SETTINGS = {
    "kicad_cli": "",
    "ipc_socket_path": "",
    "jlcpcb_db_path": "",
    "log_level": "info",
    "transport": "stdio",
}

LOG_LEVELS = ["error", "warn", "info", "debug", "trace"]

TRANSPORTS = ["stdio", "http", "both"]


# ─── Settings I/O ────────────────────────────────────────────────────────────

def load_settings(settings_path):
    """Load settings from JSON, falling back to defaults."""
    settings = dict(DEFAULT_SETTINGS)
    if os.path.exists(settings_path):
        try:
            with open(settings_path, "r") as f:
                saved = json.load(f)
            settings.update(saved)
        except (json.JSONDecodeError, IOError):
            pass
    return settings


def save_settings(settings_path, settings):
    """Write settings to JSON."""
    with open(settings_path, "w") as f:
        json.dump(settings, f, indent=2)


# ─── KiCAD CLI auto-detection ────────────────────────────────────────────────

def detect_kicad_cli():
    """Try to find kicad-cli by scanning common locations, registry, and PATH."""
    binary = "kicad-cli.exe" if sys.platform == "win32" else "kicad-cli"

    if sys.platform == "win32":
        # Check Windows registry for KiCAD install path
        try:
            import winreg

            # Machine-wide install: HKLM\SOFTWARE\KiCad\KiCad, InstallDir.
            for key_path in [
                r"SOFTWARE\KiCad\KiCad",
                r"SOFTWARE\WOW6432Node\KiCad\KiCad",
            ]:
                try:
                    key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, key_path)
                    install_dir, _ = winreg.QueryValueEx(key, "InstallDir")
                    winreg.CloseKey(key)
                    cli = os.path.join(install_dir, "bin", "kicad-cli.exe")
                    if os.path.isfile(cli):
                        return cli
                except (FileNotFoundError, OSError):
                    pass

            # Per-user install (the default when the KiCAD installer is run
            # without admin rights): the uninstall key it writes lives in
            # HKCU (and, for some installer versions, HKLM), keyed by
            # "InstallLocation" rather than "InstallDir".
            for hive in (winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE):
                for version in ["10.0", "9.0", "8.0"]:
                    key_path = (
                        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"
                        r"\KiCad {}".format(version)
                    )
                    try:
                        key = winreg.OpenKey(hive, key_path)
                        install_dir, _ = winreg.QueryValueEx(key, "InstallLocation")
                        winreg.CloseKey(key)
                        cli = os.path.join(install_dir, "bin", "kicad-cli.exe")
                        if os.path.isfile(cli):
                            return cli
                    except (FileNotFoundError, OSError):
                        pass
        except ImportError:
            pass

        # Scan common root directories for KiCad installations. Newest version
        # first, and every root of one version before any root of an older one,
        # so a per-user KiCad 10 wins over a system-wide KiCad 9 — the same
        # order the server uses in install.rs::kicad_standard_paths, so the two
        # never point at different KiCads.
        versions = ["10.0", "9.0", "8.0"]
        roots = ["C:\\Program Files", "C:\\", "D:\\", "D:\\Program Files"]
        local_appdata = os.environ.get("LOCALAPPDATA")
        for ver in versions:
            for root in roots:
                cli = os.path.join(root, "KiCad", ver, "bin", "kicad-cli.exe")
                if os.path.isfile(cli):
                    return cli
            # Per-user install prefix: %LOCALAPPDATA%\Programs\KiCad\<ver>\bin\.
            if local_appdata:
                cli = os.path.join(
                    local_appdata, "Programs", "KiCad", ver, "bin", "kicad-cli.exe"
                )
                if os.path.isfile(cli):
                    return cli

        # Unversioned layout, checked only once every versioned one has missed.
        for root in roots:
            cli = os.path.join(root, "KiCad", "bin", "kicad-cli.exe")
            if os.path.isfile(cli):
                return cli

    elif sys.platform == "darwin":
        candidates = [
            "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli",
            "/usr/local/bin/kicad-cli",
        ]
        for path in candidates:
            if os.path.isfile(path):
                return path
    else:
        candidates = [
            "/usr/bin/kicad-cli",
            "/usr/local/bin/kicad-cli",
            "/snap/kicad/current/usr/bin/kicad-cli",
        ]
        for path in candidates:
            if os.path.isfile(path):
                return path

    # Try PATH as last resort
    try:
        result = subprocess.run(
            [binary, "--version"],
            capture_output=True, text=True, timeout=5,
        )
        if result.returncode == 0:
            return binary
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass

    return ""


# ─── Dialog ──────────────────────────────────────────────────────────────────

class KonnectSettingsDialog(wx.Dialog):
    """Settings dialog for the Konnect plugin."""

    def __init__(self, parent, plugin_dir, binary_path, server_running=False):
        # No fixed pixel size: on high-DPI/scaled Windows displays a fixed
        # (520, 480) clipped the Save/Close buttons off-screen (issue #18).
        # The dialog is sized to fit its content after _build_ui instead.
        super().__init__(
            parent,
            title="Konnect Settings",
            style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER,
        )
        self.plugin_dir = plugin_dir
        self.binary_path = binary_path
        self.settings_path = os.path.join(plugin_dir, "settings.json")
        self.settings = load_settings(self.settings_path)
        self._server_running = server_running

        self._build_ui()
        self._populate_fields()

        # Size to content (DPI-aware), with a sensible minimum width so the
        # path fields aren't cramped. FromDIP scales with the display.
        self.SetMinClientSize(self.FromDIP(wx.Size(520, 0)))
        self.Fit()
        self.CenterOnParent()

    def _build_ui(self):
        panel = wx.Panel(self)
        main_sizer = wx.BoxSizer(wx.VERTICAL)

        # ── Paths section ────────────────────────────────────────────
        paths_box = wx.StaticBoxSizer(wx.VERTICAL, panel, "Paths")
        pgrid = wx.FlexGridSizer(2, 3, 5, 5)
        pgrid.AddGrowableCol(1, 1)

        pgrid.Add(wx.StaticText(panel, label="kicad-cli:"), 0, wx.ALIGN_CENTER_VERTICAL)
        self.kicad_cli_ctrl = wx.TextCtrl(panel)
        pgrid.Add(self.kicad_cli_ctrl, 1, wx.EXPAND)
        self.browse_cli_btn = wx.Button(panel, label="Browse")
        self.browse_cli_btn.Bind(wx.EVT_BUTTON, self._on_browse_cli)
        pgrid.Add(self.browse_cli_btn, 0)

        pgrid.Add(wx.StaticText(panel, label="JLCPCB DB:"), 0, wx.ALIGN_CENTER_VERTICAL)
        self.jlcpcb_ctrl = wx.TextCtrl(panel)
        pgrid.Add(self.jlcpcb_ctrl, 1, wx.EXPAND)
        self.browse_jlcpcb_btn = wx.Button(panel, label="Browse")
        self.browse_jlcpcb_btn.Bind(wx.EVT_BUTTON, self._on_browse_jlcpcb)
        pgrid.Add(self.browse_jlcpcb_btn, 0)

        paths_box.Add(pgrid, 0, wx.EXPAND | wx.ALL, 5)
        main_sizer.Add(paths_box, 0, wx.EXPAND | wx.ALL, 8)

        # ── Advanced section ─────────────────────────────────────────
        adv_box = wx.StaticBoxSizer(wx.VERTICAL, panel, "Advanced")
        agrid = wx.FlexGridSizer(3, 2, 5, 5)
        agrid.AddGrowableCol(1, 1)

        agrid.Add(wx.StaticText(panel, label="IPC Socket:"), 0, wx.ALIGN_CENTER_VERTICAL)
        self.ipc_ctrl = wx.TextCtrl(panel)
        agrid.Add(self.ipc_ctrl, 1, wx.EXPAND)

        agrid.Add(wx.StaticText(panel, label="Log Level:"), 0, wx.ALIGN_CENTER_VERTICAL)
        self.log_level_ctrl = wx.Choice(panel, choices=LOG_LEVELS)
        agrid.Add(self.log_level_ctrl, 1, wx.EXPAND)

        agrid.Add(wx.StaticText(panel, label="Transport:"), 0, wx.ALIGN_CENTER_VERTICAL)
        self.transport_ctrl = wx.Choice(panel, choices=TRANSPORTS)
        agrid.Add(self.transport_ctrl, 1, wx.EXPAND)

        adv_box.Add(agrid, 0, wx.EXPAND | wx.ALL, 5)
        main_sizer.Add(adv_box, 0, wx.EXPAND | wx.ALL, 8)

        # ── Server status section ────────────────────────────────────
        server_box = wx.StaticBoxSizer(wx.HORIZONTAL, panel, "Server")
        self.server_status = wx.StaticText(panel, label="Stopped")
        server_box.Add(self.server_status, 1, wx.ALIGN_CENTER_VERTICAL | wx.LEFT, 5)
        self.start_stop_btn = wx.Button(panel, label="Start Server")
        self.start_stop_btn.Bind(wx.EVT_BUTTON, self._on_start_stop)
        server_box.Add(self.start_stop_btn, 0, wx.ALL, 5)
        main_sizer.Add(server_box, 0, wx.EXPAND | wx.ALL, 8)

        # ── Bottom buttons ───────────────────────────────────────────
        btn_sizer = wx.StdDialogButtonSizer()
        self.save_btn = wx.Button(panel, wx.ID_SAVE, "Save")
        self.save_btn.Bind(wx.EVT_BUTTON, self._on_save)
        btn_sizer.AddButton(self.save_btn)

        close_btn = wx.Button(panel, wx.ID_CLOSE, "Close")
        close_btn.Bind(wx.EVT_BUTTON, self._on_close)
        btn_sizer.AddButton(close_btn)
        btn_sizer.Realize()

        main_sizer.Add(btn_sizer, 0, wx.ALIGN_RIGHT | wx.ALL, 8)

        panel.SetSizer(main_sizer)

        # Dialog-level sizer so Fit() sizes the dialog to the panel's content.
        dlg_sizer = wx.BoxSizer(wx.VERTICAL)
        dlg_sizer.Add(panel, 1, wx.EXPAND)
        self.SetSizer(dlg_sizer)

        self._update_server_status()

    def _populate_fields(self):
        """Fill dialog fields from loaded settings."""
        cli = self.settings.get("kicad_cli", "")
        if not cli:
            cli = detect_kicad_cli()
        self.kicad_cli_ctrl.SetValue(cli)

        self.jlcpcb_ctrl.SetValue(self.settings.get("jlcpcb_db_path", ""))
        ipc_path = self.settings.get("ipc_socket_path", "")
        if not ipc_path:
            ipc_path = os.environ.get("KICAD_API_SOCKET", "")
        self.ipc_ctrl.SetValue(ipc_path)

        level = self.settings.get("log_level", "info")
        if level in LOG_LEVELS:
            self.log_level_ctrl.SetSelection(LOG_LEVELS.index(level))
        else:
            self.log_level_ctrl.SetSelection(2)  # "info"

        transport = self.settings.get("transport", "stdio")
        if transport in TRANSPORTS:
            self.transport_ctrl.SetSelection(TRANSPORTS.index(transport))
        else:
            self.transport_ctrl.SetSelection(0)  # "stdio"

    def _collect_settings(self):
        """Read current field values into a settings dict."""
        return {
            "kicad_cli": self.kicad_cli_ctrl.GetValue().strip(),
            "ipc_socket_path": self.ipc_ctrl.GetValue().strip(),
            "jlcpcb_db_path": self.jlcpcb_ctrl.GetValue().strip(),
            "log_level": LOG_LEVELS[self.log_level_ctrl.GetSelection()],
            "transport": TRANSPORTS[self.transport_ctrl.GetSelection()],
        }

    def _update_server_status(self):
        """Update the server status display."""
        if self._server_running:
            self.server_status.SetLabel("Running")
            self.server_status.SetForegroundColour(wx.Colour(0, 128, 0))
            self.start_stop_btn.SetLabel("Stop Server")
        else:
            self.server_status.SetLabel("Stopped")
            self.server_status.SetForegroundColour(wx.Colour(180, 0, 0))
            self.start_stop_btn.SetLabel("Start Server")

    # ── Event handlers ───────────────────────────────────────────────

    def _on_browse_cli(self, event):
        wildcard = "Executables (*.exe)|*.exe|All files (*)|*" if sys.platform == "win32" else "All files (*)|*"
        dlg = wx.FileDialog(self, "Select kicad-cli", wildcard=wildcard, style=wx.FD_OPEN)
        if dlg.ShowModal() == wx.ID_OK:
            self.kicad_cli_ctrl.SetValue(dlg.GetPath())
        dlg.Destroy()

    def _on_browse_jlcpcb(self, event):
        dlg = wx.FileDialog(self, "Select JLCPCB database", wildcard="SQLite (*.db)|*.db|All files (*)|*", style=wx.FD_OPEN)
        if dlg.ShowModal() == wx.ID_OK:
            self.jlcpcb_ctrl.SetValue(dlg.GetPath())
        dlg.Destroy()

    def _on_save(self, event):
        self.settings = self._collect_settings()
        save_settings(self.settings_path, self.settings)
        wx.MessageBox("Settings saved.", "Konnect", wx.OK | wx.ICON_INFORMATION)

    def _on_start_stop(self, event):
        """Toggle server — delegates to parent plugin via return value."""
        self.settings = self._collect_settings()
        save_settings(self.settings_path, self.settings)
        self.EndModal(wx.ID_YES if not self._server_running else wx.ID_NO)

    def _on_close(self, event):
        self.EndModal(wx.ID_CLOSE)
