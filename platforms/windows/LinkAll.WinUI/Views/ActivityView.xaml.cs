using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System.Collections.Generic;
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
            ShowFeed();
        }

        // The daemon's activity log: transfers, clips and connections, each
        // as its own sentence. Clipboard history lives on the Clipboard page.
        private void ShowFeed()
        {
            try
            {
                IEnumerable<ActivityEntry> feed = ActivityEntry.WithoutRepeats(LinkAllStore.Shared.ActivityFeed?.ToList() ?? new List<ActivityEntry>());
                var text = TxtSearch?.Text?.Trim();
                if (!string.IsNullOrEmpty(text))
                {
                    feed = feed.Where(e => e.summary.Contains(text, StringComparison.OrdinalIgnoreCase));
                }
                TimelineList.ItemsSource = feed.ToList();
            }
            catch (Exception ex) { App.HandleError(ex); }
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
            if (e.PropertyName == nameof(LinkAllStore.Shared.ActivityFeed))
            {
                DispatcherQueue?.TryEnqueue(ShowFeed);
            }
        }

        private void TxtSearch_TextChanged(object sender, TextChangedEventArgs e) => ShowFeed();

        // A received file opens; everything else has nothing to open.
        private void ActivityRow_Tapped(object sender, Microsoft.UI.Xaml.Input.TappedRoutedEventArgs e)
        {
            if (sender is FrameworkElement el && el.DataContext is ActivityEntry entry
                && !string.IsNullOrEmpty(entry.dest_path) && System.IO.File.Exists(entry.dest_path))
            {
                try
                {
                    System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(entry.dest_path) { UseShellExecute = true });
                }
                catch (Exception ex) { App.HandleError(ex); }
            }
        }
    }
}
