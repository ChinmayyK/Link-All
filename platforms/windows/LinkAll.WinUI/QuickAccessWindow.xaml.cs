using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Windowing;
using Microsoft.UI.Composition.SystemBackdrops;
using WinRT.Interop;
using System.Collections.ObjectModel;
using System.Linq;
using System;
using System.ComponentModel;

namespace LinkAll.WinUI
{
    public sealed partial class QuickAccessWindow : Window
    {
        public event EventHandler? DashboardRequested;
        public LinkAllStore mgr => LinkAllStore.Shared;
        private DispatcherTimer _searchDebounceTimer;

        public QuickAccessWindow()
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

            // Resize the window. AppWindow.Resize takes physical pixels while
            // the XAML content is measured in DIPs - scale by the monitor's
            // DPI so this is 360x600 DIPs on every display, not just 100%
            // scaled ones (see the matching note in DashboardWindow.xaml.cs).
            var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
            var windowId = Microsoft.UI.Win32Interop.GetWindowIdFromWindow(hwnd);
            var appWindow = Microsoft.UI.Windowing.AppWindow.GetFromWindowId(windowId);
            LinkAll.WinUI.Services.WindowIconHelper.Apply(appWindow);
            LinkAll.WinUI.Services.WindowIconHelper.ResizeDips(appWindow, hwnd, 360, 600);

            TimelineList.ItemsSource = LinkAllStore.Shared.History;
            if (DeviceTargetsList != null) DeviceTargetsList.ItemsSource = LinkAllStore.WithoutStaleDuplicates(LinkAllStore.Shared.Peers).ToList();
            LinkAllStore.Shared.PropertyChanged += OnStoreChanged;
            this.Closed += (s, e) => {
                LinkAllStore.Shared.PropertyChanged -= OnStoreChanged;
                _searchDebounceTimer?.Stop();
            };

            _searchDebounceTimer = new DispatcherTimer();
            _searchDebounceTimer.Interval = TimeSpan.FromMilliseconds(300);
            _searchDebounceTimer.Tick += SearchDebounceTimer_Tick;
        }

        private void OnStoreChanged(object? sender, PropertyChangedEventArgs e)
        {
            DispatcherQueue?.TryEnqueue(() => {
                try
                {
                    if (e.PropertyName == nameof(LinkAllStore.History))
                    {
                        if (string.IsNullOrWhiteSpace(TxtSearch?.Text))
                            TimelineList.ItemsSource = LinkAllStore.Shared.History;
                    }
                    else if (e.PropertyName == nameof(LinkAllStore.Peers) && DeviceTargetsList != null)
                    {
                        DeviceTargetsList.ItemsSource = LinkAllStore.WithoutStaleDuplicates(LinkAllStore.Shared.Peers).ToList();
                    }
                }
                catch (Exception ex) { App.HandleError(ex); }
            });
        }

        private void BtnHeaderDiagnostics_Click(object sender, RoutedEventArgs e)
        {
            DashboardRequested?.Invoke(this, EventArgs.Empty);
            Close();
        }

        private void BtnHeaderQuit_Click(object sender, RoutedEventArgs e)
        {
            if (Application.Current is App app)
            {
                app.ExitApplicationCommand.Execute(null);
            }
            else
            {
                Application.Current.Exit();
                Environment.Exit(0);
            }
        }

        private void TxtSearch_TextChanged(AutoSuggestBox sender, AutoSuggestBoxTextChangedEventArgs args)
        {
            _searchDebounceTimer.Stop();
            _searchDebounceTimer.Start();
        }

        private void SearchDebounceTimer_Tick(object? sender, object e)
        {
            _searchDebounceTimer.Stop();
            try
            {
                var query = TxtSearch?.Text?.ToLowerInvariant() ?? "";
                var snapshot = LinkAllStore.Shared.History.ToList();
                if (string.IsNullOrWhiteSpace(query))
                {
                    TimelineList.ItemsSource = LinkAllStore.Shared.History;
                }
                else
                {
                    var results = snapshot
                        .Where(h => (h.display_text?.ToLowerInvariant().Contains(query) == true) || 
                                    (h.path?.ToLowerInvariant().Contains(query) == true));
                    TimelineList.ItemsSource = new ObservableCollection<HistoryItem>(results);
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void BtnPinItem_Click(object sender, RoutedEventArgs e)
        {
            try
            {
                if (((FrameworkElement)sender).DataContext is HistoryItem item)
                {
                    item.IsPinned = !item.IsPinned;
                    var snapshot = LinkAllStore.Shared.History.ToList();
                    var results = snapshot
                        .OrderByDescending(h => h.IsPinned)
                        .ThenByDescending(h => h.Time);
                    TimelineList.ItemsSource = new ObservableCollection<HistoryItem>(results);
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void BtnDeleteItem_Click(object sender, RoutedEventArgs e)
        {
            try
            {
                if (((FrameworkElement)sender).DataContext is HistoryItem item)
                {
                    LinkAllStore.Shared.History.Remove(item);
                    App.Clipboard?.History.Remove(item);
                    LinkAllStore.Shared.TriggerHistoryUpdate();
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // Bound to Tapped on the whole row rather than Click on a row-sized
        // Button: the row now hosts its own pin/delete buttons, and nesting
        // buttons inside a button made those two unreachable in some hit-test
        // orders.
        private void HistoryItem_Click(object sender, Microsoft.UI.Xaml.Input.TappedRoutedEventArgs e)
        {
            if (((FrameworkElement)sender).DataContext is HistoryItem item)
            {
                if (item.is_text && !string.IsNullOrEmpty(item.FullText))
                {
                    try {
                        var dp = new Windows.ApplicationModel.DataTransfer.DataPackage();
                        dp.SetText(item.FullText);
                        Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(dp);
                    } catch (Exception ex) { App.HandleError(ex); }
                }
                else if (LinkAll.WinUI.Services.ClipboardManager.IsCachedImage(item.path))
                {
                    // A clipboard image: copy it again, like text.
                    var path = item.path;
                    _ = App.Clipboard?.CopyImageAsync(path).ContinueWith(t => { if (t.Exception != null) App.HandleError(t.Exception); });
                }
                else if (!string.IsNullOrEmpty(item.path) && System.IO.File.Exists(item.path))
                {
                    try {
                        System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(item.path) { UseShellExecute = true });
                    } catch (Exception ex) { App.HandleError(ex); }
                }
                Close();
            }
        }

        private async void DeviceTarget_Click(object sender, RoutedEventArgs e)
        {
            if (((FrameworkElement)sender).DataContext is PeerViewModel peer)
            {
                try
                {
                    // Read the text here and send it inline: the daemon has no
                    // OS clipboard access, so "push_clipboard" finds nothing.
                    var content = Windows.ApplicationModel.DataTransfer.Clipboard.GetContent();
                    if (content.Contains(Windows.ApplicationModel.DataTransfer.StandardDataFormats.Text))
                    {
                        var text = await content.GetTextAsync();
                        var targetId = peer.device_id;
                        if (!string.IsNullOrEmpty(text))
                            DaemonActions.RunFireAndForget("Push Clipboard", () => DaemonClient.PushTextTo(text, targetId));
                    }
                }
                catch (Exception ex) { App.HandleError(ex); }
                Close();
            }
        }
    }
}


