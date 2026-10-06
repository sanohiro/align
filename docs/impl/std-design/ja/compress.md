このディレクトリには、ロードマップの本文ではカバーしきれない `std` モジュールについて、Opus がそのまま実装に着手できる粒度の設計仕様を収めている。執筆はメインループ（Fable）が担当しており、各モジュールの実装においてこれが信頼できる情報源（source of truth）となる。

# std.compress — implementation design (M11)

> 🌐 [English](../compress.md) · **日本語**

> **ステータス:** M11 で完了済みです。gzip と zstd の圧縮・展開は実装済みです。

## Overview

gzip および zstd による圧縮・展開（draft §18.2）。要となるライブラリ戦略は、**メモリのラッパーは自前で用意し、数学的な計算エンジンは外部から借りる** というものである（draft §15）。チューニング済みの DEFLATE や zstd を自前で書き直すのではなく、`extern "C" link("z"|"zstd")` 経由で `libz` / `libzstd` をラップする。出力バッファの確保は Align 側（arena / buffer）が担い、C のエンジンがそこへ書き込む形をとる。

## Signatures

```text
compress.gzip_compress(data: bytes, level: i64) -> Result<buffer, Error>    // owned output
compress.gzip_decompress(data: bytes) -> Result<buffer, Error>
compress.zstd_compress(data: bytes, level: i64) -> Result<buffer, Error>
compress.zstd_decompress(data: bytes) -> Result<buffer, Error>
```

## Type & ownership classification

純粋な byte → byte 処理である。入力の `bytes` は借用ビューであり、そのデータポインタが FFI を越えて渡され、長さは別途渡される（draft §15 の FFI ルールに従う）。出力は所有権を持つ `buffer` であり、Align が buffer 機構を通じてメモリを確保し、C のエンジンがそこへ書き込む。新しい Move 型を追加する必要はない — 既存の `buffer`（#346）を再利用する。

## Effect classification

**Impure** である。`extern "C"` の呼び出しは非 Pure と推論される（draft §15: extern を呼ぶ関数はすべて non-Pure になる）ため、compress の関数群を `par_map` の対象クロージャ内に記述することはできない。I/O 的な用途で利用する分には問題にならない。

## Error policy

C エンジンのエラーコードは `Error.Invalid`（破損・切り詰め）または `Error.Code`（エンジンのカテゴリ）へマッピングする。ネイティブ資源および出力領域の確保失敗は既存の `Error.Code` を維持する。最終 buffer ヘッダーの OOM は既存ランタイムの終了方針に従う。

## Complete-input decompression

両デコーダーはバイナリ入力を呼び出し中だけ借用し、入力全体の処理に成功した後で独立した所有 buffer を一つ返す。後続部分が失敗した場合、途中までの出力は返さない。

- gzip は一つ以上の完全な RFC 1952 メンバーを受け入れ、空メンバーも含めて出力を順番に連結する。
- zstd は一つ以上の現行 RFC 8878 通常フレームまたはスキップ可能フレームを受け入れる。各フレーム境界のリトルエンディアン magic は `0xFD2FB528` または `0x184D2A50..=0x184D2A5F` に限定し、1.0 より前の形式はネイティブのビルド設定に関係なく拒否する。通常フレームの出力を順番に連結し、スキップ可能フレームは出力に加えない。スキップ可能フレームだけの入力は空 buffer として成功する。
- 空入力、不完全・破損したメンバーやフレーム（後続のチェックサムも含む）、別形式、末尾の余分なバイト（gzip のゼロパディングも含む）は `Error.Invalid` となる。
- 展開後の合計ペイロードは全メンバー・フレームを通じて **1 GiB 以下** とする。超過は `Error.Invalid`。上限ちょうどでも後続の正常な空メンバーやスキップ可能フレームは許可する。この制限は返却ペイロードに対するものであり、ネイティブ作業領域やプロセス RSS の上限ではない。

