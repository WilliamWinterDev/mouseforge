# Installs MouseForge for the current user (no admin needed):
#   - copies the release build to %LOCALAPPDATA%\Programs\MouseForge
#   - creates a Start Menu shortcut carrying the app's AppUserModelID,
#     which Windows requires before it will show the app's notifications
#   - registers "start with Windows" (minimised to the tray)
# Usage: powershell -ExecutionPolicy Bypass -File install.ps1 [path\to\mouseforge.exe]
param([string]$Source = "$PSScriptRoot\src-tauri\target\release\mouseforge.exe")

$ErrorActionPreference = 'Stop'
$AppId = 'com.mouseforge.app'
$dest = Join-Path $env:LOCALAPPDATA 'Programs\MouseForge'
$exe = Join-Path $dest 'MouseForge.exe'

Get-Process mouseforge -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500
New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item $Source $exe -Force

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;

public static class AumidShortcut {
    [ComImport, Guid("000214F9-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IShellLinkW {
        void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder f, int c, IntPtr fd, uint fl);
        void GetIDList(out IntPtr p); void SetIDList(IntPtr p);
        void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder s, int c);
        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string s);
        void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder s, int c);
        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string s);
        void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder s, int c);
        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string s);
        void GetHotkey(out short k); void SetHotkey(short k);
        void GetShowCmd(out int c); void SetShowCmd(int c);
        void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder s, int c, out int i);
        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string s, int i);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string s, uint r);
        void Resolve(IntPtr h, uint f);
        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string s);
    }
    [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IPropertyStore {
        void GetCount(out uint c); void GetAt(uint i, out PropertyKey k);
        void GetValue(ref PropertyKey k, out PropVariant v);
        void SetValue(ref PropertyKey k, ref PropVariant v);
        void Commit();
    }
    [StructLayout(LayoutKind.Sequential, Pack = 4)]
    struct PropertyKey { public Guid fmtid; public uint pid; }
    [StructLayout(LayoutKind.Sequential)]
    struct PropVariant { public ushort vt; ushort r1, r2, r3; public IntPtr p; IntPtr p2; }
    [ComImport, Guid("00021401-0000-0000-C000-000000000046")] class ShellLink {}

    public static void Create(string lnk, string target, string appId) {
        var link = (IShellLinkW)new ShellLink();
        link.SetPath(target);
        link.SetWorkingDirectory(System.IO.Path.GetDirectoryName(target));
        link.SetDescription("MouseForge mouse control");
        var key = new PropertyKey { fmtid = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), pid = 5 }; // System.AppUserModel.ID
        var val = new PropVariant { vt = 31 /* VT_LPWSTR */, p = Marshal.StringToCoTaskMemUni(appId) };
        var store = (IPropertyStore)link;
        store.SetValue(ref key, ref val);
        store.Commit();
        Marshal.FreeCoTaskMem(val.p);
        ((IPersistFile)link).Save(lnk, true);
    }
}
'@

$lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\MouseForge.lnk'
[AumidShortcut]::Create($lnk, $exe, $AppId)

Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'MouseForge' -Value "`"$exe`" --minimized"
Start-Process $exe
"Installed $exe"
"Shortcut  $lnk"
