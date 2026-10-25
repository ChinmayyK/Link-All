using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Threading.Tasks;

namespace LinkAll.WinUI.Views
{
    // What another device is playing, with its cover and controls, at the top of Devices.
    public sealed partial class DevicesView
    {
        private readonly DispatcherTimer _nowPlayingTimer = new() { Interval = TimeSpan.FromSeconds(2) };
        private string _nowPlayingShown = "";

        private void StartNowPlaying()
        {
            _nowPlayingTimer.Tick += async (s, e) => await RefreshNowPlayingAsync();
            this.Loaded += async (s, e) => { _nowPlayingTimer.Start(); await RefreshNowPlayingAsync(); };
            this.Unloaded += (s, e) => _nowPlayingTimer.Stop();
        }

        private bool _nowPlayingRefreshing;

        private async Task RefreshNowPlayingAsync()
        {
            // The timer and opening a device's page can both ask; one at a time
            // keeps the cover cache, filled off the UI thread, consistent.
            if (_nowPlayingRefreshing) return;
            _nowPlayingRefreshing = true;
            try
            {
                // On a device's page, what that device plays; on Home, the first.
                var pageDevice = DevicePage.Visibility == Visibility.Visible
                    ? (DevicePage.DataContext as PeerViewModel)?.device_id
                    : null;
                var host = pageDevice != null ? DeviceNowPlayingHost : NowPlayingHost;
                var item = await Task.Run(() =>
                {
                    // Without covers: they're most of the reply. art_key says
                    // when the cover changed, and only then is it fetched.
                    using var doc = DaemonClient.Send(DaemonClient.Req("now_playing_list", ("art", false)));
                    if (doc != null
                        && doc.RootElement.TryGetProperty("data", out var data)
                        && data.ValueKind == JsonValueKind.Array
                        && data.GetArrayLength() > 0)
                    {
                        foreach (var entry in data.EnumerateArray())
                        {
                            if (pageDevice != null && entry.GetProperty("device_id").GetString() != pageDevice) continue;
                            var chosen = entry.Clone();
                            LoadCover(chosen);
                            return chosen;
                        }
                    }
                    return (JsonElement?)null;
                });
                var key = (pageDevice ?? "home") + "|" + (item == null
                    ? ""
                    : $"{item.Value.GetProperty("device_id").GetString()}|{item.Value.GetProperty("title").GetString()}|{item.Value.GetProperty("playing").GetBoolean()}|{item.Value.GetProperty("position_ms").GetUInt64() / 2000}");
                if (key == _nowPlayingShown) return;
                _nowPlayingShown = key;
                NowPlayingHost.Children.Clear();
                DeviceNowPlayingHost.Children.Clear();
                if (item != null) host.Children.Add(NowPlayingCard(item.Value));
                // Empty but visible, Home's host would still take a gap in the page.
                NowPlayingHost.Visibility = NowPlayingHost.Children.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
            }
            catch (Exception ex) { App.HandleError(ex); }
            finally { _nowPlayingRefreshing = false; }
        }

        private string? _coverKey;
        private byte[]? _coverBytes;

        // Off the UI thread: fetches the cover once each time its key changes.
        private void LoadCover(JsonElement item)
        {
            var artKey = item.TryGetProperty("art_key", out var k) && k.ValueKind == JsonValueKind.String ? k.GetString() : null;
            if (artKey == _coverKey) return;
            _coverKey = artKey;
            _coverBytes = null;
            if (artKey == null) return;
            var device = item.GetProperty("device_id").GetString() ?? "";
            using var doc = DaemonClient.Send(DaemonClient.Req("now_playing_art", ("device_id", device)));
            if (doc != null
                && doc.RootElement.TryGetProperty("data", out var data)
                && data.TryGetProperty("art_base64", out var art)
                && art.ValueKind == JsonValueKind.String)
            {
                try { _coverBytes = Convert.FromBase64String(art.GetString()!); } catch (FormatException) { }
            }
        }

