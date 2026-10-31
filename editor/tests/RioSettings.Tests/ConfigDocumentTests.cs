using RioSettings.Toml;

namespace RioSettings.Tests;

public class ConfigDocumentTests
{
    const string Sample = """
        # See the full configuration reference: https://rioterm.com/docs/config

        theme = "dracula"   # my theme

        [editor]
        program = "/opt/homebrew/bin/zed"
        args = []

        # fonts below
        [fonts]
        family = "FiraCode Nerd Font Mono"

        [[hints.rules]]
        regex = "https?://\\S+"
        hyperlinks = true

        [hints.rules.action]
        command = "open"

        [window]
        opacity = 0.9 # translucent
        """;

    static string[] P(string dotted) => dotted.Split('.');

    [Test]
    public async Task Parse_then_no_edits_is_byte_identical()
    {
        var doc = ConfigDocument.Parse(Sample);
        await Assert.That(doc.Text).IsEqualTo(Sample);
    }

    [Test]
    public async Task Replacing_a_value_keeps_comments_and_spacing()
    {
        var doc = ConfigDocument.Parse(Sample);
        doc.Set(P("window.opacity"), "0.75");
        await Assert.That(doc.Text).IsEqualTo(Sample.Replace("opacity = 0.9 # translucent", "opacity = 0.75 # translucent"));
    }

    [Test]
    public async Task Replacing_root_value_keeps_trailing_comment()
    {
        var doc = ConfigDocument.Parse(Sample);
        doc.Set(P("theme"), "\"nord\"");
        await Assert.That(doc.Text).Contains("theme = \"nord\"   # my theme");
    }

    [Test]
    public async Task New_key_goes_into_existing_table_after_its_last_key()
    {
        var doc = ConfigDocument.Parse(Sample);
        doc.Set(P("fonts.size"), "16.0");
        await Assert.That(doc.Text).Contains("[fonts]\nfamily = \"FiraCode Nerd Font Mono\"\nsize = 16.0\n");
    }

    [Test]
    public async Task Nested_key_uses_dotted_key_in_nearest_parent_table()
    {
        var doc = ConfigDocument.Parse(Sample);
        doc.Set(P("fonts.regular.family"), "\"Iosevka\"");
        await Assert.That(doc.Text).Contains("regular.family = \"Iosevka\"");
        await Assert.That(doc.Values()["fonts.regular.family"]!.GetValue<string>()).IsEqualTo("Iosevka");
    }

    [Test]
    public async Task New_table_is_appended_when_missing()
    {
        var doc = ConfigDocument.Parse(Sample);
        doc.Set(P("cursor.shape"), "\"beam\"");
        await Assert.That(doc.Text).EndsWith("opacity = 0.9 # translucent\n\n[cursor]\nshape = \"beam\"\n");
    }

    [Test]
    public async Task New_root_key_goes_after_existing_root_keys()
    {
        var doc = ConfigDocument.Parse(Sample);
        doc.Set(P("copy-on-select"), "true");
        await Assert.That(doc.Text).Contains("theme = \"dracula\"   # my theme\ncopy-on-select = true\n");
    }

    [Test]
    public async Task New_root_key_without_root_keys_goes_before_first_table()
    {
        var doc = ConfigDocument.Parse("# header\n\n[window]\nopacity = 1.0\n");
        doc.Set(P("line-height"), "1.2");
        await Assert.That(doc.Text).IsEqualTo("# header\n\nline-height = 1.2\n\n[window]\nopacity = 1.0\n");
    }

    [Test]
    public async Task Remove_deletes_the_whole_line_including_comment()
    {
        var doc = ConfigDocument.Parse(Sample);
        await Assert.That(doc.Remove(P("window.opacity"))).IsTrue();
        await Assert.That(doc.Text).DoesNotContain("opacity");
        await Assert.That(doc.Text).EndsWith("[window]\n");
        await Assert.That(doc.Remove(P("window.opacity"))).IsFalse();
    }

