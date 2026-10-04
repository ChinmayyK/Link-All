using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Composition.SystemBackdrops;
using Microsoft.UI.Xaml.Media;
using System;
using System.Runtime.InteropServices;

namespace LinkAll.WinUI.UI
{
    public sealed partial class EdgeDropWindow : Window
    {
        public EdgeDropWindow()
        {
            this.InitializeComponent();
            ExtendsContentIntoTitleBar = true;
            SetTitleBar(AppTitleBar);
            LinkAll.WinUI.Services.ThemeService.Register(this);

            if (Microsoft.UI.Composition.SystemBackdrops.MicaController.IsSupported())
            {
                this.SystemBackdrop = new Microsoft.UI.Xaml.Media.MicaBackdrop();
            }
            else if (Microsoft.UI.Composition.SystemBackdrops.DesktopAcrylicController.IsSupported())
            {
                this.SystemBackdrop = new Microsoft.UI.Xaml.Media.DesktopAcrylicBackdrop();
            }

            var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
            var windowId = Microsoft.UI.Win32Interop.GetWindowIdFromWindow(hwnd);
            var appWindow = Microsoft.UI.Windowing.AppWindow.GetFromWindowId(windowId);
            LinkAll.WinUI.Services.WindowIconHelper.Apply(appWindow);

            var presenter = appWindow.Presenter as Microsoft.UI.Windowing.OverlappedPresenter;
            if (presenter != null)
            {
                presenter.IsAlwaysOnTop = true;
                presenter.IsResizable = false;
                presenter.IsMaximizable = false;
                presenter.SetBorderAndTitleBar(false, false);
            }
            LinkAll.WinUI.Services.WindowIconHelper.ResizeDips(appWindow, hwnd, 20, 200);
        }

        [DllImport("user32.dll")]
        private static extern int GetSystemMetrics(int nIndex);
        private const int SM_CXSCREEN = 0;
        private const int SM_CYSCREEN = 1;

        private Microsoft.UI.Windowing.AppWindow GetAppWindow()
        {
            var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
            var windowId = Microsoft.UI.Win32Interop.GetWindowIdFromWindow(hwnd);
            return Microsoft.UI.Windowing.AppWindow.GetFromWindowId(windowId);
        }

        private void Grid_DragEnter(object sender, Microsoft.UI.Xaml.DragEventArgs e)
        {
            e.AcceptedOperation = Windows.ApplicationModel.DataTransfer.DataPackageOperation.Copy;
            RestingSliver.Visibility = Visibility.Collapsed;
            ExpandedCard.Visibility = Visibility.Visible;
            ExpandedCard.Opacity = 1.0;
            ExpandedCard.Scale = new System.Numerics.Vector3(1.0f, 1.0f, 1.0f);
            
            try
            {
                int screenW = GetSystemMetrics(SM_CXSCREEN);
                int screenH = GetSystemMetrics(SM_CYSCREEN);
                GetAppWindow().MoveAndResize(new Windows.Graphics.RectInt32(screenW - 250, (screenH - 300) / 2, 250, 300));
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void Grid_DragLeave(object sender, Microsoft.UI.Xaml.DragEventArgs e)
        {
            RestingSliver.Visibility = Visibility.Visible;
            ExpandedCard.Visibility = Visibility.Collapsed;
            ExpandedCard.Opacity = 0;
            ExpandedCard.Scale = new System.Numerics.Vector3(0.95f, 0.95f, 1.0f);
            
            try
            {
                int screenW = GetSystemMetrics(SM_CXSCREEN);
                int screenH = GetSystemMetrics(SM_CYSCREEN);
                GetAppWindow().MoveAndResize(new Windows.Graphics.RectInt32(screenW - 20, (screenH - 200) / 2, 20, 200));
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private async void Grid_Drop(object sender, Microsoft.UI.Xaml.DragEventArgs e)
        {
            RestingSliver.Visibility = Visibility.Visible;
            ExpandedCard.Visibility = Visibility.Collapsed;
            ExpandedCard.Opacity = 0;
            ExpandedCard.Scale = new System.Numerics.Vector3(0.95f, 0.95f, 1.0f);
            try
            {
                var target = LinkAllStore.Shared.SelectedPeer?.device_id;
                if (string.IsNullOrEmpty(target))
                {
                    var peers = System.Linq.Enumerable.ToList(LinkAllStore.Shared.Peers);
                    target = peers.Count > 0 ? peers[0].device_id : "";
                }
                if (string.IsNullOrEmpty(target)) return;

                if (e.DataView.Contains(Windows.ApplicationModel.DataTransfer.StandardDataFormats.StorageItems))
                {
                    var items = await e.DataView.GetStorageItemsAsync();
                    // Every dropped file and folder; PushFile sends a folder whole.
                    foreach (var item in items)
                    {
                        var path = item.Path;
                        if (string.IsNullOrEmpty(path)) continue;
                        DaemonActions.RunFireAndForget("Send File", () => DaemonClient.PushFile(target, path));
                    }
                }
                else if (e.DataView.Contains(Windows.ApplicationModel.DataTransfer.StandardDataFormats.Text))
                {
                    var text = await e.DataView.GetTextAsync();
                    DaemonActions.RunFireAndForget("Send Text", () => DaemonClient.PushTextTo(text, target));
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }
    }
}


