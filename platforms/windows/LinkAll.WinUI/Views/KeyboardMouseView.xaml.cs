using LinkAll.WinUI.Services;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using System;
using System.Linq;
using System.Text.Json;
using System.Threading.Tasks;

namespace LinkAll.WinUI.Views
{
    /// <summary>
    /// The page for sharing this PC's keyboard and mouse: on or off, where each
    /// computer sits, what is happening now, and whether each linked computer is
    /// ready.
    /// </summary>
    public sealed partial class KeyboardMouseView : Page
    {
        // Set while the boxes show the saved layout, so that isn't saved straight back.
        private bool _loading;
        private readonly DispatcherQueueTimer _timer;

        public KeyboardMouseView()
        {
            this.InitializeComponent();
            LoadLayout();
            _timer = DispatcherQueue.GetForCurrentThread().CreateTimer();
            _timer.Interval = TimeSpan.FromSeconds(1);
            _timer.Tick += (s, e) => _ = RefreshAsync();
            this.Loaded += (s, e) => { _timer.Start(); _ = RefreshAsync(); _ = LoadPcKeyboardAsync(); };
            this.Unloaded += (s, e) => _timer.Stop();
        }

        private void LoadLayout()
        {
            _loading = true;
            try
            {
                EdgeControlToggle.IsOn = LocalSettingsStore.GetBool(EdgeControlService.EnabledKey, false);
                // Paired computers; a phone can't take a keyboard and mouse.
                var peers = LinkAllStore.Shared.Peers
                    .Where(p => p.is_trusted && !(p.platform ?? "").Contains("android", StringComparison.OrdinalIgnoreCase))
                    .ToList();
                foreach (var box in new[] { EdgeAbove, EdgeLeft, EdgeRight, EdgeBelow })
                {
                    var saved = LocalSettingsStore.Get(EdgeControlService.LinkKey(int.Parse((string)box.Tag))) ?? "";
                    box.Items.Clear();
                    box.Items.Add(new ComboBoxItem { Content = "Nobody", Tag = "" });
                    int selected = 0;
                    foreach (var p in peers)
                    {
                        box.Items.Add(new ComboBoxItem { Content = p.DisplayName, Tag = p.device_id });
                        if (p.device_id == saved) selected = box.Items.Count - 1;
                    }
                    box.SelectedIndex = selected;
                }
            }
            finally { _loading = false; }
        }

        private void OnEdgeControlToggled(object sender, RoutedEventArgs e)
        {
            if (_loading) return;
            LocalSettingsStore.SetBool(EdgeControlService.EnabledKey, EdgeControlToggle.IsOn);
            EdgeControlService.Current?.Reload();
            _ = RefreshAsync();
        }

        private void OnEdgeLinkChanged(object sender, SelectionChangedEventArgs e)
        {
            if (_loading || sender is not ComboBox box) return;
            var id = (box.SelectedItem as ComboBoxItem)?.Tag as string ?? "";
            LocalSettingsStore.Set(EdgeControlService.LinkKey(int.Parse((string)box.Tag)), id);
            EdgeControlService.Current?.Reload();
            _ = RefreshAsync();
        }

        private void OnEndClicked(object sender, RoutedEventArgs e) => EdgeControlService.Current?.EndSharing();

        // Set while the switch shows the saved value, so that isn't saved straight back.
        private bool _showingSavedPcKeyboard;

        private async Task LoadPcKeyboardAsync()
        {
            var on = await Task.Run(() =>
            {
                using var doc = DaemonClient.GetSettings();
                return doc != null
                    && doc.RootElement.TryGetProperty("data", out var data)
                    && data.TryGetProperty("pc_keyboard_on_mac", out var value)
                    && value.ValueKind == JsonValueKind.True;
            });
            _showingSavedPcKeyboard = true;
            PcKeyboardToggle.IsOn = on;
            _showingSavedPcKeyboard = false;
        }

        private async void OnPcKeyboardToggled(object sender, RoutedEventArgs e)
        {
            if (_showingSavedPcKeyboard) return;
            var on = PcKeyboardToggle.IsOn;
            var resp = await Task.Run(() => DaemonClient.PatchSettings(DaemonClient.Fields(("pc_keyboard_on_mac", on))));
            DaemonActions.ReportIfFailed("The Mac uses a PC keyboard", resp);
        }

