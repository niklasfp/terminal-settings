using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;

namespace Settings.Generators;

/// <summary>
/// Merges ghostty-source.json (generated from `ghostty +show-config --default --docs` and Config.zig)
/// with ghostty-overlay.json (groups, widget types, ranges). Labels and docs come from Ghostty itself.
/// </summary>
internal static class GhosttySchemaMerger
{
    // Repeatable keys with long built-in defaults show "Ghostty's built-ins" instead of listing them.
    const int MaxListedDefaults = 12;

    public static SchemaDoc Merge(string sourceJson, string overlayJson, List<SchemaDiagnostic> diags)
    {
        var source = (JObj)MiniJson.Parse(sourceJson)!;
        var overlay = (JObj)MiniJson.Parse(overlayJson)!;

        var schema = new SchemaDoc { Revision = source.Str("ghosttyVersion") ?? "unknown" };
        SchemaCommon.ReadApp(schema, overlay);

        foreach (var g in (overlay.Arr("groups") ?? new List<object?>()).OfType<JObj>())
            schema.Groups.Add(new GroupDef { Id = g.Str("id")!, Title = g.Str("title")!, Icon = g.Str("icon") ?? "", Doc = g.Str("doc") });
        var groupIds = new HashSet<string>(schema.Groups.Select(g => g.Id));

        var rules = new List<(string group, string prefix)>();
        foreach (var r in (overlay.Arr("rules") ?? new List<object?>()).OfType<JObj>())
            foreach (var p in (r.Arr("prefix") ?? new List<object?>()).OfType<string>())
                rules.Add((r.Str("group")!, p));
        foreach (var (group, _) in rules)
            if (!groupIds.Contains(group))
                diags.Add(new SchemaDiagnostic("GHO007", true, $"Rule uses unknown group '{group}'."));

        var platformPrefixes = overlay.Obj("platformPrefixes") ?? new JObj();
        var typeMap = overlay.Obj("typeMap") ?? new JObj();
        var colorKeywords = overlay.Obj("colorKeywords") ?? new JObj();
        var overrides = overlay.Obj("settings") ?? new JObj();
        var seen = new HashSet<string>(StringComparer.Ordinal);

        foreach (var opt in (source.Arr("options") ?? new List<object?>()).OfType<JObj>())
        {
            var key = opt.Str("key")!;
            seen.Add(key);
            var ov = overrides.Obj(key) ?? new JObj();
            var kind = opt.Str("kind") ?? "other";
            var zigType = opt.Str("zigType") ?? "";
            bool repeatable = opt.Bool("repeatable");
            bool optional = opt.Bool("optional");
            var defaults = (opt.Arr("defaults") ?? new List<object?>()).OfType<string>().ToList();

            var type = ov.Str("type") ?? MapKind(key, kind, zigType, repeatable, typeMap, diags);
            var (summary, more) = SplitDoc(opt.Str("doc"));
            // Durations are free text in Ghostty's own syntax; say what it looks like.
            var placeholder = ov.Str("placeholder") ?? (kind == "duration" ? "e.g. 750ms or 5s" : null);

            var def = new SettingDef
            {
                Key = key,
                Group = GroupFor(key, rules),
                Label = SchemaCommon.Humanize(key),
                Type = type,
                Doc = summary,
                DocMore = more,
                Min = ov.Num("min"),
                Max = ov.Num("max"),
                Step = ov.Num("step"),
                Unit = ov.Str("unit"),
                Placeholder = placeholder,
                Pick = ov.Str("pick"),
                Compact = kind == "duration" || IsCompactType(zigType, overlay),
                Repeatable = repeatable,
                Platforms = opt.Arr("platforms")?.OfType<string>().ToList() ?? PlatformsByPrefix(key, platformPrefixes),
            };

            if (opt.Str("since") is { } since) def.Tags.Add("Since " + since);
            switch (opt.Str("reload"))
            {
                case "restart": def.Tags.Add("Restart needed"); break;
                case "new-terminals": def.Tags.Add("New windows only"); break;
            }

            def.Values = ov.Arr("values")?.OfType<string>().ToList()
                ?? opt.Arr("values")?.OfType<string>().ToList()
                ?? opt.Arr("flags")?.OfType<JObj>().Select(f => f.Str("name")!).ToList();
            if (type == "enum" && (def.Values is null || def.Values.Count == 0))
            {
                diags.Add(new SchemaDiagnostic("GHO004", false, $"Enum '{key}' has no known values; it's shown as text."));
                def.Type = type = "string";
            }

            // Colors that also accept keywords (e.g. cell-foreground) keep them as allowed extra values.
            if (type == "color" && colorKeywords.Arr(zigType) is { } keywords)
                def.Values = keywords.OfType<string>().ToList();

            if (type == "records") def.Fields = RecordFields(kind);

            def.Default = DefaultFor(def, defaults, opt);
            def.Nullable = ov.Has("nullable") ? ov.Bool("nullable") : optional || def.Default is null;
            if (repeatable && defaults.Count > MaxListedDefaults) def.BuiltInsNote = $"{defaults.Count} built-in entries";

            schema.Settings.Add(def);
        }

        foreach (var kv in overrides.Members)
            if (!seen.Contains(kv.Key))
                diags.Add(new SchemaDiagnostic("GHO003", false, $"Overlay setting '{kv.Key}' is not in this Ghostty version's options."));

        return schema;
    }

