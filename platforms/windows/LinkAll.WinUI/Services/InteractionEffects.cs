using System;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;

namespace LinkAll.WinUI.Services
{
    // Microinteractions for Border-based cards and rows, applied through
    // attached properties. Cards nest their own action buttons, so turning
    // them into templated Buttons would fight click routing; attaching
    // behaviour keeps the markup declarative without that risk.
    //
    // Everything here swaps brush *references* or animates
    // Opacity/RenderTransform. Nothing mutates a shared ThemeResource
    // brush's Color, which is what makes these safe to apply to hundreds of
    // recycled ListView items at once.
    public static class HoverEffects
    {
        // Row/card hover: a surface + stroke change, no scale. Scaling a row
        // inside a scrolling list re-rasterises its text every frame and
        // reads as cheap; a tone shift is what Windows itself does.
        public static readonly DependencyProperty EnableCardHoverProperty =
            DependencyProperty.RegisterAttached(
                "EnableCardHover",
                typeof(bool),
                typeof(HoverEffects),
                new PropertyMetadata(false, OnEnableCardHoverChanged));

        public static void SetEnableCardHover(DependencyObject element, bool value) => element.SetValue(EnableCardHoverProperty, value);
        public static bool GetEnableCardHover(DependencyObject element) => (bool)element.GetValue(EnableCardHoverProperty);

        // Standalone cards that are themselves the click target get a
        // barely-there lift as well - 1.5%, enough to register as "this
        // responds" without turning the page into a trampoline.
        public static readonly DependencyProperty EnableLiftProperty =
            DependencyProperty.RegisterAttached(
                "EnableLift",
                typeof(bool),
                typeof(HoverEffects),
                new PropertyMetadata(false, OnEnableLiftChanged));

        public static void SetEnableLift(DependencyObject element, bool value) => element.SetValue(EnableLiftProperty, value);
        public static bool GetEnableLift(DependencyObject element) => (bool)element.GetValue(EnableLiftProperty);

        private static readonly DependencyProperty OriginalBorderBrushProperty =
            DependencyProperty.RegisterAttached("OriginalBorderBrush", typeof(Brush), typeof(HoverEffects), new PropertyMetadata(null));

        private static readonly DependencyProperty OriginalBackgroundProperty =
            DependencyProperty.RegisterAttached("OriginalBackground", typeof(Brush), typeof(HoverEffects), new PropertyMetadata(null));

        private static void OnEnableCardHoverChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
        {
            if (d is not Border border || e.NewValue is not bool enabled || !enabled) return;

            border.PointerEntered += Card_PointerEntered;
            border.PointerExited += Card_PointerExited;
            border.PointerCanceled += Card_PointerExited;
            border.PointerCaptureLost += Card_PointerExited;
        }

        private static void OnEnableLiftChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
        {
            if (d is not Border border || e.NewValue is not bool enabled || !enabled) return;

            border.PointerEntered += Lift_PointerEntered;
            border.PointerExited += Lift_PointerExited;
            border.PointerCanceled += Lift_PointerExited;
            border.PointerPressed += Lift_PointerPressed;
            border.PointerReleased += Lift_PointerEntered;
        }

        private static void Card_PointerEntered(object sender, PointerRoutedEventArgs e) => SwapSurface((Border)sender, hover: true);

        private static void Card_PointerExited(object sender, PointerRoutedEventArgs e) => SwapSurface((Border)sender, hover: false);

        private static void Lift_PointerEntered(object sender, PointerRoutedEventArgs e)
        {
            AnimateScale((Border)sender, 1.015, 140);
            SwapSurface((Border)sender, hover: true);
        }

        private static void Lift_PointerExited(object sender, PointerRoutedEventArgs e)
        {
            AnimateScale((Border)sender, 1.0, 140);
            SwapSurface((Border)sender, hover: false);
        }

        private static void Lift_PointerPressed(object sender, PointerRoutedEventArgs e) => AnimateScale((Border)sender, 0.99, 70);

