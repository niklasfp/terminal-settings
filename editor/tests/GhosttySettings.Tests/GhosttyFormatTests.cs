using System.Text.Json.Nodes;
using GhosttySettings;
using Settings.Core;
using Settings.Generated;

namespace GhosttySettings.Tests;

public class GhosttyFormatTests
{
    static readonly SettingCatalog Catalog = new(AppSchema.Settings);
    static readonly GhosttyFormat Format = new();

    const string Sample = """
        # Danish @ / [ / ] / | are typed with Option.
        macos-option-as-alt = right

        # SSH helpers
        shell-integration-features = ssh-env,ssh-terminfo

        font-family = JetBrains Mono
        font-family = Symbols Nerd Font
        keybind = super+t=new_tab
        background-opacity = 0.9
        background-opacity = 0.8

        """;

    static string Apply(string text, string key, JsonNode? value) =>
        Format.Apply(text, [new SettingChange(Catalog.Find(key)!, value)], "");

    [Test]
    public async Task Parse_reads_last_value_lists_flags_and_records()
    {
        var v = Format.Parse(Sample, Catalog).Values;
        await Assert.That(v["background-opacity"]!.GetValue<double>()).IsEqualTo(0.8);
        await Assert.That(v["macos-option-as-alt"]!.GetValue<string>()).IsEqualTo("right");
        await Assert.That(v["font-family"]!.AsArray().Select(n => n!.GetValue<string>())).IsEquivalentTo(new[] { "JetBrains Mono", "Symbols Nerd Font" });
        // Flags merge with Ghostty's defaults (cursor, title, path are on by default).
        await Assert.That(v["shell-integration-features"]!.AsArray().Select(n => n!.GetValue<string>()))
            .IsEquivalentTo(new[] { "cursor", "title", "ssh-env", "ssh-terminfo", "path" });
        var kb = v["keybind"]!.AsArray()[0]!;
        await Assert.That(kb["trigger"]!.GetValue<string>()).IsEqualTo("super+t");
        await Assert.That(kb["action"]!.GetValue<string>()).IsEqualTo("new_tab");
    }

    [Test]
    public async Task Scalar_edit_rewrites_the_winning_line_only()
    {
        var after = Apply(Sample, "background-opacity", 0.5);
        await Assert.That(after).Contains("background-opacity = 0.9\nbackground-opacity = 0.5\n");
        await Assert.That(after).Contains("# Danish @ / [ / ] / | are typed with Option.\n");
    }

    [Test]
    public async Task New_key_is_appended_after_a_blank_line()
    {
        var after = Apply("# mine\nfont-size = 13\n", "cursor-style", "bar");
        await Assert.That(after).IsEqualTo("# mine\nfont-size = 13\n\ncursor-style = bar\n");
    }

    [Test]
    public async Task Several_new_keys_form_one_block()
    {
        var after = Format.Apply("font-size = 13\n",
        [
            new SettingChange(Catalog.Find("cursor-style")!, "bar"),
            new SettingChange(Catalog.Find("theme")!, "Nord"),
        ], "");
        await Assert.That(after).IsEqualTo("font-size = 13\n\ncursor-style = bar\ntheme = Nord\n");
    }

    [Test]
    public async Task Reset_removes_every_line_for_the_key()
    {
        var after = Apply(Sample, "background-opacity", null);
        await Assert.That(after).DoesNotContain("background-opacity");
        await Assert.That(after).Contains("keybind = super+t=new_tab\n");
    }

    [Test]
    public async Task Repeatable_list_is_replaced_in_place()
    {
        var after = Apply(Sample, "font-family", new JsonArray("Iosevka", "Symbols Nerd Font"));
        await Assert.That(after).Contains("\nfont-family = Iosevka\nfont-family = Symbols Nerd Font\nkeybind = super+t=new_tab\n");
    }

    [Test]
    public async Task Flags_are_written_out_in_full()
    {
        var after = Apply(Sample, "shell-integration-features", new JsonArray("cursor", "ssh-env"));
        await Assert.That(after).Contains("shell-integration-features = cursor,no-sudo,no-title,ssh-env,no-ssh-terminfo,no-path\n");
    }

    [Test]
    public async Task Records_round_trip_as_trigger_equals_action()
    {
        var after = Apply(Sample, "keybind", JsonNode.Parse("""[{"trigger":"super+t","action":"new_tab"},{"trigger":"super+d","action":"new_split:right"}]"""));
        await Assert.That(after).Contains("keybind = super+t=new_tab\nkeybind = super+d=new_split:right\n");
    }

    [Test]
    public async Task Values_with_leading_spaces_are_quoted_and_read_back_exactly()
    {
        var after = Apply("", "selection-word-chars", " \t'\"");
        await Assert.That(after).IsEqualTo("selection-word-chars = \" \t'\"\"\n");
        await Assert.That(Format.Parse(after, Catalog).Values["selection-word-chars"]!.GetValue<string>()).IsEqualTo(" \t'\"");
    }

