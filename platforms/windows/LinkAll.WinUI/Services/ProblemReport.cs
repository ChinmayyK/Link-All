using System;
using System.IO;
using System.Text.Json;
using System.Threading.Tasks;

namespace LinkAll.WinUI.Services
{
    /// <summary>
    /// The text a user can send when something doesn't work: the daemon's
    /// redacted report plus what only this app knows.
    /// </summary>
    internal static class ProblemReport
    {
        public static async Task<string> BuildAsync()
        {
            var v = typeof(ProblemReport).Assembly.GetName().Version;
            var build = StartupRegistration.IsPackaged ? "Microsoft Store" : "installer or portable";
            var header = $"Windows: {Environment.OSVersion.Version}\nBuild: {build}, {(v == null ? "unknown" : $"{v.Major}.{v.Minor}.{v.Build}")}";
            try
            {
                using var doc = await DaemonClient.SendAsync(DaemonClient.Req("diagnostic_report", ("header", header)));
                if (doc != null
                    && doc.RootElement.TryGetProperty("data", out var data)
                    && data.TryGetProperty("text", out var text)
                    && text.GetString() is { Length: > 0 } report)
                    return report;
            }
            catch
            {
            }
            return $"Link All problem report\n{header}\n\nThe Link All background service did not answer, so there is no device state or log.\n";
        }

        /// <summary>Writes the report to Downloads and returns its path.</summary>
        public static string SaveToDownloads(string text)
        {
            var downloads = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads");
            Directory.CreateDirectory(downloads);
            var path = Path.Combine(downloads, "link-all-report.txt");
            File.WriteAllText(path, text);
            return path;
        }
    }
}
