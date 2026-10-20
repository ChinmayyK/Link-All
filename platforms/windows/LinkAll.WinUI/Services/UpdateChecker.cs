using System;
using System.Net.Http;
using System.Text.Json;
using System.Threading.Tasks;

namespace LinkAll.WinUI.Services
{
    /// <summary>
    /// Looks for a newer Link All on GitHub, at most once a day, and only tells
    /// the user: it never installs anything. The Microsoft Store build is
    /// updated by the Store, so it never checks. Failures are silent.
    /// </summary>
    internal static class UpdateChecker
    {
        public sealed record Update(string Version, string Url);

        private const string EnabledKey = "update_check_enabled";
        private const string LastCheckKey = "update_last_check";
        private const string LatestVersionKey = "update_latest_version";
        private const string LatestUrlKey = "update_latest_url";
        private const string LatestEndpoint = "https://api.github.com/repos/ChinmayyK/Link-All/releases/latest";
        private const string RepoPage = "https://github.com/ChinmayyK/Link-All/";
        private static readonly TimeSpan Every = TimeSpan.FromHours(24);

        private static readonly HttpClient Http = CreateClient();

        /// <summary>The update to offer, or null. Raised from a background thread.</summary>
        public static event Action<Update?>? Changed;

        public static bool Supported => !StartupRegistration.IsPackaged;

        public static bool Enabled
        {
            get => Supported && LocalSettingsStore.GetBool(EnabledKey, true);
            set
            {
                LocalSettingsStore.SetBool(EnabledKey, value);
                if (value) _ = CheckIfDueAsync();
                else Changed?.Invoke(null);
            }
        }

        private static HttpClient CreateClient()
        {
            var client = new HttpClient { Timeout = TimeSpan.FromSeconds(8) };
            client.DefaultRequestHeaders.UserAgent.ParseAdd("link-all");
            client.DefaultRequestHeaders.Accept.ParseAdd("application/vnd.github+json");
            return client;
        }

        private static string Current
        {
            get
            {
                var v = typeof(UpdateChecker).Assembly.GetName().Version;
                return v == null ? "" : $"{v.Major}.{v.Minor}.{v.Build}";
            }
        }

        /// <summary>Shows what the last check found, then asks GitHub if a day has passed.</summary>
        public static async Task CheckIfDueAsync()
        {
            if (!Enabled) return;
            try
            {
                var known = LocalSettingsStore.Get(LatestVersionKey);
                var knownUrl = LocalSettingsStore.Get(LatestUrlKey);
                Changed?.Invoke(known != null && knownUrl != null && IsNewer(known, Current) ? new Update(known, knownUrl) : null);

                var last = DateTimeOffset.TryParse(LocalSettingsStore.Get(LastCheckKey), out var t) ? t : DateTimeOffset.MinValue;
                if (DateTimeOffset.UtcNow - last < Every) return;

                var latest = Parse(await Http.GetStringAsync(LatestEndpoint));
                if (latest == null) return;
                LocalSettingsStore.Set(LastCheckKey, DateTimeOffset.UtcNow.ToString("o"));
                LocalSettingsStore.Set(LatestVersionKey, latest.Version);
                LocalSettingsStore.Set(LatestUrlKey, latest.Url);
                if (Enabled) Changed?.Invoke(IsNewer(latest.Version, Current) ? latest : null);
            }
            catch
            {
                // Offline, rate limited or unreadable: no banner, no error.
            }
        }

        internal static Update? Parse(string json)
        {
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            if (IsTrue(root, "draft") || IsTrue(root, "prerelease")) return null;
            if (!root.TryGetProperty("tag_name", out var tag) || !root.TryGetProperty("html_url", out var page)) return null;
            var version = (tag.GetString() ?? "").TrimStart('v');
            var url = page.GetString() ?? "";
            // Only ever open a page on the project's own repository.
            if (version.Length == 0 || !url.StartsWith(RepoPage, StringComparison.Ordinal)) return null;
            return new Update(version, url);
        }

        private static bool IsTrue(JsonElement root, string name) =>
            root.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.True;

        /// <summary>True when <paramref name="latest"/> is a higher major.minor.patch than <paramref name="current"/>.</summary>
        internal static bool IsNewer(string latest, string current)
        {
            static int[] Parts(string v)
            {
                var result = new int[3];
                var pieces = v.Split('.', '-', '+');
                for (var i = 0; i < 3 && i < pieces.Length; i++)
                {
                    var digits = new string(Array.FindAll(pieces[i].ToCharArray(), char.IsAsciiDigit));
                    result[i] = int.TryParse(digits, out var n) ? n : 0;
                }
                return result;
            }
            var a = Parts(latest);
            var b = Parts(current);
            for (var i = 0; i < 3; i++)
                if (a[i] != b[i]) return a[i] > b[i];
            return false;
        }
    }
}
