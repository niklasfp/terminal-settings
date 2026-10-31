using System.Diagnostics;

namespace Settings.Core;

/// <summary>
/// Opens the operating system's own file or folder dialog. A web page can't do this: browsers
/// never reveal real paths, so the server opens the dialog and hands the chosen path back.
/// </summary>
public static class FileChooser
{
    /// <param name="kind">"image", "audio", "file" or "folder".</param>
    /// <returns>The chosen path, or null when cancelled or no dialog is available.</returns>
    public static async Task<string?> ChooseAsync(string kind, string prompt, CancellationToken ct = default)
    {
        prompt = new string(prompt.Where(c => !char.IsControl(c) && c != '"' && c != '\\').ToArray());
        if (prompt.Length == 0) prompt = kind == "folder" ? "Choose a folder" : "Choose a file";

        if (OperatingSystem.IsMacOS()) return await MacAsync(kind, prompt, ct);
        if (OperatingSystem.IsWindows()) return await WindowsAsync(kind, prompt, ct);
        return await LinuxAsync(kind, prompt, ct);
    }

    static Task<string?> MacAsync(string kind, string prompt, CancellationToken ct)
    {
        var what = kind switch
        {
            "folder" => $"choose folder with prompt \"{prompt}\"",
            "image" => $"choose file of type {{\"public.image\"}} with prompt \"{prompt}\"",
            "audio" => $"choose file of type {{\"public.audio\"}} with prompt \"{prompt}\"",
            _ => $"choose file with prompt \"{prompt}\"",
        };
        // Bring the dialog to the front; cancelling raises error -128, which we treat as "no choice".
        var script = $"""
            try
              tell current application
                activate
                set chosen to {what}
              end tell
              return POSIX path of chosen
            on error number -128
              return ""
            end try
            """;
        return RunAsync("/usr/bin/osascript", ["-e", script], ct);
    }

    static Task<string?> WindowsAsync(string kind, string prompt, CancellationToken ct)
    {
        var filter = kind switch
        {
            "image" => "Images|*.png;*.jpg;*.jpeg;*.gif;*.bmp;*.webp|All files|*.*",
            "audio" => "Audio|*.wav;*.mp3;*.ogg;*.flac;*.aiff|All files|*.*",
            _ => "All files|*.*",
        };
        // A background process can't take the foreground on Windows, so a plain dialog opens behind
        // the browser. An invisible, always-on-top owner window brings it to the front.
        const string owner = "$owner = New-Object System.Windows.Forms.Form -Property @{ TopMost = $true; ShowInTaskbar = $false; Opacity = 0 }; $owner.Show(); $owner.Activate(); ";
        // Start somewhere sensible instead of the process's working directory (often System32).
        var start = kind switch { "image" => "MyPictures", "audio" => "MyMusic", _ => "UserProfile" };
        var ps = kind == "folder"
            ? $"Add-Type -AssemblyName System.Windows.Forms; {owner}$d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = '{prompt}'; $d.SelectedPath = [Environment]::GetFolderPath('UserProfile'); if ($d.ShowDialog($owner) -eq 'OK') {{ $d.SelectedPath }}"
            : $"Add-Type -AssemblyName System.Windows.Forms; {owner}$d = New-Object System.Windows.Forms.OpenFileDialog; $d.Title = '{prompt}'; $d.Filter = '{filter}'; $d.InitialDirectory = [Environment]::GetFolderPath('{start}'); if ($d.ShowDialog($owner) -eq 'OK') {{ $d.FileName }}";
        return RunAsync("powershell", ["-NoProfile", "-STA", "-Command", ps], ct);
    }

    static async Task<string?> LinuxAsync(string kind, string prompt, CancellationToken ct)
    {
        var zenity = new List<string> { "--file-selection", "--title=" + prompt };
        if (kind == "folder") zenity.Add("--directory");
        if (kind == "image") zenity.Add("--file-filter=Images | *.png *.jpg *.jpeg *.gif *.bmp *.webp");
        if (kind == "audio") zenity.Add("--file-filter=Audio | *.wav *.mp3 *.ogg *.flac *.oga");
        if (await RunAsync("zenity", zenity, ct) is { } z) return z;
        string[] kdialog = kind == "folder"
            ? ["--getexistingdirectory", ".", "--title", prompt]
            : ["--getopenfilename", ".", "--title", prompt];
        return await RunAsync("kdialog", kdialog, ct);
    }

    static async Task<string?> RunAsync(string file, IEnumerable<string> args, CancellationToken ct)
    {
        try
        {
            var psi = new ProcessStartInfo(file, args) { RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false };
            using var p = Process.Start(psi);
            if (p is null) return null;
            var output = await p.StandardOutput.ReadToEndAsync(ct);
            await p.WaitForExitAsync(ct);
            var path = output.Trim();
            return p.ExitCode == 0 && path.Length > 0 ? path : null;
        }
        catch (Exception ex) when (ex is System.ComponentModel.Win32Exception or InvalidOperationException or OperationCanceledException)
        {
            return null;
        }
    }
}
