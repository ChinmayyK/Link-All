using System;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Foundation;

namespace LinkAll.WinUI.UI
{
    // Centres a page's content in a column no wider than MaxContentWidth.
    //
    // Pages used to put MaxWidth on their root StackPanel/Grid and let
    // Stretch alignment centre it. WinUI centres such an element by its
    // desired width but draws it at MaxWidth, so once the window was wider
    // than the cap the content slid right by half the difference and its
    // right edge (header buttons, row timestamps) was cut off. Arranging the
    // child into an exact, already-centred rectangle leaves nothing for
    // alignment to get wrong.
    // partial: CsWinRT's AOT generator only wires up the Measure/Arrange
    // overrides for partial classes; without it WinUI calls the base and throws.
    public sealed partial class PageColumn : Panel
    {
        public static readonly DependencyProperty MaxContentWidthProperty =
            DependencyProperty.Register(nameof(MaxContentWidth), typeof(double), typeof(PageColumn),
                new PropertyMetadata(double.PositiveInfinity, (d, _) => ((PageColumn)d).InvalidateMeasure()));

        public double MaxContentWidth
        {
            get => (double)GetValue(MaxContentWidthProperty);
            set => SetValue(MaxContentWidthProperty, value);
        }

        protected override Size MeasureOverride(Size availableSize)
        {
            var width = Math.Min(availableSize.Width, MaxContentWidth);
            double childWidth = 0, height = 0;
            foreach (var child in Children)
            {
                child.Measure(new Size(width, availableSize.Height));
                childWidth = Math.Max(childWidth, child.DesiredSize.Width);
                height = Math.Max(height, child.DesiredSize.Height);
            }
            return new Size(double.IsInfinity(width) ? childWidth : width, height);
        }

        protected override Size ArrangeOverride(Size finalSize)
        {
            var width = Math.Min(finalSize.Width, MaxContentWidth);
            var left = (finalSize.Width - width) / 2;
            foreach (var child in Children)
            {
                child.Arrange(new Rect(left, 0, width, finalSize.Height));
            }
            return finalSize;
        }
    }
}
