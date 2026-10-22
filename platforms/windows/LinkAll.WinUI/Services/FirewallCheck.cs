using System;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading.Tasks;

namespace LinkAll.WinUI.Services
{
    // Windows keeps a Block rule for a program whose firewall prompt was dismissed, and
    // that rule beats any Allow. Reading rules needs no administrator; fixing them does,
    // so the app can only tell the user. Outgoing connections still work either way.
    internal static class FirewallCheck
    {
        public static Task<bool> IncomingBlockedAsync() => Task.Run(() =>
        {
            try
            {
                var exe = Environment.ProcessPath;
                if (string.IsNullOrEmpty(exe)) return false;
                var psi = new ProcessStartInfo("netsh", "advfirewall firewall show rule name=all dir=in verbose")
                {
                    RedirectStandardOutput = true,
                    UseShellExecute = false,
                    CreateNoWindow = true,
                };
                using var p = Process.Start(psi);
                if (p == null) return false;
                var text = p.StandardOutput.ReadToEnd();
                if (!p.WaitForExit(10000)) return false;
                return IsBlocked(text, exe);
            }
            catch (Exception ex) when (ex is IOException or InvalidOperationException or System.ComponentModel.Win32Exception)
            {
                return false;
            }
        });

        // netsh prints one "Label:  value" block per rule. Labels are localized, so on a
        // non-English Windows nothing matches and no warning shows.
        internal static bool IsBlocked(string netshOutput, string exePath)
        {
            foreach (var rule in netshOutput.Split(new[] { "\r\n\r\n", "\n\n" }, StringSplitOptions.RemoveEmptyEntries))
            {
                var fields = rule.Split('\n')
                    .Select(l => l.Split(':', 2))
                    .Where(kv => kv.Length == 2)
                    .GroupBy(kv => kv[0].Trim())
                    .ToDictionary(g => g.Key, g => g.First()[1].Trim(), StringComparer.OrdinalIgnoreCase);
                if (fields.GetValueOrDefault("Enabled") == "Yes"
                    && fields.GetValueOrDefault("Action") == "Block"
                    && string.Equals(fields.GetValueOrDefault("Program"), exePath, StringComparison.OrdinalIgnoreCase))
                    return true;
            }
            return false;
        }
    }
}
