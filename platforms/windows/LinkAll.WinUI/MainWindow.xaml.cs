using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using System.Collections.ObjectModel;
using WinRT.Interop;
using Microsoft.UI.Windowing;
using Microsoft.UI.Composition.SystemBackdrops;

namespace LinkAll.WinUI
{
    public sealed partial class MainWindow : Window
    {
        public LinkAllStore mgr => LinkAllStore.Shared;
        public string localMachineName => LinkAll.WinUI.Services.LocalSettingsStore.DeviceName;

        public MainWindow()
        {
            this.InitializeComponent();

            if (Microsoft.UI.Composition.SystemBackdrops.MicaController.IsSupported())
            {
                this.SystemBackdrop = new Microsoft.UI.Xaml.Media.MicaBackdrop();
            }
            else if (Microsoft.UI.Composition.SystemBackdrops.DesktopAcrylicController.IsSupported())
            {
                this.SystemBackdrop = new Microsoft.UI.Xaml.Media.DesktopAcrylicBackdrop();
            }

            ExtendsContentIntoTitleBar = true;
            SetTitleBar(AppTitleBar);
            LinkAll.WinUI.Services.ThemeService.Register(this);

            var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
            var windowId = Microsoft.UI.Win32Interop.GetWindowIdFromWindow(hwnd);
            var appWindow = Microsoft.UI.Windowing.AppWindow.GetFromWindowId(windowId);
            LinkAll.WinUI.Services.WindowIconHelper.Apply(appWindow);
            LinkAll.WinUI.Services.WindowIconHelper.ResizeDips(appWindow, hwnd, 400, 500);
        }

        private void PairDevice_Click(object sender, RoutedEventArgs e)
        {
            try
            {
                // This popup has its own XamlRoot but is only 400px wide -
                // too narrow to host the pairing sheet comfortably - so
                // pairing from here still opens the dedicated window.
                var qrWindow = new QRPairingWindow();
                qrWindow.Activate();
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private async void SendFile_Click(object sender, RoutedEventArgs e)
        {
            try
            {
                var picker = new Windows.Storage.Pickers.FileOpenPicker();
                var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
                WinRT.Interop.InitializeWithWindow.Initialize(picker, hwnd);
                picker.FileTypeFilter.Add("*");
                picker.SuggestedStartLocation = Windows.Storage.Pickers.PickerLocationId.Downloads;

                var files = await picker.PickMultipleFilesAsync();
                if (files == null || files.Count == 0) return;

                var target = await LinkAll.WinUI.Services.DevicePicker.PickSendTargetAsync(
                    (this.Content as FrameworkElement)?.XamlRoot, mgr.ConnectedPeers);
                if (target == null) return; // user cancelled

                foreach (var file in files)
                {
                    // Argument order is (path, name, mime, targetDevice, ...); see the
                    // matching fix note in DashboardWindow.xaml.cs.
                    var path = file.Path; var name = file.Name; var mime = file.ContentType; var targetId = target.DeviceId;
                    DaemonActions.RunFireAndForget("Send File", () => DaemonClient.SendFilePath(path, name, mime, targetId));
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // Reuse the existing dashboard rather than spawning a second one.
        // Opening the tray popup repeatedly used to create a new
        // DashboardWindow each time, leaving orphaned windows behind.
        private void BtnOpenDashboard_Click(object sender, RoutedEventArgs e)
        {
            try
            {
                if (DashboardWindow.Current != null)
                {
                    DashboardWindow.Current.Activate();
                    return;
                }

                var dashboard = new DashboardWindow();
                dashboard.Activate();
            }
            catch (Exception ex) { App.HandleError(ex); }
        }
    }
}
