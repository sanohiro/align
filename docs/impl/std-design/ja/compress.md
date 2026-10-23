このディレクトリは、ロードマップを補う `std` モジュールの詳細な設計仕様をまとめる。
各モジュールの実装は、この仕様に従う。

# std.compress — implementation design (M11)

> 🌐 [English](../compress.md) · **日本語**

> **ステータス:** M11 で完了済みです。gzip・zlib・raw DEFLATE・zstd の圧縮・展開は実装済みです。

## Overview

gzip・zlib・raw DEFLATE・zstd による圧縮・展開（draft §18.2）。
ランタイムのラッパーが `libz` / `libzstd` を呼び、Align が所有する出力バッファへ
ネイティブのエンジンが書き込む。

## Signatures

```text
compress.gzip_compress(data: bytes, level: i64) -> Result<buffer, Error>    // owned output
compress.gzip_decompress(data: bytes, max_bytes: i64 = 1073741824) -> Result<buffer, Error>
compress.zstd_compress(data: bytes, level: i64) -> Result<buffer, Error>
compress.zstd_decompress(data: bytes, max_bytes: i64 = 1073741824) -> Result<buffer, Error>
compress.zlib_compress(data: bytes, level: i64) -> Result<buffer, Error>
compress.zlib_decompress(data: bytes, max_bytes: i64 = 1073741824) -> Result<buffer, Error>
compress.deflate_raw_compress(data: bytes, level: i64) -> Result<buffer, Error>
compress.deflate_raw_decompress(data: bytes, max_bytes: i64 = 1073741824) -> Result<buffer, Error>
```

