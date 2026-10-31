using System.Text.RegularExpressions;
using Settings.Core;

namespace GhosttySettings;

/// <summary>
/// Ghostty ships its themes (key = value files) with the app, and also reads &lt;config dir&gt;/themes.
/// Everything listed is installed; the files give the picker its preview colors.
/// </summary>
internal static partial class GhosttyThemes
{
    [GeneratedRegex(@"^\s*palette\s*=\s*([0-7])\s*=\s*(#?[0-9a-fA-F]{6})\s*$")]
    private static partial Regex PaletteLine();

    [GeneratedRegex(@"^\s*(background|foreground)\s*=\s*(#?[0-9a-fA-F]{6})\s*$")]
    private static partial Regex BaseLine();

    static IReadOnlyList<ThemeInfo>? _bundled;

    public static IReadOnlyList<ThemeInfo> List(string configPath, string? binary)
    {
        _bundled ??= BundledDirs(binary).SelectMany(Read).ToList();
        var user = Read(Path.Combine(Path.GetDirectoryName(configPath)!, "themes")).ToList();
        var userNames = user.Select(t => t.Name).ToHashSet(StringComparer.Ordinal);
        return
        [
            .. user.OrderBy(t => t.Name, StringComparer.OrdinalIgnoreCase),
            .. _bundled.Where(t => !userNames.Contains(t.Name)).OrderBy(t => t.Name, StringComparer.OrdinalIgnoreCase),
        ];
    }

    /// <summary>macOS: Ghostty.app/Contents/Resources/ghostty/themes; elsewhere: &lt;prefix&gt;/share/ghostty/themes.</summary>
    static IEnumerable<string> BundledDirs(string? binary)
    {
        if (binary is null) yield break;
        var binDir = Path.GetDirectoryName(Path.GetFullPath(binary))!;
        yield return Path.GetFullPath(Path.Combine(binDir, "..", "Resources", "ghostty", "themes"));
        yield return Path.GetFullPath(Path.Combine(binDir, "..", "share", "ghostty", "themes"));
    }

    static IEnumerable<ThemeInfo> Read(string dir)
    {
        if (!Directory.Exists(dir)) yield break;
        foreach (var file in Directory.EnumerateFiles(dir))
        {
            var name = Path.GetFileName(file);
            if (name.StartsWith('.')) continue;
            string? bg = null, fg = null;
            var ansi = new string?[8];
            foreach (var line in File.ReadLines(file))
            {
                if (PaletteLine().Match(line) is { Success: true } p) ansi[p.Groups[1].Value[0] - '0'] = Hex(p.Groups[2].Value);
                else if (BaseLine().Match(line) is { Success: true } b)
                {
                    if (b.Groups[1].Value == "background") bg = Hex(b.Groups[2].Value);
                    else fg = Hex(b.Groups[2].Value);
                }
            }
            yield return new ThemeInfo(name, true, bg, fg, ansi);
        }
    }

    static string Hex(string v) => (v.StartsWith('#') ? v : "#" + v).ToLowerInvariant();
}
