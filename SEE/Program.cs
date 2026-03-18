using SEE;

await foreach (var re in JournalFetch.GetLogsAsync(new JournalFilter(Limit: 10000)))
{
    Console.WriteLine(re.RawTimestamp);
}
await foreach (var re in JournalFetch.fetchServices())
{
    Console.WriteLine(re);
}
