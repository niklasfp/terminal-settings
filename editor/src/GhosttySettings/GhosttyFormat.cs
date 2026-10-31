using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Settings.Core;

namespace GhosttySettings;

/// <summary>
/// Ghostty's config format: one <c>key = value</c> per line, comments on their own lines,
/// later lines win, an empty value resets to the default, and some keys repeat.
/// Edits replace whole lines and leave every other line (comments included) untouched.
/// </summary>
internal sealed partial class GhosttyFormat : IConfigFormat
{
    [GeneratedRegex(@"^\s*([a-z0-9][a-z0-9-]*)\s*=(.*)$")]
    private static partial Regex KeyValueLine();

    [GeneratedRegex("^#?([0-9a-fA-F]{6})$")]
    private static partial Regex HexColor();

    sealed record Line(int Index, string Key, string Value);

    static List<Line> Scan(string[] lines)
    {
        var result = new List<Line>();
        for (int i = 0; i < lines.Length; i++)
        {
            var t = lines[i].TrimStart();
            if (t.Length == 0 || t[0] == '#') continue;
            var m = KeyValueLine().Match(lines[i]);
            if (m.Success) result.Add(new Line(i, m.Groups[1].Value, Unquote(m.Groups[2].Value)));
        }
        return result;
    }

    static string[] SplitLines(string text) =>
        text.Length == 0 ? [] : text.Replace("\r\n", "\n").TrimEnd('\n').Split('\n');

    /// <summary>Ghostty trims spaces around values; a value in double quotes keeps its inner text exactly.</summary>
    static string Unquote(string raw)
    {
        var v = raw.Trim();
        return v.Length >= 2 && v[0] == '"' && v[^1] == '"' ? v[1..^1] : v;
    }

    static string Quote(string v) =>
        v.Length == 0 || v != v.Trim() || v.StartsWith('"') ? "\"" + v + "\"" : v;

    public ParsedConfig Parse(string text, SettingCatalog settings)
    {
        var values = new Dictionary<string, JsonNode?>(StringComparer.Ordinal);
        var notices = new List<string>();
        var byKey = Scan(SplitLines(text)).GroupBy(l => l.Key);
        foreach (var group in byKey)
        {
            var setting = settings.Find(group.Key);
            if (setting is null)
            {
                notices.Add($"Unknown key '{group.Key}' in the file; it is left untouched.");
                continue;
            }
            // An empty value resets the key (and clears a repeatable list); only lines after it count.
            var lines = group.ToList();
            var lastReset = lines.FindLastIndex(l => l.Value.Length == 0);
            var effective = lines.Skip(lastReset + 1).Select(l => l.Value).ToList();
            if (setting.Repeatable)
                values[setting.Key] = ToJsonList(setting, effective);
            else if (effective.Count > 0)
                values[setting.Key] = ToJson(setting, effective[^1]);
        }
        if (values.ContainsKey("config-file"))
            notices.Add("This file includes other files with config-file. Values set there aren't shown here and may override these.");
        return new ParsedConfig(values, Notices: notices);
    }

    static JsonNode? ToJsonList(Setting s, List<string> lines)
    {
        if (s.Type == SettingType.Records)
        {
            var fields = s.Fields ?? [];
            var arr = new JsonArray();
            foreach (var line in lines)
            {
                var eq = line.IndexOf('=');
                var obj = new JsonObject
                {
                    [fields[0].Key] = eq < 0 ? line : line[..eq].Trim(),
                    [fields[1].Key] = eq < 0 ? "" : line[(eq + 1)..].Trim(),
                };
                arr.Add((JsonNode)obj);
            }
            return arr;
        }
        return new JsonArray([.. lines.Select(l => (JsonNode?)JsonValue.Create(l))]);
    }

    static JsonNode? ToJson(Setting s, string v)
    {
        switch (s.Type)
        {
            case SettingType.Bool when v is "true" or "false":
                return JsonValue.Create(v == "true");
            case SettingType.Int when long.TryParse(v, NumberStyles.Integer, CultureInfo.InvariantCulture, out var l):
                return JsonValue.Create(l);
            case SettingType.Float when double.TryParse(v, NumberStyles.Float, CultureInfo.InvariantCulture, out var d):
                return JsonValue.Create(d);
            case SettingType.Flags:
            {
                // "a,no-b" only mentions some flags; the rest keep their defaults.
                var on = new HashSet<string>((s.Default as JsonArray ?? []).Select(n => n!.GetValue<string>()), StringComparer.Ordinal);
                foreach (var part in v.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
                {
                    if (part == "true") on.UnionWith(s.Values ?? []);
                    else if (part == "false") on.Clear();
                    else if (part.StartsWith("no-", StringComparison.Ordinal)) on.Remove(part[3..]);
                    else on.Add(part);
                }
                return new JsonArray([.. (s.Values ?? []).Where(on.Contains).Select(f => (JsonNode?)JsonValue.Create(f))]);
            }
            case SettingType.FontStyle when v == "false":
                return JsonValue.Create(false);
            case SettingType.Color when HexColor().Match(v) is { Success: true } m:
                return JsonValue.Create("#" + m.Groups[1].Value.ToLowerInvariant());
            default:
                return JsonValue.Create(v);
        }
    }

    public string Apply(string text, IReadOnlyList<SettingChange> changes, string scope)
    {
        var lines = SplitLines(text).ToList();
        var errors = new Dictionary<string, string>(StringComparer.Ordinal);
        bool appended = false;

        foreach (var (setting, value) in changes)
        {
            List<string> newValues;
            try
            {
                newValues = value is null ? [] : GhosttyValues.ToLines(setting, value);
            }
            catch (SettingValueException ex)
            {
                errors[setting.Key] = ex.Message;
                continue;
            }

            var existing = Scan([.. lines]).Where(l => l.Key == setting.Key).Select(l => l.Index).ToList();
            if (!setting.Repeatable && value is not null && existing.Count > 0)
            {
                // Scalar: rewrite the line that wins (the last one), keep earlier lines as they are.
                lines[existing[^1]] = $"{setting.Key} = {Quote(newValues[0])}";
                continue;
            }

            // Reset, or a repeatable list: replace all of its lines at the position of the first one.
            int at = existing.Count > 0 ? existing[0] : lines.Count;
            for (int k = existing.Count - 1; k >= 0; k--) lines.RemoveAt(existing[k]);
            // Keys new to the file go at the end, as one block after a blank line.
            if (existing.Count == 0 && newValues.Count > 0 && !appended && lines.Count > 0 && lines[^1].Trim().Length > 0)
            {
                lines.Add("");
                at = lines.Count;
            }
            if (existing.Count == 0 && newValues.Count > 0) appended = true;
            lines.InsertRange(Math.Min(at, lines.Count), newValues.Select(v => $"{setting.Key} = {Quote(v)}"));
        }

        if (errors.Count > 0) throw new ChangeErrors(errors);
        var sb = new StringBuilder();
        foreach (var l in lines) sb.Append(l).Append('\n');
        return sb.ToString();
    }
}
