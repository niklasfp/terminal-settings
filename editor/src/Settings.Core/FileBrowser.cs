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
            // IgnoreInaccessible stays off: with it, an unreadable folder (e.g. macOS privacy-protected
            // Desktop/Documents) would just look empty instead of reporting the error below.
            var options = new EnumerationOptions
            {
                IgnoreInaccessible = false,
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
                items.Add((info.Name, isDir));
            }
            // Sort before capping so a huge folder shows its first items, not an arbitrary subset.
            items.Sort((a, b) => a.IsDir != b.IsDir ? (a.IsDir ? -1 : 1) : string.Compare(a.Name, b.Name, StringComparison.OrdinalIgnoreCase));
            truncated = items.Count > MaxEntries;
            foreach (var (name, isDir) in items.Take(MaxEntries))
            {
                var e = new JsonObject { ["n"] = name };
                if (isDir) e["d"] = true;
                entries.Add((JsonNode)e);
            }
        }
        catch (Exception ex) when (ex is UnauthorizedAccessException or IOException or System.Security.SecurityException or ArgumentException)
        {
            entries.Clear();
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

    static string Home
    {
        get
        {
            var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
            // Without a profile (e.g. a service account) fall back to the current drive's root.
            return home.Length > 0 ? home : System.IO.Path.GetPathRoot(Environment.CurrentDirectory) ?? "/";
        }
    }

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
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or IOException or System.Security.SecurityException or UnauthorizedAccessException)
        {
            // Not a usable path; fall back to the default below.
        }
        return Home;
    }

    static JsonArray? places;
    static string? placesFor;
    static readonly Lock PlacesLock = new();

    /// <summary>Computed once per config folder; responses get a copy.</summary>
    static JsonNode Places(string configDir)
    {
        lock (PlacesLock)
        {
            if (places is null || placesFor != configDir)
            {
                places = ComputePlaces(configDir);
                placesFor = configDir;
            }
            return places.DeepClone();
        }
    }

    static JsonArray ComputePlaces(string configDir)
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
                if ((drive.DriveType is DriveType.Fixed or DriveType.Removable or DriveType.Network) && drive.IsReady)
                    places.Add((JsonNode)new JsonObject { ["name"] = drive.Name.TrimEnd('\\'), ["path"] = drive.RootDirectory.FullName });
        }
        else
        {
            Add("Computer", "/");
        }
        return places;
    }
}
