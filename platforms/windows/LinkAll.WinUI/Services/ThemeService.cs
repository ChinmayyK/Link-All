using System;
using System.Collections.Generic;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace LinkAll.WinUI.Services
{
    // Centralizes light/dark/system theme selection for every top-level
    // Window. WinUI3 has no app-wide "RequestedTheme" the way WPF does -
    // each Window's root element carries its own RequestedTheme - so every
    // window must call ThemeService.Register(this) once after
    // InitializeComponent()/SetTitleBar(). Preference is a plain local
    // setting (ApplicationData.LocalSettings), independent of the Rust
    // core / cross-device settings sync: this is presentation-only state.
    //
    // Default is Light (not System) - the app has always shipped light-only
    // and should keep looking the same out of the box; Dark is opt-in via
    // Settings.
    public static class ThemeService
    {
        private const string SettingKey = "AppThemePreference";
        private static readonly List<WeakReference<Window>> TrackedWindows = new();

        public static string CurrentPreference
        {
            get
            {
                var value = LocalSettingsStore.Get(SettingKey);
                return value is "Light" or "Dark" or "System" ? value : "Light";
            }
        }

        // Registers a window so it (a) has the saved theme applied
        // immediately and (b) gets re-themed live if the user changes the
        // setting while this window is still open (e.g. the long-lived
        // Dashboard/tray windows).
        public static void Register(Window window)
        {
            var entry = new WeakReference<Window>(window);
            TrackedWindows.Add(entry);
            // A closed window can stay alive (still referenced) but throws
            // "window has already been closed" on any access, so stop
            // re-theming it once it closes.
            window.Closed += (_, _) => TrackedWindows.Remove(entry);
            Apply(window);
        }

        public static void SetPreference(string preference)
        {
            if (preference is not ("Light" or "Dark" or "System")) preference = "Light";
            LocalSettingsStore.Set(SettingKey, preference);

            foreach (var weakRef in TrackedWindows.ToList())
            {
                if (weakRef.TryGetTarget(out var window)) Apply(window);
                else TrackedWindows.Remove(weakRef);
            }
        }

        private static void Apply(Window window)
        {
            try
            {
                var elementTheme = CurrentPreference switch
                {
                    "Dark" => ElementTheme.Dark,
                    "System" => ElementTheme.Default,
                    _ => ElementTheme.Light,
                };

                if (window.Content is FrameworkElement root)
                {
                    root.RequestedTheme = elementTheme;
                    ApplyTitleBarButtonColors(window, root.ActualTheme == ElementTheme.Dark);
                    root.ActualThemeChanged -= RootOnActualThemeChanged;
                    root.ActualThemeChanged += RootOnActualThemeChanged;
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // A ContentDialog opens in the window's popup layer, outside the root
        // element that carries RequestedTheme, so left alone it follows the
        // Windows theme: a dark dialog over a light app, or the reverse. The
        // app's dialog style sets this so each dialog takes its window's
        // theme when it opens, and keeps following it while open.
        public static readonly DependencyProperty FollowWindowThemeProperty =
            DependencyProperty.RegisterAttached(
                "FollowWindowTheme",
                typeof(bool),
                typeof(ThemeService),
                new PropertyMetadata(false, OnFollowWindowThemeChanged));

        public static void SetFollowWindowTheme(DependencyObject element, bool value) => element.SetValue(FollowWindowThemeProperty, value);
        public static bool GetFollowWindowTheme(DependencyObject element) => (bool)element.GetValue(FollowWindowThemeProperty);

        private static void OnFollowWindowThemeChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
        {
            if (d is not ContentDialog dialog || e.NewValue is not true) return;

            FrameworkElement? followed = null;
            void Follow(FrameworkElement root, object args) => dialog.RequestedTheme = root.ActualTheme;
            void Stop()
            {
                if (followed != null) followed.ActualThemeChanged -= Follow;
                followed = null;
            }

            // Loading comes before the first frame, so the dialog never
            // shows in the wrong theme.
            dialog.Loading += (_, _) =>
            {
                Stop();
                if (dialog.XamlRoot?.Content is not FrameworkElement root) return;
                dialog.RequestedTheme = root.ActualTheme;
                root.ActualThemeChanged += Follow;
                followed = root;
            };
            dialog.Closed += (_, _) => Stop();
        }

        private static void RootOnActualThemeChanged(FrameworkElement sender, object args)
        {
            // Find the owning window so we can re-theme its caption buttons
            // when "System" preference tracks a live OS theme change.
            foreach (var weakRef in TrackedWindows.ToList())
            {
                if (weakRef.TryGetTarget(out var window) && ReferenceEquals(window.Content, sender))
                {
                    ApplyTitleBarButtonColors(window, sender.ActualTheme == ElementTheme.Dark);
                    return;
                }
            }
        }

        private static void ApplyTitleBarButtonColors(Window window, bool isDark)
        {
            try
            {
                var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(window);
                var windowId = Microsoft.UI.Win32Interop.GetWindowIdFromWindow(hwnd);
                var appWindow = Microsoft.UI.Windowing.AppWindow.GetFromWindowId(windowId);
                if (appWindow != null) WindowIconHelper.Apply(appWindow, isDark);
                var titleBar = appWindow?.TitleBar;
                if (titleBar == null) return;

                var foreground = isDark
                    ? Windows.UI.Color.FromArgb(255, 255, 255, 255)
                    : Windows.UI.Color.FromArgb(255, 0, 0, 0);
                var inactiveForeground = isDark
                    ? Windows.UI.Color.FromArgb(128, 255, 255, 255)
                    : Windows.UI.Color.FromArgb(128, 0, 0, 0);
                var hoverBackground = isDark
                    ? Windows.UI.Color.FromArgb(25, 255, 255, 255)
                    : Windows.UI.Color.FromArgb(20, 0, 0, 0);
                var pressedBackground = isDark
                    ? Windows.UI.Color.FromArgb(40, 255, 255, 255)
                    : Windows.UI.Color.FromArgb(35, 0, 0, 0);

                titleBar.ButtonBackgroundColor = Microsoft.UI.Colors.Transparent;
                titleBar.ButtonInactiveBackgroundColor = Microsoft.UI.Colors.Transparent;
                titleBar.ButtonForegroundColor = foreground;
                titleBar.ButtonInactiveForegroundColor = inactiveForeground;
                titleBar.ButtonHoverForegroundColor = foreground;
                titleBar.ButtonHoverBackgroundColor = hoverBackground;
                titleBar.ButtonPressedForegroundColor = foreground;
                titleBar.ButtonPressedBackgroundColor = pressedBackground;
            }
            catch (Exception ex) { App.HandleError(ex); }
        }
    }
}
