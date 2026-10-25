using System;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Security.Principal;
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

        /// <summary>
        /// Whether this account can change a firewall rule: an administrator can
        /// approve the prompt it takes, a standard account can't. Link All
        /// installs and runs without one, so it mustn't offer a fix that dead-ends.
        /// </summary>
        public static bool CanChangeFirewall()
        {
            try
            {
                using var id = WindowsIdentity.GetCurrent();
                // Running elevated, or UAC is off.
                if (new WindowsPrincipal(id).IsInRole(WindowsBuiltInRole.Administrator)) return true;
                // With UAC on, an administrator runs with a limited token that can be raised.
                return GetTokenInformation(id.Token, TokenElevationType, out int type, sizeof(int), out _)
                    && type == TokenElevationTypeLimited;
            }
            catch (Exception ex) when (ex is System.Security.SecurityException or UnauthorizedAccessException)
            {
                return false;
            }
        }

        private const int TokenElevationType = 18, TokenElevationTypeLimited = 3;

        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool GetTokenInformation(IntPtr token, int infoClass, out int info, int length, out int returned);

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
