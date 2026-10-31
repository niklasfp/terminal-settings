using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Text;

namespace Settings.Generators;

/// <summary>
/// Emits the whole settings page: one section per group, one row per setting, with the
/// control picked from the setting's type. app.js wires the controls generically via data-*.
/// </summary>
internal static class HtmlEmitter
{
    static readonly Dictionary<string, string> ValueLabels = new()
    {
        ["nativetab"] = "Native tabs", ["tab"] = "Tabs", ["plain"] = "Plain",
        ["srgb"] = "sRGB", ["display-p3"] = "Display P3", ["rec2020"] = "Rec. 2020",
        ["donotround"] = "Square", ["roundsmall"] = "Small", ["round"] = "Round", ["default"] = "Default",
        ["webgpu"] = "WebGPU", ["metal"] = "Metal", ["vulkan"] = "Vulkan",
        ["false"] = "Off", ["true"] = "On",
        ["OFF"] = "Off", ["ERROR"] = "Error", ["WARN"] = "Warn", ["INFO"] = "Info", ["DEBUG"] = "Debug", ["TRACE"] = "Trace",
    };

    static readonly Dictionary<string, string> PlatformLabels = new()
    {
        ["macos"] = "macOS", ["linux"] = "Linux", ["windows"] = "Windows",
    };

    // 24x24 stroke icons, drawn for this app.
    static readonly Dictionary<string, string> Icons = new()
    {
        ["sliders"] = "<path d='M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0'/><circle cx='16' cy='6' r='2'/><circle cx='10' cy='12' r='2'/><circle cx='18' cy='18' r='2'/>",
        ["window"] = "<rect x='3' y='4' width='18' height='16' rx='2'/><path d='M3 9h18'/><circle cx='6.5' cy='6.5' r='.5'/><circle cx='9' cy='6.5' r='.5'/>",
        ["type"] = "<path d='M5 7V5h14v2M12 5v14M9 19h6'/>",
        ["palette"] = "<path d='M12 3a9 9 0 1 0 0 18c1.1 0 1.6-.8 1.6-1.6 0-.9-.6-1.4-.6-2.2 0-.9.7-1.6 1.6-1.6H17a4 4 0 0 0 4-4c0-4.7-4-8.6-9-8.6z'/><circle cx='7.5' cy='11' r='1'/><circle cx='10' cy='7' r='1'/><circle cx='15' cy='7.5' r='1'/>",
        ["droplet"] = "<path d='M12 3s6 6.4 6 11a6 6 0 0 1-12 0c0-4.6 6-11 6-11z'/>",
        ["cursor"] = "<rect x='7' y='4' width='6' height='16' rx='1'/><path d='M17 8v8'/>",
        ["tabs"] = "<path d='M3 19V8a2 2 0 0 1 2-2h4l2 3h8a2 2 0 0 1 2 2v8H3z'/>",
        ["layout"] = "<rect x='3' y='3' width='18' height='18' rx='2'/><path d='M12 3v18M12 12h9'/>",
        ["terminal"] = "<rect x='3' y='4' width='18' height='16' rx='2'/><path d='M7 9l3 3-3 3M13 15h4'/>",
        ["keyboard"] = "<rect x='2' y='6' width='20' height='12' rx='2'/><path d='M6 10h0M10 10h0M14 10h0M18 10h0M7 14h10'/>",
        ["command"] = "<path d='M9 6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3V6z'/>",
        ["link"] = "<path d='M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1'/>",
        ["bell"] = "<path d='M6 16V11a6 6 0 0 1 12 0v5l2 2H4l2-2zM10 20a2 2 0 0 0 4 0'/>",
        ["cpu"] = "<rect x='6' y='6' width='12' height='12' rx='1'/><path d='M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4'/>",
        ["bug"] = "<rect x='8' y='7' width='8' height='12' rx='4'/><path d='M12 7V5M9 4l1.5 2M15 4l-1.5 2M4 12h4M16 12h4M5 18l3-2M19 18l-3-2M5 7l3 2M19 7l-3 2'/>",
        ["reset"] = "<path d='M4 12a8 8 0 1 0 2.3-5.7L4 8.6'/><path d='M4 4v4.6h4.6'/>",
        ["clear"] = "<circle cx='12' cy='12' r='8'/><path d='M9 9l6 6M15 9l-6 6'/>",
        ["search"] = "<circle cx='11' cy='11' r='6'/><path d='M20 20l-4.5-4.5'/>",
        ["sun"] = "<circle cx='12' cy='12' r='4'/><path d='M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4'/>",
        ["moon"] = "<path d='M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z'/>",
        ["auto"] = "<rect x='3' y='4' width='18' height='13' rx='2'/><path d='M8 21h8M12 17v4'/>",
    };

