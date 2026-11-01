using System.Text.Json.Nodes;
using Settings.Core;

namespace RioSettings.Tests;

public class FileBrowserTests
{
    static string TempDir()
    {
        var dir = Path.Combine(Path.GetTempPath(), "rio-settings-tests", Guid.NewGuid().ToString("n"));
        Directory.CreateDirectory(dir);
        return dir;
    }

    static string[] Names(JsonObject r) => [.. r["entries"]!.AsArray().Select(e => e!["n"]!.GetValue<string>() + (e["d"] is null ? "" : "/"))];

    [Test]
    public async Task A_file_path_opens_its_folder_with_folders_first_and_only_matching_files()
    {
        var dir = TempDir();
        Directory.CreateDirectory(Path.Combine(dir, "zeta"));
        Directory.CreateDirectory(Path.Combine(dir, "Alpha"));
        foreach (var f in new[] { "b.png", "A.JPG", "notes.txt" }) File.WriteAllText(Path.Combine(dir, f), "");

        var r = FileBrowser.List(Path.Combine(dir, "b.png"), "image", hidden: false, Path.Combine(dir, "config.toml"));

        await Assert.That(r["path"]!.GetValue<string>()).IsEqualTo(dir);
        await Assert.That(Names(r)).IsEquivalentTo(new[] { "Alpha/", "zeta/", "A.JPG", "b.png" }, TUnit.Assertions.Enums.CollectionOrdering.Matching);
        await Assert.That(r["error"]).IsNull();
    }

    [Test]
    public async Task Folder_kind_lists_only_folders()
    {
        var dir = TempDir();
        Directory.CreateDirectory(Path.Combine(dir, "sub"));
        File.WriteAllText(Path.Combine(dir, "x.glsl"), "");

        var r = FileBrowser.List(dir, "folder", hidden: false, Path.Combine(dir, "config.toml"));

        await Assert.That(Names(r)).IsEquivalentTo(new[] { "sub/" });
    }

    [Test]
    public async Task Dot_names_are_hidden_unless_asked_for()
    {
        var dir = TempDir();
        File.WriteAllText(Path.Combine(dir, ".secret.glsl"), "");
        File.WriteAllText(Path.Combine(dir, "crt.glsl"), "");
        var config = Path.Combine(dir, "config.toml");

        await Assert.That(Names(FileBrowser.List(dir, "file", hidden: false, config))).IsEquivalentTo(new[] { "crt.glsl" });
        await Assert.That(Names(FileBrowser.List(dir, "file", hidden: true, config))).Contains(".secret.glsl");
    }

    [Test]
    public async Task A_missing_path_falls_back_to_the_nearest_existing_folder()
    {
        var dir = TempDir();
        var r = FileBrowser.List(Path.Combine(dir, "nope", "deeper", "x.png"), "image", hidden: false, Path.Combine(dir, "config.toml"));
        await Assert.That(r["path"]!.GetValue<string>()).IsEqualTo(dir);
    }

    [Test]
    public async Task Relative_paths_resolve_against_the_config_folder()
    {
        var dir = TempDir();
        Directory.CreateDirectory(Path.Combine(dir, "shaders"));
        var r = FileBrowser.List("shaders", "file", hidden: false, Path.Combine(dir, "config.toml"));
        await Assert.That(r["path"]!.GetValue<string>()).IsEqualTo(Path.Combine(dir, "shaders"));
    }

    [Test]
    public async Task Huge_folders_show_the_first_items_in_sort_order()
    {
        var dir = TempDir();
        for (var i = 5004; i >= 0; i--) File.WriteAllText(Path.Combine(dir, $"f{i:D5}.txt"), "");

        var r = FileBrowser.List(dir, "file", hidden: false, Path.Combine(dir, "config.toml"));
        var names = Names(r);

        await Assert.That(r["truncated"]?.GetValue<bool>() == true).IsTrue();
        await Assert.That(names.Length).IsEqualTo(5000);
        await Assert.That(names[0]).IsEqualTo("f00000.txt");
        await Assert.That(names[^1]).IsEqualTo("f04999.txt");
    }

    [Test]
    public async Task An_unreadable_folder_reports_an_error_instead_of_looking_empty()
    {
        if (OperatingSystem.IsWindows()) return; // permissions are set differently there
        var dir = TempDir();
        var locked = Path.Combine(dir, "locked");
        Directory.CreateDirectory(locked);
        File.WriteAllText(Path.Combine(locked, "a.txt"), "");
        File.SetUnixFileMode(locked, UnixFileMode.None);
        try
        {
            if (Directory.EnumerateFileSystemEntries(locked).Any()) return; // running as root: nothing is unreadable
        }
        catch (UnauthorizedAccessException)
        {
            // Expected: the folder really is unreadable.
        }
        try
        {
            var r = FileBrowser.List(locked, "file", hidden: false, Path.Combine(dir, "config.toml"));
            await Assert.That(r["error"]?.GetValue<string>()).IsEqualTo("This folder can't be read.");
            await Assert.That(r["entries"]!.AsArray().Count).IsEqualTo(0);
        }
        finally
        {
            File.SetUnixFileMode(locked, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
        }
    }
}
