using System.Text;
using System.Text.Json;

Console.InputEncoding = Encoding.UTF8;
Console.OutputEncoding = Encoding.UTF8;

var options = new JsonSerializerOptions { PropertyNameCaseInsensitive = true };
var request = JsonSerializer.Deserialize<Request>(Console.In.ReadToEnd(), options)
    ?? throw new InvalidOperationException("empty request");

var total = Math.Max(1, request.Files.Count);
for (var index = 0; index < request.Files.Count; index++)
{
    var file = request.Files[index];
    var input = Path.GetFullPath(file.Path);
    var outputDir = string.IsNullOrWhiteSpace(request.OutputDir)
        ? Path.GetDirectoryName(input)!
        : Path.GetFullPath(request.OutputDir);
    Directory.CreateDirectory(outputDir);

    var output = Path.Combine(
        outputDir,
        Path.GetFileNameWithoutExtension(input) + "-copy" + Path.GetExtension(input));
    File.Copy(input, output, true);

    Emit(new { type = "log", message = $"copied {file.Name}" });
    Emit(new { type = "done", input, output });
    Emit(new { type = "progress", value = (index + 1) / (double)total });
}

static void Emit(object payload)
{
    Console.Out.WriteLine(JsonSerializer.Serialize(payload));
    Console.Out.Flush();
}

sealed class Request
{
    public string? PluginId { get; set; }
    public string? PluginName { get; set; }
    public string? ActionId { get; set; }
    public string? ActionName { get; set; }
    public string? PluginDir { get; set; }
    public string? HostDir { get; set; }
    public string? Ffmpeg { get; set; }
    public string? OutputDir { get; set; }
    public List<FileItem> Files { get; set; } = new();
}

sealed class FileItem
{
    public string Name { get; set; } = "";
    public string Path { get; set; } = "";
    public string Ext { get; set; } = "";
    public long Size { get; set; }
}
