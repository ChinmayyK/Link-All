using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System.Linq;

namespace LinkAll.WinUI.Views
{
    public sealed partial class ActivityView : Page
    {
        public ActivityView()
        {
            this.InitializeComponent();
            LinkAllStore.Shared.PropertyChanged += OnStorePropertyChanged;
            this.Unloaded += (s, e) => {
                try { LinkAllStore.Shared.PropertyChanged -= OnStorePropertyChanged; } catch (Exception ex) { App.HandleError(ex); }
            };
            try { TimelineList.ItemsSource = LinkAllStore.Shared.History.ToList(); } catch (Exception ex) { App.HandleError(ex); }
            UpdateEmptyState();
        }

        // Two different empty states, because they need two different
        // answers: an untouched log tells you what will appear here, whereas
        // a filtered-to-nothing list tells you to widen the filter. A single
        // "No activity yet" message for both is actively misleading.
        private void UpdateEmptyState()
        {
            var count = TimelineList.ItemsSource is System.Collections.ICollection collection ? collection.Count : 0;
            var isFiltered = !string.IsNullOrWhiteSpace(TxtSearch?.Text);

            CountText.Text = count switch
            {
                0 => "",
                1 => "1 event",
                _ => $"{count} events",
            };

            EmptyStateBorder.Visibility = count == 0 ? Visibility.Visible : Visibility.Collapsed;
            // An empty grouped panel would draw as a stray outline.
            TimelinePanel.Visibility = count == 0 ? Visibility.Collapsed : Visibility.Visible;
            if (count > 0) return;

            if (isFiltered)
            {
                EmptyTitle.Text = "No matching events";
                EmptyDetail.Text = "Nothing in the log matches that filter. Try a shorter search term.";
            }
            else
            {
                EmptyTitle.Text = "No activity yet";
                EmptyDetail.Text = "Clipboard syncs, file transfers, and connection events will be recorded here.";
            }
        }

        private void OnStorePropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
        {
            if (e.PropertyName == nameof(LinkAllStore.Shared.History))
            {
                DispatcherQueue?.TryEnqueue(() => {
                    try { TimelineList.ItemsSource = LinkAllStore.Shared.History.ToList(); } catch (Exception ex) { App.HandleError(ex); }
                    UpdateEmptyState();
                });
            }
        }

        private void TxtSearch_TextChanged(object sender, TextChangedEventArgs e)
        {
            try
            {
                var text = TxtSearch.Text?.ToLowerInvariant() ?? "";
                if (string.IsNullOrWhiteSpace(text))
                {
                    TimelineList.ItemsSource = LinkAllStore.Shared.History.ToList();
                }
                else
                {
                    var snapshot = LinkAllStore.Shared.History.ToList();
                    TimelineList.ItemsSource = snapshot.Where(h =>
                        (h.display_text?.ToLowerInvariant().Contains(text) == true) ||
                        (h.path?.ToLowerInvariant().Contains(text) == true)).ToList();
                }
                UpdateEmptyState();
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void HistoryItem_Click(object sender, RoutedEventArgs e)
        {
            if (sender is Button btn && btn.DataContext is HistoryItem item)
            {
                if (item.is_text && !string.IsNullOrEmpty(item.FullText))
                {
                    try {
                        var package = new Windows.ApplicationModel.DataTransfer.DataPackage();
                        package.SetText(item.FullText);
                        Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
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
            }
        }

        private void BtnPinItem_Click(object sender, RoutedEventArgs e)
        {
            try
            {
                if (sender is Button btn && btn.DataContext is HistoryItem item)
                {
                    item.IsPinned = !item.IsPinned;
                    var snapshot = LinkAllStore.Shared.History.ToList();
                    TimelineList.ItemsSource = snapshot
                        .OrderByDescending(h => h.IsPinned)
                        .ThenByDescending(h => h.Time)
                        .ToList();
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void BtnDeleteItem_Click(object sender, RoutedEventArgs e)
        {
            try
            {
                if (sender is Button btn && btn.DataContext is HistoryItem item)
                {
                    LinkAllStore.Shared.History.Remove(item);
                    App.Clipboard?.History.Remove(item);
                    TimelineList.ItemsSource = LinkAllStore.Shared.History.ToList();
                    UpdateEmptyState();
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }
    }
}
