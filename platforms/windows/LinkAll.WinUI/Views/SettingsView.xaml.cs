using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System;
using System.Text.Json;
using System.Threading.Tasks;

namespace LinkAll.WinUI.Views
{
    public sealed partial class SettingsView : Page
    {
        public LinkAllStore mgr => LinkAllStore.Shared;
        public string DeviceName => LinkAll.WinUI.Services.LocalSettingsStore.DeviceName;

        // Set while the switch shows the saved state, so that doesn't count as the user toggling it.
        private bool _showingSavedStartup;

        public SettingsView()
        {
            this.InitializeComponent();
            var version = typeof(SettingsView).Assembly.GetName().Version;
            if (version != null)
                VersionText.Text = $"Version {version.Major}.{version.Minor}.{version.Build}  \u00B7  No cloud, no account, no telemetry";
            _ = LoadStartupAsync();
            ScreenshotSyncToggle.IsOn = App.ScreenshotSyncEnabled;
            PhoneNotificationsToggle.IsOn = App.PhoneNotificationMirroringEnabled;
            _ = LoadFileBrowsingAsync();
            _ = LoadRemoteControlAsync();
            _ = LoadDropFolderAsync();
            UpdateRow.Visibility = LinkAll.WinUI.Services.UpdateChecker.Supported ? Visibility.Visible : Visibility.Collapsed;
            _showingSavedUpdateCheck = true;
            UpdateCheckToggle.IsOn = LinkAll.WinUI.Services.UpdateChecker.Enabled;
            _showingSavedUpdateCheck = false;

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

        // Set while the switch shows the saved value, so that isn't saved straight back.
        private bool _showingSavedFileBrowsing;

        private async Task LoadFileBrowsingAsync()
        {
            var on = await Task.Run(() =>
            {
                using var doc = DaemonClient.GetSettings();
                return doc != null
                    && doc.RootElement.TryGetProperty("data", out var data)
                    && data.TryGetProperty("allow_file_browsing", out var value)
                    && value.ValueKind == JsonValueKind.True;
            });
            _showingSavedFileBrowsing = true;
            FileBrowsingToggle.IsOn = on;
            _showingSavedFileBrowsing = false;
        }

        // Set while the switch shows the saved value, so that isn't saved straight back.
        private bool _showingSavedRemoteControl;

        private async Task LoadRemoteControlAsync()
        {
            var on = await Task.Run(() =>
            {
                using var doc = DaemonClient.GetSettings();
                return doc != null
                    && doc.RootElement.TryGetProperty("data", out var data)
                    && data.TryGetProperty("allow_remote_control", out var value)
                    && value.ValueKind == JsonValueKind.True;
            });
            _showingSavedRemoteControl = true;
            RemoteControlToggle.IsOn = on;
            _showingSavedRemoteControl = false;
        }

        private async void OnRemoteControlToggled(object sender, RoutedEventArgs e)
        {
            if (_showingSavedRemoteControl) return;
            var on = RemoteControlToggle.IsOn;
            var resp = await Task.Run(() => DaemonClient.PatchSettings(DaemonClient.Fields(("allow_remote_control", on))));
            DaemonActions.ReportIfFailed("Let phones control this PC", resp);
        }

        // Set while the switch shows the saved value, so that isn't saved straight back.
        private bool _showingSavedDropFolder;

        private async Task LoadDropFolderAsync()
        {
            var on = await Task.Run(() =>
            {
                using var doc = DaemonClient.GetSettings();
                return doc != null
                    && doc.RootElement.TryGetProperty("data", out var data)
                    && data.TryGetProperty("drop_folder_enabled", out var value)
                    && value.ValueKind == JsonValueKind.True;
            });
            _showingSavedDropFolder = true;
            DropFolderToggle.IsOn = on;
            _showingSavedDropFolder = false;
        }

        private async void OnDropFolderToggled(object sender, RoutedEventArgs e)
        {
            if (_showingSavedDropFolder) return;
            var on = DropFolderToggle.IsOn;
            var resp = await Task.Run(() => DaemonClient.PatchSettings(DaemonClient.Fields(("drop_folder_enabled", on))));
            DaemonActions.ReportIfFailed("Link All Drop", resp);
        }

        private async void OnFileBrowsingToggled(object sender, RoutedEventArgs e)
        {
            if (_showingSavedFileBrowsing) return;
            var on = FileBrowsingToggle.IsOn;
            var resp = await Task.Run(() => DaemonClient.PatchSettings(DaemonClient.Fields(("allow_file_browsing", on))));
            DaemonActions.ReportIfFailed("Let devices browse files", resp);
        }

        // Set while the switch shows the saved value, so that isn't saved straight back.
        private bool _showingSavedUpdateCheck;

        private void OnUpdateCheckToggled(object sender, RoutedEventArgs e)
        {
            if (_showingSavedUpdateCheck) return;
            LinkAll.WinUI.Services.UpdateChecker.Enabled = UpdateCheckToggle.IsOn;
        }

        private async void OnCopyReportClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                ReportStatusText.Text = "Preparing the report...";
                var text = await LinkAll.WinUI.Services.ProblemReport.BuildAsync();
                var package = new Windows.ApplicationModel.DataTransfer.DataPackage();
                package.SetText(text);
                Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
                ReportStatusText.Text = "Copied. Paste it into your message or issue.";
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
                ReportStatusText.Text = "Couldn't prepare the report.";
            }
        }

        private async void OnSaveReportClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                ReportStatusText.Text = "Preparing the report...";
                var text = await LinkAll.WinUI.Services.ProblemReport.BuildAsync();
                var path = LinkAll.WinUI.Services.ProblemReport.SaveToDownloads(text);
                ReportStatusText.Text = $"Saved to {path}";
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
                {
                    FileName = "explorer.exe",
                    Arguments = $"/select,\"{path}\"",
                    UseShellExecute = true,
                });
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
                ReportStatusText.Text = "Couldn't save the report.";
            }
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

        private async Task LoadStartupAsync()
        {
            try
            {
                var on = await LinkAll.WinUI.Services.StartupRegistration.IsEnabledAsync();
                _showingSavedStartup = true;
                StartupToggle.IsOn = on;
            }
            catch (Exception ex) { App.HandleError(ex); }
            finally { _showingSavedStartup = false; }
        }

        private async void OnStartupToggled(object sender, RoutedEventArgs e)
        {
            if (_showingSavedStartup) return;
            try
            {
                var wanted = StartupToggle.IsOn;
                var on = await LinkAll.WinUI.Services.StartupRegistration.SetEnabledAsync(wanted);
                // Windows can refuse (turned off in Task Manager, or by policy): show what it did.
                if (on != wanted)
                {
                    _showingSavedStartup = true;
                    StartupToggle.IsOn = on;
                    _showingSavedStartup = false;
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
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
