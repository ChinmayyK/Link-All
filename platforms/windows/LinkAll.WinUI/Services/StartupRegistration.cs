using System.Runtime.InteropServices;

namespace LinkAll.WinUI.Services
{
    /// <summary>
    /// "Start with Windows" for both kinds of install. The MSI and zip builds
    /// use the per-user Run key; the Microsoft Store (MSIX) build can't run a
    /// packaged exe from there, so it uses the package's StartupTask instead
    /// (declared in Package.appxmanifest, off until the user turns it on).
    /// </summary>
    internal static class StartupRegistration
    {
        private const string RunKeyPath = @"Software\Microsoft\Windows\CurrentVersion\Run";
        private const string RunValueName = "LinkAll";
        /// <summary>Must match the StartupTask TaskId in Package.appxmanifest.</summary>
        private const string StartupTaskId = "LinkAllStartup";

        private const int AppModelErrorNoPackage = 15700;

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
        private static extern int GetCurrentPackageFullName(ref uint packageFullNameLength, IntPtr packageFullName);

        /// <summary>True when running from an MSIX package (the Store build).</summary>
        public static bool IsPackaged { get; } = DetectPackaged();

        private static bool DetectPackaged()
        {
            try
            {
                uint length = 0;
                return GetCurrentPackageFullName(ref length, IntPtr.Zero) != AppModelErrorNoPackage;
            }
            catch
            {
                return false;
            }
        }

        public static async Task<bool> IsEnabledAsync()
        {
            if (IsPackaged)
            {
                var task = await Windows.ApplicationModel.StartupTask.GetAsync(StartupTaskId);
                return task.State is Windows.ApplicationModel.StartupTaskState.Enabled
                    or Windows.ApplicationModel.StartupTaskState.EnabledByPolicy;
            }
            using var key = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(RunKeyPath, writable: false);
            return key?.GetValue(RunValueName) != null;
        }

        /// <summary>
        /// Turns starting at sign-in on or off and returns whether it is on
        /// afterwards: Windows can refuse (the user turned it off in Task
        /// Manager's Startup apps, or policy forbids it).
        /// </summary>
        public static async Task<bool> SetEnabledAsync(bool enabled)
        {
            if (IsPackaged)
            {
                var task = await Windows.ApplicationModel.StartupTask.GetAsync(StartupTaskId);
                if (!enabled)
                {
                    task.Disable();
                    return false;
                }
                var state = await task.RequestEnableAsync();
                return state is Windows.ApplicationModel.StartupTaskState.Enabled
                    or Windows.ApplicationModel.StartupTaskState.EnabledByPolicy;
            }

            using var key = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(RunKeyPath, writable: true);
            if (key == null) return false;
            if (enabled)
            {
                var exePath = Environment.ProcessPath ?? Path.Combine(AppContext.BaseDirectory, "LinkAll.exe");
                // --background: start in the tray at sign-in, without a window.
                key.SetValue(RunValueName, $"\"{exePath}\" --background");
                return true;
            }
            key.DeleteValue(RunValueName, throwOnMissingValue: false);
            return false;
        }
    }
}