    [Test]
    public async Task Empty_value_line_resets_earlier_lines()
    {
        var v = Format.Parse("font-family = A\nfont-family = \nfont-family = B\n", Catalog).Values;
        await Assert.That(v["font-family"]!.AsArray().Select(n => n!.GetValue<string>())).IsEquivalentTo(new[] { "B" });
    }

    [Test]
    [Arguments("background-opacity", "1.5")]
    [Arguments("cursor-style", "\"triangle\"")]
    [Arguments("background", "\"#12\"")]
    [Arguments("shell-integration-features", "[\"bogus\"]")]
    [Arguments("keybind", "[{\"trigger\":\"super+t\"}]")]
    public async Task Rejects_invalid_values(string key, string json)
    {
        await Assert.That(() => Apply("", key, JsonNode.Parse(json))).Throws<ChangeErrors>();
    }

    [Test]
    public async Task Ghostty_itself_validates_the_candidate_file_when_installed()
    {
        if (!File.Exists("/Applications/Ghostty.app/Contents/MacOS/ghostty")) return;
        var app = GhosttyApp.Create();
        var ok = await app.ValidateAsync!("font-size = 13\ncursor-style = bar\n");
        await Assert.That(ok).IsNull();
        var bad = await app.ValidateAsync!("cursor-style = bar\nwindow-padding-x = nope\n");
        await Assert.That(bad!.Keys).Contains("window-padding-x");
    }

    [Test]
    public async Task Every_default_is_accepted_by_its_own_writer()
    {
        foreach (var s in AppSchema.Settings)
        {
            if (s.Default is not { } d) continue;
            await Assert.That(() => GhosttyValues.ToLines(s, d)).ThrowsNothing().Because($"default of {s.Key}");
        }
    }
}

public class GhosttyThemesTests
{
    [Test]
    public async Task Bundled_themes_have_preview_colors_when_ghostty_is_installed()
    {
        const string bin = "/Applications/Ghostty.app/Contents/MacOS/ghostty";
        if (!File.Exists(bin)) return;
        var config = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("n"), "config");
        var themes = GhosttyThemes.List(config, bin);
        await Assert.That(themes.Count).IsGreaterThan(500);
        var dracula = themes.Single(t => t.Name == "Dracula");
        await Assert.That(dracula.Installed).IsTrue();
        await Assert.That(dracula.Background).IsEqualTo("#282a36");
        await Assert.That(dracula.Ansi[1]).IsEqualTo("#ff5555");
    }
}

public class GhosttyWidgetValuesTests
{
    /// <summary>
    /// Every value a widget can produce must be something Ghostty itself accepts: every default,
    /// every choice of every enum, and every single flag, written through our writer and checked
    /// by `ghostty +validate-config`.
    /// </summary>
    [Test]
    public async Task Every_widget_value_is_accepted_by_ghostty()
    {
        const string bin = "/Applications/Ghostty.app/Contents/MacOS/ghostty";
        if (!File.Exists(bin)) return;
        var lines = new List<string>();
        foreach (var s in AppSchema.Settings)
        {
            if (s.Default is { } d)
                lines.AddRange(GhosttyValues.ToLines(s, d).Select(v => $"{s.Key} = {v}"));
            if (s.Type == SettingType.Enum)
                foreach (var v in s.Values ?? [])
                    lines.AddRange(GhosttyValues.ToLines(s, JsonValue.Create(v)!).Select(x => $"{s.Key} = {x}"));
            if (s.Type == SettingType.Flags)
                foreach (var f in s.Values ?? [])
                    lines.AddRange(GhosttyValues.ToLines(s, new JsonArray(f)).Select(x => $"{s.Key} = {x}"));
            if (s.Type == SettingType.Bool)
                foreach (var b in new[] { true, false })
                    lines.AddRange(GhosttyValues.ToLines(s, JsonValue.Create(b)!).Select(x => $"{s.Key} = {x}"));
            if (s.Type == SettingType.Color && s.Values is { } keywords)
                foreach (var k in keywords)
                    lines.AddRange(GhosttyValues.ToLines(s, JsonValue.Create(k)!).Select(x => $"{s.Key} = {x}"));
        }
        var file = Path.Combine(Path.GetTempPath(), $"ghostty-widgets-{Guid.NewGuid():n}.ghostty");
        await File.WriteAllLinesAsync(file, lines);
        try
        {
            var psi = new System.Diagnostics.ProcessStartInfo(bin, ["+validate-config", "--config-file=" + file])
                { RedirectStandardOutput = true, RedirectStandardError = true };
            using var p = System.Diagnostics.Process.Start(psi)!;
            var output = await p.StandardOutput.ReadToEndAsync() + await p.StandardError.ReadToEndAsync();
            await p.WaitForExitAsync();
            await Assert.That(output.Trim()).IsEmpty().Because($"{lines.Count} lines; Ghostty said:\n{output}");
        }
        finally
        {
            File.Delete(file);
        }
    }
}
