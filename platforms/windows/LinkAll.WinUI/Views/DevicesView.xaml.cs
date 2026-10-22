using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using System.Threading.Tasks;
using Microsoft.UI.Xaml.Media.Media3D;

namespace LinkAll.WinUI.Views
{
    public sealed partial class DevicesView : Page
    {
        public LinkAllStore mgr => LinkAllStore.Shared;

        public DevicesView()
        {
            TraceLog.Write("DevicesView constructor starting");
            this.InitializeComponent();
            TraceLog.Write("DevicesView InitializeComponent done");
            StartNowPlaying();
        }

        // A row opens its device's page, unless the tap was on one of its own buttons.
        private void OnOpenDeviceTapped(object sender, TappedRoutedEventArgs e)
        {
            for (var el = e.OriginalSource as DependencyObject; el != null && el != sender; el = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(el))
            {
                if (el is ButtonBase) return;
            }
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer) OpenDevice(peer);
        }

        // The page sits over Home in this view, so Back returns to where Home was scrolled.
        private void OpenDevice(PeerViewModel peer)
        {
            mgr.SelectedPeer = peer;
            DevicePage.DataContext = peer;
            HomeScroll.Visibility = Visibility.Collapsed;
            DevicePage.Visibility = Visibility.Visible;
            DevicePage.ChangeView(null, 0, null, true);
            _ = RefreshNowPlayingAsync();
        }

        private void CloseDevice()
        {
            DevicePage.Visibility = Visibility.Collapsed;
            HomeScroll.Visibility = Visibility.Visible;
            DevicePage.DataContext = null;
            _ = RefreshNowPlayingAsync();
        }

        private void OnCloseDeviceClicked(object sender, RoutedEventArgs e) => CloseDevice();

        private void OnOpenNotesClicked(object sender, RoutedEventArgs e)
        {
            DashboardWindow.Current?.NavigateTo("Scratchpad");
        }

