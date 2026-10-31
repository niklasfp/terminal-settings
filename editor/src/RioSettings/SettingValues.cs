using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using RioSettings.Toml;
using Settings.Core;

namespace RioSettings;

internal sealed class SettingValueException(string key, string message) : Exception(message)
{
    public string Key { get; } = key;
}

/// <summary>Validates a value coming from the UI against its setting and turns it into a TOML literal.</summary>
internal static partial class SettingValues
{
    [GeneratedRegex("^#?([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$")]
    private static partial Regex HexColor();

    static readonly string[] BlurStyles = ["macos-glass-regular", "macos-glass-clear"];

    public static string ToToml(Setting setting, JsonNode? value)
    {
        if (value is null) throw Fail(setting, "A value is required. Use reset to remove it instead.");
        return Literal(setting.Key, setting.Type, value, setting);
    }

    static string Literal(string key, SettingType type, JsonNode value, Setting? setting)
    {
        switch (type)
        {
            case SettingType.Bool:
                return Bool(key, value) ? "true" : "false";

            case SettingType.Int:
            {
                var d = Number(key, value);
                if (d != Math.Floor(d)) throw new SettingValueException(key, "Must be a whole number.");
                CheckRange(key, d, setting);
                return TomlValues.Int((long)d);
            }

            case SettingType.Float:
            {
                var d = Number(key, value);
                CheckRange(key, d, setting);
                return TomlValues.Float(d);
            }

            case SettingType.String:
            case SettingType.Path:
            case SettingType.Font:
            case SettingType.Theme:
                return TomlValues.Quote(Str(key, value));

            case SettingType.Enum:
            {
                var s = Str(key, value);
                if (setting?.Values is { } values && !values.Contains(s, StringComparer.Ordinal))
                    throw new SettingValueException(key, $"Must be one of: {string.Join(", ", values)}.");
                return TomlValues.Quote(s);
            }

            case SettingType.Color:
            {
                var s = Str(key, value).Trim();
                if (!HexColor().IsMatch(s)) throw new SettingValueException(key, "Use a hex color like #1e1e2e or #1e1e2ecc.");
                return TomlValues.Quote(s.StartsWith('#') ? s.ToLowerInvariant() : "#" + s.ToLowerInvariant());
            }

            case SettingType.Strings:
                return Array(Items(key, value).Select(i => TomlValues.Quote(Str(key, i))));

            case SettingType.Flags:
            {
                var items = Items(key, value).Select(i => Str(key, i)).ToList();
                if (setting?.Values is { } allowed && items.FirstOrDefault(i => !allowed.Contains(i)) is { } bad)
                    throw new SettingValueException(key, $"Unknown value '{bad}'.");
                return Array(items.Select(TomlValues.Quote));
            }

            case SettingType.Margin:
            {
                var nums = Items(key, value).Select(i => Number(key, i)).ToList();
                if (nums.Count is not (1 or 2 or 4)) throw new SettingValueException(key, "Use 1, 2 or 4 numbers (CSS-style).");
                if (nums.Any(n => n < 0)) throw new SettingValueException(key, "Margins can't be negative.");
                return Array(nums.Select(Num));
            }

            case SettingType.Pair:
            {
                var nums = Items(key, value).Select(i => Number(key, i)).ToList();
                if (nums.Count != 2 || nums.Any(n => n < 0 || n != Math.Floor(n)))
                    throw new SettingValueException(key, "Use two whole numbers.");
                return Array(nums.Select(n => TomlValues.Int((long)n)));
            }

            case SettingType.Blur:
                if (value.GetValueKind() is JsonValueKind.True or JsonValueKind.False) return Bool(key, value) ? "true" : "false";
                var style = Str(key, value);
                if (!BlurStyles.Contains(style)) throw new SettingValueException(key, $"Must be true, false or one of: {string.Join(", ", BlurStyles)}.");
                return TomlValues.Quote(style);

            case SettingType.FontStyle:
                if (value.GetValueKind() == JsonValueKind.False) return "false";
                var name = Str(key, value).Trim();
                return TomlValues.Quote(name.Length == 0 ? "default" : name);

            case SettingType.Records:
            {
                var fields = setting?.Fields ?? [];
                var rows = new List<string>();
                foreach (var item in Items(key, value))
                {
                    if (item is not JsonObject obj) throw new SettingValueException(key, "Each entry must be an object.");
                    var parts = new List<string>();
                    foreach (var f in fields)
                    {
                        var v = obj[f.Key];
                        if (v is null || (v.GetValueKind() == JsonValueKind.String && v.GetValue<string>().Length == 0)) continue;
                        parts.Add($"{ConfigDocument.FormatKey([f.Key])} = {Literal(key, f.Type, v, null)}");
                    }
                    if (parts.Count > 0) rows.Add("{ " + string.Join(", ", parts) + " }");
                }
                // Multi-line arrays are TOML 1.0; multi-line inline tables are not, so each entry stays on one line.
                return rows.Count == 0 ? "[]" : "[\n  " + string.Join(",\n  ", rows) + ",\n]";
            }

            default:
                throw new SettingValueException(key, $"'{type}' values can't be set this way.");
        }
    }

    static void CheckRange(string key, double d, Setting? s)
    {
        if (s?.Min is { } min && d < min) throw new SettingValueException(key, $"Must be at least {Num(min)}.");
        if (s?.Max is { } max && d > max) throw new SettingValueException(key, $"Must be at most {Num(max)}.");
    }

    static string Num(double d) =>
        d == Math.Floor(d) && Math.Abs(d) < 1e15 ? ((long)d).ToString(CultureInfo.InvariantCulture) : TomlValues.Float(d);

    static string Array(IEnumerable<string> items) => "[" + string.Join(", ", items) + "]";

    static IEnumerable<JsonNode> Items(string key, JsonNode value) =>
        value is JsonArray a ? a.Select(i => i ?? throw new SettingValueException(key, "Lists can't contain null.")) : throw new SettingValueException(key, "Expected a list.");

    static bool Bool(string key, JsonNode v) => v.GetValueKind() switch
    {
        JsonValueKind.True => true,
        JsonValueKind.False => false,
        _ => throw new SettingValueException(key, "Expected true or false."),
    };

    static double Number(string key, JsonNode v)
    {
        // A JsonValue built from an int won't hand out a double, so go through its JSON text.
        if (v.GetValueKind() == JsonValueKind.Number) return double.Parse(v.ToJsonString(), NumberStyles.Float, CultureInfo.InvariantCulture);
        if (v.GetValueKind() == JsonValueKind.String &&
            double.TryParse(v.GetValue<string>(), NumberStyles.Float, CultureInfo.InvariantCulture, out var d)) return d;
        throw new SettingValueException(key, "Expected a number.");
    }

    static string Str(string key, JsonNode v) =>
        v.GetValueKind() == JsonValueKind.String ? v.GetValue<string>() : throw new SettingValueException(key, "Expected text.");

    static SettingValueException Fail(Setting s, string message) => new(s.Key, message);
}
