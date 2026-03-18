using System.Diagnostics;
using System.Text.Json;

namespace SEE;

public static class JournalFetch
{
    private static readonly ProcessStartInfo Ptemplate = new()
    {
        FileName = "journalctl",
        RedirectStandardOutput = true,
        RedirectStandardError = true,
        UseShellExecute = false,
        CreateNoWindow = true,
    };

    /// <summary>
    /// Fetches logs from journalctl based on provided filters.
    /// </summary>
    public static async IAsyncEnumerable<JournalEntry> GetLogsAsync(
        JournalFilter filter,
        [System.Runtime.CompilerServices.EnumeratorCancellation] CancellationToken ct = default
    )
    {
        var args = BuildArguments(filter);

        var psi = Ptemplate;
        psi.Arguments = args;
        using var process = Process.Start(psi);
        if (process == null)
            yield break;

        using var reader = process.StandardOutput;

        while (!reader.EndOfStream && !ct.IsCancellationRequested)
        {
            var line = await reader.ReadLineAsync(ct);
            if (string.IsNullOrWhiteSpace(line))
                continue;

            JournalEntry? entry = null;
            try
            {
                entry = JsonSerializer.Deserialize<JournalEntry>(line);
            }
            catch (JsonException)
            {
                // Skip lines that aren't valid JSON (rare in -o json mode)
                continue;
            }

            if (entry != null)
                yield return entry;
        }

        if (!process.HasExited)
            process.Kill();
    }

    public static async IAsyncEnumerable<string> fetchServices(
        [System.Runtime.CompilerServices.EnumeratorCancellation] CancellationToken ct = default
    )
    {
        var psi = Ptemplate;
        psi.Arguments = "--field _SYSTEMD_UNIT";
        using var process = Process.Start(psi);
        if (process == null)
            yield break;
        using var reader = process.StandardOutput;
        while (!reader.EndOfStream && !ct.IsCancellationRequested)
        {
            var line = await reader.ReadLineAsync(ct);
            if (string.IsNullOrWhiteSpace(line))
                continue;
            yield return line;
        }
    }

    private static string BuildArguments(JournalFilter filter)
    {
        var args = new List<string> { "-o json" };

        if (!string.IsNullOrEmpty(filter.Unit))
            args.Add($"-u {filter.Unit}");

        if (filter.Since.HasValue)
            args.Add($"--since \"{filter.Since.Value:yyyy-MM-dd HH:mm:ss}\"");

        if (filter.Until.HasValue)
            args.Add($"--until \"{filter.Until.Value:yyyy-MM-dd HH:mm:ss}\"");

        if (filter.Priority.HasValue)
            args.Add($"-p {(int)filter.Priority.Value}");

        if (filter.Follow)
            args.Add("-f");

        if (filter.Limit.HasValue && !filter.Follow)
            args.Add($"-n {filter.Limit.Value}");

        return string.Join(" ", args);
    }
}
