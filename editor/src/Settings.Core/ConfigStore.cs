using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;

namespace Settings.Core;

public sealed record ApplyResult(string Before, string After, string Version);

/// <summary>Reads and writes one config file through the app's format. Writes are atomic and go through symlinks to their target.</summary>
public sealed class ConfigStore(AppDefinition app, string path)
{
    readonly SemaphoreSlim _lock = new(1, 1);

    public AppDefinition App { get; } = app;

    public string Path { get; } = path;

    /// <summary>The file actually written: the final symlink target, or the path itself.</summary>
    public string TargetPath
    {
        get
        {
            var info = new FileInfo(Path);
            return info.LinkTarget is not null ? info.ResolveLinkTarget(returnFinalTarget: true)?.FullName ?? Path : Path;
        }
    }

    public string ReadText() => File.Exists(Path) ? File.ReadAllText(Path) : "";

    public static string VersionOf(string text) => Convert.ToHexStringLower(SHA256.HashData(Encoding.UTF8.GetBytes(text)))[..16];

    public JsonObject State()
    {
        var text = ReadText();
        var state = new JsonObject
        {
            ["path"] = Path,
            ["target"] = TargetPath,
            ["exists"] = File.Exists(Path),
            ["version"] = VersionOf(text),
        };
        var values = new JsonObject();
        var raw = new JsonObject();
        var notices = new JsonArray();
        try
        {
            var parsed = App.Format.Parse(text, App.Settings);
            foreach (var (k, v) in parsed.Values) values[k] = v?.DeepClone();
            foreach (var (k, v) in parsed.Raw ?? new Dictionary<string, string>()) raw[k] = v;
            foreach (var n in parsed.Notices ?? []) notices.Add((JsonNode?)JsonValue.Create(n));
        }
        catch (ConfigFormatException ex)
        {
            state["parseError"] = ex.Message;
        }
        state["values"] = values;
        state["raw"] = raw;
        state["notices"] = notices;
        return state;
    }

    /// <summary>Applies UI changes to the current text. Throws on conflicts or invalid values.</summary>
    public async Task<ApplyResult> ApplyAsync(JsonObject request)
    {
        var before = ReadText();
        var expected = request["version"]?.GetValue<string>();
        if (expected is not null && expected != VersionOf(before)) throw new ConflictException();

        var scope = request["scope"]?.GetValue<string>() ?? "";
        if (scope.Length > 0 && !App.Scopes.Contains(scope))
            throw new ChangeErrors(new() { [""] = $"Unknown platform '{scope}'." });

        var errors = new Dictionary<string, string>(StringComparer.Ordinal);
        var changes = new List<SettingChange>();
        foreach (var (key, value) in request["changes"] as JsonObject ?? [])
        {
            var setting = App.Settings.Find(key);
            if (setting is null) { errors[key] = "Unknown setting."; continue; }
            if (scope.Length > 0 && !setting.Overridable) { errors[key] = "This setting can't be overridden per platform."; continue; }
            changes.Add(new SettingChange(setting, value));
        }
        string after;
        try
        {
            after = App.Format.Apply(before, changes, scope);
            after = MatchLineEndings(before, after);
        }
        catch (ChangeErrors ex)
        {
            // Report value errors together with unknown or locked keys.
            foreach (var (k, v) in ex.Errors) errors[k] = v;
            throw new ChangeErrors(errors);
        }
        if (errors.Count > 0) throw new ChangeErrors(errors);

        if (after != before && App.ValidateAsync is { } validate && await validate(after) is { Count: > 0 } invalid)
            throw new ChangeErrors(invalid);
        return new ApplyResult(before, after, VersionOf(before));
    }

    public async Task<(ApplyResult Result, string? Note)> SaveAsync(JsonObject request)
    {
        await _lock.WaitAsync();
        try
        {
            var result = await ApplyAsync(request);
            if (result.After == result.Before) return (result, null);
            WriteAtomically(result.Before, result.After);
            var note = App.AfterSaveAsync is { } after ? await after() : null;
            return (result with { Version = VersionOf(result.After) }, note);
        }
        finally
        {
            _lock.Release();
        }
    }

    /// <summary>Keeps the file's line-ending style: lines a format adds use \n, which would mix with CRLF files on Windows.</summary>
    internal static string MatchLineEndings(string before, string after)
    {
        int crlf = 0, lf = 0;
        for (int i = 0; i < before.Length; i++)
            if (before[i] == '\n') { if (i > 0 && before[i - 1] == '\r') crlf++; else lf++; }
        var unix = after.Replace("\r\n", "\n");
        return crlf > lf ? unix.Replace("\n", "\r\n") : unix;
    }

    void WriteAtomically(string before, string after)
    {
        var target = TargetPath;
        var dir = System.IO.Path.GetDirectoryName(target)!;
        Directory.CreateDirectory(dir);
        if (File.Exists(target)) File.WriteAllText(target + ".bak", before);
        var temp = System.IO.Path.Combine(dir, $".{System.IO.Path.GetFileName(target)}.{Environment.ProcessId}.tmp");
        File.WriteAllText(temp, after);
        File.Move(temp, target, overwrite: true);
    }
}
