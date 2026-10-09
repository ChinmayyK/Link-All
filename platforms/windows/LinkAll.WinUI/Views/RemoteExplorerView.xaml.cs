using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using System;
using System.Collections.ObjectModel;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;

namespace LinkAll.WinUI.Views
{
    public sealed partial class RemoteExplorerView : Page
    {
        public LinkAllStore mgr => LinkAllStore.Shared;
        public ObservableCollection<RemoteFile> RemoteFiles { get; } = new ObservableCollection<RemoteFile>();

        // Breadcrumb trail for the location bar. Index 0 is always the root,
        // labelled for humans rather than shown as "/".
        public ObservableCollection<string> PathSegments { get; } = new ObservableCollection<string> { RootSegmentLabel };
        private const string RootSegmentLabel = "All files";
        // Parallel to PathSegments (same indices), holding the real "/"-rooted
        // path each breadcrumb entry navigates to - needed because a shortcut
        // segment's display label ("Pictures") isn't the wire path
        // ("/category/Images") a breadcrumb click must send back through
        // LoadRemoteDirectory.
        private readonly System.Collections.Generic.List<string> _pathSegmentTargets = new() { "/" };

        private string _currentPath = "/";
        // Bumped at the start of every LoadRemoteDirectory call and captured
        // as a ticket; a completion whose ticket no longer matches means a
        // newer navigation has since started, so its result is discarded
        // instead of clobbering the UI with a stale directory listing.
        private int _loadGeneration = 0;
        private static readonly System.Collections.Generic.Dictionary<string, JsonDocument> _cache = new();
        private static readonly System.Collections.Generic.Dictionary<string, Microsoft.UI.Xaml.Media.Imaging.BitmapImage> _thumbnailCache = new();
        // Insertion order for _thumbnailCache, so the oldest previews are
        // dropped once MaxCachedThumbnails is reached. A decoded 320px
        // preview is ~0.4 MB and the cache outlives this page, so unbounded
        // it grew with every library browsed for the life of the process.
        private static readonly System.Collections.Generic.Queue<string> _thumbnailCacheOrder = new();
        private const int MaxCachedThumbnails = 150;

        // Each thumbnail is a peer-to-peer round trip (the daemon asks the
        // Android device to generate/send one). The request/response
        // protocol is keyed by a per-call request_id (engine/mod.rs's
        // remote_thumb_waiters), not serialized on a single lane, and
        // Android services these on an unbounded thread pool - so there's
        // no protocol-level reason to keep this near-sequential. It was
        // capped at 2 on the (incorrect) assumption that the IPC pipe only
        // carries one request at a time; raised to let a scrolled-into-view
        // batch actually fetch in parallel instead of queueing almost
        // one-at-a-time behind each other's timeouts.
        private static readonly SemaphoreSlim _thumbnailThrottle = new(8, 8);

        // Sized for the grid view's large tiles (~200 DIP wide at up to 150%
        // scaling) so previews stay sharp; the list's small thumbnail shares
        // the same cached bitmap.
        private const uint ThumbnailSizePx = 320;

        public RemoteExplorerView()
        {
            this.InitializeComponent();
            this.Loaded += (s, e) =>
            {
                mgr.PropertyChanged += OnStorePropertyChanged;
                SyncDeviceSwitcher();
                ApplyViewMode();
                _ = LoadRemoteDirectory("/");
            };
            this.Unloaded += (s, e) => mgr.PropertyChanged -= OnStorePropertyChanged;
        }

        // Set while SyncDeviceSwitcher writes the ComboBox, so the programmatic
        // selection isn't mistaken for the user picking a device.
        private bool _syncingDeviceSwitcher;

        private void OnStorePropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
        {
            if (e.PropertyName is nameof(LinkAllStore.ConnectedPeers) or nameof(LinkAllStore.SelectedPeer))
            {
                DispatcherQueue.TryEnqueue(SyncDeviceSwitcher);
            }
        }

        // Mirror the store's connected devices into the switcher. It only
        // appears when there is a real choice; a single device keeps the
        // plain name label.
        private void SyncDeviceSwitcher()
        {
            var peers = mgr.ConnectedPeers;
            var selectedId = mgr.SelectedPeer?.device_id;
            var showSwitcher = peers.Count > 1;

            _syncingDeviceSwitcher = true;
            try
            {
                if (!ReferenceEquals(DeviceSwitcher.ItemsSource, peers)) DeviceSwitcher.ItemsSource = peers;
                DeviceSwitcher.SelectedItem = System.Linq.Enumerable.FirstOrDefault(peers, p => p.device_id == selectedId);
            }
            finally { _syncingDeviceSwitcher = false; }

            DeviceSwitcher.Visibility = showSwitcher ? Visibility.Visible : Visibility.Collapsed;
            DeviceNameText.Visibility = showSwitcher ? Visibility.Collapsed : Visibility.Visible;
        }

