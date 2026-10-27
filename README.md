# Extension Change

Windows 11 の右クリックメニューからファイルを変換するツールです。画像、動画、音声、フォント、Minecraft の構造物に対応しています。変換の手順は JS プラグインで追加できます。C# と Rust のネイティブプラグインにも対応しています。

## 対応形式

### 画像

- 入力: PNG / JPG / WebP / BMP / GIF / TIFF / ICO / HEIC / HEIF
- 出力: PNG / JPG / WebP / BMP / GIF / TIFF / ICO
- HEIC / HEIF は FFmpeg でデコードします。

### 動画

- 入力: MP4 / MKV / MOV / AVI / WebM / M4V / WMV / FLV / TS / M2TS / HEVC / AV1 / IVF
- 出力: MP4 / MKV / WebM / GIF / JPEG（先頭フレーム）/ MP3 / WAV
- MP4 と MKV はコーデックを変えずにコンテナだけ変換できます。コピーに失敗した場合は H.264 で再エンコードします。
- M2TS と AV1 は MP4 へ変換できます。

### 音声

- 入力: MP3 / WAV / FLAC / OGG / M4A / AAC / OPUS / WMA / AIFF
- 出力: MP3 / WAV / FLAC / OGG / M4A / OPUS

### フォント

- TTF / OTF / WOFF / WOFF2
- TTF と OTF の相互変換は行いません。同じアウトラインのままコンテナだけ変えます。

### Minecraft の構造物

- `.nbt`（バニラのストラクチャーブロック）
- `.litematic`（Litematica）
- `.schem`（WorldEdit / Sponge v3）
- `.schematic`（MCEdit）
- `.bp`（Axiom）

## 必要環境

- Windows 11 22H2 以降
- インストール時の管理者権限
- ビルドする場合: Rust stable (MSVC)、Windows SDK、Go（`schem-convert.exe` を再ビルドするときだけ）

利用者側に FFmpeg、Python、Go のインストールは不要です。FFmpeg と `schem-convert.exe` はパッケージに同梱します。

## インストール

```powershell
powershell -ExecutionPolicy Bypass -File packaging\install.ps1
```

Windows 11 の新しい右クリックメニューは、パッケージ ID を持つ MSIX で登録します。

## 配布用

ビルド済み MSIX と公開証明書を同梱した zip を作る:

```powershell
powershell -ExecutionPolicy Bypass -File packaging\make_release.ps1
```

`dist\ExtensionChange-0.4.0.zip` ができます。中身は MSIX、公開証明書（`.cer`）、`install_prebuilt.ps1` です。受け取った人は zip を展開して `install.bat` をダブルクリックします。UAC の確認後、インストールされます。証明書は zip に含まれるので、受け取る側で証明書を用意する必要はありません。秘密鍵は含まれません。

既存の MSIX と証明書を使う場合は `-Build` を付けません。ビルドし直す場合は `-Build` を付けます。ビルド自体に管理者権限は不要です。インストール時に証明書を信頼するため管理者権限が必要です。

アンインストール:

```powershell
powershell -ExecutionPolicy Bypass -File packaging\uninstall.ps1
```

`-Purge` を付けると、設定、プラグイン、署名証明書も削除します。

## 使い方

エクスプローラーでファイルを右クリックし、「変換」から目的の分類を選びます。

- 音声: 「変換」→「音声に変換」
- フォント: 「変換」→「フォントに変換」
- 画像: 「変換」→「画像に変換」
- 動画: 「変換」→「動画に変換」「音声に変換」「画像に変換」
- 構造物: 「変換」→「構造物に変換」
- 新しい分類: 「変換」→ プラグインが定義した分類（対応する操作がある場合だけ表示）

右クリックから実行した場合は、小さな進捗画面だけが表示されます。完了すると自動で閉じます。

ホスト画面では、対象ファイル、操作、出力先を確認してから実行できます。右クリックから実行した場合は、元のファイルと同じフォルダへ出力します。ホスト画面から実行した場合だけ、出力先の指定を使います。

「プラグイン設定」では、操作の有効・無効、プラグインフォルダを開く、プラグインの再読み込みができます。閉じるときはウィンドウの×を使います。

## MCP サーバー

`ExtensionChange.exe --mcp` で MCP サーバーとして起動する。stdio で JSON-RPC を改行区切りでやり取りし、GUI は開かない。

ツール:

| 名前 | 内容 |
| --- | --- |
| `list_actions` | 利用できる操作の一覧を返す |
| `convert` | `action`、`files`、`output_dir` を指定して変換する |

