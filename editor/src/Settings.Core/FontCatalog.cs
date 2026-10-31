using System.Diagnostics;
using System.Runtime.InteropServices;

namespace Settings.Core;

/// <summary>Installed font family names, for the font pickers.</summary>
public static partial class FontCatalog
{
    static readonly Lazy<string[]> Families = new(Load);

    public static string[] All => Families.Value;

    static string[] Load()
    {
        try
        {
            if (OperatingSystem.IsMacOS()) return CoreText.FamilyNames();
            if (OperatingSystem.IsLinux() || OperatingSystem.IsFreeBSD()) return FcList();
            if (OperatingSystem.IsWindows()) return WindowsFonts();
        }
        catch (Exception)
        {
            // Font lists are a convenience; the field still accepts any name.
        }
        return [];
    }

    /// <summary>
    /// Windows lists installed fonts in the registry as e.g. "Cascadia Mono SemiBold (TrueType)" or
    /// "MS Gothic & MS UI Gothic (TrueType)"; reduce those to family names.
    /// </summary>
    [System.Runtime.Versioning.SupportedOSPlatform("windows")]
    static string[] WindowsFonts()
    {
        const string key = @"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts";
        var names = new List<string>();
        foreach (var root in new[] { Microsoft.Win32.Registry.LocalMachine, Microsoft.Win32.Registry.CurrentUser })
        {
            using var fonts = root.OpenSubKey(key);
            if (fonts is not null) names.AddRange(fonts.GetValueNames());
        }
        return names
            .Select(n => System.Text.RegularExpressions.Regex.Replace(n, @"\s*\([^)]*\)\s*$", ""))
            .SelectMany(n => n.Split(" & ", StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
            .Select(StripStyle)
            .Where(f => f.Length > 0)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .Order(StringComparer.OrdinalIgnoreCase)
            .ToArray();
    }

    static readonly string[] StyleWords =
        ["Regular", "Bold", "Italic", "Oblique", "Light", "Semilight", "SemiLight", "Semibold", "SemiBold", "Demibold",
         "Medium", "Black", "Heavy", "Thin", "ExtraLight", "UltraLight", "ExtraBold", "UltraBold", "Condensed", "Book"];

    static string StripStyle(string name)
    {
        var words = name.Split(' ', StringSplitOptions.RemoveEmptyEntries).ToList();
        while (words.Count > 1 && StyleWords.Contains(words[^1], StringComparer.OrdinalIgnoreCase)) words.RemoveAt(words.Count - 1);
        return string.Join(' ', words);
    }

    static string[] FcList()
    {
        var psi = new ProcessStartInfo("fc-list", [":", "family"]) { RedirectStandardOutput = true, UseShellExecute = false };
        using var p = Process.Start(psi);
        if (p is null) return [];
        var output = p.StandardOutput.ReadToEnd();
        p.WaitForExit(5000);
        return output.Split('\n', StringSplitOptions.RemoveEmptyEntries)
            .SelectMany(l => l.Split(','))
            .Select(f => f.Trim())
            .Where(f => f.Length > 0)
            .Distinct(StringComparer.Ordinal)
            .Order(StringComparer.OrdinalIgnoreCase)
            .ToArray();
    }

    static partial class CoreText
    {
        const string CoreTextLib = "/System/Library/Frameworks/CoreText.framework/CoreText";
        const string CoreFoundationLib = "/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation";

        [LibraryImport(CoreTextLib)]
        private static partial nint CTFontManagerCopyAvailableFontFamilyNames();

        [LibraryImport(CoreFoundationLib)]
        private static partial nint CFArrayGetCount(nint array);

        [LibraryImport(CoreFoundationLib)]
        private static partial nint CFArrayGetValueAtIndex(nint array, nint index);

        [LibraryImport(CoreFoundationLib)]
        private static partial nint CFStringGetLength(nint str);

        [LibraryImport(CoreFoundationLib)]
        private static unsafe partial void CFStringGetCharacters(nint str, CFRange range, char* buffer);

        [LibraryImport(CoreFoundationLib)]
        private static partial void CFRelease(nint obj);

        [StructLayout(LayoutKind.Sequential)]
        private struct CFRange(nint location, nint length)
        {
            public nint Location = location;
            public nint Length = length;
        }

        public static unsafe string[] FamilyNames()
        {
            nint array = CTFontManagerCopyAvailableFontFamilyNames();
            if (array == 0) return [];
            try
            {
                var count = CFArrayGetCount(array);
                var names = new List<string>((int)count);
                for (nint i = 0; i < count; i++)
                {
                    nint str = CFArrayGetValueAtIndex(array, i);
                    var len = CFStringGetLength(str);
                    var chars = new char[len];
                    fixed (char* p = chars) CFStringGetCharacters(str, new CFRange(0, len), p);
                    var name = new string(chars);
                    // Hidden system families start with a dot.
                    if (!name.StartsWith('.')) names.Add(name);
                }
                return [.. names.Distinct(StringComparer.Ordinal).Order(StringComparer.OrdinalIgnoreCase)];
            }
            finally
            {
                CFRelease(array);
            }
        }
    }
}
