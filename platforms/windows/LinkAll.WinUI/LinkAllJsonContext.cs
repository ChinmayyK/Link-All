using System.Collections.Generic;
using System.Text.Json.Serialization;

namespace LinkAll.WinUI
{
    // Source-generated JSON metadata for every type the app reads from the daemon or disk.
    // Reflection-based JsonSerializer calls do not survive trimming or Native AOT; these do.
    // Case-insensitive to match the options the reflection path used.
    [JsonSourceGenerationOptions(PropertyNameCaseInsensitive = true)]
    [JsonSerializable(typeof(List<PeerViewModel>))]
    [JsonSerializable(typeof(List<PeerBatteryState>))]
    [JsonSerializable(typeof(List<PeerStorageState>))]
    [JsonSerializable(typeof(List<FileTransferState>))]
    [JsonSerializable(typeof(List<SpeedTestState>))]
    [JsonSerializable(typeof(List<ActivityEntry>))]
    [JsonSerializable(typeof(List<PendingClipboard>))]
    [JsonSerializable(typeof(ActiveCallState))]
    [JsonSerializable(typeof(RemoteFileListResponse))]
    [JsonSerializable(typeof(Dictionary<string, string>))]
    internal partial class LinkAllJsonContext : JsonSerializerContext
    {
    }
}