一つのネイティブストリームと伸長する出力ベクターを呼び出し中に再利用する。上限ではスタック上の 1 バイトを出力先としてメタデータの処理を進め、実データが出力された場合は `Error.Invalid` とする。ネイティブエラーはこの判定より優先する。失敗時はネイティブ状態と部分出力を解放する。圧縮は引き続き一つのメンバー・フレームを生成する。ストリーミング入力 API、辞書、設定可能な上限、部分展開 API は追加しない。正確な契約と検証範囲は [Plan 124](../../124-complete-decompression.md) に定める。

## New machinery required

既存の FFI の `link()` 経路（M8 #265-269）に加えて、以下を行う safe な unsafe ラッパーが必要である。出力バッファを確保し、C の関数を `(in_ptr, in_len, out_ptr, out_cap)` の形式で呼び出し、「出力先の容量が足りない場合はバッファを拡大してリトライする」ループを処理し、最終的に所有権を持つ buffer を返す。ビルド時には `-lz` / `-lzstd` をリンクする必要がある（ドライバのリンクステップ）。これは新しい外部依存となるため、ビルド環境に libz / libzstd が存在している必要があることを文書化する。
ライブラリが存在しない環境向けに、このモジュールをオプトイン（フィーチャーゲート化）にすることも検討する。

## Slice breakdown

1. gzip (libz) — compress + decompress + サイズ上限の適用。
2. zstd (libzstd) — 同様の構成。

## Pitfalls

- **P1 (FFI memory safety — the align-self-review Gate 2 core)**: i64 から usize への変換には `as usize` ではなく `try_from` を使用する。バッファサイズの計算には `checked_mul` を使う。`from_raw_parts` を呼び出す前には null ガードを入れる。出力を拡大してリトライするループで、バッファサイズをオーバーフローさせないこと。これらは最もリスクの高い箇所であり — そもそも FFI やメモリアロケーションに関するバグを防ぐためにレビュースキルが存在する、まさにその核心部分である。
- **P2 (decompress bomb)**: ごく小さな入力データが、ギガバイト規模に展開される可能性がある。合計出力ペイロードを 1 GiB 以下に制限し、超過した場合は `Error.Invalid` を返すようにする。攻撃者が制御可能な入力データから無制限に（青天井に）メモリを確保してはならない。
- **P3 (external lib dependency)**: libz / libzstd はリンク可能でなければならず、ドライバのリンクステップにおいて `-lz` / `-lzstd` が必要になる。存在しない場合はビルドに失敗する。この依存関係を明確に文書化すること。ライブラリが存在しなくてもビルドが通るようにフィーチャーゲート化する（単に compress モジュールが使えなくなるだけにする）ことも検討する。
- **P4 (view → FFI ptr)**: `bytes` の入力はデータポインタのみに切り詰められる（draft §15）。長さは別途渡す必要がある。ビュー自体は C 言語の raw ポインタとなるため、FFI の戻り値の型としては使用できない。出力はビューではなく、所有権を持つ buffer でなければならない。

## Test checklist

- 空 / 小さいデータ / 1MB のランダムデータ / 高圧縮率のデータ に対する gzip / zstd の往復処理 → `decompress(compress(x)) == x`
- メンバー・フレームの連結、空メンバー、16 種類すべてのスキップ可能フレーム
- 後続部分の破損・切り詰め、別形式、旧 zstd、末尾の余分なバイト → `Error.Invalid`
- 合計上限と上限ちょうどでの空の後続部分。失敗時は部分 buffer を返さない
- 展開爆弾（小さく細工した入力 → 巨大な出力） → 上限に達して `Error.Invalid`（P2）
- level 指定の境界値チェック
- buffer が所有権を持ち、Drop 時に正しく解放されること
- モジュールの使用に import が必須であること
- （テストの実行には libz / libzstd の存在が必要となるため、利用可能かどうかに応じてテストの実行をゲートすること）
