using System;
using System.Collections.Generic;
using System.IO;
using System.Text.Json;

namespace LinkAll.WinUI.Services
{
    // Windows.Storage.ApplicationData.Current.LocalSettings reproducibly
    // throws ("Operation is not valid due to the current state of the
    // object") in this unpackaged WinAppSDK app - not just when called too
    // early in OnLaunched, but even from a DispatcherQueue-deferred retry
    // after the first window is fully activated and the message pump is
    // running. Whatever WinAppSDK identity/bootstrap state it needs never
    // actually becomes available here. Plain file I/O under the same
    // %LOCALAPPDATA%\LinkAll folder already used for winui_trace.txt
    // sidesteps the whole problem and is reliable from the first line of
    // the process - this is what ThemeService/screenshot-sync should have
    // been using from the start; every setting saved via ApplicationData
    // was silently never actually persisting across restarts.
    internal static class LocalSettingsStore
    {
        private static readonly string FilePath = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "LinkAll", "settings.json");

        private static readonly object Lock = new();
        private static Dictionary<string, string>? _cache;

        private static Dictionary<string, string> Load()
        {
            lock (Lock)
            {
                if (_cache != null) return _cache;
                try
                {
                    _cache = File.Exists(FilePath)
                        ? JsonSerializer.Deserialize(File.ReadAllText(FilePath), LinkAllJsonContext.Default.DictionaryStringString) ?? new()
                        : new();
                }
                catch (Exception ex)
                {
                    App.HandleError(ex);
                    _cache = new();
                }
                return _cache;
            }
        }

        public static string? Get(string key)
        {
            lock (Lock) { return Load().TryGetValue(key, out var v) ? v : null; }
        }

        public static void Set(string key, string value)
        {
            lock (Lock)
            {
                var dict = Load();
                dict[key] = value;
                try
                {
                    var dir = Path.GetDirectoryName(FilePath);
                    if (!string.IsNullOrEmpty(dir)) Directory.CreateDirectory(dir);
                    File.WriteAllText(FilePath, JsonSerializer.Serialize(dict, LinkAllJsonContext.Default.DictionaryStringString));
                }
                catch (Exception ex) { App.HandleError(ex); }
            }
        }

        public static bool GetBool(string key, bool defaultValue = false) =>
            bool.TryParse(Get(key), out var b) ? b : defaultValue;

        public static void SetBool(string key, bool value) => Set(key, value.ToString());

        public static string DeviceName
        {
            get
            {
                var custom = Get("DeviceName") ?? Get("pc_name") ?? Get("device_name");
                return !string.IsNullOrWhiteSpace(custom) ? custom : Environment.MachineName;
            }
            set => Set("DeviceName", value);
        }
    }
}
