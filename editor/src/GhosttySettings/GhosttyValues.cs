using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Settings.Core;

namespace GhosttySettings;

internal sealed class SettingValueException(string key, string message) : Exception(message)
{
    public string Key { get; } = key;
}

/// <summary>
/// Turns a UI value into Ghostty config values (one per line). Checks the obvious things here;
/// Ghostty's own `+validate-config` then checks the whole file before it's written.
/// </summary>
internal static partial class GhosttyValues
{
    [GeneratedRegex("^#?([0-9a-fA-F]{6})$")]
    private static partial Regex HexColor();

    [GeneratedRegex(@"^[A-Za-z][A-Za-z0-9 ]*$")]
    private static partial Regex ColorName();

    public static List<string> ToLines(Setting s, JsonNode value)
    {
        switch (s.Type)
        {
            case SettingType.Bool:
                return [Bool(s, value) ? "true" : "false"];

            case SettingType.Int:
            {
                var d = Number(s, value);
                if (d != Math.Floor(d)) throw new SettingValueException(s.Key, "Must be a whole number.");
                CheckRange(s, d);
                return [((long)d).ToString(CultureInfo.InvariantCulture)];
            }

            case SettingType.Float:
            {
                var d = Number(s, value);
                CheckRange(s, d);
                return [d.ToString("R", CultureInfo.InvariantCulture)];
            }

            case SettingType.Enum:
            {
                var v = Text(s, value);
                if (s.Values is { } values && !values.Contains(v, StringComparer.Ordinal))
                    throw new SettingValueException(s.Key, $"Must be one of: {string.Join(", ", values)}.");
                return [v];
            }

            case SettingType.Color:
            {
                var color = Text(s, value).Trim();
                var m = HexColor().Match(color);
                if (m.Success) return ["#" + m.Groups[1].Value.ToLowerInvariant()];
                // Keywords (cell-foreground, bright) and X11 names (red, dark slate gray); Ghostty validates names.
                if ((s.Values?.Contains(color) ?? false) || ColorName().IsMatch(color)) return [color];
                throw new SettingValueException(s.Key, "Use a hex color like #1e1e2e or a color name.");
            }

            case SettingType.Flags:
            {
                // Spell out every flag so the line means the same thing whatever the defaults become.
                var on = Items(s, value).Select(i => Text(s, i)).ToHashSet(StringComparer.Ordinal);
                var known = s.Values ?? [];
                if (on.FirstOrDefault(f => !known.Contains(f)) is { } bad)
                    throw new SettingValueException(s.Key, $"Unknown option '{bad}'.");
                return [string.Join(",", known.Select(f => on.Contains(f) ? f : "no-" + f))];
            }

            case SettingType.FontStyle:
                if (value.GetValueKind() == JsonValueKind.False) return ["false"];
                var style = Text(s, value).Trim();
                return [style.Length == 0 ? "default" : style];

            case SettingType.Strings:
            case SettingType.Fonts:
                return [.. Items(s, value).Select(i => Text(s, i)).Where(v => v.Length > 0)];

            case SettingType.Records:
            {
                var fields = s.Fields ?? [];
                var lines = new List<string>();
                foreach (var item in Items(s, value))
                {
                    if (item is not JsonObject obj || fields.Length < 2) throw new SettingValueException(s.Key, "Each entry needs two parts.");
                    var a = (obj[fields[0].Key]?.ToString() ?? "").Trim();
                    var b = (obj[fields[1].Key]?.ToString() ?? "").Trim();
                    if (a.Length == 0 && b.Length == 0) continue;
                    if (a.Length == 0 || b.Length == 0) throw new SettingValueException(s.Key, $"Each entry needs both {fields[0].Key} and {fields[1].Key}.");
                    lines.Add($"{a}={b}");
                }
                return lines;
            }

            default:
                // Strings, paths, themes, durations and Ghostty's own formats (padding, modifiers, …):
                // passed through exactly (no trimming: some values start with spaces) and checked by Ghostty.
                return [Text(s, value)];
        }
    }

    static void CheckRange(Setting s, double d)
    {
        if (s.Min is { } min && d < min) throw new SettingValueException(s.Key, $"Must be at least {min.ToString(CultureInfo.InvariantCulture)}.");
        if (s.Max is { } max && d > max) throw new SettingValueException(s.Key, $"Must be at most {max.ToString(CultureInfo.InvariantCulture)}.");
    }

    static IEnumerable<JsonNode> Items(Setting s, JsonNode v) =>
        v is JsonArray a ? a.Select(i => i ?? throw new SettingValueException(s.Key, "Lists can't contain null.")) : throw new SettingValueException(s.Key, "Expected a list.");

    static bool Bool(Setting s, JsonNode v) => v.GetValueKind() switch
    {
        JsonValueKind.True => true,
        JsonValueKind.False => false,
        _ => throw new SettingValueException(s.Key, "Expected true or false."),
    };

    static double Number(Setting s, JsonNode v)
    {
        if (v.GetValueKind() == JsonValueKind.Number) return double.Parse(v.ToJsonString(), NumberStyles.Float, CultureInfo.InvariantCulture);
        if (v.GetValueKind() == JsonValueKind.String && double.TryParse(v.GetValue<string>(), NumberStyles.Float, CultureInfo.InvariantCulture, out var d)) return d;
        throw new SettingValueException(s.Key, "Expected a number.");
    }

    static string Text(Setting s, JsonNode v) => v.GetValueKind() switch
    {
        JsonValueKind.String => v.GetValue<string>(),
        JsonValueKind.Number or JsonValueKind.True or JsonValueKind.False => v.ToJsonString(),
        _ => throw new SettingValueException(s.Key, "Expected text."),
    };
}
