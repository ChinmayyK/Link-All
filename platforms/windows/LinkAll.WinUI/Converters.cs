using Microsoft.UI.Dispatching;
using System;
using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Data;

namespace LinkAll.WinUI
{
    public sealed partial class StringToVisibilityConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language)
        {
            return string.IsNullOrWhiteSpace(value?.ToString()) ? Visibility.Collapsed : Visibility.Visible;
        }

        public object ConvertBack(object value, Type targetType, object parameter, string language)
        {
            throw new NotSupportedException();
        }
    }

    public sealed partial class EmptyStringToVisibilityConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language)
        {
            return string.IsNullOrWhiteSpace(value?.ToString()) ? Visibility.Visible : Visibility.Collapsed;
        }

        public object ConvertBack(object value, Type targetType, object parameter, string language)
        {
            throw new NotSupportedException();
        }
    }

    public partial class NullToVisibilityConverter : IValueConverter
    {
        public Visibility NullValue { get; set; } = Visibility.Collapsed;
        public Visibility NonNullValue { get; set; } = Visibility.Visible;

        public object Convert(object value, Type targetType, object parameter, string language)
        {
            return value == null ? NullValue : NonNullValue;
        }

        public object ConvertBack(object value, Type targetType, object parameter, string language)
        {
            throw new NotImplementedException();
        }
    }

    public partial class MultiplyConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language)
        {
            if (value is IConvertible val && parameter != null)
            {
                if (double.TryParse(parameter.ToString(), NumberStyles.Any, CultureInfo.InvariantCulture, out double multiplier))
                {
                    return val.ToDouble(CultureInfo.InvariantCulture) * multiplier;
                }
            }
            return value;
        }

        public object ConvertBack(object value, Type targetType, object parameter, string language)
        {
            throw new NotImplementedException();
        }
    }

    public sealed partial class BoolToFolderIconConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language)
        {
            return (value is bool b && b) ? "\uE8B7" : "\uE896";
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    public sealed partial class BoolToItemCountDotConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language)
        {
            return (value is bool b && b) ? "·" : "";
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    public sealed partial class ItemCountTextConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language)
        {
            if (value is int count)
            {
                return count == 1 ? "1 item" : $"{count} items";
            }
            return "";
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    public sealed partial class InverseBoolToVisibilityConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language)
        {
            return (value is bool b && b) ? Visibility.Collapsed : Visibility.Visible;
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    public sealed partial class CountToVisibilityConverter : IValueConverter
    {
        public static Visibility ToVisibility(int count) => count > 0 ? Visibility.Visible : Visibility.Collapsed;

        public object Convert(object value, Type targetType, object parameter, string language)
        {
            if (value is int count) return ToVisibility(count);
            if (value is long lCount) return lCount > 0 ? Visibility.Visible : Visibility.Collapsed;
            return Visibility.Collapsed;
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    public sealed partial class InverseCountToVisibilityConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language)
        {
            if (value is int count) return count == 0 ? Visibility.Visible : Visibility.Collapsed;
            if (value is long lCount) return lCount == 0 ? Visibility.Visible : Visibility.Collapsed;
            return Visibility.Visible;
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    public sealed partial class PlatformToGlyphConverter : IValueConverter
    {
        public static string ToGlyph(string? value)
        {
            var str = value?.ToLowerInvariant() ?? "";
            if (str.Contains("mac") || str.Contains("apple")) return "\uE7F8"; // Laptop
            if (str.Contains("windows") || str.Contains("pc") || str.Contains("desktop")) return "\uE7F4"; // Monitor / PC
            if (str.Contains("linux") || str.Contains("server")) return "\uE975"; // Server
            return "\uE8EA"; // Smartphone
        }

        public object Convert(object value, Type targetType, object parameter, string language)
        {
            return ToGlyph(value?.ToString());
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    public sealed partial class BatteryGlyphConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language)
        {
            if (value is int level)
            {
                if (level <= 15) return "\uEBAA"; // Battery 1
                if (level <= 35) return "\uEBAC"; // Battery 3
                if (level <= 65) return "\uEBAE"; // Battery 6
                if (level <= 85) return "\uEBAF"; // Battery 8
                return "\uEBB5"; // Battery 10
            }
            return "\uEBA0";
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    public sealed partial class NetworkTypeToGlyphConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language)
        {
            var type = value?.ToString()?.ToLowerInvariant() ?? "";
            if (type.Contains("wifi") || type.Contains("wi-fi")) return "\uE701";
            if (type.Contains("cellular") || type.Contains("mobile")) return "\uEC3B";
            if (type.Contains("ethernet") || type.Contains("lan")) return "\uE839";
            return "\uEB55"; // Offline
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    public sealed partial class FileTypeToGlyphConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language)
        {
            var name = value?.ToString()?.ToLowerInvariant() ?? "";
            if (name.EndsWith(".jpg") || name.EndsWith(".jpeg") || name.EndsWith(".png") || name.EndsWith(".gif") || name.EndsWith(".webp") || name.EndsWith(".svg") || name.EndsWith(".bmp"))
                return "\uEB9F"; // Photo
            if (name.EndsWith(".mp4") || name.EndsWith(".mov") || name.EndsWith(".mkv") || name.EndsWith(".avi") || name.EndsWith(".webm"))
                return "\uE714"; // Video
            if (name.EndsWith(".mp3") || name.EndsWith(".wav") || name.EndsWith(".flac") || name.EndsWith(".m4a") || name.EndsWith(".aac"))
                return "\uE8D6"; // Audio
            if (name.EndsWith(".pdf") || name.EndsWith(".doc") || name.EndsWith(".docx") || name.EndsWith(".txt") || name.EndsWith(".md"))
                return "\uE8A5"; // Document
            if (name.EndsWith(".zip") || name.EndsWith(".rar") || name.EndsWith(".7z") || name.EndsWith(".tar") || name.EndsWith(".gz"))
                return "\uF012"; // Zip
            if (name.EndsWith(".apk") || name.EndsWith(".exe") || name.EndsWith(".msi"))
                return "\uE71D"; // App
            return "\uE8A5"; // Generic File
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    public sealed partial class BoolToVisibleConverter : IValueConverter
    {
        // Exposed as a static method, not just Convert(), because WinUI 3's
        // x:Bind codegen calls SetConverterLookupRoot(this) for ANY {x:Bind
        // ..., Converter={StaticResource ...}} in a file - anywhere in the
        // file, not just at the binding site - and `this` is the code-behind
        // instance. That's fine in a Page (FrameworkElement), but every
        // Window-rooted file in this app fails to compile with "cannot
        // convert from Window to FrameworkElement" the moment Converter= is
        // used anywhere in it. Calling the static method directly from
        // x:Bind sidesteps the lookup-root mechanism entirely.
        public static Visibility ToVisibility(bool value) => value ? Visibility.Visible : Visibility.Collapsed;

        public object Convert(object value, Type targetType, object parameter, string language)
        {
            return (value is bool b && b) ? Visibility.Visible : Visibility.Collapsed;
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    public sealed partial class StringToBrushConverter : IValueConverter
    {
        private static readonly System.Collections.Concurrent.ConcurrentDictionary<string, Microsoft.UI.Xaml.Media.SolidColorBrush> _cache = new();
        private static readonly Microsoft.UI.Xaml.Media.SolidColorBrush _transparent = new(Microsoft.UI.Colors.Transparent);

        public static Microsoft.UI.Xaml.Media.SolidColorBrush ToSolidColorBrush(string? hex)
        {
            var h = hex?.Trim() ?? "";
            if (string.IsNullOrWhiteSpace(h)) return _transparent;

            return _cache.GetOrAdd(h, key =>
            {
                try
                {
                    string cleaned = key.StartsWith("#") ? key.Substring(1) : key;
                    if (cleaned.Length == 6)
                    {
                        byte r = byte.Parse(cleaned.Substring(0, 2), NumberStyles.HexNumber);
                        byte g = byte.Parse(cleaned.Substring(2, 2), NumberStyles.HexNumber);
                        byte b = byte.Parse(cleaned.Substring(4, 2), NumberStyles.HexNumber);
                        return new Microsoft.UI.Xaml.Media.SolidColorBrush(Windows.UI.Color.FromArgb(255, r, g, b));
                    }
                    if (cleaned.Length == 8)
                    {
                        byte a = byte.Parse(cleaned.Substring(0, 2), NumberStyles.HexNumber);
                        byte r = byte.Parse(cleaned.Substring(2, 2), NumberStyles.HexNumber);
                        byte g = byte.Parse(cleaned.Substring(4, 2), NumberStyles.HexNumber);
                        byte b = byte.Parse(cleaned.Substring(6, 2), NumberStyles.HexNumber);
                        return new Microsoft.UI.Xaml.Media.SolidColorBrush(Windows.UI.Color.FromArgb(a, r, g, b));
                    }
                }
                catch { }
                return _transparent;
            });
        }

        public object Convert(object value, Type targetType, object parameter, string language)
        {
            return ToSolidColorBrush(value?.ToString());
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    // 0-100 -> a pixel width, for the hand-drawn meters (battery pip,
    // storage bar) where a full ProgressBar would be visually far heavier
    // than the single number it carries. `parameter` is the track width in
    // DIPs, so the same converter serves a 22px battery and a 120px bar.
    public sealed partial class PercentToWidthConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language)
        {
            double percent = value switch
            {
                int i => i,
                long l => l,
                double d => d,
                _ => 0,
            };

            if (percent < 0) percent = 0;
            if (percent > 100) percent = 100;

            double track = 100;
            if (parameter != null &&
                double.TryParse(parameter.ToString(), NumberStyles.Any, CultureInfo.InvariantCulture, out var parsed) &&
                parsed > 0)
            {
                track = parsed;
            }

            return track * percent / 100.0;
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    // Activity-feed kinds -> Segoe Fluent Icons. ActivityEntry.TypeIcon
    // already maps the same kinds to platform-neutral icon *names* shared
    // with the macOS/Android clients; this is the Windows glyph table for
    // that set, kept separate so neither side has to compromise.
    //
    // Glyphs are built from code points rather than written as literals so
    // this file stays pure ASCII and survives any tooling that re-encodes it.
    public sealed partial class ActivityKindToGlyphConverter : IValueConverter
    {
        private static string G(int codePoint) => char.ConvertFromUtf32(codePoint);

        // Static entry point for x:Bind sites in Window-rooted XAML - see the
        // comment on BoolToVisibleConverter.ToVisibility for why Converter=
        // can't be used there.
        public static string ToGlyph(string? kind) => kind switch
        {
            "remote_clipboard_available" => G(0xE77F), // Paste
            "clipboard_applied" => G(0xE73E),         // CheckMark
            "clipboard_text" => G(0xE8C8),            // Copy
            "clipboard_image" => G(0xEB9F),           // Photo
            "file_transfer_started" => G(0xE898),     // Upload
            "file_transfer_complete" => G(0xE896),    // Download
            "file_transfer_failed" => G(0xE783),      // Error
            "peer_connected" => G(0xE701),            // Wifi
            "peer_disconnected" => G(0xEB55),         // Network offline
            "sync_paused" => G(0xE769),               // Pause
            "sync_resumed" => G(0xE768),              // Play
            "remote_notification" => G(0xE946),       // Info
            _ => G(0xEC42),                           // Generic event
        };

        public object Convert(object value, Type targetType, object parameter, string language)
        {
            if (value is ActivityEntry entry) return entry.Glyph;
            return ToGlyph(value?.ToString());
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }

    // Dims an element instead of collapsing it, so a de-emphasised row keeps
    // its place in the layout and nothing reflows when the state flips.
    // `parameter` overrides the "false" opacity; default 0.45.
    public sealed partial class BoolToOpacityConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, string language)
        {
            if (value is bool b && b) return 1.0;

            if (parameter != null &&
                double.TryParse(parameter.ToString(), NumberStyles.Any, CultureInfo.InvariantCulture, out var dimmed))
            {
                return dimmed;
            }
            return 0.45;
        }
        public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
    }
}










