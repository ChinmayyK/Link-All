using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System.Threading.Tasks;

namespace LinkAll.WinUI.Views
{
    public sealed partial class TransfersView : Page
    {
        public LinkAllStore mgr => LinkAllStore.Shared;

        public TransfersView()
        {
            this.InitializeComponent();
        }

        private void OnAcceptClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is FileTransferState transfer)
            {
                mgr.AcceptTransfer(transfer.FirstItemId ?? transfer.transfer_id);
            }
        }

        private void OnRejectClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is FileTransferState transfer)
            {
                mgr.RejectTransfer(transfer.FirstItemId ?? transfer.transfer_id);
            }
        }

        private void OnCancelClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is FileTransferState transfer)
            {
                mgr.CancelTransfer(transfer);
            }
        }

        private async void OnSendFolderClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                var picker = new Windows.Storage.Pickers.FolderPicker();
                var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(App.MainWindow);
                WinRT.Interop.InitializeWithWindow.Initialize(picker, hwnd);
                picker.FileTypeFilter.Add("*");
                var folder = await picker.PickSingleFolderAsync();
                if (folder == null) return;
                var target = await LinkAll.WinUI.Services.DevicePicker.PickSendTargetAsync(this.XamlRoot, mgr.ConnectedPeers);
                if (target == null) return; // user cancelled
                var path = folder.Path; var targetId = target.DeviceId;
                DaemonActions.RunFireAndForget("Send Folder", () => DaemonClient.SendFolder(path, targetId));
            }
            catch (System.Exception ex)
            {
                App.HandleError(ex);
            }
        }

        // "Show" on a finished transfer reveals the file itself, falling back
        // to the containing folder - which is what File Explorer's own
        // "Show in folder" does, and what people expect from a completed
        // download.
        private void OnOpenDestinationClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is not FileTransferState transfer) return;

            try
            {
                var destination = transfer.destination;
                if (!string.IsNullOrWhiteSpace(destination) && System.IO.File.Exists(destination))
                {
                    System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
                    {
                        FileName = "explorer.exe",
                        Arguments = $"/select,\"{destination}\"",
                        UseShellExecute = true
                    });
                    return;
                }

                var folder = !string.IsNullOrWhiteSpace(destination)
                    ? System.IO.Path.GetDirectoryName(destination)
                    : null;

                if (string.IsNullOrWhiteSpace(folder) || !System.IO.Directory.Exists(folder))
                {
                    folder = System.IO.Path.Combine(
                        Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads");
                }

                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
                {
                    FileName = folder,
                    UseShellExecute = true
                });
            }
            catch (System.Exception ex)
            {
                App.HandleError(ex);
            }
        }

        private void OnOpenDownloadFolderClicked(object sender, RoutedEventArgs e)
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
            catch (System.Exception ex)
            {
                App.HandleError(ex);
            }
        }

        private async void OnSendFilesClicked(object sender, RoutedEventArgs e)
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
                    var target = await LinkAll.WinUI.Services.DevicePicker.PickSendTargetAsync(this.XamlRoot, mgr.ConnectedPeers);
                    if (target == null) return; // user cancelled
                    foreach (var file in files)
                    {
                        // Argument order is (path, name, mime, targetDevice, ...); see the
                        // matching fix note in DashboardWindow.xaml.cs.
                        var path = file.Path; var name = file.Name; var mime = file.ContentType; var targetId = target.DeviceId;
                        DaemonActions.RunFireAndForget("Send File", () => DaemonClient.SendFilePath(path, name, mime, targetId));
                    }
                }
            }
            catch (System.Exception ex)
            {
                App.HandleError(ex);
            }
        }
    }
}