        private void OnDeviceSwitcherSelectionChanged(object sender, SelectionChangedEventArgs e)
        {
            if (_syncingDeviceSwitcher) return;
            if (DeviceSwitcher.SelectedItem is not PeerViewModel peer) return;
            if (peer.device_id == mgr.SelectedPeer?.device_id) return;

            mgr.SelectedPeer = peer;
            // A different device has a different file tree: start again from
            // its root rather than reusing the previous device's path.
            RemoteFiles.Clear();
            _ = LoadRemoteDirectory("/");
        }

        private async System.Threading.Tasks.Task LoadRemoteDirectory(string path, bool forceRefresh = false)
        {
            var myGeneration = ++_loadGeneration;
            if (string.IsNullOrEmpty(path)) path = "/";
            _currentPath = path;
            if (PathBox != null) PathBox.Text = _currentPath;
            UpdatePathSegments();
            UpdateActiveLibrary();

            var peer = mgr.SelectedPeer;
            if (peer == null || string.IsNullOrEmpty(peer.device_id))
            {
                UpdateEmptyStates();
                return;
            }

            try
            {
                string? category = null;
                string? source = null;
                if (_currentPath.StartsWith("/category/", StringComparison.OrdinalIgnoreCase))
                    category = _currentPath.Substring("/category/".Length);
                else if (_currentPath.StartsWith("/source/", StringComparison.OrdinalIgnoreCase))
                    source = _currentPath.Substring("/source/".Length);

                string cacheKey = $"{peer.device_id}_{_currentPath}";
                JsonDocument? doc = null;

                if (!forceRefresh && _cache.TryGetValue(cacheKey, out var cachedDoc))
                {
                    doc = cachedDoc;
                }
                else
                {
                    doc = await DaemonClient.RemoteFilesQueryAsync(peer.device_id, summaryOnly: false, category: category, source: source);
                    if (doc != null)
                    {
                        _cache[cacheKey] = doc;
                    }
                }

                if (doc != null && doc.RootElement.ValueKind != JsonValueKind.Null)
                {
                    RemoteFileListResponse? resp = null;
                    if (doc.RootElement.TryGetProperty("files", out _))
                    {
                        resp = JsonSerializer.Deserialize(doc.RootElement.GetRawText(), LinkAllJsonContext.Default.RemoteFileListResponse);
                    }
                    else if (doc.RootElement.TryGetProperty("data", out var dataEl) && dataEl.TryGetProperty("files", out _))
                    {
                        resp = JsonSerializer.Deserialize(dataEl.GetRawText(), LinkAllJsonContext.Default.RemoteFileListResponse);
                    }
                    
                    App.MainDispatcherQueue?.TryEnqueue(() =>
                    {
                        if (myGeneration != _loadGeneration) return;
                        RemoteFiles.Clear();
                        if (resp != null && resp.files != null)
                        {
                            foreach (var f in resp.files)
                            {
                                RemoteFiles.Add(f);
                            }
                        }
                        UpdateEmptyStates();
                    });
                }
            }
            catch (Exception)
            {
                // Handle network or serialization errors gracefully
            }
        }

        // Mirrors the sidebar's own labels (RemoteExplorerView.xaml) for the
        // "/category/<x>" and "/source/<x>" paths those buttons navigate to,
        // so the breadcrumb reads "All files > Pictures" instead of the raw
        // "category > Images" wire values.
        private static readonly System.Collections.Generic.Dictionary<string, string> ShortcutLabels = new(StringComparer.OrdinalIgnoreCase)
        {
            ["category/Images"] = "Pictures",
            ["category/Documents"] = "Documents",
            ["category/Audio"] = "Music",
            ["category/Videos"] = "Videos",
            ["source/Camera"] = "Camera",
            ["source/Downloads"] = "Downloads",
        };

