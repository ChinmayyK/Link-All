using System;
using System.IO;
using Microsoft.UI.Windowing;

namespace LinkAll.WinUI.Services
{
    public static class WindowIconHelper
    {
        private static readonly string IconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico");
        private static readonly string DarkIconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "AppIconDark.ico");

        // Unpackaged WinUI3 apps don't automatically pick up the exe's
        // embedded icon for window/titlebar/taskbar - it must be set
        // explicitly per AppWindow, otherwise it falls back to a generic
        // WinUI icon.
        public static void Apply(AppWindow appWindow) => Apply(appWindow, isDark: false);

        // The window, taskbar and Alt+Tab icon in the logo that matches the
        // window's theme: the dark logo on a dark title bar.
        public static void Apply(AppWindow appWindow, bool isDark)
        {
            try
            {
                var path = isDark ? DarkIconPath : IconPath;
                if (!File.Exists(path)) path = IconPath;
                if (File.Exists(path)) appWindow.SetIcon(path);
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        [System.Runtime.InteropServices.DllImport("user32.dll")]
        private static extern uint GetDpiForWindow(IntPtr hWnd);

        // AppWindow.Resize/Move take physical pixels, but every size in this
        // app (fonts, paddings, and every hardcoded window width/height) is
        // authored in DIPs. Without this, a window is only the intended size
        // on a 100%-scaled display; on anything scaled higher its physical
        // pixel dimensions are smaller in DIP terms than the layout assumes,
        // so text/buttons/icons all read as oversized for the space they're
        // crammed into.
        public static double GetDpiScale(IntPtr hwnd)
        {
            try { return GetDpiForWindow(hwnd) / 96.0; }
            catch { return 1.0; }
        }

        public static void ResizeDips(AppWindow appWindow, IntPtr hwnd, int widthDips, int heightDips)
        {
            appWindow.Resize(FitToWorkArea(appWindow, hwnd, widthDips, heightDips));
        }

        // A DIP size scaled for the monitor, but never larger than that
        // monitor's work area. At 150-175% scaling a laptop screen is only
        // ~1100x600 DIPs, so a fixed DIP size ran off the bottom and right
        // of the screen with no way to reach what was cut off.
        public static Windows.Graphics.SizeInt32 FitToWorkArea(AppWindow appWindow, IntPtr hwnd, int widthDips, int heightDips)
        {
            double scale = GetDpiScale(hwnd);
            int w = (int)(widthDips * scale);
            int h = (int)(heightDips * scale);
            var work = GetWorkArea(appWindow);
            if (work is { } area)
            {
                w = Math.Min(w, (int)(area.Width * 0.94));
                h = Math.Min(h, (int)(area.Height * 0.94));
            }
            return new Windows.Graphics.SizeInt32(w, h);
        }

        // Size the window (see FitToWorkArea) and centre it on its monitor.
        public static void ResizeAndCenterDips(AppWindow appWindow, IntPtr hwnd, int widthDips, int heightDips)
        {
            var size = FitToWorkArea(appWindow, hwnd, widthDips, heightDips);
            appWindow.Resize(size);
            if (GetWorkArea(appWindow) is { } area)
            {
                appWindow.Move(new Windows.Graphics.PointInt32(
                    area.X + (area.Width - size.Width) / 2,
                    area.Y + (area.Height - size.Height) / 2));
            }
        }

        // Size the window and pin it to the top-right corner of its monitor's
        // work area, where Windows puts its own call and meeting banners.
        public static void ResizeAndPlaceTopRightDips(AppWindow appWindow, IntPtr hwnd, int widthDips, int heightDips, int marginDips)
        {
            var size = FitToWorkArea(appWindow, hwnd, widthDips, heightDips);
            appWindow.Resize(size);
            if (GetWorkArea(appWindow) is { } area)
            {
                int margin = (int)(marginDips * GetDpiScale(hwnd));
                appWindow.Move(new Windows.Graphics.PointInt32(
                    area.X + area.Width - size.Width - margin,
                    area.Y + margin));
            }
        }

        private static Windows.Graphics.RectInt32? GetWorkArea(AppWindow appWindow)
        {
            try
            {
                return DisplayArea.GetFromWindowId(appWindow.Id, DisplayAreaFallback.Nearest)?.WorkArea;
            }
            catch { return null; }
        }
    }
}
