using MellowMesh.Core.Pairing;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Windows.Foundation;
using XPath = Microsoft.UI.Xaml.Shapes.Path;

namespace MellowMesh.App.Controls;

/// <summary>
/// A QR code drawn as vector rectangles (one per horizontal run of dark
/// modules) on white, with the 4-module quiet zone. The module size is
/// snapped to whole device pixels for the current display scale, so every
/// edge lands on a pixel boundary: crisp at 100 %, 125 %, 150 %… and never a
/// scaled, blurry bitmap. Redrawn when the window moves to another monitor.
/// </summary>
public sealed class QrCodeView : UserControl
{
    private readonly Grid _root = new() { Background = Ui.Brush(Colors.White) };
    private QrMatrix? _matrix;
    private double _targetSize = 300;
    private double _scale = 1;
    private XamlRoot? _hookedRoot;

    public QrCodeView()
    {
        IsTabStop = false;
        HorizontalAlignment = HorizontalAlignment.Center;
        VerticalAlignment = VerticalAlignment.Center;
        Content = _root;
        AutomationProperties.SetName(this, "QR code to scan with the MellowMesh app on your phone");
        Loaded += (_, _) =>
        {
            Hook();
            Rebuild();
        };
        Unloaded += (_, _) => Unhook();
    }

    /// <summary>The code to draw (null = blank white square).</summary>
    public QrMatrix? Matrix
    {
        get => _matrix;
        set
        {
            _matrix = value;
            Rebuild();
        }
    }

    /// <summary>Largest side in DIPs, quiet zone included (the drawn side is at most this).</summary>
    public double TargetSize
    {
        get => _targetSize;
        set
        {
            _targetSize = Math.Max(64, value);
            Rebuild();
        }
    }

    private void Hook()
    {
        Unhook();
        _hookedRoot = XamlRoot;
        if (_hookedRoot is not null)
        {
            _hookedRoot.Changed += OnRootChanged;
        }
    }

    private void Unhook()
    {
        if (_hookedRoot is not null)
        {
            _hookedRoot.Changed -= OnRootChanged;
            _hookedRoot = null;
        }
    }

    private void OnRootChanged(XamlRoot sender, XamlRootChangedEventArgs args)
    {
        if (Math.Abs(sender.RasterizationScale - _scale) > 0.001)
        {
            Rebuild();
        }
    }

    private void Rebuild()
    {
        _scale = XamlRoot?.RasterizationScale is double s && s > 0 ? s : 1;
        _root.Children.Clear();
        var matrix = _matrix;
        if (matrix is null)
        {
            _root.Width = _targetSize;
            _root.Height = _targetSize;
            return;
        }
        // Whole device pixels per module, then back to DIPs.
        var modulePx = matrix.ModulePixels(_targetSize * _scale, minModulePixels: 2);
        var module = modulePx / _scale;
        var side = matrix.SizeWithQuietZone * module;
        var group = new GeometryGroup { FillRule = FillRule.Nonzero };
        foreach (var (row, col, length) in matrix.DarkRuns())
        {
            group.Children.Add(new RectangleGeometry
            {
                Rect = new Rect(
                    (col + QrMatrix.QuietZone) * module,
                    (row + QrMatrix.QuietZone) * module,
                    length * module,
                    module),
            });
        }
        _root.Width = side;
        _root.Height = side;
        _root.Children.Add(new XPath
        {
            Data = group,
            Fill = Ui.Brush(Colors.Black),
            Width = side,
            Height = side,
            Stretch = Stretch.None,
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Top,
        });
    }
}