        // Rebuilds the breadcrumb from the current path. Kept as a plain
        // rebuild rather than a diff: the trail is at most a handful of
        // items, and correctness beats cleverness here.
        private void UpdatePathSegments()
        {
            try
            {
                PathSegments.Clear();
                PathSegments.Add(RootSegmentLabel);
                _pathSegmentTargets.Clear();
                _pathSegmentTargets.Add("/");

                var trimmed = (_currentPath ?? "/").Trim('/');
                if (trimmed.Length == 0) return;

                if (ShortcutLabels.TryGetValue(trimmed, out var label))
                {
                    PathSegments.Add(label);
                    _pathSegmentTargets.Add(_currentPath);
                    return;
                }

                var parts = trimmed.Split('/', StringSplitOptions.RemoveEmptyEntries);
                var built = "";
                foreach (var segment in parts)
                {
                    built += "/" + segment;
                    PathSegments.Add(segment);
                    _pathSegmentTargets.Add(built);
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void OnBreadcrumbItemClicked(Microsoft.UI.Xaml.Controls.BreadcrumbBar sender,
                                             Microsoft.UI.Xaml.Controls.BreadcrumbBarItemClickedEventArgs args)
        {
            try
            {
                // Index 0 is the synthetic root label; anything beyond it maps
                // back onto _pathSegmentTargets, the real path each
                // breadcrumb entry was built from (not always a plain join of
                // the display labels - see UpdatePathSegments).
                if (args.Index >= 0 && args.Index < _pathSegmentTargets.Count)
                {
                    _ = LoadRemoteDirectory(_pathSegmentTargets[args.Index]);
                }
                else
                {
                    _ = LoadRemoteDirectory("/");
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // "No device" and "empty folder" are different problems with
        // different fixes, so they get different empty states instead of one
        // message that is wrong half the time.
        private void UpdateEmptyStates()
        {
            try
            {
                var hasPeer = mgr.SelectedPeer != null && !string.IsNullOrEmpty(mgr.SelectedPeer.device_id);
                var hasFiles = RemoteFiles.Count > 0;

                ItemCountText.Text = RemoteFiles.Count == 1 ? "1 item" : $"{RemoteFiles.Count} items";

                NoDeviceState.Visibility = hasPeer ? Visibility.Collapsed : Visibility.Visible;
                EmptyFolderState.Visibility = (hasPeer && !hasFiles) ? Visibility.Visible : Visibility.Collapsed;
                FileGrid.Visibility = (hasFiles && _gridMode) ? Visibility.Visible : Visibility.Collapsed;
                FilePanel.Visibility = (hasFiles && !_gridMode) ? Visibility.Visible : Visibility.Collapsed;
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void OnGoToDevicesClicked(object sender, RoutedEventArgs e)
        {
            DashboardWindow.Current?.NavigateTo("Devices");
        }

        private void OnBackClicked(object sender, RoutedEventArgs e)
        {
            if (_currentPath == "/" || string.IsNullOrEmpty(_currentPath))
            {
                DashboardWindow.Current?.NavigateTo("Devices");
                return;
            }

            var trimmed = _currentPath.TrimEnd('/');
            var lastSlash = trimmed.LastIndexOf('/');
            if (lastSlash <= 0)
            {
                _ = LoadRemoteDirectory("/");
            }
            else
            {
                _ = LoadRemoteDirectory(trimmed.Substring(0, lastSlash));
            }
        }

        private void OnPathKeyDown(object sender, KeyRoutedEventArgs e)
        {
            if (e.Key == Windows.System.VirtualKey.Enter && PathBox != null)
            {
                _ = LoadRemoteDirectory(PathBox.Text.Trim());
            }
        }

        private void OnGoClicked(object sender, RoutedEventArgs e)
        {
            if (PathBox != null)
            {
                _ = LoadRemoteDirectory(PathBox.Text.Trim());
            }
        }

        private void OnSendFilesClicked(object sender, RoutedEventArgs e)
        {
            var peer = mgr.SelectedPeer;
            if (peer != null)
            {
                _ = mgr.PickAndSendFiles(peer.device_id);
            }
        }

        private void OnRefreshClicked(object sender, RoutedEventArgs e)
        {
            _ = LoadRemoteDirectory(_currentPath, true);
        }

        // Android's remote browsing is flat/category-based (LinkAllStore.cs's
        // RemoteFile comment), not real folders - each library row's Tag is a
        // "/category/<RemoteFileCategory>" or "/source/<RemoteFileSource>"
        // path that LoadRemoteDirectory parses (protocol.rs's enum variant
        // names), or "/" for the unfiltered listing.
        private void OnShortcutClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.Tag is string path) _ = LoadRemoteDirectory(path);
        }

        // Tints the library row being browsed. A path that isn't exactly a
        // library (typed into "Go to path") highlights nothing rather than
        // guessing.
        private void UpdateActiveLibrary()
        {
            foreach (var child in LibraryList.Children)
            {
                if (child is Button button)
                    SetActive(button, string.Equals(button.Tag as string, _currentPath, StringComparison.OrdinalIgnoreCase));
            }
        }

        private static void SetActive(Button button, bool active)
        {
            if (active)
            {
                button.Background = (Microsoft.UI.Xaml.Media.Brush)Application.Current.Resources["AppAccentSubtleBrush"];
                button.Foreground = (Microsoft.UI.Xaml.Media.Brush)Application.Current.Resources["AppAccentBrush"];
            }
            else
            {
                button.ClearValue(Control.BackgroundProperty);
                button.ClearValue(Control.ForegroundProperty);
            }
        }

        // ------------------------------------------------ view mode

        // Large previews by default: the point of this page is recognising
        // what's on the phone, which a 36px thumbnail can't do.
        private const string ViewModeSettingKey = "RemoteFilesViewMode";
        private bool _gridMode = Services.LocalSettingsStore.Get(ViewModeSettingKey) != "list";

        private void OnGridViewClicked(object sender, RoutedEventArgs e) => SetViewMode(grid: true);
        private void OnListViewClicked(object sender, RoutedEventArgs e) => SetViewMode(grid: false);

        private void SetViewMode(bool grid)
        {
            if (_gridMode == grid) return;
            _gridMode = grid;
            Services.LocalSettingsStore.Set(ViewModeSettingKey, grid ? "grid" : "list");
            ApplyViewMode();
        }

        private void ApplyViewMode()
        {
            SetActive(GridViewButton, _gridMode);
            SetActive(ListViewButton, !_gridMode);
            UpdateEmptyStates();
        }

        // Grid tiles are at least MinTileWidth wide and stretched so each row
        // fills edge to edge. The preview keeps a fixed aspect ratio and the
        // text under it is a fixed height (two name lines plus the size/date
        // line), so ItemHeight follows directly from the width.
        private const double MinTileWidth = 160;
        private const double TileGap = 10;          // AppPlainGridItem's right/bottom margin
        private const double TileChrome = 14;       // tile border (1+1) + padding (6+6)
        private const double TileTextHeight = 64;   // row spacing 8 + name 36 + spacing 3 + caption ~15 + bottom padding 4
        private const double PreviewAspect = 0.8;   // height / width
        private const double ScrollbarGutter = 16;

        private void OnFileGridSizeChanged(object sender, SizeChangedEventArgs e) => FitTiles();

        private ScrollViewer? _fileGridScroller;

        private static T? FindDescendant<T>(DependencyObject root) where T : DependencyObject
        {
            var count = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChildrenCount(root);
            for (var i = 0; i < count; i++)
            {
                var child = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChild(root, i);
                if (child is T match) return match;
                if (FindDescendant<T>(child) is { } nested) return nested;
            }
            return null;
        }

        // Measured from the GridView's inner ScrollViewer viewport, not the
        // GridView itself: the template's gutters make the panel a few DIPs
        // narrower than the control, which was enough to wrap the third tile.
        private void FitTiles()
        {
            if (_fileGridScroller == null)
            {
                _fileGridScroller = FindDescendant<ScrollViewer>(FileGrid);
                if (_fileGridScroller != null) _fileGridScroller.SizeChanged += (_, _) => FitTiles();
            }
            if (FileGrid.ItemsPanelRoot is not ItemsWrapGrid panel)
            {
                // The panel is only created once the first items realise,
                // which can land after the first size change - retry then.
                DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, () =>
                {
                    if (FileGrid.ItemsPanelRoot is ItemsWrapGrid) FitTiles();
                });
                return;
            }
            // ScrollbarGutter: the row loses a few DIPs to the vertical
            // scrollbar that ViewportWidth does not report, which was enough
            // to wrap the third tile at ~1000 DIP windows. Reserving it also
            // keeps the scrollbar off the last column.
            var available = (_fileGridScroller?.ViewportWidth ?? FileGrid.ActualWidth) - FileGrid.Padding.Left - FileGrid.Padding.Right - ScrollbarGutter;
            if (available <= 0) return;

            var columns = Math.Max(1, (int)((available + TileGap) / (MinTileWidth + TileGap)));
            // Snap to whole physical pixels: at 150% a 175 DIP cell rounds up
            // to 263px, three of those overflow the row by a pixel and the
            // third tile wraps, leaving two tiles and a gap.
            var scale = XamlRoot?.RasterizationScale ?? 1.0;
            var cell = Math.Floor((available * scale - 1) / columns) / scale;
            var previewWidth = cell - TileGap - TileChrome;

            panel.ItemWidth = cell;
            panel.ItemHeight = Math.Round(previewWidth * PreviewAspect + TileTextHeight + TileChrome + TileGap);
            // ItemsWrapGrid keeps the column count from its first measure
            // (taken before the page had its final width) and does not
            // re-wrap when only ItemWidth changes - that is why the default
            // window showed two columns until the user resized it. Pinning
            // the count and re-measuring makes the layout match this math.
            panel.MaximumRowsOrColumns = columns;
            panel.InvalidateMeasure();
            DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, () =>
            {
                panel.InvalidateMeasure();
                FileGrid.InvalidateMeasure();
            });
        }

        // The file menu is one shared resource used by every tile and row, so
        // its items don't inherit a DataContext from whichever element opened
        // it - hand them the target's file before the menu shows so the Click
        // handlers can resolve which file they act on.
        private void OnFileMenuOpening(object sender, object e)
        {
            if (sender is not MenuFlyout menu) return;
            var item = (menu.Target as FrameworkElement)?.DataContext;
            foreach (var entry in menu.Items) entry.DataContext = item;
        }

        // Double-click to open, matching File Explorer. Single-click used to
        // navigate, which made it impossible to hover a row without being
        // taken somewhere - the explicit Open button and the context menu
        // cover the same ground for anyone who prefers them.
        private void OnFileRowDoubleTapped(object sender, DoubleTappedRoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is RemoteFile item)
            {
                OpenIfDirectory(item);
            }
        }

        private void OpenIfDirectory(RemoteFile item)
        {
            if (!item.is_dir) return;
            var nextPath = _currentPath.TrimEnd('/') + "/" + item.display_name;
            _ = LoadRemoteDirectory(nextPath);
        }

        private async void OnDownloadClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is RemoteFile item)
            {
                var peer = mgr.SelectedPeer;
                if (peer != null)
                {
                    ulong fileId = item.file_id > 0 ? item.file_id : (ulong.TryParse(item.id, out var fid) ? fid : 0);
                    if (fileId > 0)
                    {
                        var resp = await Task.Run(() => DaemonClient.RemoteFilePullRequest(peer.device_id, fileId));
                        DaemonActions.ReportIfFailed("Download", resp);
                    }
                }
            }
        }

        private static ulong ResolveFileId(RemoteFile item) =>
            item.file_id > 0 ? item.file_id : (ulong.TryParse(item.id, out var fid) ? fid : 0);

        private async void OnRenameRemoteFileClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is not RemoteFile item) return;
            var peer = mgr.SelectedPeer;
            var fileId = ResolveFileId(item);
            if (peer == null || fileId == 0) return;

