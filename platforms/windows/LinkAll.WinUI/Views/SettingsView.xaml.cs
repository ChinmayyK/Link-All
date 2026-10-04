using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System;
using System.Threading.Tasks;

namespace LinkAll.WinUI.Views
{
    public sealed partial class SettingsView : Page
    {
        public LinkAllStore mgr => LinkAllStore.Shared;
        public string DeviceName => LinkAll.WinUI.Services.LocalSettingsStore.DeviceName;

        private const string StartupRegistryKeyPath = @"Software\Microsoft\Windows\CurrentVersion\Run";
        private const string StartupRegistryValueName = "LinkAll";

        public SettingsView()
        {
            this.InitializeComponent();
            StartupToggle.IsOn = IsLaunchAtStartupEnabled();
            ScreenshotSyncToggle.IsOn = App.ScreenshotSyncEnabled;
            PhoneNotificationsToggle.IsOn = App.PhoneNotificationMirroringEnabled;

            ThemeSelector.SelectedIndex = LinkAll.WinUI.Services.ThemeService.CurrentPreference switch
            {
                "Dark" => 1,
                "System" => 2,
                _ => 0,
            };
        }

        private void OnScreenshotSyncToggled(object sender, RoutedEventArgs e)
        {
            App.SetScreenshotSyncEnabled(ScreenshotSyncToggle.IsOn);
        }

        private void OnPhoneNotificationsToggled(object sender, RoutedEventArgs e)
        {
            App.SetPhoneNotificationMirroringEnabled(PhoneNotificationsToggle.IsOn);
        }

        private void OnThemeSelectionChanged(object sender, SelectionChangedEventArgs e)
        {
            var preference = ThemeSelector.SelectedIndex switch
            {
                1 => "Dark",
                2 => "System",
                _ => "Light",
            };
            LinkAll.WinUI.Services.ThemeService.SetPreference(preference);
        }

        private void OnManageDevicesClicked(object sender, RoutedEventArgs e)
        {
            DashboardWindow.Current?.NavigateTo("Devices");
        }

        private static bool IsLaunchAtStartupEnabled()
        {
            try
            {
                using var key = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(StartupRegistryKeyPath, writable: false);
                return key?.GetValue(StartupRegistryValueName) != null;
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
                return false;
            }
        }

        private void OnStartupToggled(object sender, RoutedEventArgs e)
        {
            try
            {
                using var key = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(StartupRegistryKeyPath, writable: true);
                if (key == null) return;

                if (StartupToggle.IsOn)
                {
                    var exePath = Environment.ProcessPath ?? System.IO.Path.Combine(AppContext.BaseDirectory, "LinkAll.exe");
                    // --background: start in the tray at sign-in, without a window.
                    key.SetValue(StartupRegistryValueName, $"\"{exePath}\" --background");
                }
                else
                {
                    key.DeleteValue(StartupRegistryValueName, throwOnMissingValue: false);
                }
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
            }
        }

        private void OnReplayOnboardingClicked(object sender, RoutedEventArgs e) => App.ShowOnboarding(force: true);

        private async void OnRescanClicked(object sender, RoutedEventArgs e)
        {
            var resp = await Task.Run(() => DaemonClient.RescanPeers());
            DaemonActions.ReportIfFailed("Rescan", resp);
        }

        private void OnOpenDownloadsClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                var downloadsPath = System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads");
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
                {
                    FileName = downloadsPath,
                    UseShellExecute = true
                });
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
            }
        }

        // Confirm before wiping every pairing key. This used to fire on a
        // single click with no way back, which is the wrong shape for an
        // irreversible security action.
        private async void OnRevokeAllClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                var count = mgr.KnownDeviceCount;
                if (count == 0) return;

                var noun = count == 1 ? "device" : "devices";
                var dialog = new ContentDialog
                {
                    Title = Services.AppDialog.Header("\uE74D", $"Forget {count} paired {noun}?", "Every device will need pairing again", danger: true),
                    Content = "Link All will clear its pairing keys on this PC. "
                            + "Every device will need to be paired again before it can connect.",
                    PrimaryButtonText = "Forget all",
                    CloseButtonText = "Cancel",
                    DefaultButton = ContentDialogButton.Close,
                    XamlRoot = this.XamlRoot,
                };

                if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;

                // Snapshot first: ForgetPeer mutates the collection we'd
                // otherwise be iterating.
                foreach (var peer in mgr.Peers.ToList())
                {
                    mgr.ForgetPeer(peer.device_id);
                }
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
            }
        }

        private void OnViewLogsClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                var dir = System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "LinkAll");
                System.IO.Directory.CreateDirectory(dir);
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
                {
                    FileName = dir,
                    UseShellExecute = true
                });
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
            }
        }

        private void OnRevertClicked(object sender, RoutedEventArgs e)
        {
            mgr.UpdateStateFromDaemon();
        }

        private async void OnSaveClicked(object sender, RoutedEventArgs e)
        {
            var resp = await Task.Run(() => DaemonClient.PatchSettings(DaemonClient.Fields(("sync_enabled", mgr.SyncEnabled))));
            DaemonActions.ReportIfFailed("Save Settings", resp);
        }
    }
}
