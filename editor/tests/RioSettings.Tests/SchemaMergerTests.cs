using Settings.Generators;

namespace RioSettings.Tests;

public class SchemaMergerTests
{
    const string Source = """
        {
          "rioRevision": "abc1234def",
          "defaults": {
            "cursor": { "shape": "Block", "blinking": false },
            "margin": { "top": 2.0, "right": 2.0, "bottom": 2.0, "left": 2.0 },
            "brand-new-option": 3,
            "platform": { "macos": null }
          },
          "enums": {
            "CursorShape": { "renameAll": null, "variants": [
              { "name": "Block", "aliases": ["block"], "isDefault": true },
              { "name": "Beam", "aliases": ["beam"], "isDefault": false } ] }
          },
          "colorDefaults": { "background": "#0f0d0e", "dim-red": null, "light-red": "#c55555" }
        }
        """;

    const string Overlay = """
        {
          "groups": [ { "id": "general", "title": "General" }, { "id": "cursor", "title": "Cursor" }, { "id": "colors", "title": "Colors" } ],
          "colorSubgroups": [ { "title": "Base", "match": ["background"] }, { "title": "Bright", "prefix": "light-" } ],
          "settings": [
            { "key": "cursor.shape", "group": "cursor", "label": "Shape", "type": "enum", "enum": "CursorShape" },
            { "key": "cursor.blinking", "group": "cursor", "label": "Blinking", "type": "bool" },
            { "key": "margin", "group": "general", "label": "Margin", "type": "margin" },
            { "key": "fonts.size", "group": "general", "label": "Size", "type": "float", "manual": true, "default": 14 }
          ]
        }
        """;

    static (SchemaDoc schema, List<SchemaDiagnostic> diags) Merge(string source = Source, string overlay = Overlay)
    {
        var diags = new List<SchemaDiagnostic>();
        return (RioSchemaMerger.Merge(source, overlay, diags), diags);
    }

    static SettingDef Get(SchemaDoc s, string key) => s.Settings.Single(x => x.Key == key);

    [Test]
    public async Task Enum_values_and_default_use_the_toml_spelling()
    {
        var (schema, _) = Merge();
        var shape = Get(schema, "cursor.shape");
        await Assert.That(shape.Values!).IsEquivalentTo(new[] { "block", "beam" });
        await Assert.That(shape.Default as string).IsEqualTo("block");
    }

    [Test]
    public async Task Margin_struct_default_collapses_to_css_shorthand()
    {
        var (schema, _) = Merge();
        var margin = (List<object?>)Get(schema, "margin").Default!;
        await Assert.That(margin.Count).IsEqualTo(1);
        await Assert.That((double)margin[0]!).IsEqualTo(2.0);
    }

    [Test]
    public async Task Unknown_dump_keys_warn_and_still_render()
    {
        var (schema, diags) = Merge();
        await Assert.That(diags.Any(d => d.Id == "RIO001" && d.Message.Contains("brand-new-option"))).IsTrue();
        var inferred = Get(schema, "brand-new-option");
        await Assert.That(inferred.Inferred).IsTrue();
        await Assert.That(inferred.Type).IsEqualTo("int");
        await Assert.That(schema.Settings.Any(s => s.Key.StartsWith("platform"))).IsFalse();
    }

    [Test]
    public async Task Colors_come_from_source_with_subgroups_and_nullable_when_derived()
    {
        var (schema, _) = Merge();
        var bg = Get(schema, "colors.background");
        await Assert.That(bg.Subgroup).IsEqualTo("Base");
        await Assert.That(bg.Default as string).IsEqualTo("#0f0d0e");
        var lightRed = Get(schema, "colors.light-red");
        await Assert.That(lightRed.Subgroup).IsEqualTo("Bright");
        await Assert.That(lightRed.Label).IsEqualTo("Red");
        await Assert.That(Get(schema, "colors.dim-red").Nullable).IsTrue();
    }

    [Test]
    public async Task Missing_enum_is_an_error_and_removed_setting_is_a_warning()
    {
        var overlay = Overlay.Replace("\"enum\": \"CursorShape\"", "\"enum\": \"Nope\"")
                             .Replace("\"key\": \"cursor.blinking\"", "\"key\": \"cursor.gone\"");
        var (_, diags) = Merge(overlay: overlay);
        await Assert.That(diags.Any(d => d.Id == "RIO002" && d.IsError)).IsTrue();
        await Assert.That(diags.Any(d => d.Id == "RIO003" && !d.IsError && d.Message.Contains("cursor.gone"))).IsTrue();
    }

    [Test]
    public async Task Html_contains_a_control_per_setting_and_escapes_text()
    {
        var overlay = Overlay.Replace("\"label\": \"Blinking\"", "\"label\": \"Blink <fast>\"");
        var (schema, _) = Merge(overlay: overlay);
        var html = HtmlEmitter.Emit(schema);
        foreach (var s in schema.Settings)
            await Assert.That(html).Contains($"data-key=\"{s.Key}\"");
        await Assert.That(html).Contains("Blink &lt;fast&gt;");
        await Assert.That(html).DoesNotContain("<fast>");
    }

    [Test]
    public async Task Real_schema_files_merge_without_errors_or_drift()
    {
        var dir = Path.Combine(AppContext.BaseDirectory, "schema");
        var diags = new List<SchemaDiagnostic>();
        var schema = RioSchemaMerger.Merge(
            File.ReadAllText(Path.Combine(dir, "rio-source.json")),
            File.ReadAllText(Path.Combine(dir, "rio-overlay.json")),
            diags);
        await Assert.That(diags.Select(d => d.Id + ": " + d.Message)).IsEmpty();
        await Assert.That(schema.Settings.Count).IsGreaterThan(100);
    }
}
