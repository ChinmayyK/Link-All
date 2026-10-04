using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using System;
using System.Collections.ObjectModel;
using System.Linq;

namespace LinkAll.WinUI.Views
{
    public sealed partial class ClipboardView : Page
    {
        public LinkAllStore mgr => LinkAllStore.Shared;
        public ObservableCollection<ActivityEntry> FilteredFeed { get; } = new ObservableCollection<ActivityEntry>();
        private string _activeFilter = "All";

        public ClipboardView()
        {
            this.InitializeComponent();
            mgr.ActivityFeed.CollectionChanged += OnActivityFeedChanged;
            this.Unloaded += (s, e) => {
                try { mgr.ActivityFeed.CollectionChanged -= OnActivityFeedChanged; } catch (Exception ex) { App.HandleError(ex); }
            };
            UpdateFilter();
        }

        private void OnActivityFeedChanged(object? sender, System.Collections.Specialized.NotifyCollectionChangedEventArgs e)
        {
            UpdateFilter();
        }

        private void UpdateFilter()
        {
            try
            {
                var query = SearchBox?.Text?.Trim() ?? "";
                var snapshot = mgr.ActivityFeed
                    .Where(i => i.kind is "clipboard_text" or "clipboard_image" or "remote_clipboard_available" or "clipboard_applied" or "file_transfer_complete")
                    .ToList();
                var items = snapshot.AsEnumerable();

                if (!string.IsNullOrEmpty(query))
                {
                    items = items.Where(i => (i.Title?.Contains(query, StringComparison.OrdinalIgnoreCase) ?? false) ||
                                             (i.Source?.Contains(query, StringComparison.OrdinalIgnoreCase) ?? false) ||
                                             (i.TypeLabel?.Contains(query, StringComparison.OrdinalIgnoreCase) ?? false));
                }

                if (_activeFilter != "All")
                {
                    items = items.Where(i => string.Equals(i.TypeLabel, _activeFilter, StringComparison.OrdinalIgnoreCase) ||
                                             (_activeFilter == "File" && (i.TypeLabel == "File" || i.TypeLabel == "Applied")) ||
                                             (_activeFilter == "Text" && i.TypeLabel == "Clipboard"));
                }

                FilteredFeed.Clear();
                foreach (var item in items)
                {
                    FilteredFeed.Add(item);
                }

                UpdateEmptyState(isNarrowed: !string.IsNullOrEmpty(query) || _activeFilter != "All");
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // "Nothing has ever been copied" and "nothing matches your filter"
        // need different words and different actions - offering "push this
        // PC's clipboard" to someone whose search just missed is noise.
        private void UpdateEmptyState(bool isNarrowed)
        {
            if (EmptyStatePanel == null) return;

            // The history list and the empty state take turns; an empty
            // grouped panel would draw as a stray outline.
            if (HistoryPanel != null)
                HistoryPanel.Visibility = FilteredFeed.Count > 0 ? Visibility.Visible : Visibility.Collapsed;

            if (FilteredFeed.Count > 0)
            {
                EmptyStatePanel.Visibility = Visibility.Collapsed;
                return;
            }

            EmptyStatePanel.Visibility = Visibility.Visible;

            if (isNarrowed)
            {
                EmptyStateTitle.Text = "No matches";
                EmptyStateDetail.Text = "Nothing in your clipboard history matches that search or filter.";
                EmptyStateAction.Visibility = Visibility.Collapsed;
            }
            else
            {
                EmptyStateTitle.Text = "Nothing here yet";
                EmptyStateDetail.Text = "Copy something on this PC or a paired device and it will show up here, "
                                     + "ready to paste on either end.";
                EmptyStateAction.Visibility = Visibility.Visible;
            }
        }

        private void OnSearchTextChanged(object sender, TextChangedEventArgs e)
        {
            UpdateFilter();
        }

        // Selection is expressed by swapping the whole style rather than
        // poking Background and Foreground individually. That way the
        // selected and unselected chips keep their full set of hover, press
        // and disabled states instead of losing them the moment we overwrite
        // one brush by hand.
        private void OnFilterChipClicked(object sender, RoutedEventArgs e)
        {
            if (sender is Button btn && btn.Content is string tag)
            {
                _activeFilter = tag;
                ApplyChipSelection();
                UpdateFilter();
            }
        }

        private void ApplyChipSelection()
        {
            try
            {
                var selected = (Style)Application.Current.Resources["AppPrimaryButton"];
                var unselected = (Style)Application.Current.Resources["AppSecondaryButton"];

                ChipAll.Style = _activeFilter == "All" ? selected : unselected;
                ChipText.Style = _activeFilter == "Text" ? selected : unselected;
                ChipImage.Style = _activeFilter == "Image" ? selected : unselected;
                ChipFile.Style = _activeFilter == "File" ? selected : unselected;
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // Pushing an older entry back out to a device: the same operation as
        // "Push clipboard", but sourced from history instead of the live
        // Windows clipboard.
        private async void OnSendActivityClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is not ActivityEntry entry) return;

            var text = !string.IsNullOrEmpty(entry.text_preview) ? entry.text_preview : entry.Title;
            if (string.IsNullOrEmpty(text)) return;

            try
            {
                var target = await LinkAll.WinUI.Services.DevicePicker.PickAsync(this.XamlRoot, mgr.ConnectedPeers);
                if (target != null)
                {
                    mgr.SendPushText(text, target.device_id);
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private async void OnPushLocalClipboardClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                await LinkAll.WinUI.Services.ClipboardManager.SendLocalClipboardAsync(this.XamlRoot);
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
            }
        }

        private void OnCopyActivityTextClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is ActivityEntry entry)
            {
                var text = !string.IsNullOrEmpty(entry.text_preview) ? entry.text_preview : entry.Title;
                if (!string.IsNullOrEmpty(text))
                {
                    var dp = new Windows.ApplicationModel.DataTransfer.DataPackage();
                    dp.SetText(text);
                    Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(dp);
                }
            }
        }

        private void OnApplyPendingClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PendingClipboard pending && !string.IsNullOrEmpty(pending.content_hash))
            {
                mgr.ApplyClipboardItem(pending.content_hash);
            }
        }

        private void OnApplyActivityClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is ActivityEntry entry && !string.IsNullOrEmpty(entry.content_hash))
            {
                mgr.ApplyClipboardItem(entry.content_hash);
            }
        }
    }
}
