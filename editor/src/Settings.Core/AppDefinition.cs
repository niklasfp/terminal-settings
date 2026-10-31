using System.Text.Json.Nodes;

namespace Settings.Core;

/// <summary>A config file format: how values are read from it and how changes are spliced in.</summary>
public interface IConfigFormat
{
    /// <summary>Reads every value, flattened to dotted keys. Throws <see cref="ConfigFormatException"/> when the file can't be parsed.</summary>
    ParsedConfig Parse(string text, SettingCatalog settings);

    /// <summary>Returns <paramref name="text"/> with the changes applied, touching nothing else. Throws <see cref="ChangeErrors"/>.</summary>
    string Apply(string text, IReadOnlyList<SettingChange> changes, string scope);
}

/// <param name="Values">Values by setting key (or dotted path).</param>
/// <param name="Raw">Raw text for settings edited as text blocks, by setting key.</param>
/// <param name="Notices">Things the user should know about the file (e.g. includes the editor doesn't follow).</param>
public sealed record ParsedConfig(
    IReadOnlyDictionary<string, JsonNode?> Values,
    IReadOnlyDictionary<string, string>? Raw = null,
    IReadOnlyList<string>? Notices = null);

public sealed record SettingChange(Setting Setting, JsonNode? Value);

/// <summary>A theme for the picker: name, whether it's usable right away, and colors for a preview.</summary>
public sealed record ThemeInfo(string Name, bool Installed, string? Background, string? Foreground, IReadOnlyList<string?> Ansi);

public sealed class ConfigFormatException(string message) : Exception(message);

public sealed class ConflictException() : Exception("The config file changed on disk since it was loaded.");

public sealed class ChangeErrors(Dictionary<string, string> errors) : Exception("Some values are invalid.")
{
    public Dictionary<string, string> Errors { get; } = errors;
}

/// <summary>Everything the shared server needs to know about one terminal's editor.</summary>
public sealed class AppDefinition
{
    /// <summary>Executable name, e.g. "rio-settings".</summary>
    public required string Name { get; init; }

    /// <summary>Human name of the terminal, e.g. "Rio".</summary>
    public required string Terminal { get; init; }

    /// <summary>The generated settings page.</summary>
    public required string Html { get; init; }

    public required SettingCatalog Settings { get; init; }

    public required IConfigFormat Format { get; init; }

    /// <summary>Resolves the config file to edit when --config isn't given.</summary>
    public required Func<string> DefaultConfigPath { get; init; }

    /// <summary>Platform scopes the format supports for per-platform overrides (empty = none).</summary>
    public string[] Scopes { get; init; } = [];

    /// <summary>Optional check of a candidate file before it's written. Returns errors by setting key ("" for general).</summary>
    public Func<string, Task<Dictionary<string, string>?>>? ValidateAsync { get; init; }

    /// <summary>Optional step after a successful save (e.g. ask the terminal to reload). Returns a note for the UI.</summary>
    public Func<Task<string?>>? AfterSaveAsync { get; init; }

    /// <summary>Themes for the theme picker (installed ones and any the app can install), given the config path.</summary>
    public Func<string, IReadOnlyList<ThemeInfo>>? Themes { get; init; }

    /// <summary>Installs a catalogue theme so the config can use it. Returns an error message, or null on success.</summary>
    public Func<string, string, Task<string?>>? InstallThemeAsync { get; init; }

    /// <summary>The terminal's logo (served at /logo, also the favicon) and its content type.</summary>
    public Func<Stream?>? Logo { get; init; }

    public string LogoContentType { get; init; } = "image/png";

    /// <summary>Font families for the font pickers.</summary>
    public Func<string[]> Fonts { get; init; } = () => FontCatalog.All;

    /// <summary>Extra lists for pickers, served at /api/list/{name}.</summary>
    public IReadOnlyDictionary<string, Func<string[]>> Lists { get; init; } = new Dictionary<string, Func<string[]>>();
}
