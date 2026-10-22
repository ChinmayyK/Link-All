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

        private const int MaxTiles = 5;

        private static readonly Windows.UI.Color TileIdle = Windows.UI.Color.FromArgb(0x1A, 0xFF, 0xFF, 0xFF);
        private static readonly Windows.UI.Color TileHot = Windows.UI.Color.FromArgb(0x59, 0xFF, 0xFF, 0xFF);

        // One tile per connected device, plus "All devices" when there are several.
        // Built here, not bound: the list is a snapshot for this drag.
        private void BuildTargets()
        {
            Targets.Children.Clear();
            var connected = System.Linq.Enumerable.ToList(LinkAllStore.Shared.ConnectedPeers);
            if (connected.Count == 0)
            {
                HeaderText.Text = "No devices connected";
                ExpandedSubText.Text = "Connect a device to send files.";
                return;
            }
            HeaderText.Text = "Send to";
            ExpandedSubText.Text = connected.Count == 1
                ? "Release to send."
                : "Drop on a device, or anywhere else here to send to all.";
            foreach (var peer in System.Linq.Enumerable.Take(connected, MaxTiles))
                Targets.Children.Add(MakeTile(peer.DeviceGlyph, peer.DisplayName, peer.device_id));
            if (connected.Count > 1)
                Targets.Children.Add(MakeTile("", "All devices", ""));
        }

        private Border MakeTile(string glyph, string name, string targetId)
        {
            var icon = new FontIcon { Glyph = glyph, FontSize = 18, Foreground = new SolidColorBrush(Microsoft.UI.Colors.White) };
            var label = new TextBlock
            {
                Text = name,
                Foreground = new SolidColorBrush(Microsoft.UI.Colors.White),
                FontSize = 13,
                TextTrimming = TextTrimming.CharacterEllipsis,
                VerticalAlignment = VerticalAlignment.Center,
            };
            var row = new Grid { ColumnSpacing = 12 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            Grid.SetColumn(label, 1);
            row.Children.Add(icon);
            row.Children.Add(label);
            var tile = new Border
            {
                Child = row,
                Padding = new Thickness(12, 10, 12, 10),
                CornerRadius = new CornerRadius(10),
                Background = new SolidColorBrush(TileIdle),
                AllowDrop = true,
                Tag = targetId,
            };
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(tile, "Send to " + name);
            tile.DragEnter += (s, e) => { e.AcceptedOperation = Windows.ApplicationModel.DataTransfer.DataPackageOperation.Copy; e.Handled = true; Highlight(tile, true); };
            tile.DragOver += (s, e) => { e.AcceptedOperation = Windows.ApplicationModel.DataTransfer.DataPackageOperation.Copy; e.Handled = true; };
            tile.DragLeave += (s, e) => { e.Handled = true; Highlight(tile, false); };
            tile.Drop += async (s, e) => { e.Handled = true; Highlight(tile, false); Collapse(); await SendAsync(e.DataView, (string)tile.Tag); };
            return tile;
        }

        private static void Highlight(Border tile, bool on) =>
            tile.Background = new SolidColorBrush(on ? TileHot : TileIdle);

        private void Expand()
        {
            BuildTargets();
            RestingSliver.Visibility = Visibility.Collapsed;
            ExpandedCard.Visibility = Visibility.Visible;
            ExpandedCard.Opacity = 1.0;
            try
            {
                var mon = LinkAll.WinUI.Services.GlobalDragMonitor.EdgeMonitor;
                var appWindow = GetAppWindow();
                var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
                int tiles = System.Math.Min(System.Math.Max(Targets.Children.Count, 1), MaxTiles + 1);
                var size = LinkAll.WinUI.Services.WindowIconHelper.FitToWorkArea(appWindow, hwnd, 280, 120 + tiles * 52);
                appWindow.MoveAndResize(new Windows.Graphics.RectInt32(mon.X + mon.Width - size.Width, mon.Y + (mon.Height - size.Height) / 2, size.Width, size.Height));
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void Collapse()
        {
            RestingSliver.Visibility = Visibility.Visible;
            ExpandedCard.Visibility = Visibility.Collapsed;
            ExpandedCard.Opacity = 0;
            try
            {
                var mon = LinkAll.WinUI.Services.GlobalDragMonitor.EdgeMonitor;
                var appWindow = GetAppWindow();
                var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
                var size = LinkAll.WinUI.Services.WindowIconHelper.FitToWorkArea(appWindow, hwnd, 20, 200);
                appWindow.MoveAndResize(new Windows.Graphics.RectInt32(mon.X + mon.Width - size.Width, mon.Y + (mon.Height - size.Height) / 2, size.Width, size.Height));
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void Grid_DragEnter(object sender, Microsoft.UI.Xaml.DragEventArgs e)
        {
            e.AcceptedOperation = Windows.ApplicationModel.DataTransfer.DataPackageOperation.Copy;
            Expand();
        }

        private void Grid_DragLeave(object sender, Microsoft.UI.Xaml.DragEventArgs e) => Collapse();

        private async void Grid_Drop(object sender, Microsoft.UI.Xaml.DragEventArgs e)
        {
            Collapse();
            await SendAsync(e.DataView, "");
        }

        // target is a device id, or "" for every connected device.
        private static async System.Threading.Tasks.Task SendAsync(Windows.ApplicationModel.DataTransfer.DataPackageView data, string target)
        {
            try
            {
                var connected = System.Linq.Enumerable.ToList(LinkAllStore.Shared.ConnectedPeers);
                if (connected.Count == 0)
                {
                    NotificationHelper.ShowToast("No devices connected", "Connect a device to send files.");
                    return;
                }
                // With one device, "all" and that device are the same; send to it by id.
                if (target.Length == 0 && connected.Count == 1) target = connected[0].device_id;
                string where = target.Length == 0
                    ? "your devices"
                    : System.Linq.Enumerable.FirstOrDefault(connected, p => p.device_id == target)?.DisplayName ?? "your device";

                if (data.Contains(Windows.ApplicationModel.DataTransfer.StandardDataFormats.StorageItems))
                {
                    var items = await data.GetStorageItemsAsync();
                    int sent = 0;
                    // Every dropped file and folder; PushFile sends a folder whole.
                    foreach (var item in items)
                    {
                        var path = item.Path;
                        if (string.IsNullOrEmpty(path)) continue;
                        sent++;
                        DaemonActions.RunFireAndForget("Send File", () => DaemonClient.PushFile(target, path));
                    }
                    if (sent > 0)
                        NotificationHelper.ShowToast("Sending", sent == 1 ? "1 item to " + where : sent + " items to " + where);
                }
                else if (data.Contains(Windows.ApplicationModel.DataTransfer.StandardDataFormats.Text))
                {
                    var text = await data.GetTextAsync();
                    if (string.IsNullOrEmpty(text)) return;
                    if (string.IsNullOrEmpty(target))
                        DaemonActions.RunFireAndForget("Send Text", () => DaemonClient.PushText(text));
                    else
                        DaemonActions.RunFireAndForget("Send Text", () => DaemonClient.PushTextTo(text, target));
                    NotificationHelper.ShowToast("Sending", "Text to " + where);
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }
    }
}