            var input = new TextBox { Text = item.display_name, SelectionStart = 0, SelectionLength = item.display_name.Length };
            var dialog = new ContentDialog
            {
                Title = Services.AppDialog.Header("\uE8AC", "Rename", $"On {peer.DisplayName}"),
                Content = input,
                PrimaryButtonText = "Rename",
                CloseButtonText = "Cancel",
                DefaultButton = ContentDialogButton.Primary,
                XamlRoot = this.XamlRoot,
            };

            var result = await dialog.ShowAsync();
            if (result != ContentDialogResult.Primary) return;

            var newName = input.Text?.Trim();
            if (string.IsNullOrEmpty(newName) || newName == item.display_name) return;

            try
            {
                var resp = await Task.Run(() => DaemonClient.RemoteFileActionRequest(peer.device_id, fileId, "rename", newName));
                DaemonActions.ReportIfFailed("Rename", resp);
                await LoadRemoteDirectory(_currentPath, forceRefresh: true);
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private async void OnDeleteRemoteFileClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is not RemoteFile item) return;
            var peer = mgr.SelectedPeer;
            var fileId = ResolveFileId(item);
            if (peer == null || fileId == 0) return;

            var dialog = new ContentDialog
            {
                Title = Services.AppDialog.Header("\uE74D", "Delete this item?", "This can't be undone", danger: true),
                Content = $"\"{item.display_name}\" will be permanently deleted from {peer.DisplayName}. This can't be undone.",
                PrimaryButtonText = "Delete",
                CloseButtonText = "Cancel",
                DefaultButton = ContentDialogButton.Close,
                XamlRoot = this.XamlRoot,
            };

