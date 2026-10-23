using LinkAll.WinUI.Services;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Threading.Tasks;
using Windows.ApplicationModel.DataTransfer;

namespace LinkAll.WinUI.Views
{
    public sealed partial class ScratchpadView : Page
    {
        private sealed record Note(string Id, string Text, bool Pinned, ulong UpdatedAtMs);

        private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromSeconds(3) };
        private string _shown = "";

        public ScratchpadView()
        {
            this.InitializeComponent();
            _timer.Tick += async (s, e) => await ReloadAsync();
            this.Loaded += async (s, e) => { _timer.Start(); await ReloadAsync(); };
            this.Unloaded += (s, e) => _timer.Stop();
        }

        // Notes written on another device show up here on the next poll.
        private async Task ReloadAsync()
        {
            try
            {
                var notes = await Task.Run(() =>
                {
                    using var doc = DaemonClient.Send(DaemonClient.Req("scratchpad_list"));
                    var list = new List<Note>();
                    if (doc != null
                        && doc.RootElement.TryGetProperty("data", out var data)
                        && data.ValueKind == JsonValueKind.Array)
                    {
                        foreach (var n in data.EnumerateArray())
                        {
                            list.Add(new Note(
                                n.GetProperty("id").GetString() ?? "",
                                n.GetProperty("text").GetString() ?? "",
                                n.TryGetProperty("pinned", out var p) && p.ValueKind == JsonValueKind.True,
                                n.TryGetProperty("updated_at_ms", out var t) && t.TryGetUInt64(out var ms) ? ms : 0));
                        }
                    }
                    return list;
                });
                // Rebuild only when something changed, so a note being read isn't redrawn under the cursor.
                var key = string.Join("\u0001", notes.Select(n => $"{n.Id}\u0002{n.Text}\u0002{n.Pinned}"));
                if (key == _shown) return;
                _shown = key;
                Render(notes);
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void Render(List<Note> notes)
        {
            NotesPanel.Children.Clear();
            CountText.Text = notes.Count == 0
                ? "Nothing here yet. Add something and it appears on your other devices."
                : $"{notes.Count} {(notes.Count == 1 ? "note" : "notes")} · shared with your paired devices only";
            ClearButton.Visibility = notes.Any(n => !n.Pinned) ? Visibility.Visible : Visibility.Collapsed;
            foreach (var note in notes) NotesPanel.Children.Add(Card(note));
        }

        private UIElement Card(Note note)
        {
            var text = new TextBlock
            {
                Text = note.Text,
                TextWrapping = TextWrapping.Wrap,
                IsTextSelectionEnabled = true,
                MaxLines = 12,
                TextTrimming = TextTrimming.CharacterEllipsis,
            };
            var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4 };
            buttons.Children.Add(IconButton("", "Copy", () => Copy(note.Text)));
            buttons.Children.Add(IconButton(note.Pinned ? "" : "", note.Pinned ? "Unpin" : "Pin", async () => await TogglePinAsync(note)));
            buttons.Children.Add(IconButton("", "Edit", async () => await EditAsync(note)));
            buttons.Children.Add(IconButton("", "Delete", async () => await DeleteAsync(note)));

            var column = new StackPanel { Spacing = 8 };
            column.Children.Add(text);
            column.Children.Add(buttons);
            return new Border
            {
                Style = (Style)Application.Current.Resources["AppCard"],
                Padding = new Thickness(16, 12, 16, 8),
                Child = column,
            };
        }

        private static Button IconButton(string glyph, string label, Action onClick)
        {
            var b = new Button
            {
                Content = new FontIcon { Glyph = glyph, FontSize = 14 },
                Background = new Microsoft.UI.Xaml.Media.SolidColorBrush(Microsoft.UI.Colors.Transparent),
                BorderThickness = new Thickness(0),
                Padding = new Thickness(8, 6, 8, 6),
            };
            ToolTipService.SetToolTip(b, label);
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(b, label);
            b.Click += (s, e) => onClick();
            return b;
        }

        private static void Copy(string text)
        {
            var package = new DataPackage();
            package.SetText(text);
            Clipboard.SetContent(package);
        }

        private async Task SendAsync(string label, JsonObject request)
        {
            var resp = await Task.Run(() => DaemonClient.Send(request));
            DaemonActions.ReportIfFailed(label, resp);
            resp?.Dispose();
            _shown = "";
            await ReloadAsync();
        }

        private async void OnAdd(object sender, RoutedEventArgs e)
        {
            var text = Draft.Text.Trim();
            if (text.Length == 0) return;
            Draft.Text = "";
            await SendAsync("Save note", DaemonClient.Req("scratchpad_save", ("text", text), ("pinned", false)));
        }

        private Task TogglePinAsync(Note note) =>
            SendAsync("Pin note", DaemonClient.Req("scratchpad_save", ("id", note.Id), ("text", note.Text), ("pinned", !note.Pinned)));

        private Task DeleteAsync(Note note) =>
            SendAsync("Delete note", DaemonClient.Req("scratchpad_delete", ("id", note.Id)));

        private async Task EditAsync(Note note)
        {
            var box = new TextBox
            {
                Text = note.Text,
                AcceptsReturn = true,
                TextWrapping = TextWrapping.Wrap,
                MinHeight = 200,
                MinWidth = 380,
                FontFamily = new Microsoft.UI.Xaml.Media.FontFamily("Consolas"),
            };
            var dialog = new ContentDialog
            {
                Title = Services.AppDialog.Header("", "Edit note", "Saved here and on your paired devices"),
                Content = box,
                PrimaryButtonText = "Save",
                CloseButtonText = "Cancel",
                DefaultButton = ContentDialogButton.Primary,
                XamlRoot = this.XamlRoot,
            };
            if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
            var text = box.Text.Trim();
            if (text.Length == 0) return;
            await SendAsync("Save note", DaemonClient.Req("scratchpad_save", ("id", note.Id), ("text", text), ("pinned", note.Pinned)));
        }

        private async void OnClearAll(object sender, RoutedEventArgs e)
        {
            var dialog = new ContentDialog
            {
                Title = Services.AppDialog.Header("", "Clear all notes?", "Pinned notes stay", danger: true),
                Content = "This deletes every note that isn't pinned, here and on your paired devices.",
                PrimaryButtonText = "Clear all",
                CloseButtonText = "Cancel",
                DefaultButton = ContentDialogButton.Close,
                XamlRoot = this.XamlRoot,
            };
            if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
            await SendAsync("Clear notes", DaemonClient.Req("scratchpad_clear"));
        }
    }
}
