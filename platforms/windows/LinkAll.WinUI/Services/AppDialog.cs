using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Markup;

namespace LinkAll.WinUI.Services
{
    // One dialog look for the whole app, matching Android's sheets and the
    // Mac's modals: an icon well, a title and a one-line subtitle on top,
    // then large tappable choices. Built from markup so ThemeResource
    // brushes follow the dialog's own theme.
    public static class AppDialog
    {
        public static ContentDialog Create(XamlRoot xamlRoot, string glyph, string title, string? subtitle = null, bool danger = false)
        {
            var dialog = new ContentDialog
            {
                XamlRoot = xamlRoot,
                Title = Header(glyph, title, subtitle, danger),
            };
            if (Application.Current.Resources.TryGetValue("AppDialogStyle", out var style) && style is Style s)
                dialog.Style = s;
            return dialog;
        }

        public static UIElement Header(string glyph, string title, string? subtitle, bool danger = false)
        {
            var tint = danger ? "AppDangerBrush" : "AppAccentBrush";
            var well = danger ? "AppDangerSubtleBrush" : "AppAccentSubtleBrush";
            var subtitleVisibility = string.IsNullOrEmpty(subtitle) ? "Collapsed" : "Visible";
            var markup = $$"""
                <Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" ColumnSpacing="14">
                    <Grid.ColumnDefinitions>
                        <ColumnDefinition Width="Auto" />
                        <ColumnDefinition Width="*" />
                    </Grid.ColumnDefinitions>
                    <Border Width="44" Height="44" CornerRadius="12" Background="{ThemeResource {{well}}}">
                        <FontIcon Glyph="{{Escape(glyph)}}" FontSize="19" Foreground="{ThemeResource {{tint}}}" />
                    </Border>
                    <StackPanel Grid.Column="1" VerticalAlignment="Center" Spacing="1">
                        <TextBlock Text="{{Escape(title)}}" FontSize="17" FontWeight="SemiBold" TextWrapping="Wrap" />
                        <TextBlock Text="{{Escape(subtitle ?? "")}}" FontSize="12.5" TextWrapping="Wrap"
                                   Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                                   Visibility="{{subtitleVisibility}}" />
                    </StackPanel>
                </Grid>
                """;
            try { return (UIElement)XamlReader.Load(markup); }
            catch (Exception ex) { App.HandleError(ex); return new TextBlock { Text = title }; }
        }

        // A large choice: icon well, title, detail. Click closes the dialog
        // through `onClick`.
        public static Button Option(string glyph, string title, string? detail, Action onClick, bool danger = false, bool selected = false)
        {
            var tint = danger ? "AppDangerBrush" : "AppAccentBrush";
            var well = danger ? "AppDangerSubtleBrush" : "AppAccentSubtleBrush";
            var titleBrush = danger ? "AppDangerBrush" : "TextFillColorPrimaryBrush";
            var detailVisibility = string.IsNullOrEmpty(detail) ? "Collapsed" : "Visible";
            var trailGlyph = selected ? "&#xE73E;" : "&#xE76C;";
            var trailBrush = selected ? "AppAccentBrush" : "TextFillColorTertiaryBrush";
            var trailVisibility = danger ? "Collapsed" : "Visible";
            var markup = $$"""
                <Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" ColumnSpacing="12">
                    <Grid.ColumnDefinitions>
                        <ColumnDefinition Width="Auto" />
                        <ColumnDefinition Width="*" />
                        <ColumnDefinition Width="Auto" />
                    </Grid.ColumnDefinitions>
                    <Border Width="36" Height="36" CornerRadius="10" Background="{ThemeResource {{well}}}">
                        <FontIcon Glyph="{{Escape(glyph)}}" FontSize="15" Foreground="{ThemeResource {{tint}}}" />
                    </Border>
                    <StackPanel Grid.Column="1" VerticalAlignment="Center">
                        <TextBlock Text="{{Escape(title)}}" FontSize="13.5" FontWeight="SemiBold"
                                   Foreground="{ThemeResource {{titleBrush}}}" />
                        <TextBlock Text="{{Escape(detail ?? "")}}" FontSize="12" TextWrapping="Wrap"
                                   Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                                   Visibility="{{detailVisibility}}" />
                    </StackPanel>
                    <FontIcon Grid.Column="2" Glyph="{{trailGlyph}}" FontSize="12"
                              Foreground="{ThemeResource {{trailBrush}}}"
                              Visibility="{{trailVisibility}}" />
                </Grid>
                """;
            UIElement content;
            try { content = (UIElement)XamlReader.Load(markup); }
            catch (Exception ex) { App.HandleError(ex); content = new TextBlock { Text = title }; }
            var button = new Button
            {
                Content = content,
                HorizontalAlignment = HorizontalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Stretch,
                MinWidth = 340,
            };
            if (Application.Current.Resources.TryGetValue("AppActionTile", out var style) && style is Style s)
                button.Style = s;
            button.Click += (_, _) => onClick();
            return button;
        }

