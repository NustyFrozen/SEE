using System.Text.Json.Serialization;

namespace SEE;

public record JournalEntry(
    [property: JsonPropertyName("MESSAGE")] string Message,
    [property: JsonPropertyName("PRIORITY")] string Priority,
    [property: JsonPropertyName("_SYSTEMD_UNIT")] string Unit,
    [property: JsonPropertyName("__REALTIME_TIMESTAMP")] string RawTimestamp,
    [property: JsonPropertyName("_PID")] string? Pid = null,
    [property: JsonPropertyName("_COMM")] string? ProcessName = null,
    [property: JsonPropertyName("_HOSTNAME")] string? Hostname = null,
    [property: JsonPropertyName("_TRANSPORT")] string? Transport = null
)
{
    // Computed property for easy date access
    public DateTime Timestamp =>
        DateTimeOffset.FromUnixTimeMilliseconds(long.Parse(RawTimestamp) / 1000).DateTime;

    // Helper to get a human-readable Level
    public string Level =>
        Priority switch
        {
            "0" => "Emergency",
            "1" => "Alert",
            "2" => "Critical",
            "3" => "Error",
            "4" => "Warning",
            "5" => "Notice",
            "6" => "Info",
            "7" => "Debug",
            _ => "Unknown",
        };
}

// Support Records
public record JournalFilter(
    string? Unit = null,
    DateTime? Since = null,
    DateTime? Until = null,
    LogPriority? Priority = null,
    int? Limit = 100,
    bool Follow = false
);

public enum LogPriority
{
    Emerg = 0,
    Alert = 1,
    Crit = 2,
    Err = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
}