            var result = await dialog.ShowAsync();
            if (result != ContentDialogResult.Primary) return;

            try
            {
                var resp = await Task.Run(() => DaemonClient.RemoteFileActionRequest(peer.device_id, fileId, "delete"));
                DaemonActions.ReportIfFailed("Delete", resp);
                RemoteFiles.Remove(item);
                await LoadRemoteDirectory(_currentPath, forceRefresh: true);
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // Fires as rows scroll into view (and on recycle) - only kick off a
        // thumbnail fetch for image/video files that don't have one yet,
        // mirroring macOS's onAppear-triggered requestRemoteThumbnail.
        private void OnFileContainerContentChanging(ListViewBase sender, ContainerContentChangingEventArgs args)
        {
            if (args.Phase != 0) return;
            if (args.Item is not RemoteFile file) return;
            if (!file.IsPreviewable || file.HasThumbnail || file.ThumbnailRequested) return;

            var peer = mgr.SelectedPeer;
            var fileId = file.file_id > 0 ? file.file_id : (ulong.TryParse(file.id, out var pid) ? pid : 0);
            var cacheKey = $"{peer?.device_id}_{fileId}";
            if (peer != null && _thumbnailCache.TryGetValue(cacheKey, out var cached))
            {
                file.ThumbnailRequested = true;
                file.Thumbnail = cached;
                return;
            }

            file.ThumbnailRequested = true;
            _ = FetchThumbnailAsync(file, fileId, cacheKey);
        }

        // UI thread only (both callers run there), so no locking.
        private static void CacheThumbnail(string key, Microsoft.UI.Xaml.Media.Imaging.BitmapImage bitmap)
        {
            if (!_thumbnailCache.ContainsKey(key)) _thumbnailCacheOrder.Enqueue(key);
            _thumbnailCache[key] = bitmap;
            while (_thumbnailCache.Count > MaxCachedThumbnails && _thumbnailCacheOrder.TryDequeue(out var oldest))
            {
                _thumbnailCache.Remove(oldest);
            }
        }

        private async Task FetchThumbnailAsync(RemoteFile file, ulong fileId, string cacheKey)
        {
            var peer = mgr.SelectedPeer;
            if (peer == null || string.IsNullOrEmpty(peer.device_id) || fileId == 0) return;

            string? base64 = null;
            const int maxAttempts = 2;
            for (var attempt = 1; attempt <= maxAttempts && base64 == null; attempt++)
            {
                await _thumbnailThrottle.WaitAsync();
                JsonDocument? doc;
                try
                {
                    doc = await DaemonClient.RemoteThumbnailRequestAsync(peer.device_id, fileId, ThumbnailSizePx);
                }
                catch (Exception ex) { App.HandleError(ex); doc = null; }
                finally { _thumbnailThrottle.Release(); }

                if (doc == null)
                {
                    if (attempt < maxAttempts) await Task.Delay(400);
                    continue;
                }

                try
                {
                    var root = doc.RootElement;
                    var dataEl = root.TryGetProperty("data", out var d) ? d : root;
                    if (dataEl.TryGetProperty("data_base64", out var b64El) && b64El.ValueKind == JsonValueKind.String)
                        base64 = b64El.GetString();
                }
                catch (Exception ex) { App.HandleError(ex); }

                if (string.IsNullOrEmpty(base64) && attempt < maxAttempts) await Task.Delay(400);
            }

            if (string.IsNullOrEmpty(base64))
            {
                // Let a later scroll-into-view try again instead of giving up
                // on this file for the rest of the session.
                file.ThumbnailRequested = false;
                return;
            }

            App.MainDispatcherQueue?.TryEnqueue(async () =>
            {
                try
                {
                    var bytes = Convert.FromBase64String(base64);
                    using var stream = new Windows.Storage.Streams.InMemoryRandomAccessStream();
                    using var writer = new Windows.Storage.Streams.DataWriter(stream.GetOutputStreamAt(0));
                    writer.WriteBytes(bytes);
                    await writer.StoreAsync();

                    var bitmap = new Microsoft.UI.Xaml.Media.Imaging.BitmapImage();
                    await bitmap.SetSourceAsync(stream);
                    CacheThumbnail(cacheKey, bitmap);
                    file.Thumbnail = bitmap;
                }
                catch (Exception ex) { App.HandleError(ex); }
            });
        }
    }
}