        private static string Clock(ulong ms) => $"{ms / 60000}:{ms / 1000 % 60:00}";

        private static void Command(string device, string action, long value = 0) =>
            _ = Task.Run(() => DaemonClient.Send(DaemonClient.Req("media_command", ("target", device), ("action", action), ("value", value)))?.Dispose());

        private UIElement NowPlayingCard(JsonElement item)
        {
            var device = item.GetProperty("device_id").GetString() ?? "";
            var playing = item.GetProperty("playing").GetBoolean();
            var position = item.GetProperty("position_ms").GetUInt64();
            var duration = item.GetProperty("duration_ms").GetUInt64();
            var artist = item.GetProperty("artist").GetString() ?? "";

            var cover = new Border
            {
                Width = 72, Height = 72, CornerRadius = new CornerRadius(10),
                Background = (Microsoft.UI.Xaml.Media.Brush)Application.Current.Resources["AppSurfaceSubtleBrush"],
            };
            // From the cache, or inline from a service older than art_key.
            var coverBytes = _coverBytes
                ?? (item.TryGetProperty("art_base64", out var art) && art.ValueKind == JsonValueKind.String
                    ? Convert.FromBase64String(art.GetString()!) : null);
            if (coverBytes != null)
            {
                try
                {
                    var image = new BitmapImage();
                    using var stream = new MemoryStream(coverBytes);
                    image.SetSource(stream.AsRandomAccessStream());
                    cover.Child = new Image { Source = image, Stretch = Microsoft.UI.Xaml.Media.Stretch.UniformToFill };
                }
                catch { cover.Child = new FontIcon { Glyph = "" }; }
            }
            else
            {
                cover.Child = new FontIcon { Glyph = "" };
            }

            var text = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
            text.Children.Add(new TextBlock { Text = item.GetProperty("title").GetString(), FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, TextTrimming = TextTrimming.CharacterEllipsis });
            text.Children.Add(new TextBlock { Text = artist.Length > 0 ? artist : item.GetProperty("album").GetString(), Opacity = 0.7, TextTrimming = TextTrimming.CharacterEllipsis });
            var app = item.GetProperty("app").GetString();
            text.Children.Add(new TextBlock
            {
                Text = $"{(string.IsNullOrEmpty(app) ? "Playing" : app)} · on {item.GetProperty("device_name").GetString()} · {Clock(position)} / {Clock(duration)}",
                Opacity = 0.55, FontSize = 12,
            });
            if (duration > 0)
            {
                var slider = new Slider { Minimum = 0, Maximum = duration, Value = Math.Min(position, duration), StepFrequency = 1000, IsThumbToolTipEnabled = false };
                slider.PointerCaptureLost += (s, e) => Command(device, "seek", (long)slider.Value);
                text.Children.Add(slider);
            }

            var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
            buttons.Children.Add(MediaButton("", "Previous", () => Command(device, "previous")));
            buttons.Children.Add(MediaButton(playing ? "" : "", playing ? "Pause" : "Play", () => Command(device, "play_pause")));
            buttons.Children.Add(MediaButton("", "Next", () => Command(device, "next")));

            var row = new Grid { ColumnSpacing = 14, Padding = new Thickness(14) };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            Grid.SetColumn(text, 1);
            Grid.SetColumn(buttons, 2);
            row.Children.Add(cover);
            row.Children.Add(text);
            row.Children.Add(buttons);
            return new Border { Style = (Style)Application.Current.Resources["AppCard"], Child = row };
        }

        private static Button MediaButton(string glyph, string label, Action onClick)
        {
            var b = new Button
            {
                Content = new FontIcon { Glyph = glyph, FontSize = 16 },
                Background = new Microsoft.UI.Xaml.Media.SolidColorBrush(Microsoft.UI.Colors.Transparent),
                BorderThickness = new Thickness(0),
                Padding = new Thickness(10),
            };
            ToolTipService.SetToolTip(b, label);
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(b, label);
            b.Click += (s, e) => onClick();
            return b;
        }
    }
}
