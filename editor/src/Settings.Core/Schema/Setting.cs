using System.Text.Json.Nodes;

namespace Settings.Core;

public enum SettingType
{
    Bool, Int, Float, String, Path, Font, Theme, Enum, Color,
    Strings, Flags, Margin, Pair, Blur, FontStyle, Records, Raw,
    Fonts,
}

public sealed record SettingGroup(string Id, string Title);

public sealed record SettingField(string Key, SettingType Type);

/// <summary>One setting. Instances are generated from each app's schema (AppSchema.g.cs).</summary>
public sealed class Setting(string key, SettingType type)
{
    public string Key { get; } = key;
    public SettingType Type { get; } = type;
    public string[] Path { get; } = key.Split('.');
    public string Group { get; init; } = "";
    public string Label { get; init; } = "";
    public double? Min { get; init; }
    public double? Max { get; init; }
    public bool Nullable { get; init; }
    public bool Overridable { get; init; }
    public bool Repeatable { get; init; }
    public string[]? Values { get; init; }
    public SettingField[]? Fields { get; init; }
    public string DefaultJson { get; init; } = "null";

    public JsonNode? Default => JsonNode.Parse(DefaultJson);
}

/// <summary>Lookup over an app's generated settings.</summary>
public sealed class SettingCatalog(IReadOnlyList<Setting> settings)
{
    readonly Dictionary<string, Setting> _byKey = settings.ToDictionary(s => s.Key, StringComparer.Ordinal);

    public IReadOnlyList<Setting> All { get; } = settings;

    public Setting? Find(string key) => _byKey.GetValueOrDefault(key);
}