        private static void SwapSurface(Border border, bool hover)
        {
            try
            {
                if (border.GetValue(OriginalBorderBrushProperty) is not Brush)
                {
                    border.SetValue(OriginalBorderBrushProperty, border.BorderBrush);
                }
                if (border.GetValue(OriginalBackgroundProperty) is not Brush)
                {
                    border.SetValue(OriginalBackgroundProperty, border.Background);
                }

                if (hover)
                {
                    if (TryGetThemeBrush(border, "AppBorderStrongBrush", out var stroke)) border.BorderBrush = stroke;
                    if (TryGetThemeBrush(border, "AppControlFillHoverBrush", out var fill)) border.Background = fill;
                }
                else
                {
                    if (border.GetValue(OriginalBorderBrushProperty) is Brush stroke) border.BorderBrush = stroke;
                    if (border.GetValue(OriginalBackgroundProperty) is Brush fill) border.Background = fill;
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // Resolves a token from the theme dictionary matching *this element's*
        // effective theme.
        //
        // A flat Application.Current.Resources lookup would return whichever
        // theme the app (or the system, when RequestedTheme is unset) defaults
        // to, and ThemeService sets RequestedTheme per window - so on a system
        // in dark mode, a Light-forced window's hovered card was picking up
        // Dark's fill colour and going nearly black. Resolving against the
        // element's own ActualTheme keeps hover correct regardless of the
        // system theme or which window it's in.
        private static bool TryGetThemeBrush(FrameworkElement scope, string key, out Brush brush)
        {
            brush = null!;
            try
            {
                var themeKey = scope.ActualTheme == ElementTheme.Dark ? "Dark" : "Light";
                var dictionary = FindThemeDictionaryContaining(Application.Current.Resources, themeKey, key);

                if (dictionary != null && dictionary.TryGetValue(key, out var themed) && themed is Brush themedBrush)
                {
                    brush = themedBrush;
                    return true;
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
            return false;
        }

        // Theme dictionaries live in a merged dictionary (Theme/Tokens.xaml),
        // not on the application dictionary itself, so this walks the merge
        // tree. Results are cached - this runs on every pointer enter.
        //
        // Matching requires the candidate dictionary to actually CONTAIN the
        // requested key, not just have a "Light"/"Dark" entry at all - WinUI's
        // own XamlControlsResources (merged first, ahead of our Tokens.xaml)
        // defines its own Light/Dark dictionaries for the stock Fluent
        // brushes, and a naive "first ThemeDictionaries match wins" search
        // stops there before ever reaching ours, silently missing every
        // custom token and falling through to the theme-ambiguous lookup
        // above (which was the actual bug).
        private static readonly System.Collections.Concurrent.ConcurrentDictionary<(string Theme, string Key), ResourceDictionary?> _themeDictionaryCache = new();

        private static ResourceDictionary? FindThemeDictionaryContaining(ResourceDictionary root, string themeKey, string brushKey)
        {
            return _themeDictionaryCache.GetOrAdd((themeKey, brushKey), _ => Search(root, depth: 0));

            ResourceDictionary? Search(ResourceDictionary dictionary, int depth)
            {
                if (depth > 6) return null;

                if (dictionary.ThemeDictionaries.TryGetValue(themeKey, out var themed) &&
                    themed is ResourceDictionary candidate &&
                    candidate.ContainsKey(brushKey))
                {
                    return candidate;
                }

                foreach (var merged in dictionary.MergedDictionaries)
                {
                    var result = Search(merged, depth + 1);
                    if (result != null) return result;
                }
                return null;
            }
        }

        private static void AnimateScale(Border border, double to, int milliseconds)
        {
            try
            {
                border.RenderTransformOrigin = new Windows.Foundation.Point(0.5, 0.5);
                if (border.RenderTransform is not ScaleTransform scale)
                {
                    scale = new ScaleTransform { ScaleX = 1.0, ScaleY = 1.0 };
                    border.RenderTransform = scale;
                }

                var storyboard = new Storyboard();
                var easing = new CubicEase { EasingMode = EasingMode.EaseOut };

                var animX = new DoubleAnimation { To = to, Duration = TimeSpan.FromMilliseconds(milliseconds), EasingFunction = easing };
                var animY = new DoubleAnimation { To = to, Duration = TimeSpan.FromMilliseconds(milliseconds), EasingFunction = easing };
                Storyboard.SetTarget(animX, scale);
                Storyboard.SetTargetProperty(animX, "ScaleX");
                Storyboard.SetTarget(animY, scale);
                Storyboard.SetTargetProperty(animY, "ScaleY");

                storyboard.Children.Add(animX);
                storyboard.Children.Add(animY);
                storyboard.Begin();
            }
            catch (Exception ex) { App.HandleError(ex); }
        }
    }

    // Progressive disclosure. Put RevealOnHover="True" on the panel holding
    // a row's secondary actions: it stays invisible until the pointer is
    // over the owning card, or until focus lands inside it.
    //
    // The focus half matters - a hover-only reveal is invisible to keyboard
    // and screen-reader users, so this listens for GotFocus/LostFocus
    // (which bubble up from the buttons inside) as well as the card's
    // pointer events. Primary actions are never hidden this way; only
    // genuinely secondary ones.
    public static class RevealEffects
    {
        public static readonly DependencyProperty RevealOnHoverProperty =
            DependencyProperty.RegisterAttached(
                "RevealOnHover",
                typeof(bool),
                typeof(RevealEffects),
                new PropertyMetadata(false, OnRevealOnHoverChanged));

        public static void SetRevealOnHover(DependencyObject element, bool value) => element.SetValue(RevealOnHoverProperty, value);
        public static bool GetRevealOnHover(DependencyObject element) => (bool)element.GetValue(RevealOnHoverProperty);

        private static void OnRevealOnHoverChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
        {
            if (d is not FrameworkElement element || e.NewValue is not bool enabled || !enabled) return;

            Hide(element, animate: false);

            element.GotFocus += (_, _) =>
            {
                element.SetValue(HasFocusProperty, true);
                Show(element, animate: true);
            };
            element.LostFocus += (_, _) =>
            {
                element.SetValue(HasFocusProperty, false);
                // Don't yank the actions away if the pointer is still on the
                // card - either signal alone is enough to hold them open.
                if (!IsHovered(element)) Hide(element, animate: true);
            };

            // The owning card isn't reachable until the template has been
            // realised, so bind to it on Loaded (which also re-fires when a
            // recycled ListView container is reused).
            element.Loaded += (_, _) =>
            {
                if (element.GetValue(HostBoundProperty) is bool bound && bound) return;

                var host = FindHost(element);
                if (host == null)
                {
                    // No card to attach to - fail open rather than leaving
                    // the actions permanently invisible.
                    Show(element, animate: false);
                    return;
                }

                element.SetValue(HostBoundProperty, true);
                Hide(element, animate: false);

                host.PointerEntered += (_, _) =>
                {
                    element.SetValue(IsHoveredProperty, true);
                    Show(element, animate: true);
                };
                host.PointerExited += (_, _) =>
                {
                    element.SetValue(IsHoveredProperty, false);
                    if (!HasFocus(element)) Hide(element, animate: true);
                };
                host.PointerCanceled += (_, _) =>
                {
                    element.SetValue(IsHoveredProperty, false);
                    if (!HasFocus(element)) Hide(element, animate: true);
                };
            };
        }

        private static readonly DependencyProperty IsHoveredProperty =
            DependencyProperty.RegisterAttached("IsHovered", typeof(bool), typeof(RevealEffects), new PropertyMetadata(false));

        private static readonly DependencyProperty HasFocusProperty =
            DependencyProperty.RegisterAttached("HasFocus", typeof(bool), typeof(RevealEffects), new PropertyMetadata(false));

        private static readonly DependencyProperty HostBoundProperty =
            DependencyProperty.RegisterAttached("HostBound", typeof(bool), typeof(RevealEffects), new PropertyMetadata(false));

        private static bool IsHovered(FrameworkElement element) => element.GetValue(IsHoveredProperty) is bool v && v;

        private static bool HasFocus(FrameworkElement element) => element.GetValue(HasFocusProperty) is bool v && v;

        // Walks up to the nearest Border ancestor - the card root in every
        // template that uses this. Walking up is stable; walking down into a
        // DataTemplate's namescope by name is not.
        private static FrameworkElement? FindHost(FrameworkElement element)
        {
            DependencyObject? current = element;
            for (var depth = 0; depth < 8 && current != null; depth++)
            {
                current = VisualTreeHelper.GetParent(current);
                if (current is Border border) return border;
            }
            return null;
        }

        private static void Show(FrameworkElement element, bool animate)
        {
            element.IsHitTestVisible = true;
            Fade(element, 1.0, animate ? 120 : 0);
        }

        private static void Hide(FrameworkElement element, bool animate)
        {
            element.IsHitTestVisible = false;
            Fade(element, 0.0, animate ? 120 : 0);
        }

        private static void Fade(FrameworkElement element, double to, int milliseconds)
        {
            try
            {
                if (milliseconds <= 0)
                {
                    element.Opacity = to;
                    return;
                }

                var storyboard = new Storyboard();
                var anim = new DoubleAnimation
                {
                    To = to,
                    Duration = TimeSpan.FromMilliseconds(milliseconds),
                    EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut },
                };
                Storyboard.SetTarget(anim, element);
                Storyboard.SetTargetProperty(anim, "Opacity");
                storyboard.Children.Add(anim);
                storyboard.Begin();
            }
            catch (Exception ex) { App.HandleError(ex); }
        }
    }

    // A slow opacity breathe for indicators that mean "in progress":
    // connecting, waiting for a pairing response, scanning. Deliberately
    // *not* used for the steady connected state - a pulse that never stops
    // is decoration, and stops meaning anything.
    //
    // Unlike a one-shot start, this can be switched back off: the storyboard
    // is kept on the element so setting EnablePulse to false stops it and
    // restores full opacity.
    public static class PulseEffects
    {
        public static readonly DependencyProperty EnablePulseProperty =
            DependencyProperty.RegisterAttached(
                "EnablePulse",
                typeof(bool),
                typeof(PulseEffects),
                new PropertyMetadata(false, OnEnablePulseChanged));

        public static void SetEnablePulse(DependencyObject element, bool value) => element.SetValue(EnablePulseProperty, value);
        public static bool GetEnablePulse(DependencyObject element) => (bool)element.GetValue(EnablePulseProperty);

        private static readonly DependencyProperty StoryboardProperty =
            DependencyProperty.RegisterAttached("PulseStoryboard", typeof(Storyboard), typeof(PulseEffects), new PropertyMetadata(null));

        private static void OnEnablePulseChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
        {
            if (d is not FrameworkElement element || e.NewValue is not bool enabled) return;

            if (!enabled)
            {
                Stop(element);
                return;
            }

            if (element.IsLoaded) Start(element);
            else element.Loaded += OnLoadedStart;
        }

        private static void OnLoadedStart(object sender, RoutedEventArgs e)
        {
            if (sender is not FrameworkElement element) return;
            element.Loaded -= OnLoadedStart;
            if (GetEnablePulse(element)) Start(element);
        }

        private static void Start(FrameworkElement element)
        {
            try
            {
                Stop(element);

                var storyboard = new Storyboard { RepeatBehavior = RepeatBehavior.Forever, AutoReverse = true };
                var anim = new DoubleAnimation
                {
                    From = 1.0,
                    To = 0.35,
                    Duration = TimeSpan.FromMilliseconds(1100),
                    EasingFunction = new QuadraticEase { EasingMode = EasingMode.EaseInOut },
                };
                Storyboard.SetTarget(anim, element);
                Storyboard.SetTargetProperty(anim, "Opacity");
                storyboard.Children.Add(anim);
                storyboard.Begin();

                element.SetValue(StoryboardProperty, storyboard);
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private static void Stop(FrameworkElement element)
        {
            try
            {
                if (element.GetValue(StoryboardProperty) is Storyboard existing)
                {
                    existing.Stop();
                    element.SetValue(StoryboardProperty, null);
                }
                element.Opacity = 1.0;
            }
            catch (Exception ex) { App.HandleError(ex); }
        }
    }
}
