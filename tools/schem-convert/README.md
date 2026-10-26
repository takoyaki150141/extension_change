# schem-convert

Minecraft のスケマティック形式を変換する単体 CLI です。Go でビルドしますが、配布する `schem-convert.exe` の実行に Go は不要です。

## 使い方

```
schem-convert.exe -in <input-path> -out <output-path> -format <litematica|schem|bp|schematic|nbt>
```

- 入力パスと出力パスには、絶対パス、空白、UTF-8 を使えます。
- 出力は `-out` に指定したパスへそのまま書き込みます。標準の拡張子は下の表のとおりです。
- 成功すると終了コード 0 を返します。失敗すると stderr に理由を書き、0 以外で終了します。

## 出力形式

| `-format` | ライブラリ ID | 標準の拡張子 | 内容 |
|---|---|---|---|
| `litematica` | `litematica_v7` | `.litematic` | Litematica v7 |
| `schem` | `sponge_v3` | `.schem` | Sponge Schematic v3 |
| `bp` | `axiom` | `.bp` | Axiom blueprint |
| `schematic` | `mcedit` | `.schematic` | MCEdit legacy schematic |
| `nbt` | 独自実装 | `.nbt` | Java ストラクチャーブロック |

`nbt` はライブラリが扱わないため、リーダーとライターを独自に実装しています。

## 入力の判定

1. 先頭 4 バイトが Axiom のマジック `0x0AE5BB36` なら Axiom。
2. gzip、zlib、無圧縮の NBT を読み、ルートに `size`、`palette`、`blocks` があればバニラ構造物。
3. ルートの `Schematic.Version` が 3 なら Sponge v3。
4. それ以外は `format.Detect` と `format.Read` に任せます。

バニラ構造物の入力は gzip と無圧縮を受け付けます。zlib も試します。

## ライブラリの癖への対応

`github.com/oriumgames/schem/format` v0.2.3 の出力には、仕様と合わない箇所があります。このツールは次の箇所を補正します。

- Sponge v3 の `Offset`、`Blocks.Data`、`Biomes.Data` は仕様上は配列です。ライブラリはリストで書き、リーダーもリストを前提にします。出力時に配列へ直し、読み込み時にライブラリ向けの形へ戻します。
- Sponge v3 のブロックエンティティとエンティティは、仕様では `Pos`、`Id`、`Data` を持ちます。ライブラリは追加データをトップレベルへ置きます。出力時に `Data` の下へまとめ、読み込み時に展開します。ブロックエンティティの `Pos` は `TAG_IntArray` で書きます。
- Axiom のライターはブロックエンティティの座標を `int` で書き、NBT エンコーダーが拒否します。また `BlockStates.Data` を `TAG_List` で書きます。ブロックエンティティをいったん隠して書き、あとから `TAG_IntArray` と `TAG_LongArray` で追加します。
- Axiom のリーダーはエンティティの `Pos`、`Rotation`、`Motion` を落とします。読み込み後に補います。
- バニラ構造物、Litematica、Axiom のパレットはプロパティ値を文字列で持ちます。Sponge や MCEdit のリーダーが返す bool や int32 を、書き込み時に文字列へ直します。

## 制限

- MCEdit（`.schematic`）は 1.12 までのブロック ID しか扱えません。ライブラリの対応表に完全一致する状態だけを書き、それ以外のブロックは空気になります。
- Litematica v7 のライターは、スケマティックのオフセットが 0 でないとき、タイルエンティティとエンティティの座標にオフセットを足しません。オフセットが 0 のファイルでは問題ありません。
- Litematica のリーダーは最初のリージョンだけを読みます。
- バニラ NBT にはオフセットを保存する場所がありません。変換するとオフセットは失われます。出力は gzip 圧縮です。
- Axiom のリーダーは、ブロックもブロックエンティティもないファイルからエンティティを復元できません。ライブラリの判定が原因です。
- 変換先の形式が持てない情報は失われます。たとえば MCEdit のブロック ID 表にないブロック、バニラ NBT のオフセットなどです。

## ビルドとテスト

`tools/schem-convert` で実行します。

```
go mod tidy
go build -trimpath -ldflags "-s -w" -o schem-convert.exe .
go test ./...
```

テストは 3x3x3 の構造物（石、チェストのブロックエンティティ、階段、防具立て）を作り、全形式への書き込みと読み戻し、形式間の変換、`.bp` のマジック、空白と日本語を含むパスでの実行ファイル起動を確認します。