        public static async Task<bool> ConfirmAsync(XamlRoot xamlRoot, string glyph, string title, string message,
            string confirm, string cancel = "Cancel", bool danger = false)
        {
            var dialog = Create(xamlRoot, glyph, title, null, danger);
            dialog.Content = new TextBlock
            {
                Text = message,
                TextWrapping = TextWrapping.Wrap,
                MaxWidth = 420,
                Opacity = 0.8,
            };
            dialog.PrimaryButtonText = confirm;
            dialog.CloseButtonText = cancel;
            dialog.DefaultButton = ContentDialogButton.Primary;
            return await dialog.ShowAsync() == ContentDialogResult.Primary;
        }

        /// <summary>
        /// "Files &amp; folders": which device (all unless one is picked),
        /// then Files or A folder. Windows' pickers take files or one folder,
        /// never both, so these are two choices. Null when cancelled; a null
        /// target means every connected device.
        /// </summary>
        public static async Task<(bool Folder, string? Target)?> ShowSendAsync(XamlRoot xamlRoot, IEnumerable<PeerViewModel> connectedPeers)
        {
            var peers = connectedPeers.ToList();
            if (peers.Count == 0) return null;
            string? target = null;
            (bool, string?)? result = null;

            var dialog = Create(xamlRoot, "", "Send files & folders",
                peers.Count == 1 ? $"To {peers[0].DisplayName}" : "To all your devices, or pick one");
            dialog.CloseButtonText = "Cancel";

            var root = new StackPanel { Spacing = 10, MinWidth = 380 };
            if (peers.Count > 1)
            {
                var chips = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
                var buttons = new List<(ToggleButton Button, string? Id)>();
                void Select(string? id)
                {
                    target = id;
                    foreach (var (b, bid) in buttons) b.IsChecked = bid == id;
                }
                void AddChip(string label, string? id)
                {
                    var chip = new ToggleButton
                    {
                        Content = label,
                        CornerRadius = new CornerRadius(999),
                        Padding = new Thickness(14, 6, 14, 6),
                        IsChecked = id == null,
                    };
                    chip.Click += (_, _) => Select(id);
                    buttons.Add((chip, id));
                    chips.Children.Add(chip);
                }
                AddChip("All devices", null);
                foreach (var p in peers) AddChip(p.DisplayName, p.device_id);
                root.Children.Add(new ScrollViewer
                {
                    Content = chips,
                    HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
                    VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
                    Margin = new Thickness(0, 0, 0, 6),
                });
            }
            else
            {
                target = peers[0].device_id;
            }

            root.Children.Add(Option("", "Files", "Photos, videos, documents, anything", () =>
            {
                result = (false, target);
                dialog.Hide();
            }));
            root.Children.Add(Option("", "A folder", "Everything in it, subfolders included", () =>
            {
                result = (true, target);
                dialog.Hide();
            }));
            dialog.Content = root;
            await dialog.ShowAsync();
            return result;
        }

        private static string Escape(string text) => System.Security.SecurityElement.Escape(text) ?? "";
    }
}
