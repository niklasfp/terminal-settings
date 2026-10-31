using System.Text.Json.Nodes;
using RioSettings.Toml;
using Settings.Core;

namespace RioSettings;

/// <summary>
/// Rio ships no themes: it loads &lt;config dir&gt;/themes/&lt;name&gt;.toml. The picker lists the installed
/// ones plus the catalogue baked in from mbadolato/iTerm2-Color-Schemes (see tools/rio/update-themes.ts);
/// picking a catalogue theme downloads that one file from the pinned commit.
/// </summary>
internal static class RioThemes
{
    static readonly string[] AnsiNames = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];

    sealed record Catalogue(string Repo, string Commit, string Path, Dictionary<string, ThemeInfo> Themes);

    static readonly Lazy<Catalogue> Embedded = new(LoadCatalogue);

    static Catalogue LoadCatalogue()
    {
        using var stream = typeof(RioThemes).Assembly.GetManifestResourceStream("rio-themes.json");
        var root = stream is null ? null : JsonNode.Parse(stream) as JsonObject;
        var themes = new Dictionary<string, ThemeInfo>(StringComparer.Ordinal);
        foreach (var t in root?["themes"] as JsonArray ?? [])
        {
            var name = t?["name"]?.GetValue<string>();
            if (name is null) continue;
            themes[name] = new ThemeInfo(name, false, t!["bg"]?.GetValue<string>(), t["fg"]?.GetValue<string>(),
                [.. (t["ansi"] as JsonArray ?? []).Select(c => c?.GetValue<string>())]);
        }
        return new Catalogue(
            root?["repo"]?.GetValue<string>() ?? "", root?["commit"]?.GetValue<string>() ?? "",
            root?["path"]?.GetValue<string>() ?? "rio", themes);
    }

    static string ThemesDir(string configPath) => Path.Combine(Path.GetDirectoryName(configPath)!, "themes");

    public static IReadOnlyList<ThemeInfo> List(string configPath)
    {
        var installed = new List<ThemeInfo>();
        var dir = ThemesDir(configPath);
        if (Directory.Exists(dir))
        {
            foreach (var file in Directory.EnumerateFiles(dir, "*.toml"))
                installed.Add(ReadInstalled(file));
        }
        var names = installed.Select(t => t.Name).ToHashSet(StringComparer.Ordinal);
        return
        [
            .. installed.OrderBy(t => t.Name, StringComparer.OrdinalIgnoreCase),
            .. Embedded.Value.Themes.Values.Where(t => !names.Contains(t.Name)).OrderBy(t => t.Name, StringComparer.OrdinalIgnoreCase),
        ];
    }

    static ThemeInfo ReadInstalled(string file)
    {
        var name = Path.GetFileNameWithoutExtension(file);
        try
        {
            var values = ConfigDocument.Parse(File.ReadAllText(file)).Values();
            string? Color(string key) => values.GetValueOrDefault("colors." + key)?.ToString();
            return new ThemeInfo(name, true, Color("background"), Color("foreground"), [.. AnsiNames.Select(Color)]);
        }
        catch (Exception ex) when (ex is TomlEditException or IOException)
        {
            return new ThemeInfo(name, true, null, null, []);
        }
    }

    static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(20) };

    /// <summary>Downloads one catalogue theme into the themes folder. Returns an error message, or null.</summary>
    public static async Task<string?> InstallAsync(string configPath, string name)
    {
        var cat = Embedded.Value;
        // Only names from the baked-in catalogue: they also become file names, so no paths sneak in.
        if (!cat.Themes.ContainsKey(name)) return $"'{name}' isn't in the theme catalogue.";
        if (name.IndexOfAny(Path.GetInvalidFileNameChars()) >= 0) return $"'{name}' can't be used as a file name on this system.";
        var url = $"https://raw.githubusercontent.com/{cat.Repo}/{cat.Commit}/{cat.Path}/{Uri.EscapeDataString(name)}.toml";
        string text;
        try
        {
            text = await Http.GetStringAsync(url);
        }
        catch (Exception ex) when (ex is HttpRequestException or TaskCanceledException)
        {
            return $"Couldn't download the theme ({ex.Message}).";
        }
        try
        {
            ConfigDocument.Parse(text);
        }
        catch (TomlEditException)
        {
            return "The downloaded theme isn't valid TOML.";
        }
        var dir = ThemesDir(configPath);
        Directory.CreateDirectory(dir);
        await File.WriteAllTextAsync(Path.Combine(dir, name + ".toml"), text);
        return null;
    }
}