        private void OnPeerCardTapped(object sender, TappedRoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer)
            {
                mgr.SelectedPeer = peer;
                if (sender is FrameworkElement element)
                {
                    FlyoutBase.ShowAttachedFlyout(element);
                }
            }
        }

        private void OnPairingAcceptClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer)
            {
                mgr.RespondToPairing(peer.device_id, true);
            }
        }

        private void OnPairingRejectClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer)
            {
                mgr.RespondToPairing(peer.device_id, false);
            }
        }

        private void OnPairDeviceClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer)
            {
                mgr.ConnectAndPair(peer.device_id);
                // Also wired to Reconnect on paired rows, which needs no code.
                if (!peer.is_trusted)
                    Services.PairingPrompt.ShowOutgoing(peer, this.XamlRoot);
            }
        }

        // Resolve the peer from the row that was clicked, not from
        // SelectedPeer: the per-row disconnect button used to act on whatever
        // card happened to be selected, so with two devices connected it
        // could disconnect the wrong one.
        private void OnDisconnectClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer)
            {
                mgr.DisconnectPeer(peer.device_id);
            }
        }

        private async void OnForgetDeviceMenuClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is not PeerViewModel peer) return;
            var dialog = Services.AppDialog.Create(this.XamlRoot, "\uE74D", $"Forget {peer.DisplayName}?",
                "This PC stops trusting it. To use it again, pair again and compare the code.", danger: true);
            dialog.PrimaryButtonText = "Forget";
            dialog.CloseButtonText = "Cancel";
            dialog.DefaultButton = ContentDialogButton.Close;
            if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
            mgr.ForgetPeer(peer.device_id);
            if (DevicePage.DataContext == peer) CloseDevice();
        }

        private async void OnRenameDeviceClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is not PeerViewModel peer) return;

            var currentName = peer.DisplayName;
            var input = new TextBox { Text = currentName, SelectionStart = 0, SelectionLength = currentName.Length };
            var dialog = new ContentDialog
            {
                Title = Services.AppDialog.Header("\uE8AC", "Rename device", "How it shows on your other devices"),
                Content = input,
                PrimaryButtonText = "Rename",
                CloseButtonText = "Cancel",
                DefaultButton = ContentDialogButton.Primary,
                XamlRoot = this.XamlRoot,
            };

            var result = await dialog.ShowAsync();
            if (result != ContentDialogResult.Primary) return;

            var newName = input.Text?.Trim();
            if (string.IsNullOrEmpty(newName) || newName == currentName) return;

            try
            {
                var resp = await Task.Run(() => DaemonClient.RenameTrustedDevice(peer.device_id, newName));
                DaemonActions.ReportIfFailed("Rename Device", resp);
                peer.friendly_name = newName;
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // Cross-device link handoff: ask a connected device to open a URL
        // immediately. Only http/https is accepted - the engine rejects
        // anything else and reports back via a toast (PB_EVENT_OPEN_URL_ON_DEVICE_ACK
        // in ClipboardManager.DrainEvents), so no client-side validation
        // beyond a basic empty-check is needed here.
        private async void OnOpenLinkOnDeviceClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is not PeerViewModel peer) return;

            var input = new TextBox { PlaceholderText = "https://example.com" };
            var dialog = new ContentDialog
            {
                Title = Services.AppDialog.Header("\uE71B", $"Open link on {peer.DisplayName}", "It opens in that device's browser"),
                Content = input,
                PrimaryButtonText = "Open",
                CloseButtonText = "Cancel",
                DefaultButton = ContentDialogButton.Primary,
                XamlRoot = this.XamlRoot,
            };

            var result = await dialog.ShowAsync();
            if (result != ContentDialogResult.Primary) return;

            var url = input.Text?.Trim();
            if (string.IsNullOrEmpty(url)) return;

            try
            {
                await Task.Run(() => DaemonClient.OpenUrlOnDevice(peer.device_id, url));
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // Rings the phone (even on silent) and keeps a dialog open with a Stop
        // button; the phone also stops by itself after a minute or on "Found it".
        private async void OnFindPhoneClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is not PeerViewModel peer) return;

            try
            {
                var answer = await Task.Run(() => DaemonClient.FindDevice(peer.device_id, true));
                var failed = answer == null
                    || (answer.RootElement.TryGetProperty("status", out var s) && s.GetString() == "error");
                if (failed)
                {
                    DaemonActions.ReportIfFailed($"Ringing {peer.DisplayName}", answer);
                    return;
                }

                var dialog = new ContentDialog
                {
                    Title = Services.AppDialog.Header("\uEA8F", $"{peer.DisplayName} is ringing", "It rings at full volume for up to a minute"),
                    CloseButtonText = "Stop ringing",
                    DefaultButton = ContentDialogButton.Close,
                    XamlRoot = this.XamlRoot,
                };
                await dialog.ShowAsync();
                await Task.Run(() => DaemonClient.FindDevice(peer.device_id, false));
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // "Insert from Phone": ask the phone for a picture; the one that arrives within ten
        // minutes is put on the clipboard and pasted where you were typing.
        private async void OnTakePhotoClicked(object sender, RoutedEventArgs e) => await AskForPictureAsync(sender, "photo");
        private async void OnScanDocumentClicked(object sender, RoutedEventArgs e) => await AskForPictureAsync(sender, "document");

        private async Task AskForPictureAsync(object sender, string mode)
        {
            if ((sender as FrameworkElement)?.DataContext is not PeerViewModel peer) return;
            try
            {
                var answer = await Task.Run(() => DaemonClient.CaptureRequest(peer.device_id, mode));
                DaemonActions.ReportIfFailed($"Asking {peer.DisplayName}", answer);
                if (answer != null && !(answer.RootElement.TryGetProperty("status", out var s) && s.GetString() == "error"))
                {
                    Services.NotificationHelperPictures.MarkAsked();
                    NotificationHelper.ShowToast($"Check {peer.DisplayName}", "Tap the notification to open its camera.");
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // The first connected phone that can be asked, or null (with a message shown).
        private static async Task<(string id, string name)?> PickPhoneAsync()
        {
            var phones = await Task.Run(() =>
            {
                var found = new List<(string id, string name)>();
                using var doc = DaemonClient.DeviceChoices("find");
                if (doc != null
                    && doc.RootElement.TryGetProperty("data", out var data)
                    && data.TryGetProperty("choices", out var choices))
                {
                    foreach (var c in choices.EnumerateArray())
                    {
                        if (c.TryGetProperty("available", out var a) && a.ValueKind == JsonValueKind.True)
                            found.Add((c.GetProperty("id").GetString() ?? "", c.GetProperty("name").GetString() ?? "your phone"));
                    }
                }
                return found;
            });
            if (phones.Count == 0)
            {
                NotificationHelper.ShowToast("No phone to use", "Connect a paired phone that runs Link All 2.1 or later.");
                return null;
            }
            return phones[0];
        }

        // Click to call: type or paste a number, and the phone shows a notification that opens its dialer.
        private async void OnCallFromPhoneClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                var box = new TextBox { PlaceholderText = "+1 234 567 8900", MinWidth = 280 };
                var dialog = Services.AppDialog.Create(this.XamlRoot, "\uE717", "Call from your phone", "Your phone shows a notification; tap it to open the dialer");
                dialog.Content = box;
                dialog.PrimaryButtonText = "Send to phone";
                dialog.CloseButtonText = "Cancel";
                dialog.DefaultButton = ContentDialogButton.Primary;
                if (await dialog.ShowAsync() != ContentDialogResult.Primary || string.IsNullOrWhiteSpace(box.Text)) return;
                var phone = await PickPhoneAsync();
                if (phone == null) return;
                var number = box.Text.Trim();
                var resp = await Task.Run(() => DaemonClient.DialNumber(phone.Value.id, number));
                DaemonActions.ReportIfFailed("Call from phone", resp);
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // Guest Drop: a link and PIN for someone who isn't paired; their files arrive in Downloads\Link All\Guest.
        private async void OnReceiveFromGuestClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                string url = "", pin = "";
                var failure = await Task.Run(() =>
                {
                    using var doc = DaemonClient.GuestDropStart();
                    if (doc != null && doc.RootElement.TryGetProperty("data", out var data) && data.ValueKind == JsonValueKind.Object)
                    {
                        url = data.GetProperty("url").GetString() ?? "";
                        pin = data.GetProperty("pin").GetString() ?? "";
                        return null;
                    }
                    return doc != null && doc.RootElement.TryGetProperty("message", out var m) ? m.GetString() : "Couldn't reach the Link All service.";
                });
                if (failure != null)
                {
                    NotificationHelper.ShowToast("Can't start Guest Drop", failure);
                    return;
                }
                var link = new TextBox { Text = url, IsReadOnly = true, MinWidth = 340 };
                var panel = new StackPanel { Spacing = 10 };
                panel.Children.Add(new TextBlock { Text = "Open this on the other device, type the PIN and send files. Nothing is paired or saved.", TextWrapping = TextWrapping.Wrap });
                panel.Children.Add(link);
                panel.Children.Add(new TextBlock { Text = $"PIN  {pin}", FontSize = 28, FontWeight = Microsoft.UI.Text.FontWeights.Bold, IsTextSelectionEnabled = true });
                panel.Children.Add(new TextBlock { Text = "Works for 5 minutes. Files go to Downloads > Link All > Guest. The page is not encrypted: use it on a network you trust.", TextWrapping = TextWrapping.Wrap, Opacity = 0.7 });
                var dialog = Services.AppDialog.Create(this.XamlRoot, "\uE8FA", "Receive from a guest");
                dialog.Content = panel;
                dialog.PrimaryButtonText = "Copy link";
                dialog.CloseButtonText = "Stop";
                dialog.DefaultButton = ContentDialogButton.Close;
                dialog.PrimaryButtonClick += (s, args) =>
                {
                    var package = new Windows.ApplicationModel.DataTransfer.DataPackage();
                    package.SetText(url);
                    Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
                    args.Cancel = true; // keep the dialog open
                };
                await dialog.ShowAsync();
                await Task.Run(() => DaemonClient.GuestDropStop()?.Dispose());
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private async void OnPauseSyncClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer)
            {
                var resp = await Task.Run(() => DaemonClient.PauseSyncPeer(peer.device_id));
                DaemonActions.ReportIfFailed("Pause Sync", resp);
            }
        }

        private async void OnResumeSyncClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer)
            {
                var resp = await Task.Run(() => DaemonClient.ResumeSyncPeer(peer.device_id));
                DaemonActions.ReportIfFailed("Resume Sync", resp);
            }
        }

        private void OnDisconnectAllClicked(object sender, RoutedEventArgs e)
        {
            DaemonActions.RunFireAndForget("Disconnect All", () => DaemonClient.DisconnectAllPeers());
        }

        // "Scan" should actually probe the network, not just re-read cached
        // daemon state - the old handler only did the latter, which made the
        // button look broken when nothing new appeared.
        private async void OnScanNearbyClicked(object sender, RoutedEventArgs e)
        {
            var resp = await Task.Run(() => DaemonClient.RescanPeers());
            DaemonActions.ReportIfFailed("Scan", resp);
            mgr.UpdateStateFromDaemon();
        }

        // Pairing is now an in-app sheet rather than a second top-level
        // window, so the QR is the focus of the screen while it's open and
        // the flow closes when the task is done.
        private async void OnShowQRCodeClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                await new PairDeviceDialog { XamlRoot = this.XamlRoot }.ShowAsync();
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
            }
        }

        private async void OnConnectByIpClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                await new ConnectByIpDialog { XamlRoot = this.XamlRoot }.ShowAsync();
                mgr.UpdateStateFromDaemon();
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
            }
        }

        private void OnOpenActivityClicked(object sender, RoutedEventArgs e)
        {
            DashboardWindow.Current?.NavigateTo("Activity");
        }

        private void OnOpenSettingsClicked(object sender, RoutedEventArgs e)
        {
            DashboardWindow.Current?.NavigateTo("Settings");
        }

        // "Open containing folder" for a completed, received activity entry -
        // the one overflow action a history row can back with a real path.
        private void OnOpenActivityFolderClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is not ActivityEntry entry) return;
            if (string.IsNullOrWhiteSpace(entry.dest_path)) return;

            try
            {
                var folder = System.IO.Path.GetDirectoryName(entry.dest_path);
                if (string.IsNullOrWhiteSpace(folder)) return;
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
                {
                    FileName = folder,
                    UseShellExecute = true
                });
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
            }
        }

        private void OnOpenDownloadsClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                var downloadsPath = System.IO.Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads");
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

        // Files first, then target: picking what to send before choosing
        // where it goes matches how people think about the task, and skips
        // the device prompt entirely when there's only one candidate.
        private async void OnQuickSendClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                var picker = new Windows.Storage.Pickers.FileOpenPicker();
                var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(App.MainWindow);
                WinRT.Interop.InitializeWithWindow.Initialize(picker, hwnd);
                picker.FileTypeFilter.Add("*");
                picker.SuggestedStartLocation = Windows.Storage.Pickers.PickerLocationId.Downloads;

                var files = await picker.PickMultipleFilesAsync();
                if (files == null || files.Count == 0) return;

                var target = await LinkAll.WinUI.Services.DevicePicker.PickSendTargetAsync(this.XamlRoot, mgr.ConnectedPeers);
                if (target == null) return; // user cancelled

                foreach (var file in files)
                {
                    // Argument order is (path, name, mime, targetDevice, ...); see the
                    // matching fix note in DashboardWindow.xaml.cs.
                    var path = file.Path; var name = file.Name; var mime = file.ContentType; var targetId = target.DeviceId;
                    DaemonActions.RunFireAndForget("Send File", () => DaemonClient.SendFilePath(path, name, mime, targetId));
                }
                DashboardWindow.Current?.NavigateTo("Transfers");
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
            }
        }

        private void OnPingDeviceClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer)
            {
                mgr.SendPushText("__LINKALL_PING__", peer.device_id);
            }
        }

        private async void OnSendFilesToDeviceClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer)
            {
                try
                {
                    var picker = new Windows.Storage.Pickers.FileOpenPicker();
                    var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(App.MainWindow);
                    WinRT.Interop.InitializeWithWindow.Initialize(picker, hwnd);
                    picker.FileTypeFilter.Add("*");
                    picker.SuggestedStartLocation = Windows.Storage.Pickers.PickerLocationId.Downloads;
                    var files = await picker.PickMultipleFilesAsync();
                    if (files != null && files.Count > 0)
                    {
                        foreach (var file in files)
                        {
                            // Argument order is (path, name, mime, targetDevice, ...); see
                            // the matching fix note in DashboardWindow.xaml.cs.
                            var path = file.Path; var name = file.Name; var mime = file.ContentType; var targetId = peer.device_id;
                            DaemonActions.RunFireAndForget("Send File", () => DaemonClient.SendFilePath(path, name, mime, targetId));
                        }
                        DashboardWindow.Current?.NavigateTo("Transfers");
                    }
                }
                catch (Exception ex)
                {
                    App.HandleError(ex);
                }
            }
        }

        private void OnExploreDeviceClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer)
            {
                mgr.SelectedPeer = peer;
                DashboardWindow.Current?.NavigateTo("DevicePeer");
            }
        }

        private void OnSpeedTestDeviceClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer)
            {
                var deviceId = peer.device_id;
                DaemonActions.RunFireAndForget("Speed Test", () => DaemonClient.StartSpeedTest(deviceId, 10));
                DashboardWindow.Current?.NavigateTo("Transfers");
            }
        }

        // ---- Home: Send section and transfer rows ----

        // Files & folders: one dialog for where (all devices unless one is
        // picked) and what (files, or a folder), then the matching picker.
        private async void OnHomeSendClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                var choice = await LinkAll.WinUI.Services.AppDialog.ShowSendAsync(this.XamlRoot, mgr.ConnectedPeers);
                if (choice is not { } c) return;
                if (c.Folder) { await SendFolderAsync(c.Target); return; }
                var picker = new Windows.Storage.Pickers.FileOpenPicker();
                WinRT.Interop.InitializeWithWindow.Initialize(picker, WinRT.Interop.WindowNative.GetWindowHandle(App.MainWindow));
                picker.FileTypeFilter.Add("*");
                picker.SuggestedStartLocation = Windows.Storage.Pickers.PickerLocationId.Downloads;
                var files = await picker.PickMultipleFilesAsync();
                if (files == null || files.Count == 0) return;
                foreach (var file in files)
                {
                    var path = file.Path; var name = file.Name; var mime = file.ContentType; var targetId = c.Target;
                    DaemonActions.RunFireAndForget("Send File", () => DaemonClient.SendFilePath(path, name, mime, targetId));
                }
            }
            catch (System.Exception ex) { App.HandleError(ex); }
        }

        private async void OnSendFolderToDeviceClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is not PeerViewModel peer) return;
            try { await SendFolderAsync(peer.device_id); }
            catch (System.Exception ex) { App.HandleError(ex); }
        }

        private static async System.Threading.Tasks.Task SendFolderAsync(string? targetId)
        {
            var picker = new Windows.Storage.Pickers.FolderPicker();
            WinRT.Interop.InitializeWithWindow.Initialize(picker, WinRT.Interop.WindowNative.GetWindowHandle(App.MainWindow));
            picker.FileTypeFilter.Add("*");
            var folder = await picker.PickSingleFolderAsync();
            if (folder == null) return;
            var path = folder.Path;
            DaemonActions.RunFireAndForget("Send Folder", () => DaemonClient.SendFolder(path, targetId));
        }

        private async void OnHomeSendClipboardClicked(object sender, RoutedEventArgs e)
        {
            try { await LinkAll.WinUI.Services.ClipboardManager.SendLocalClipboardAsync(this.XamlRoot); }
            catch (System.Exception ex) { App.HandleError(ex); }
        }

        private async void OnHomeBrowseClicked(object sender, RoutedEventArgs e)
        {
            var target = await LinkAll.WinUI.Services.DevicePicker.PickAsync(this.XamlRoot, mgr.ConnectedPeers);
            if (target == null) return;
            mgr.SelectedPeer = target;
            DashboardWindow.Current?.NavigateTo("DevicePeer");
        }

        private void OnHomeCancelTransferClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is FileTransferState transfer)
                mgr.CancelTransfer(transfer);
        }

        private void OnTransferFilesTapped(object sender, RoutedEventArgs e)
        {
            DashboardWindow.Current?.NavigateTo("Transfers");
        }

        private void OnBrowseDeviceTapped(object sender, RoutedEventArgs e)
        {
            DashboardWindow.Current?.NavigateTo("DevicePeer");
        }

        private void OnClipboardTapped(object sender, RoutedEventArgs e)
        {
            DashboardWindow.Current?.NavigateTo("Clipboard");
        }

        private async void OnSpeedTestTapped(object sender, RoutedEventArgs e)
        {
            var target = await LinkAll.WinUI.Services.DevicePicker.PickAsync(this.XamlRoot, mgr.ConnectedPeers);
            if (target != null)
            {
                var deviceId = target.device_id;
                DaemonActions.RunFireAndForget("Speed Test", () => DaemonClient.StartSpeedTest(deviceId, 10));
                DashboardWindow.Current?.NavigateTo("Transfers");
            }
        }

    }
}
