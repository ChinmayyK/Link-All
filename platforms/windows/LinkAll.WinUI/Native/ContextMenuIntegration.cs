using Microsoft.Win32;
using System;
using System.Diagnostics;
using System.IO;

namespace LinkAll.WinUI.Native
{
    public static class ContextMenuIntegration
    {
        public static void RegisterContextMenu()
        {
            try
            {
                string exePath = Process.GetCurrentProcess().MainModule?.FileName ?? "";
                if (string.IsNullOrEmpty(exePath)) return;

                // Register for all files
                using (RegistryKey key = Registry.CurrentUser.CreateSubKey(@"Software\Classes\*\shell\LinkAll"))
                {
                    key.SetValue("", "Send via Link All");
                    key.SetValue("Icon", $"\"{exePath}\",0");
                    using (RegistryKey commandKey = key.CreateSubKey("command"))
                    {
                        commandKey.SetValue("", $"\"{exePath}\" \"%1\"");
                    }
                }

                // Register for directories
                using (RegistryKey key = Registry.CurrentUser.CreateSubKey(@"Software\Classes\Directory\shell\LinkAll"))
                {
                    key.SetValue("", "Send via Link All");
                    key.SetValue("Icon", $"\"{exePath}\",0");
                    using (RegistryKey commandKey = key.CreateSubKey("command"))
                    {
                        commandKey.SetValue("", $"\"{exePath}\" \"%1\"");
                    }
                }
            }
            catch (Exception ex)
            {
                try { TraceLog.Write($"ContextMenu Error: {ex.Message}"); TraceLog.Flush(); } catch (Exception innerEx) { App.HandleError(innerEx); }
            }
        }

        // Registers the linkall:// URI scheme under HKCU (no admin needed,
        // same as the context menu above) so Windows actually routes
        // linkall://accept/{id}, linkall://reject/{id}, and the QR
        // pairing deep link to this app instead of failing to find a
        // handler - none of that worked without this.
        public static void RegisterUriProtocol()
        {
            try
            {
                string exePath = Process.GetCurrentProcess().MainModule?.FileName ?? "";
                if (string.IsNullOrEmpty(exePath)) return;

                using (RegistryKey key = Registry.CurrentUser.CreateSubKey(@"Software\Classes\linkall"))
                {
                    key.SetValue("", "URL:Link All Protocol");
                    key.SetValue("URL Protocol", "");
                    using (RegistryKey iconKey = key.CreateSubKey("DefaultIcon"))
                    {
                        iconKey.SetValue("", $"\"{exePath}\",0");
                    }
                    using (RegistryKey shellKey = key.CreateSubKey(@"shell\open\command"))
                    {
                        shellKey.SetValue("", $"\"{exePath}\" \"%1\"");
                    }
                }
            }
            catch (Exception ex)
            {
                try { TraceLog.Write($"URI Protocol Registration Error: {ex.Message}"); TraceLog.Flush(); } catch (Exception innerEx) { App.HandleError(innerEx); }
            }
        }
    }
}