    public static string Emit(SchemaDoc schema)
    {
        var sb = new StringBuilder();
        sb.Append("<!doctype html>\n<html lang=\"en\"");
        if (schema.Brand.Length > 0) sb.Append(" data-brand=\"").Append(H(schema.Brand)).Append('"');
        sb.Append(">\n<head>\n<meta charset=\"utf-8\">\n");
        if (schema.Brand.Length > 0) sb.Append("<link rel=\"icon\" href=\"/logo\">\n");
        sb.Append("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
        sb.Append("<meta name=\"color-scheme\" content=\"light dark\">\n");
        sb.Append("<title>").Append(H(schema.Title)).Append("</title>\n");
        // Apply the theme before first paint so there is no light/dark flash.
        sb.Append("<script>(function(){var p=localStorage.getItem('settings-theme')||'system';document.documentElement.dataset.themePref=p;if(p!=='system')document.documentElement.dataset.theme=p;})();</script>\n");
        sb.Append("<link rel=\"stylesheet\" href=\"/app.css\">\n</head>\n<body>\n");

        // Top bar
        sb.Append("<header class=\"topbar\">\n");
        sb.Append("  <div class=\"brand\">");
        if (schema.Brand.Length > 0) sb.Append("<img class=\"logo-img\" src=\"/logo\" alt=\"\" width=\"34\" height=\"34\">");
        else sb.Append("<span class=\"logo\" aria-hidden=\"true\">").Append(H(schema.Logo)).Append("</span>");
        sb.Append("<div><h1>").Append(H(schema.Title)).Append("</h1>");
        sb.Append("<p class=\"path\" id=\"config-path\" title=\"Config file\">…</p></div></div>\n");
        sb.Append("  <div class=\"top-actions\">\n");
        if (schema.Scopes.Count > 0)
        {
            sb.Append("    <label class=\"scope\" title=\"Edit the global settings or a per-platform override\"><span>Applies to</span>\n");
            sb.Append("      <select id=\"scope\"><option value=\"\">All platforms</option>");
            foreach (var s in schema.Scopes)
                sb.Append("<option value=\"").Append(H(s)).Append("\">").Append(PlatformLabels.TryGetValue(s, out var pl) ? pl : s).Append(" only</option>");
            sb.Append("</select></label>\n");
        }
        sb.Append("    <div class=\"segmented theme-switch\" role=\"radiogroup\" aria-label=\"Appearance\" id=\"theme-switch\">");
        foreach (var (val, icon, label) in new[] { ("system", "auto", "System"), ("light", "sun", "Light"), ("dark", "moon", "Dark") })
            sb.Append("<button type=\"button\" role=\"radio\" data-value=\"").Append(val).Append("\" title=\"").Append(label).Append("\">").Append(Icon(icon)).Append("<span>").Append(label).Append("</span></button>");
        sb.Append("</div>\n");
        sb.Append("    <button type=\"button\" class=\"btn primary\" id=\"save\" disabled><span>Save</span><span class=\"count\" id=\"dirty-count\" hidden></span></button>\n");
        sb.Append("  </div>\n</header>\n");
        sb.Append("<div class=\"banner\" id=\"banner\" hidden></div>\n");
        sb.Append("<div class=\"banner scope-banner\" id=\"scope-banner\" hidden></div>\n");
        sb.Append("<div class=\"banner notices\" id=\"notices\" hidden></div>\n");

        sb.Append("<div class=\"layout\">\n");

        // Sidebar
        sb.Append("<nav class=\"sidebar\" aria-label=\"Sections\">\n");
        sb.Append("  <div class=\"search\">").Append(Icon("search")).Append("<input type=\"search\" id=\"search\" placeholder=\"Search settings\" autocomplete=\"off\" spellcheck=\"false\"><kbd>/</kbd></div>\n");
        sb.Append("  <ul>\n");
        foreach (var g in schema.Groups)
        {
            if (!schema.Settings.Any(s => s.Group == g.Id)) continue;
            sb.Append("    <li><a href=\"#g-").Append(g.Id).Append("\" data-group=\"").Append(g.Id).Append("\">")
              .Append(Icon(g.Icon)).Append("<span class=\"name\">").Append(H(g.Title)).Append("</span><span class=\"badge-count\" data-modified-count=\"").Append(g.Id).Append("\"></span></a></li>\n");
        }
        sb.Append("  </ul>\n");
        // The app version is only known at run time; the server replaces this marker.
        sb.Append("  <!--app-version-->\n");
        sb.Append("  <p class=\"rev\">Schema from ").Append(H(schema.Terminal)).Append(" <code>").Append(H(Short(schema.Revision))).Append("</code></p>\n");
        sb.Append("</nav>\n");

        // Content
        sb.Append("<main id=\"main\">\n");
        sb.Append("<p class=\"empty-search\" id=\"empty-search\" hidden>No settings match your search.</p>\n");
        foreach (var g in schema.Groups)
        {
            var settings = schema.Settings.Where(s => s.Group == g.Id).ToList();
            if (settings.Count == 0) continue;
            sb.Append("<section class=\"group\" id=\"g-").Append(g.Id).Append("\" data-group=\"").Append(g.Id).Append("\">\n");
            sb.Append("  <header><h2>").Append(Icon(g.Icon)).Append(H(g.Title)).Append("</h2>");
            if (g.Doc is { } gdoc) sb.Append("<p>").Append(Doc(gdoc)).Append("</p>");
            sb.Append("</header>\n");
            if (g.Id == "colors" && schema.ThemeLocksColors) // Rio: [colors] table preview + theme notice
            {
                sb.Append("  <div class=\"notice\" id=\"colors-theme-notice\" hidden><span>A theme is set, so Rio ignores these colors.</span> <button type=\"button\" class=\"btn small\" id=\"clear-theme\">Clear theme to edit colors</button></div>\n");
                sb.Append("  <div class=\"terminal-preview\" id=\"terminal-preview\" aria-hidden=\"true\"></div>\n");
            }
            foreach (var sub in settings.GroupBy(s => s.Subgroup ?? ""))
            {
                sb.Append("  <div class=\"card\">");
                if (sub.Key.Length > 0) sb.Append("<h3>").Append(H(sub.Key)).Append("</h3>");
                sb.Append('\n');
                foreach (var s in sub) EmitSetting(sb, s);
                sb.Append("  </div>\n");
            }
            sb.Append("</section>\n");
        }
        sb.Append("</main>\n</div>\n");

        // Shared datalists
        sb.Append("<datalist id=\"action-list\"></datalist>\n");
        sb.Append("<datalist id=\"font-style-list\">");
        foreach (var st in new[] { "default", "Thin", "ExtraLight", "Light", "Regular", "Medium", "SemiBold", "Bold", "ExtraBold", "Heavy" })
            sb.Append("<option value=\"").Append(st).Append("\">");
        sb.Append("</datalist>\n");

        // Save dialog
        sb.Append("<dialog id=\"save-dialog\">\n  <form method=\"dialog\">\n");
        sb.Append("    <h2>Save changes to <span id=\"save-file\">the config file</span>?</h2>\n    <p class=\"muted\">Only the lines below change. Comments and formatting stay as they are.</p>\n");
        sb.Append("    <pre class=\"diff\" id=\"diff\"></pre>\n    <p class=\"error\" id=\"save-error\" hidden></p>\n");
        sb.Append("    <div class=\"dialog-actions\"><button value=\"cancel\" class=\"btn\">Cancel</button><button value=\"save\" class=\"btn primary\" id=\"confirm-save\">Save</button></div>\n");
        sb.Append("  </form>\n</dialog>\n");
        sb.Append("<div class=\"toast\" id=\"toast\" role=\"status\" aria-live=\"polite\"></div>\n");

        // Metadata for app.js
        sb.Append("<script type=\"application/json\" id=\"rio-schema\">");
        MiniJson.Write(sb, Metadata(schema));
        sb.Append("</script>\n");
        sb.Append("<script src=\"/app.js\"></script>\n</body>\n</html>\n");
        return sb.ToString();
    }

    static void EmitSetting(StringBuilder sb, SettingDef s)
    {
        var slug = Slug(s.Key);
        bool wide = s.Type is "records" or "raw";
        var haystack = (s.Label + " " + s.Key + " " + s.Subgroup + " " + s.Doc).ToLowerInvariant();

        sb.Append("    <div class=\"setting").Append(wide ? " wide" : "").Append("\" id=\"s-").Append(slug)
          .Append("\" data-key=\"").Append(H(s.Key)).Append("\" data-type=\"").Append(s.Type)
          .Append("\" data-search=\"").Append(H(haystack)).Append("\">\n");

        sb.Append("      <div class=\"meta\"><div class=\"title\"><label id=\"l-").Append(slug).Append("\" for=\"c-").Append(slug).Append("\">").Append(H(s.Label)).Append("</label>");
        sb.Append("<span class=\"dot\" title=\"Set in config.toml\"></span>");
        foreach (var p in s.Platforms ?? new List<string>())
            sb.Append("<span class=\"badge platform\" data-platform=\"").Append(p).Append("\">").Append(PlatformLabels.TryGetValue(p, out var pl) ? pl : p).Append("</span>");
        foreach (var tag in s.Tags)
            sb.Append("<span class=\"badge tag").Append(tag.StartsWith("Restart") ? " warn" : "").Append("\">").Append(H(tag)).Append("</span>");
        if (s.Inferred) sb.Append("<span class=\"badge new\" title=\"In the terminal's source but not described in the overlay yet\">new</span>");
        sb.Append("</div><div class=\"keyline\"><code class=\"key\">").Append(H(s.Key)).Append("</code><span class=\"default\" data-role=\"default\"></span></div>");
        if (s.Doc is { } doc) sb.Append("<p class=\"doc\">").Append(Doc(doc)).Append("</p>");
        if (s.BuiltInsNote is { } builtIns) sb.Append("<p class=\"doc\">Lines here are added to Ghostty's ").Append(H(builtIns)).Append(".</p>");
        if (s.DocMore is { } more) sb.Append("<details class=\"more\"><summary>More</summary>").Append(Markdown(more)).Append("</details>");
        sb.Append("<p class=\"field-error\" id=\"e-").Append(slug).Append("\" data-role=\"error\" hidden></p></div>\n");

        // Fixed-width control column plus its own reset slot, so every row shares one right edge.
        sb.Append("      <div class=\"control\" data-widget=\"").Append(s.Type).Append("\">");
        EmitWidget(sb, s, slug);
        if (s.Pick is { } pick)
            sb.Append("<button type=\"button\" class=\"btn small browse\" data-role=\"browse\" data-pick=\"").Append(H(pick)).Append("\">").Append(pick == "folder" ? "Choose folder…" : "Choose…").Append("</button>");
        sb.Append("</div>");
        sb.Append("<button type=\"button\" class=\"reset\" data-role=\"reset\" title=\"Reset to default (removes the line)\" aria-label=\"Reset ").Append(H(s.Label)).Append(" to default\"><span class=\"i-undo\">").Append(Icon("reset")).Append("</span><span class=\"i-clear\">").Append(Icon("clear")).Append("</span></button>");
        sb.Append("\n    </div>\n");
    }

    static void EmitWidget(StringBuilder sb, SettingDef s, string slug)
    {
        string id = "c-" + slug;
        switch (s.Type)
        {
            case "bool" when s.Nullable && s.Default is null:
                // Unset means "the terminal decides", which a switch can't show: offer On / Off, none selected = unset.
                sb.Append("<div class=\"segmented\" role=\"radiogroup\" id=\"").Append(id).Append("\" aria-labelledby=\"l-").Append(slug).Append("\" data-role=\"value\">");
                sb.Append("<button type=\"button\" role=\"radio\" data-json=\"true\">On</button><button type=\"button\" role=\"radio\" data-json=\"false\">Off</button></div>");
                break;

            case "bool":
                sb.Append("<label class=\"switch\"><input type=\"checkbox\" role=\"switch\" id=\"").Append(id).Append("\" data-role=\"value\"><span class=\"track\"></span></label>");
                break;

            case "enum":
            {
                var values = s.Values ?? new List<string>();
                bool segmented = values.Count <= 5 && values.Sum(v => Label(v).Length) <= 32;
                if (segmented)
                {
                    sb.Append("<div class=\"segmented\" role=\"radiogroup\" id=\"").Append(id).Append("\" aria-labelledby=\"l-").Append(slug).Append("\" data-role=\"value\">");
                    foreach (var v in values)
                        sb.Append("<button type=\"button\" role=\"radio\" data-value=\"").Append(H(v)).Append("\">").Append(H(Label(v))).Append("</button>");
                    sb.Append("</div>");
                }
                else
                {
                    sb.Append("<select id=\"").Append(id).Append("\" data-role=\"value\">");
                    foreach (var v in values)
                        sb.Append("<option value=\"").Append(H(v)).Append("\">").Append(H(Label(v))).Append("</option>");
                    sb.Append("</select>");
                }
                break;
            }

            case "blur":
                sb.Append("<div class=\"segmented\" role=\"radiogroup\" id=\"").Append(id).Append("\" aria-labelledby=\"l-").Append(slug).Append("\" data-role=\"value\">");
                foreach (var (json, label) in new[] { ("false", "Off"), ("true", "Blur"), ("\"macos-glass-regular\"", "Glass"), ("\"macos-glass-clear\"", "Clear glass") })
                    sb.Append("<button type=\"button\" role=\"radio\" data-json=\"").Append(H(json)).Append("\">").Append(label).Append("</button>");
                sb.Append("</div>");
                break;

            case "int":
            case "float":
            {
                string step = s.Step is { } st ? Num(st) : (s.Type == "int" ? "1" : "any");
                string attrs = (s.Min is { } mn ? " min=\"" + Num(mn) + "\"" : "") + (s.Max is { } mx ? " max=\"" + Num(mx) + "\"" : "") + " step=\"" + step + "\"";
                bool range = s.Min is not null && s.Max is not null;
                sb.Append(range ? "<div class=\"range\">" : "<div class=\"number\">");
                if (range) sb.Append("<input type=\"range\" data-role=\"slider\" tabindex=\"-1\" aria-hidden=\"true\"").Append(attrs).Append('>');
                // The unit sits inside the box so boxes with and without units line up.
                sb.Append("<span class=\"num\"><input type=\"number\" id=\"").Append(id).Append("\" data-role=\"value\" inputmode=\"decimal\"").Append(attrs);
                if (s.Nullable) sb.Append(" placeholder=\"auto\"");
                sb.Append('>');
                if (s.Unit is { } unit) sb.Append("<span class=\"unit\">").Append(H(unit)).Append("</span>");
                sb.Append("</span></div>");
                break;
            }

            case "string":
            case "path":
            case "font":
            case "theme":
            {
                sb.Append("<input type=\"text\" class=\"text").Append(s.Type == "path" ? " mono" : "").Append(s.Compact ? " short" : "").Append("\" id=\"").Append(id).Append("\" data-role=\"value\" spellcheck=\"false\" autocomplete=\"off\"");
                if (s.Type == "font") sb.Append(" data-font-picker");
                if (s.Type == "theme") sb.Append(" data-theme-picker");
                var ph = s.Placeholder ?? (s.Nullable ? "Not set" : s.Type == "theme" ? "No theme" : null);
                if (ph is not null) sb.Append(" placeholder=\"").Append(H(ph)).Append('"');
                sb.Append('>');
                break;
            }

            case "color":
                sb.Append("<div class=\"color\"><span class=\"swatch\"><input type=\"color\" data-role=\"picker\" aria-label=\"Pick ").Append(H(s.Label)).Append(" color\"></span>");
            {
                // Some colors also accept keywords (Ghostty: cell-foreground, bright); offer them as suggestions.
                var keywords = s.Values ?? new List<string>();
                sb.Append("<input type=\"text\" class=\"mono\" id=\"").Append(id).Append("\" data-role=\"value\" placeholder=\"")
                  .Append(keywords.Count > 0 ? "#rrggbb, name or keyword" : "#rrggbb")
                  .Append("\" spellcheck=\"false\" autocomplete=\"off\"");
                if (keywords.Count > 0) sb.Append(" list=\"kw-").Append(slug).Append('"');
                sb.Append('>');
                if (keywords.Count > 0)
                {
                    sb.Append("<datalist id=\"kw-").Append(slug).Append("\">");
                    foreach (var k in keywords) sb.Append("<option value=\"").Append(H(k)).Append("\">");
                    sb.Append("</datalist>");
                }
                sb.Append("</div>");
                break;
            }

            case "fonts":
            case "strings":
                sb.Append("<div class=\"chips\" data-role=\"chips\"><span class=\"chip-list\" data-role=\"chip-list\"></span>");
                sb.Append("<input type=\"text\" id=\"").Append(id).Append("\" data-role=\"chip-input\" spellcheck=\"false\" autocomplete=\"off\" placeholder=\"").Append(H(s.Placeholder ?? "Add…")).Append("\"").Append(s.Type == "fonts" ? " data-font-picker" : "").Append("></div>");
                break;

            case "flags":
                sb.Append("<div class=\"flags\" id=\"").Append(id).Append("\" aria-labelledby=\"l-").Append(slug).Append("\" data-role=\"value\">");
                foreach (var v in s.Values ?? new List<string>())
                    sb.Append("<label class=\"flag\"><input type=\"checkbox\" data-flag=\"").Append(H(v)).Append("\"><span>").Append(H(v)).Append("</span></label>");
                sb.Append("</div>");
                break;

            case "margin":
                // Inline side labels keep the row one line tall, like every other row.
                sb.Append("<div class=\"multi sides\" id=\"").Append(id).Append("\" aria-labelledby=\"l-").Append(slug).Append("\" data-role=\"value\">");
                foreach (var (i, label, title) in new[] { (0, "T", "Top"), (1, "R", "Right"), (2, "B", "Bottom"), (3, "L", "Left") })
                    sb.Append("<label class=\"num\" title=\"").Append(title).Append("\"><span class=\"prefix\">").Append(label)
                      .Append("</span><input type=\"number\" min=\"0\" step=\"1\" data-i=\"").Append(i).Append("\" aria-label=\"").Append(title).Append("\"></label>");
                sb.Append("</div>");
                break;

            case "pair":
                sb.Append("<div class=\"multi pair\" id=\"").Append(id).Append("\" aria-labelledby=\"l-").Append(slug).Append("\" data-role=\"value\">");
                foreach (var (i, label) in new[] { (0, "Fast"), (1, "Slow") })
                {
                    sb.Append("<label class=\"num\"><span class=\"prefix\">").Append(label).Append("</span><input type=\"number\" min=\"0\" step=\"10\" data-i=\"")
                      .Append(i).Append("\" aria-label=\"").Append(label).Append("\">");
                    if (s.Unit is { } u) sb.Append("<span class=\"unit\">").Append(H(u)).Append("</span>");
                    sb.Append("</label>");
                }
                sb.Append("</div>");
                break;

            case "fontStyle":
                sb.Append("<div class=\"fontstyle\"><input type=\"text\" class=\"text\" id=\"").Append(id).Append("\" data-role=\"value\" list=\"font-style-list\" spellcheck=\"false\" autocomplete=\"off\" placeholder=\"default\">");
                sb.Append("<label class=\"flag\" title=\"Never use a face for this style\"><input type=\"checkbox\" data-role=\"disabled\"><span>Off</span></label></div>");
                break;

            case "records":
                sb.Append("<div class=\"records\" id=\"").Append(id).Append("\" aria-labelledby=\"l-").Append(slug).Append("\" data-role=\"records\"><div class=\"record-list\" data-role=\"record-list\"></div>");
                sb.Append("<button type=\"button\" class=\"btn small\" data-role=\"add\">Add</button></div>");
                break;

            case "raw":
                sb.Append("<textarea class=\"raw mono\" id=\"").Append(id).Append("\" data-role=\"value\" rows=\"10\" spellcheck=\"false\" placeholder=\"[[hints.rules]]&#10;regex = &quot;…&quot;\"></textarea>");
                break;
        }
    }

    static JObj Metadata(SchemaDoc schema)
    {
        var root = new JObj();
        root.Members.Add(new KeyValuePair<string, object?>("rev", schema.Revision));
        var app = new JObj();
        app.Members.Add(new KeyValuePair<string, object?>("terminal", schema.Terminal));
        app.Members.Add(new KeyValuePair<string, object?>("title", schema.Title));
        app.Members.Add(new KeyValuePair<string, object?>("themeLocksColors", schema.ThemeLocksColors));
        root.Members.Add(new KeyValuePair<string, object?>("app", app));
        var settings = new JObj();
        foreach (var s in schema.Settings)
        {
            var o = new JObj();
            void Add(string k, object? v) => o.Members.Add(new KeyValuePair<string, object?>(k, v));
            Add("t", s.Type);
            Add("l", s.Label);
            Add("g", s.Group);
            Add("d", s.Default);
            if (s.Min is { } min) Add("min", min);
            if (s.Max is { } max) Add("max", max);
            if (s.Values is { } v) Add("v", v.Cast<object?>().ToList());
            if (s.Nullable) Add("n", true);
            if (s.Overridable) Add("o", true);
            if (s.Platforms is { } p) Add("p", p.Cast<object?>().ToList());
            if (s.Fields is { } fields)
            {
                Add("f", fields.Select(f =>
                {
                    var fo = new JObj();
                    fo.Members.Add(new KeyValuePair<string, object?>("k", f.Key));
                    fo.Members.Add(new KeyValuePair<string, object?>("l", f.Label));
                    fo.Members.Add(new KeyValuePair<string, object?>("t", f.Type));
                    if (f.Placeholder is not null) fo.Members.Add(new KeyValuePair<string, object?>("p", f.Placeholder));
                    if (f.List is not null) fo.Members.Add(new KeyValuePair<string, object?>("list", f.List));
                    return (object?)fo;
                }).ToList());
            }
            settings.Members.Add(new KeyValuePair<string, object?>(s.Key, o));
        }
        root.Members.Add(new KeyValuePair<string, object?>("settings", settings));
        return root;
    }

    static string Label(string value) =>
        ValueLabels.TryGetValue(value, out var l) ? l : SchemaCommon.Humanize(value);

    static string Icon(string name) =>
        "<svg class=\"icon\" viewBox=\"0 0 24 24\" aria-hidden=\"true\">" + (Icons.TryGetValue(name, out var p) ? p : "") + "</svg>";

    static string Short(string rev) => rev.Length > 7 ? rev.Substring(0, 7) : rev;

    static string Num(double d) => d.ToString("R", CultureInfo.InvariantCulture);

    internal static string Slug(string key)
    {
        var sb = new StringBuilder();
        foreach (char c in key) sb.Append(char.IsLetterOrDigit(c) ? char.ToLowerInvariant(c) : '-');
        return sb.ToString();
    }

    internal static string H(string? s)
    {
        if (string.IsNullOrEmpty(s)) return "";
        var sb = new StringBuilder(s!.Length);
        foreach (char c in s)
        {
            switch (c)
            {
                case '&': sb.Append("&amp;"); break;
                case '<': sb.Append("&lt;"); break;
                case '>': sb.Append("&gt;"); break;
                case '"': sb.Append("&quot;"); break;
                case '\'': sb.Append("&#39;"); break;
                default: sb.Append(c); break;
            }
        }
        return sb.ToString();
    }

    /// <summary>
    /// Minimal markdown for terminal docs: paragraphs, indented code blocks, "*"/"-" lists and `code` spans.
    /// Everything is HTML-escaped first, so docs can't inject markup.
    /// </summary>
    internal static string Markdown(string text)
    {
        var sb = new StringBuilder();
        var lines = text.Replace("\r\n", "\n").Split('\n');
        int i = 0;
        while (i < lines.Length)
        {
            var line = lines[i];
            if (line.Trim().Length == 0) { i++; continue; }

            if (line.StartsWith("    ", StringComparison.Ordinal))
            {
                var code = new List<string>();
                while (i < lines.Length && (lines[i].StartsWith("    ", StringComparison.Ordinal) || lines[i].Trim().Length == 0))
                    code.Add(lines[i++]);
                while (code.Count > 0 && code[code.Count - 1].Trim().Length == 0) code.RemoveAt(code.Count - 1);
                sb.Append("<pre><code>").Append(H(string.Join("\n", code.Select(c => c.Length >= 4 ? c.Substring(4) : c)))).Append("</code></pre>");
                continue;
            }

            if (IsBullet(line))
            {
                sb.Append("<ul>");
                while (i < lines.Length && lines[i].Trim().Length > 0)
                {
                    if (IsBullet(lines[i]))
                    {
                        var item = new StringBuilder(lines[i].TrimStart().Substring(2));
                        i++;
                        while (i < lines.Length && lines[i].Trim().Length > 0 && !IsBullet(lines[i]))
                            item.Append(' ').Append(lines[i++].Trim());
                        sb.Append("<li>").Append(Doc(item.ToString())).Append("</li>");
                    }
                    else i++;
                }
                sb.Append("</ul>");
                continue;
            }

            var para = new StringBuilder();
            while (i < lines.Length && lines[i].Trim().Length > 0 && !IsBullet(lines[i]) && !lines[i].StartsWith("    ", StringComparison.Ordinal))
                para.Append(para.Length > 0 ? " " : "").Append(lines[i++].Trim());
            sb.Append("<p>").Append(Doc(para.ToString())).Append("</p>");
        }
        return sb.ToString();
    }

    static bool IsBullet(string line)
    {
        var t = line.TrimStart();
        return line.Length - t.Length < 4 && (t.StartsWith("* ", StringComparison.Ordinal) || t.StartsWith("- ", StringComparison.Ordinal));
    }

    /// <summary>Escapes doc text and turns `code` spans into &lt;code&gt;.</summary>
    static string Doc(string text)
    {
        var parts = H(text).Split('`');
        var sb = new StringBuilder();
        for (int i = 0; i < parts.Length; i++)
            sb.Append(i % 2 == 1 ? "<code>" + parts[i] + "</code>" : parts[i]);
        return sb.ToString();
    }
}
