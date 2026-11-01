using System.Text.Json.Nodes;

namespace Settings.Core;

/// <summary>
/// Lists one folder for the page's own file picker. Browsers never reveal real paths, so the page
/// browses the disk through the server instead and gets real paths back.
/// </summary>
public static class FileBrowser
{
    const int MaxEntries = 5000;

    static readonly string[] ImageExtensions = [".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp"];
    static readonly string[] AudioExtensions = [".wav", ".mp3", ".ogg", ".oga", ".flac", ".aiff"];

    /// <param name="path">A folder or file to start from; empty for the default for <paramref name="kind"/>.</param>
    /// <param name="kind">"image", "audio", "file" or "folder". Decides which files are listed.</param>
    /// <param name="hidden">Include hidden files and folders (dotfiles on macOS and Linux).</param>
    /// <param name="configPath">The config file being edited; its folder is offered as a place.</param>
    public static JsonObject List(string? path, string kind, bool hidden, string configPath)
    {
        var configDir = System.IO.Path.GetDirectoryName(configPath) ?? Home;
        var dir = Resolve(path, kind, configDir);

        var entries = new JsonArray();
        string? error = null;
        var truncated = false;
        try
        {
            var options = new EnumerationOptions
            {
                IgnoreInaccessible = true,
                RecurseSubdirectories = false,
                AttributesToSkip = hidden ? 0 : FileAttributes.Hidden | FileAttributes.System,
            };
            var items = new List<(string Name, bool IsDir)>();
            foreach (var info in new DirectoryInfo(dir).EnumerateFileSystemInfos("*", options))
            {
                var isDir = info is DirectoryInfo || (info.LinkTarget is not null && Directory.Exists(info.FullName));
                // Dot-names count as hidden everywhere, also on Windows where they have no Hidden attribute.
                if (!hidden && info.Name.StartsWith('.')) continue;
                if (!isDir && !Matches(info.Name, kind)) continue;
                if (items.Count == MaxEntries) { truncated = true; break; }
                items.Add((info.Name, isDir));
            }
            items.Sort((a, b) => a.IsDir != b.IsDir ? (a.IsDir ? -1 : 1) : string.Compare(a.Name, b.Name, StringComparison.OrdinalIgnoreCase));
            foreach (var (name, isDir) in items)
            {
                var e = new JsonObject { ["n"] = name };
                if (isDir) e["d"] = true;
                entries.Add((JsonNode)e);
            }
        }
        catch (Exception ex) when (ex is UnauthorizedAccessException or IOException or System.Security.SecurityException)
        {
            error = "This folder can't be read.";
        }

        var result = new JsonObject
        {
            ["path"] = dir,
            ["parent"] = Directory.GetParent(dir)?.FullName,
            ["sep"] = System.IO.Path.DirectorySeparatorChar.ToString(),
            ["entries"] = entries,
            ["places"] = Places(configDir),
        };
        if (truncated) result["truncated"] = true;
        if (error is not null) result["error"] = error;
        return result;
    }

    static string Home => Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);

    static bool Matches(string name, string kind) => kind switch
    {
        "folder" => false,
        "image" => ImageExtensions.Contains(System.IO.Path.GetExtension(name), StringComparer.OrdinalIgnoreCase),
        "audio" => AudioExtensions.Contains(System.IO.Path.GetExtension(name), StringComparer.OrdinalIgnoreCase),
        _ => true,
    };

    /// <summary>The nearest existing folder for the requested path, or a sensible default.</summary>
    static string Resolve(string? path, string kind, string configDir)
    {
        path = path?.Trim().Trim('"');
        if (string.IsNullOrEmpty(path))
        {
            var start = kind switch
            {
                "image" => Environment.GetFolderPath(Environment.SpecialFolder.MyPictures),
                "audio" => Environment.GetFolderPath(Environment.SpecialFolder.MyMusic),
                _ => configDir,
            };
            return Directory.Exists(start) ? start : Home;
        }
        if (path == "~") path = Home;
        else if (path.StartsWith("~/", StringComparison.Ordinal) || path.StartsWith("~\\", StringComparison.Ordinal))
            path = System.IO.Path.Join(Home, path[2..]);
        if (!System.IO.Path.IsPathFullyQualified(path)) path = System.IO.Path.Join(configDir, path);

        try
        {
            var full = System.IO.Path.GetFullPath(path);
            for (var d = new DirectoryInfo(full); d is not null; d = d.Parent)
                if (d.Exists) return d.FullName;
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or IOException or System.Security.SecurityException)
        {
            // Not a usable path; fall back to the default below.
        }
        return Home;
    }

    static JsonArray Places(string configDir)
    {
        var places = new JsonArray();
        var seen = new HashSet<string>(OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal);
        void Add(string name, string? path)
        {
            if (string.IsNullOrEmpty(path) || !Directory.Exists(path) || !seen.Add(path)) return;
            places.Add((JsonNode)new JsonObject { ["name"] = name, ["path"] = path });
        }

        Add("Config folder", configDir);
        Add("Home", Home);
        Add("Desktop", Environment.GetFolderPath(Environment.SpecialFolder.Desktop));
        Add("Documents", Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments));
        Add("Downloads", System.IO.Path.Join(Home, "Downloads"));
        Add("Pictures", Environment.GetFolderPath(Environment.SpecialFolder.MyPictures));
        Add("Music", Environment.GetFolderPath(Environment.SpecialFolder.MyMusic));
        if (OperatingSystem.IsWindows())
        {
            foreach (var drive in DriveInfo.GetDrives())
                if (drive.DriveType is DriveType.Fixed or DriveType.Removable or DriveType.Network)
                    Add(drive.Name.TrimEnd('\\'), drive.RootDirectory.FullName);
        }
        else
        {
            Add("Computer", "/");
        }
        return places;
    }
}
