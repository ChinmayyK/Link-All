using System;
using System.IO;
using Microsoft.Win32;

namespace LinkAll.WinUI.Services
{
    // The app was Deskdrop before it became Link All. On the first run after
    // the update this carries the old install's state over under the new
    // names, so nothing the user set up is lost or left pointing at files the
    // installer removed. Every step is safe to run again.
    public static class RenameMigration
    {
        private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";

        public static void Run()
        {
            MoveSettingsFolder();
            MoveSignInEntry();
            RemoveOldShellRegistrations();
        }

        // %LocalAppData%\Deskdrop holds settings, the clipboard cache and logs.
        private static void MoveSettingsFolder()
        {
            try
            {
                var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
                var oldDir = Path.Combine(local, "Deskdrop");
                var newDir = Path.Combine(local, "LinkAll");
                if (Directory.Exists(oldDir) && !Directory.Exists(newDir))
                    Directory.Move(oldDir, newDir);
            }
            catch (Exception) { /* keep going; the app starts with defaults */ }
        }

        // The old entry starts Deskdrop.exe from a folder the update removed.
        private static void MoveSignInEntry()
        {
            try
            {
                using var key = Registry.CurrentUser.OpenSubKey(RunKey, writable: true);
                if (key?.GetValue("Deskdrop") == null) return;
                key.DeleteValue("Deskdrop", throwOnMissingValue: false);
                var exe = Environment.ProcessPath;
                if (!string.IsNullOrEmpty(exe) && key.GetValue("LinkAll") == null)
                    key.SetValue("LinkAll", $"\"{exe}\" --background");
            }
            catch (Exception) { }
        }

        // "Send via" menu entries and the deskdrop:// handler; the app
        // registers the new ones itself at startup.
        private static void RemoveOldShellRegistrations()
        {
            foreach (var path in new[]
            {
                @"Software\Classes\*\shell\Deskdrop",
                @"Software\Classes\Directory\shell\Deskdrop",
                @"Software\Classes\deskdrop",
            })
            {
                try { Registry.CurrentUser.DeleteSubKeyTree(path, throwOnMissingSubKey: false); }
                catch (Exception) { }
            }
        }
    }
}