インストール先の exe:

```powershell
$exe = Join-Path (Get-AppxPackage -Name ExtensionChange).InstallLocation "ExtensionChange.exe"
$exe
```

MCP クライアントの設定例:

```json
{
  "mcpServers": {
    "extension-change": {
      "command": "C:\\Program Files\\WindowsApps\\...\\ExtensionChange.exe",
      "args": ["--mcp"]
    }
  }
}
```

`convert` の引数:

```json
{
  "action": "image-convert/jpg",
  "files": ["C:\\work\\sample.png"],
  "output_dir": "C:\\work\\out"
}
```

`output_dir` を省略するか空にすると、入力ファイルと同じフォルダへ出力する。MCP サーバーはローカル専用で、認証はない。起動できるプロセスは、このアプリの権限でファイルを読み書きできる。

## プラグイン

同梱プラグインはパッケージ内の `plugins` にあります。追加のプラグインは `%LOCALAPPDATA%\ExtensionChange\plugins\<id>\` に置きます。

JS プラグインは `plugin.json` と `index.js`、ネイティブプラグインは `plugin.json` と `entry` の exe を置きます。

`plugin.json` の例:

```json
{
  "id": "image-convert",
  "name": "画像変換",
  "category": "画像に変換",
  "extensions": ["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff", "ico"],
  "actions": [
    { "id": "png", "name": "PNGに変換" },
    { "id": "jpg", "name": "JPGに変換" }
  ],
  "entry": "index.js"
}
```

`category` は右クリックメニュー「変換」の下に出す分類名です。プラグイン全体に付けると全アクションに適用され、アクション側の `category` で上書きできます。`"category": "PDFに変換"` のように新しい名前を書けば、コードを変えずに新しい分類を追加できます。既知の分類は「動画に変換」「音声に変換」「画像に変換」「フォントに変換」「構造物に変換」の順に並び、それ以外は名前順になります。

`runtime` を省略すると `js` になります。値は `js`、`native`、`csharp`、`rust` のいずれかです。`csharp` と `rust` はどちらもネイティブプラグインとして扱い、プラグインの exe をホストが起動します。

`index.js` は関数を1つ export します。

```js
module.exports = function (ctx) {
  for (let i = 0; i < ctx.files.length; i++) {
    const file = ctx.files[i];
    const out = ctx.convert(file, ctx.action, { quality: 92 });
    ctx.log(file.name + " -> " + out);
    ctx.progress((i + 1) / ctx.files.length);
  }
};
```

`ctx` で使えるもの:

| 名前 | 内容 |
| --- | --- |
| `action` | 選ばれた操作の id |
| `files` | 対象ファイルの一覧。`name`、`path`、`ext`、`size` を持つ |
| `outputDir` | 出力先。未指定なら空文字 |
| `log(message)` | ログへ1行追加する |
| `progress(value)` | 進捗を 0 から 1 で指定する |
| `convert(file, format, options)` | 画像を変換し、出力パスを返す |
| `video(file, format, options)` | 動画を変換し、出力パスを返す |
| `audio(file, format, options)` | 音声を変換し、出力パスを返す |
| `font(file, format, options)` | フォントを変換し、出力パスを返す |
| `schematic(file, format, options)` | Minecraft の構造物を変換し、出力パスを返す |
| `name(file, ext)` | 保存先のパスだけを決める |

画像の `format` は `png`、`jpg`、`webp`、`bmp`、`gif`、`tiff`、`ico` です。`options.quality` は JPG の品質です。

動画の `format` は `mp4`、`mp4copy`、`mkv`、`webm`、`gif`、`jpeg`、`png`、`mp3`、`wav`、`flac`、`ogg`、`m4a`、`aac`、`opus` です。

音声の `format` は `mp3`、`wav`、`flac`、`ogg`、`m4a`、`opus` です。

フォントの `format` は `ttf`、`otf`、`woff`、`woff2` です。

構造物の `format` は `litematica`、`schem`、`bp`、`nbt`、`schematic` です。

JS プラグインは QuickJS の sandbox で動きます。ファイルの読み書き、ネットワーク、環境変数、プロセス起動は API に出していません。

### C# / Rust のネイティブプラグイン

`templates/csharp-plugin` と `templates/rust-plugin` に入力ファイルをコピーする雛形があります。exe はプラグインのフォルダに置きます。

C# のビルド:

```powershell
cd templates\csharp-plugin
dotnet publish -c Release
$dst = "$env:LOCALAPPDATA\ExtensionChange\plugins\my-native"
New-Item -ItemType Directory -Force $dst
Copy-Item bin\Release\net8.0\win-x64\publish\Plugin.exe $dst
Copy-Item plugin.json $dst
```

`dotnet publish` は自己完結の単一 exe を作るので、利用者に .NET のインストールは要りません。NativeAOT にする場合は `-p:PublishAot=true` を付けます。

Rust のビルド:

```powershell
cd templates\rust-plugin
cargo build --release
$dst = "$env:LOCALAPPDATA\ExtensionChange\plugins\my-native"
New-Item -ItemType Directory -Force $dst
Copy-Item target\release\plugin.exe $dst
Copy-Item plugin.json $dst
```

ネイティブプラグインの `plugin.json`:

```json
{
  "id": "my-native",
  "name": "My Native",
  "extensions": ["txt"],
  "actions": [
    { "id": "copy", "name": "Copy" }
  ],
  "runtime": "rust",
  "entry": "plugin.exe"
}
```

`entry` はプラグインのフォルダからの相対パスで、exe を指定します。

ホストはプラグインの exe を起動し、stdin に JSON を1つ渡します。プラグインは stdout に1行1イベントの JSON を返します。stderr は失敗時のエラー表示に使います。

要求:

```json
{
  "protocol": 1,
  "pluginId": "my-native",
  "pluginName": "My Native",
  "actionId": "copy",
  "actionName": "Copy",
  "pluginDir": "C:\\...\\plugins\\my-native",
  "hostDir": "C:\\Program Files\\WindowsApps\\...",
  "ffmpeg": "C:\\...\\ffmpeg.exe",
  "outputDir": null,
  "files": [
    { "name": "a.txt", "path": "C:\\...\\a.txt", "ext": "txt", "size": 12 }
  ]
}
```

返すイベント:

```json
{"type":"log","message":"copied a.txt"}
{"type":"progress","value":0.5}
{"type":"done","input":"C:\\...\\a.txt","output":"C:\\...\\a-copy.txt"}
{"type":"error","message":"..."}
```

`done` の `input` と `output` は絶対パスにします。`outputDir` が null のときは、入力ファイルと同じフォルダへ出力します。イベント以外の行はログとして扱います。

ネイティブプラグインはホストと同じ権限で動きます。ファイル、ネットワーク、プロセス起動、Win32 API を自由に使えます。その代わり、JS の sandbox はありません。信頼できるプラグインだけを入れてください。

## 構成

```
crates/core      マニフェスト、設定、プラグイン探索
crates/host      Win32 GUI、QuickJS / ネイティブ実行、画像・動画・音声・フォント変換
crates/shell_ext IExplorerCommand の COM DLL
plugins/         同梱プラグイン
templates/       C# / Rust のプラグイン雛形
tools/schem-convert  Minecraft 構造物の変換ヘルパー（Go）
packaging/       MSIX マニフェスト、インストーラ
```

GUI は `native-windows-gui` で組んだネイティブ Win32 です。WebView と GPU を使わないため、常駐メモリは小さく済みます。

画像と動画と音声は FFmpeg、フォントは Rust の `woff` クレート、Minecraft は同梱の `schem-convert.exe` を使います。

## 制限

- TTF と OTF のアウトライン変換は行いません。
- MCEdit `.schematic` は古いブロック ID にしか対応しません。対応表にないブロックは空気になります。
- Litematica は最初のリージョンだけを読みます。
- `.nbt` にはオフセットがないため、変換するとオフセットは失われます。
- Axiom の `.bp` は、ブロックもブロックエンティティもないファイルだと復元できない場合があります。

## セキュリティ

JS プラグインは QuickJS の sandbox で動き、能力はホストが渡した関数だけに限られます。変換できる入力は右クリックで選ばれたファイルだけで、出力先も GUI で指定した場所か元のフォルダに限ります。1回の実行にメモリ 64MB、時間 30分の上限を掛けています。

C# / Rust のネイティブプラグインは別プロセスで動きますが、sandbox はありません。ホストと同じユーザー権限でファイル、ネットワーク、プロセス起動を使えます。信頼できるプラグインだけを入れてください。ホストは 30分でプラグインを停止します。

同梱の `schem-convert.exe` は固定の引数で呼びます。JS プラグインから任意のコマンドは実行できません。

配布する場合、自己署名証明書の信頼を利用者に求めます。証明書を配りたくない場合は、正規のコード署名証明書を用意してください。

## ライセンス

MIT
