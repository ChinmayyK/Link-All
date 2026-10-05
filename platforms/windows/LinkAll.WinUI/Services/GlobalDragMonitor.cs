using System;
using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Dispatching;

namespace LinkAll.WinUI.Services
{
    public class GlobalDragMonitor : IDisposable
    {
        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool GetCursorPos(out POINT lpPoint);

        [DllImport("user32.dll")]
        private static extern short GetAsyncKeyState(int vKey);

        [DllImport("user32.dll")]
        private static extern int GetSystemMetrics(int nIndex);

        private const int SM_CXSCREEN = 0;
        private const int SM_CYSCREEN = 1;

        [StructLayout(LayoutKind.Sequential)]
        public struct POINT
        {
            public int X;
            public int Y;
        }

        private readonly DispatcherTimer _timer;
        // The UI thread's queue, taken here because the main window does not
        // exist when Link All starts in the background.
        private readonly DispatcherQueue _queue = DispatcherQueue.GetForCurrentThread();
        private bool _wasLeftButtonDown = false;
        private UI.EdgeDropWindow? _dropZoneWindow;
        private readonly ClipboardManager _clipboardManager;
        private readonly int _edgeThreshold = 20;

        public GlobalDragMonitor(ClipboardManager clipboardManager)
        {
            _clipboardManager = clipboardManager;
            _timer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(100) };
            _timer.Tick += OnTick;
            _timer.Start();
        }

        private void OnTick(object? sender, object e)
        {
            try
            {
                // VK_LBUTTON is 0x01
                bool isLeftButtonDown = (GetAsyncKeyState(0x01) & 0x8000) != 0;

                if (isLeftButtonDown && _wasLeftButtonDown)
                {
                    // User is dragging. Are they at the edge?
                    if (GetCursorPos(out POINT p))
                    {
                        double screenWidth = GetSystemMetrics(SM_CXSCREEN);
                        
                        // If they are on the right edge
                        if (p.X >= screenWidth - _edgeThreshold)
                        {
                            if (_dropZoneWindow == null) // WinUI 3 Window doesn't have IsLoaded
                            {
                                ShowDropZone();
                            }
                        }
                        else if (p.X < screenWidth - 300 - _edgeThreshold) // Hardcoding width since WinUI 3 Window.Bounds is tricky
                        {
                            // Move away from edge
                        }
                    }
                }
                else if (!isLeftButtonDown && _wasLeftButtonDown)
                {
                    // User released the drag. Delay close slightly to allow drop event to process
                    System.Threading.Tasks.Task.Delay(500).ContinueWith(_ =>
                    {
                        _queue.TryEnqueue(HideDropZone);
                    });
                }

                _wasLeftButtonDown = isLeftButtonDown;
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void ShowDropZone()
        {
            try
            {
                if (_dropZoneWindow != null) return;
                
                _dropZoneWindow = new UI.EdgeDropWindow();
                
                var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(_dropZoneWindow);
                var windowId = Microsoft.UI.Win32Interop.GetWindowIdFromWindow(hwnd);
                var appWindow = Microsoft.UI.Windowing.AppWindow.GetFromWindowId(windowId);
                
                int screenW = GetSystemMetrics(SM_CXSCREEN);
                int screenH = GetSystemMetrics(SM_CYSCREEN);
                
                int w = 20;
                int h = 200;
                
                appWindow.MoveAndResize(new Windows.Graphics.RectInt32(screenW - w, (screenH - h) / 2, w, h));
                
                // Shown without taking focus: the user is mid-drag in another
                // app, and activating this window could end that drag.
                appWindow.Show(false);
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void HideDropZone()
        {
            try
            {
                if (_dropZoneWindow != null)
                {
                    _dropZoneWindow.Close();
                    _dropZoneWindow = null;
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        public void Dispose()
        {
            _timer.Stop();
            HideDropZone();
        }
    }
}