    static string MapKind(string key, string kind, string zigType, bool repeatable, JObj typeMap, List<SchemaDiagnostic> diags)
    {
        switch (kind)
        {
            case "bool": return "bool";
            case "int": return "int";
            case "float": return "float";
            case "string": return "string";
            case "enum": return "enum";
            case "flags": return "flags";
            case "color": return "color";
            case "duration": return "string";
            case "repeatable-string": return "strings";
            case "palette":
            case "keybind": return "records";
        }
        // "other": look the Zig type up in the overlay (exact, then by its generic name, e.g. Limit(...)).
        if (typeMap.Str(zigType) is { } mapped) return mapped;
        var paren = zigType.IndexOf('(');
        if (paren > 0 && typeMap.Str(zigType.Substring(0, paren)) is { } generic) return generic;
        diags.Add(new SchemaDiagnostic("GHO001", false, $"'{key}' has Zig type {zigType} with no widget mapping; it's shown as text."));
        return repeatable ? "strings" : "string";
    }

    /// <summary>Zig types whose values are short (listed in the overlay's "compactTypes", matched with or without generics).</summary>
    static bool IsCompactType(string zigType, JObj overlay)
    {
        var paren = zigType.IndexOf('(');
        var bare = paren > 0 ? zigType.Substring(0, paren) : zigType;
        return (overlay.Arr("compactTypes") ?? new List<object?>()).OfType<string>().Any(t => t == zigType || t == bare);
    }

    static List<FieldDef> RecordFields(string kind) => kind == "palette"
        ? new List<FieldDef>
        {
            new() { Key = "index", Label = "Index", Type = "string", Placeholder = "0–255" },
            new() { Key = "color", Label = "Color", Type = "color", Placeholder = "#rrggbb" },
        }
        : new List<FieldDef>
        {
            new() { Key = "trigger", Label = "Trigger", Type = "string", Placeholder = "super+shift+t" },
            new() { Key = "action", Label = "Action", Type = "string", Placeholder = "new_tab", List = "action-list" },
        };

    static object? DefaultFor(SettingDef def, List<string> defaults, JObj opt)
    {
        var first = defaults.FirstOrDefault();
        switch (def.Type)
        {
            case "bool":
                return first switch { "true" => true, "false" => false, _ => null };
            case "int":
                return long.TryParse(first, NumberStyles.Integer, CultureInfo.InvariantCulture, out var l) ? l : null;
            case "float":
                return double.TryParse(first, NumberStyles.Float, CultureInfo.InvariantCulture, out var d) ? d : null;
            case "flags":
                return (opt.Arr("flags") ?? new List<object?>()).OfType<JObj>()
                    .Where(f => f.Bool("default")).Select(f => (object?)f.Str("name")).ToList();
            case "strings":
            case "fonts":
                return defaults.Count > MaxListedDefaults
                    ? new List<object?>()
                    : defaults.Where(v => v.Length > 0).Select(v => (object?)v).ToList();
            case "records":
                return new List<object?>();
            case "fontStyle":
                return first == "false" ? false : string.IsNullOrEmpty(first) ? null : first;
            default:
                return string.IsNullOrEmpty(first) ? null : first;
        }
    }

    static string GroupFor(string key, List<(string group, string prefix)> rules)
    {
        // Longest matching prefix wins, so "background-image" beats "background".
        var best = rules.Where(r => key.StartsWith(r.prefix, StringComparison.Ordinal))
                        .OrderByDescending(r => r.prefix.Length)
                        .FirstOrDefault();
        return best.group ?? "general";
    }

    static List<string>? PlatformsByPrefix(string key, JObj prefixes)
    {
        foreach (var kv in prefixes.Members)
            if (key.StartsWith(kv.Key, StringComparison.Ordinal))
                return (kv.Value as List<object?>)?.OfType<string>().ToList();
        return null;
    }

    /// <summary>First paragraph as the summary, the rest (markdown) behind "More".</summary>
    static (string? summary, string? more) SplitDoc(string? doc)
    {
        if (string.IsNullOrWhiteSpace(doc)) return (null, null);
        var text = doc!.Replace("\r\n", "\n").Trim('\n');
        var cut = text.IndexOf("\n\n", StringComparison.Ordinal);
        var summary = (cut < 0 ? text : text.Substring(0, cut)).Replace('\n', ' ').Trim();
        var more = cut < 0 ? null : text.Substring(cut + 2).Trim('\n');
        return (summary, string.IsNullOrWhiteSpace(more) ? null : more);
    }
}
