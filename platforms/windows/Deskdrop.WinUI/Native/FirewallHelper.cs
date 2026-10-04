using Microsoft.UI.Dispatching;
using System;

namespace Deskdrop.WinUI
{
    internal static class FirewallHelper
    {
        private static readonly (string Name, int Port, string Protocol)[] RequiredRules = new[]
        {
            ("Deskdrop TCP",           47823, "TCP"),
            ("Deskdrop UDP Broadcast", 47824, "UDP"),
            ("Deskdrop UDP Multicast", 47825, "UDP"),
            ("Deskdrop mDNS",          5353,  "UDP"),
        };

        // Deskdrop never elevates to add firewall rules: a UAC "runas"
        // prompt asks for an *administrator's* credentials, which is a
        // dead end for any user who isn't a local admin - they can't ever
        // satisfy it, so surfacing it is just a recurring interruption
        // with no way out. This only detects and logs whether the rules
        // are missing; LAN discovery/transfer instead relies on Windows'
        // own per-app firewall consent prompt (the "Allow access" dialog),
        // which any standard user can accept without a password.
        public static void EnsureRules()
        {
            try
            {
                foreach (var (name, port, protocol) in RequiredRules)
                {
                    if (!RuleExists(name))
                    {
                        System.Diagnostics.Debug.WriteLine($"[Link All] Firewall rule missing: {name} ({protocol}/{port}). Not auto-elevating - LAN discovery/transfer may be blocked until this is added manually or by an admin.");
                    }
                }
            }
            catch (Exception ex)
            {
                System.Diagnostics.Debug.WriteLine($"[Link All] Firewall rule check failed (non-fatal): {ex.Message}");
            }
        }

        private static bool RuleExists(string ruleName)
        {
            try
            {
                var psi = new System.Diagnostics.ProcessStartInfo
                {
                    FileName = "netsh",
                    Arguments = $"advfirewall firewall show rule name=\"{ruleName}\"",
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    UseShellExecute = false,
                    CreateNoWindow = true,
                };
                using var proc = System.Diagnostics.Process.Start(psi);
                if (proc == null) return false;
                string output = proc.StandardOutput.ReadToEnd();
                proc.WaitForExit(3000);
                // netsh returns exit code 0 and prints rule details when it exists.
                return proc.ExitCode == 0 && output.Contains(ruleName);
            }
            catch { return false; }
        }
    }
}