    [Test]
    public async Task Values_flattens_tables_and_arrays_of_tables()
    {
        var values = ConfigDocument.Parse(Sample).Values();
        await Assert.That(values["editor.program"]!.GetValue<string>()).IsEqualTo("/opt/homebrew/bin/zed");
        await Assert.That(values["window.opacity"]!.GetValue<double>()).IsEqualTo(0.9);
        await Assert.That(values["hints.rules[0].hyperlinks"]!.GetValue<bool>()).IsTrue();
    }

    [Test]
    public async Task Inline_table_values_are_flattened_but_not_edited_in_place()
    {
        var doc = ConfigDocument.Parse("[fonts]\nregular = { family = \"Iosevka\", style = \"Light\" }\n");
        await Assert.That(doc.Values()["fonts.regular.style"]!.GetValue<string>()).IsEqualTo("Light");
        await Assert.That(() => doc.Set(P("fonts.regular.style"), "\"Bold\"")).Throws<TomlEditException>();
    }

    [Test]
    public async Task Table_array_round_trips_and_can_be_replaced()
    {
        var doc = ConfigDocument.Parse(Sample);
        var raw = doc.GetTableArray(P("hints.rules"));
        await Assert.That(raw).StartsWith("[[hints.rules]]");
        await Assert.That(raw).Contains("[hints.rules.action]");
        await Assert.That(raw).DoesNotContain("[window]");

        doc.SetTableArray(P("hints.rules"), "[[hints.rules]]\nregex = \"x\"\n");
        await Assert.That(doc.Text).Contains("[[hints.rules]]\nregex = \"x\"\n\n[window]");
        await Assert.That(doc.Text).DoesNotContain("hyperlinks");

        doc.SetTableArray(P("hints.rules"), "");
        await Assert.That(doc.Text).DoesNotContain("hints.rules");
    }

    [Test]
    public async Task Table_array_rejects_foreign_tables()
    {
        var doc = ConfigDocument.Parse(Sample);
        await Assert.That(() => doc.SetTableArray(P("hints.rules"), "[window]\nopacity = 1.0\n")).Throws<TomlEditException>();
    }

    [Test]
    public async Task Edits_producing_invalid_toml_are_rejected_and_text_is_unchanged()
    {
        var doc = ConfigDocument.Parse(Sample);
        await Assert.That(() => doc.Set(P("window.opacity"), "\"unterminated")).Throws<TomlEditException>();
        await Assert.That(doc.Text).IsEqualTo(Sample);
    }

    [Test]
    public async Task Invalid_file_reports_parse_error()
    {
        await Assert.That(() => ConfigDocument.Parse("[window\nopacity = ")).Throws<TomlEditException>();
    }

    [Test]
    public async Task Empty_file_gets_table_without_leading_blank_line()
    {
        var doc = ConfigDocument.Parse("");
        doc.Set(P("window.opacity"), "0.5");
        await Assert.That(doc.Text).IsEqualTo("[window]\nopacity = 0.5\n");
    }

    [Test]
    public async Task File_without_trailing_newline_still_appends_cleanly()
    {
        var doc = ConfigDocument.Parse("[window]\nopacity = 1.0");
        doc.Set(P("window.mode"), "\"Maximized\"");
        await Assert.That(doc.Text).IsEqualTo("[window]\nopacity = 1.0\nmode = \"Maximized\"\n");
    }

    [Test]
    public async Task Platform_override_goes_into_nested_table()
    {
        var doc = ConfigDocument.Parse(Sample);
        doc.Set(["platform", "macos", "window", "opacity"], "0.8");
        await Assert.That(doc.Text).EndsWith("[platform.macos.window]\nopacity = 0.8\n");
        await Assert.That(doc.Values()["platform.macos.window.opacity"]!.GetValue<double>()).IsEqualTo(0.8);
    }
}
