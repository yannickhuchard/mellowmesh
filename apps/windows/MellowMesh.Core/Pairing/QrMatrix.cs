using QRCoder;

namespace MellowMesh.Core.Pairing;

/// <summary>Error correction of a QR code: more correction = bigger code, more damage tolerated.</summary>
public enum QrErrorCorrection
{
    /// <summary>~7 %.</summary>
    Low,
    /// <summary>~15 % (the default: a pairing link stays small and scans fast).</summary>
    Medium,
    /// <summary>~25 %.</summary>
    Quartile,
    /// <summary>~30 %.</summary>
    High,
}

/// <summary>
/// A QR code as a square grid of dark/light modules, WITHOUT the quiet zone
/// (renderers add <see cref="QuietZone"/> light modules on every side). The
/// app draws it module by module on whole device pixels, so it stays crisp
/// at any size (no bitmap scaling).
/// </summary>
public sealed class QrMatrix
{
    /// <summary>Light modules required around the code by ISO/IEC 18004.</summary>
    public const int QuietZone = 4;

    private readonly bool[,] _dark;

    private QrMatrix(bool[,] dark, int version)
    {
        _dark = dark;
        Version = version;
    }

    /// <summary>Modules per side, without the quiet zone (21 for version 1, +4 per version).</summary>
    public int Size => _dark.GetLength(0);

    /// <summary>Modules per side including the quiet zone on both sides.</summary>
    public int SizeWithQuietZone => Size + 2 * QuietZone;

    /// <summary>QR version (1–40).</summary>
    public int Version { get; }

    /// <summary>True when the module at (<paramref name="row"/>, <paramref name="col"/>) is dark.</summary>
    public bool this[int row, int col] => _dark[row, col];

    /// <summary>Encode <paramref name="text"/> (UTF-8 when needed) as a QR code.</summary>
    public static QrMatrix Encode(string text, QrErrorCorrection correction = QrErrorCorrection.Medium)
    {
        ArgumentNullException.ThrowIfNull(text);
        if (text.Length == 0)
        {
            throw new ArgumentException("nothing to encode", nameof(text));
        }
        var level = correction switch
        {
            QrErrorCorrection.Low => QRCodeGenerator.ECCLevel.L,
            QrErrorCorrection.Quartile => QRCodeGenerator.ECCLevel.Q,
            QrErrorCorrection.High => QRCodeGenerator.ECCLevel.H,
            _ => QRCodeGenerator.ECCLevel.M,
        };
        using var generator = new QRCodeGenerator();
        using var data = generator.CreateQrCode(text, level);
        var rows = data.ModuleMatrix;
        // QRCoder's matrix carries its own quiet zone; strip whatever border it added.
        var size = 17 + 4 * data.Version;
        var offset = (rows.Count - size) / 2;
        if (offset < 0 || rows.Count != size + 2 * offset)
        {
            throw new InvalidOperationException("unexpected QR matrix size");
        }
        var dark = new bool[size, size];
        for (var r = 0; r < size; r++)
        {
            var row = rows[r + offset];
            for (var c = 0; c < size; c++)
            {
                dark[r, c] = row[c + offset];
            }
        }
        return new QrMatrix(dark, data.Version);
    }

    /// <summary>
    /// Horizontal runs of dark modules (row, first column, length), so a
    /// renderer draws one rectangle per run instead of one per module.
    /// Coordinates exclude the quiet zone.
    /// </summary>
    public IEnumerable<(int Row, int Col, int Length)> DarkRuns()
    {
        for (var r = 0; r < Size; r++)
        {
            var c = 0;
            while (c < Size)
            {
                if (!_dark[r, c])
                {
                    c++;
                    continue;
                }
                var start = c;
                while (c < Size && _dark[r, c])
                {
                    c++;
                }
                yield return (r, start, c - start);
            }
        }
    }

    /// <summary>
    /// Whole device pixels per module so the code (quiet zone included) fits
    /// in <paramref name="maxPixels"/>; never fewer than <paramref name="minModulePixels"/>.
    /// </summary>
    public int ModulePixels(double maxPixels, int minModulePixels = 2)
    {
        var px = double.IsFinite(maxPixels) ? (int)Math.Floor(maxPixels / SizeWithQuietZone) : 0;
        return Math.Max(Math.Max(1, minModulePixels), px);
    }

    /// <summary>
    /// 8-bit grayscale pixels (0 = dark, 255 = light), quiet zone included,
    /// <paramref name="modulePixels"/> pixels per module (nearest neighbour).
    /// Returns the buffer and its side length in pixels.
    /// </summary>
    public (byte[] Pixels, int Side) ToGrayscale(int modulePixels)
    {
        if (modulePixels < 1)
        {
            throw new ArgumentOutOfRangeException(nameof(modulePixels));
        }
        var side = SizeWithQuietZone * modulePixels;
        var pixels = new byte[side * side];
        Array.Fill(pixels, (byte)255);
        foreach (var (row, col, length) in DarkRuns())
        {
            var y0 = (row + QuietZone) * modulePixels;
            var x0 = (col + QuietZone) * modulePixels;
            for (var y = y0; y < y0 + modulePixels; y++)
            {
                Array.Fill(pixels, (byte)0, y * side + x0, length * modulePixels);
            }
        }
        return (pixels, side);
    }
}