        private async Task RefreshAsync()
        {
            try
            {
                // Looks without clearing: the hand-off itself acts on what it finds.
                using var doc = await Task.Run(() => DaemonClient.EdgeStatus(peek: true));
                var data = doc != null && doc.RootElement.TryGetProperty("data", out var d) ? d.Clone() : default;
                bool Flag(string name) => data.ValueKind == JsonValueKind.Object && data.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.True;
                string? Text(string name) => data.ValueKind == JsonValueKind.Object && data.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;

                var enabled = EdgeControlToggle.IsOn;
                ShowStatus(enabled, Flag("hosting"), Flag("controlled"), NameOf(Text("peer")));
                ShowReadiness(enabled, Text("refused_target"), Text("refused_reason"));
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private static string? NameOf(string? deviceId) =>
            deviceId == null ? null : LinkAllStore.Shared.Peers.FirstOrDefault(p => p.device_id == deviceId)?.DisplayName;

        private static Brush Resource(string key) => (Brush)Application.Current.Resources[key];

        private void ShowStatus(bool enabled, bool hosting, bool controlled, string? peerName)
        {
            EndButton.Visibility = hosting ? Visibility.Visible : Visibility.Collapsed;
            if (hosting)
            {
                StatusTitle.Text = $"Controlling {peerName ?? "another computer"}";
                StatusDetail.Text = "This PC's keyboard and mouse are on that computer. Push the pointer back past its edge, or press Ctrl+Alt+Esc.";
                StatusDot.Background = Resource("AppAccentBrush");
            }
            else if (controlled)
            {
                StatusTitle.Text = $"Being controlled by {peerName ?? "another computer"}";
                StatusDetail.Text = "That computer's keyboard and mouse are driving this PC.";
                StatusDot.Background = Resource("AppAccentBrush");
            }
            else if (enabled)
            {
                StatusTitle.Text = "Ready";
                StatusDetail.Text = "Push the pointer against a screen edge that has a computer next to it.";
                StatusDot.Background = Resource("AppSuccessBrush");
            }
            else
            {
                StatusTitle.Text = "Off";
                StatusDetail.Text = "Turn on sharing to move between computers with one keyboard and mouse.";
                StatusDot.Background = Resource("AppNeutralBrush");
            }
        }

        private void ShowReadiness(bool enabled, string? refusedTarget, string? refusedReason)
        {
            ReadinessList.Children.Clear();
            var linked = Enumerable.Range(0, 4)
                .Select(e => (Edge: e, Id: LocalSettingsStore.Get(EdgeControlService.LinkKey(e)) ?? ""))
                .Where(x => x.Id.Length > 0)
                .ToList();
            ReadinessSection.Visibility = enabled && linked.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
            foreach (var (edge, id) in linked)
            {
                var peer = LinkAllStore.Shared.Peers.FirstOrDefault(p => p.device_id == id);
                string verdict;
                bool ok = false;
                if (peer == null) verdict = "Not paired any more. Choose another computer.";
                else if (!peer.is_trusted) verdict = "Not paired yet.";
                else if (!string.Equals(peer.status, "connected", StringComparison.OrdinalIgnoreCase)) verdict = "Not connected right now. Open Link All on it, on the same Wi-Fi.";
                else if (refusedTarget == id && !string.IsNullOrEmpty(refusedReason)) verdict = refusedReason!;
                else { verdict = "Connected. If it does not take over, turn on Remote control in Link All on that computer."; ok = true; }

                var icon = new FontIcon
                {
                    Glyph = ok ? "" : "",
                    FontSize = 16,
                    Foreground = Resource(ok ? "AppSuccessBrush" : "AppWarningBrush"),
                };
                var grid = new Grid { Padding = new Thickness(16, 10, 16, 10), ColumnSpacing = 12 };
                grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                var text = new StackPanel { Spacing = 1 };
                text.Children.Add(new TextBlock
                {
                    Text = $"{peer?.DisplayName ?? "A computer"} ({EdgeControlService.EdgeNames[edge]})",
                    Style = (Style)Application.Current.Resources["AppBodyStrong"],
                });
                text.Children.Add(new TextBlock
                {
                    Text = verdict,
                    Style = (Style)Application.Current.Resources["AppSecondary"],
                    TextWrapping = TextWrapping.Wrap,
                });
                Grid.SetColumn(text, 1);
                grid.Children.Add(icon);
                grid.Children.Add(text);
                ReadinessList.Children.Add(grid);
            }
        }
    }
}
