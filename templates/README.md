# プラグイン雛形

C# と Rust のネイティブプラグインの雛形です。どちらも入力ファイルを `-copy` 付きの名前でコピーします。変換処理は `Program.cs` または `src/main.rs` を書き換えてください。

## C#

```powershell
cd csharp-plugin
dotnet publish -c Release
$dst = "$env:LOCALAPPDATA\ExtensionChange\plugins\csharp-copy"
New-Item -ItemType Directory -Force $dst
Copy-Item bin\Release\net8.0\win-x64\publish\Plugin.exe $dst
Copy-Item plugin.json $dst
```

`dotnet publish` は自己完結の単一 exe を作ります。利用者に .NET のインストールは要りません。NativeAOT にする場合は `-p:PublishAot=true` を付けます。

## Rust

```powershell
cd rust-plugin
cargo build --release
$dst = "$env:LOCALAPPDATA\ExtensionChange\plugins\rust-copy"
New-Item -ItemType Directory -Force $dst
Copy-Item target\release\plugin.exe $dst
Copy-Item plugin.json $dst
```

## プロトコル

ホストは exe を起動し、stdin に要求 JSON を1つ渡します。プラグインは stdout に1行1イベントの JSON を返します。

要求の主な項目:

| 項目 | 内容 |
| --- | --- |
| `actionId` | 選ばれた操作の id |
| `pluginDir` | プラグインのフォルダ |
| `hostDir` | ホストの exe があるフォルダ |
| `ffmpeg` | 同梱 FFmpeg のパス。見つからなければ null |
| `outputDir` | 出力先。未指定なら null |
| `files` | `name`、`path`、`ext`、`size` を持つ配列 |

イベント:

```json
{"type":"log","message":"..."}
{"type":"progress","value":0.5}
{"type":"done","input":"C:\\...\\a.txt","output":"C:\\...\\a-copy.txt"}
{"type":"error","message":"..."}
```

`outputDir` が null のときは、入力ファイルと同じフォルダへ出力します。`done` のパスは絶対パスにします。イベント以外の行はログとして扱います。

ネイティブプラグインは sandbox なしで動きます。信頼できるコードだけを入れてください。