4種類の展開関数は省略可能な末尾引数 `max_bytes` を受け取り、既定値は 1073741824。
型は厳密に i64、範囲は `0..=1073741824`。引数をソース順に一度ずつ評価した後、
範囲外なら入力検査・ネイティブ初期化より前に abort する。入力は上限引数の評価中も生存する。
0 は出力が空の完全なストリームを許可し、空の符号化入力は拒否する。超過は `Error.Invalid`。
出力確保要求とペイロード領域への書き込みは指定上限以内とし、上限全体を先行確保しない。
超過判別用の1バイトは別のスタック領域に置く。アロケータの丸め、realloc の旧新領域の重複、
ネイティブ作業領域、buffer ヘッダ、RSS はこの上限に含まない。
[Plan226 の R156](../../226-deflate-framing.md#caller-selected-output-limits-llm-r156) が拡張契約を定める。

## Type & ownership classification

バイト列をバイト列へ変換する処理である。入力の `bytes` は借用ビューであり、そのデータポインタが FFI を越えて渡され、長さは別途渡される（draft §15 の FFI ルールに従う）。出力は所有権を持つ `buffer` であり、Align が buffer 機構を通じてメモリを確保し、C のエンジンがそこへ書き込む。既存の Move 型 `buffer`（#346）を使う。

## Effect classification

**Impure** である。`extern "C"` の呼び出しは非 Pure と推論される（draft §15: extern を呼ぶ関数はすべて non-Pure になる）ため、compress の関数群を `par_map` の対象クロージャ内に記述することはできない。I/O 的な用途で利用する分には問題にならない。

## Error policy

C エンジンのエラーコードは `Error.Invalid`（破損・切り詰め）または `Error.Code`（エンジンのカテゴリ）へマッピングする。ネイティブ資源および出力領域の確保失敗は既存の `Error.Code` を維持する。最終 buffer ヘッダーの OOM は既存ランタイムの終了方針に従う。

## Complete-input decompression

すべてのデコーダーはバイナリ入力を呼び出し中だけ借用し、入力全体の処理に成功した後で独立した所有 buffer を一つ返す。後続部分が失敗した場合、途中までの出力は返さない。

- gzip は一つ以上の完全な RFC 1952 メンバーを受け入れ、空メンバーも含めて出力を順番に連結する。
- zstd は一つ以上の現行 RFC 8878 通常フレームまたはスキップ可能フレームを受け入れる。各フレーム境界のリトルエンディアン magic は `0xFD2FB528` または `0x184D2A50..=0x184D2A5F` に限定し、1.0 より前の形式はネイティブのビルド設定に関係なく拒否する。通常フレームの出力を順番に連結し、スキップ可能フレームは出力に加えない。スキップ可能フレームだけの入力は空 buffer として成功する。
- 空入力、不完全・破損したメンバーやフレーム（後続のチェックサムも含む）、末尾の余分なバイト（gzip のゼロパディングも含む）は `Error.Invalid` となる。
- 展開後の合計ペイロードは全メンバー・フレームを通じて呼び出し側の上限以下（省略時は **1 GiB**）とする。超過は `Error.Invalid`。gzip/zstd は上限ちょうどでも後続の正常な空メンバーやスキップ可能フレームを許可する。この制限は返却ペイロードに対するものであり、ネイティブ作業領域やプロセス RSS の上限ではない。

一つのネイティブストリームと伸長する出力ベクターを呼び出し中に再利用する。上限ではスタック上の 1 バイトを出力先としてメタデータの処理を進め、実データが出力された場合は `Error.Invalid` とする。ネイティブエラーはこの判定より優先する。失敗時はネイティブ状態と部分出力を解放する。圧縮は引き続き一つのメンバー・フレームを生成する。ストリーミング入力 API、辞書、部分展開 API は追加しない。正確な契約と検証範囲は [Plan 124](../../124-complete-decompression.md) に定める。

zlib と raw DEFLATE はそれぞれ window bits 15・-15 で `libz` を利用する。
各デコーダーは完全な1ストリームで入力全体を消費し、後続ストリームや余分な
バイトは `Error.Invalid` にする。raw の最終バイト内のパディングビットは
そのバイトの一部であり、raw にはチェックサムや形式を識別するマジック値はない。
zlib は不正な Adler-32 とプリセット辞書を拒否する。呼び出し側の上限、成功時だけの
独立した所有出力、Impure 効果、既存のエラー対応を共有する。
圧縮は空入力でも1ストリームを生成する。level は `i64` の 0..=9 で、範囲外は
ネイティブ初期化前に abort する。引数はソース順に評価する。
ストリーミング API・辞書 API・PNG 方針は追加しない。
[Plan226](../../226-deflate-framing.md) が正確な契約を定める。

## Runtime and dependencies

ランタイムのラッパーがネイティブの状態、上限を確認した出力領域の拡張、解放を管理し、
成功時に所有 buffer を返す。圧縮を使うプログラムは `-lz` / `-lzstd` をリンクする。
どちらもビルド環境の必須依存ライブラリである。

## Original M11 slices (completed)

1. gzip (libz) — compress + decompress + サイズ上限の適用。
2. zstd (libzstd) — 同様の構成。

## Pitfalls

- **P1 (FFI memory safety — the align-self-review Gate 2 core)**: i64 から usize への変換には `try_from` を使う。バッファサイズの計算には `checked_mul`、`from_raw_parts` の前には null ガードが必要。出力領域の拡張時もサイズのオーバーフローを検査する。
- **P2 (decompress bomb)**: 合計出力には呼び出し側の上限（省略時は 1 GiB）を適用し、超過は `Error.Invalid` にする。
- **P3 (external lib dependency)**: 圧縮を使うプログラムは `-lz` / `-lzstd` をリンクする。ライブラリがなければリンクに失敗する。
- **P4 (view → FFI ptr)**: `bytes` の入力はデータポインタのみに切り詰められる（draft §15）。長さは別途渡す必要がある。ビュー自体は C 言語の raw ポインタとなるため、FFI の戻り値の型としては使用できない。出力はビューではなく、所有権を持つ buffer でなければならない。

## Test checklist

- 空 / 小さいデータ / 1MB のランダムデータ / 高圧縮率のデータ に対する gzip / zlib / raw DEFLATE / zstd の往復処理 → `decompress(compress(x)) == x`
- メンバー・フレームの連結、空メンバー、16 種類すべてのスキップ可能フレーム
- 後続部分の破損・切り詰め、別形式、旧 zstd、末尾の余分なバイト → `Error.Invalid`
- 合計上限と上限ちょうどでの空の後続部分。失敗時は部分 buffer を返さない
- 展開爆弾（小さく細工した入力 → 巨大な出力） → 上限に達して `Error.Invalid`（P2）
- level 指定の境界値チェック
- buffer が所有権を持ち、Drop 時に正しく解放されること
- モジュールの使用に import が必須であること
- テストには libz / libzstd が必要であり、対応するビルド環境に含める。
