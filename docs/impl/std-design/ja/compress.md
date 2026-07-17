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

C エンジンのエラーコード（`Z_DATA_ERROR` や ZSTD のエラーコード）は、`Error.Invalid`（壊れている、あるいは切り詰められた入力）または `Error.Code`（エンジンのエラーカテゴリを反映）へマッピングする。展開爆弾（Decompression bomb）対策として、出力サイズには上限を設ける（展開サイズの上限パラメータ、あるいはハードキャップによる制限） — これは v1 の安全機構（ノブ）として記録しておく。

## New machinery required

既存の FFI の `link()` 経路（M8 #265-269）に加えて、以下を行う safe な unsafe ラッパーが必要である。出力バッファを確保し、C の関数を `(in_ptr, in_len, out_ptr, out_cap)` の形式で呼び出し、「出力先の容量が足りない場合はバッファを拡大してリトライする」ループを処理し、最終的に所有権を持つ buffer を返す。ビルド時には `-lz` / `-lzstd` をリンクする必要がある（ドライバのリンクステップ）。これは新しい外部依存となるため、ビルド環境に libz / libzstd が存在している必要があることを文書化する。
ライブラリが存在しない環境向けに、このモジュールをオプトイン（フィーチャーゲート化）にすることも検討する。

## Slice breakdown

1. gzip (libz) — compress + decompress + サイズ上限の適用。
2. zstd (libzstd) — 同様の構成。

## Pitfalls

- **P1 (FFI memory safety — the align-self-review Gate 2 core)**: i64 から usize への変換には `as usize` ではなく `try_from` を使用する。バッファサイズの計算には `checked_mul` を使う。`from_raw_parts` を呼び出す前には null ガードを入れる。出力を拡大してリトライするループで、バッファサイズをオーバーフローさせないこと。これらは最もリスクの高い箇所であり — そもそも FFI やメモリアロケーションに関するバグを防ぐためにレビュースキルが存在する、まさにその核心部分である。
- **P2 (decompress bomb)**: ごく小さな入力データが、ギガバイト規模に展開される可能性がある。出力サイズに上限を設け（パラメータ指定またはハードリミット）、超過した場合は `Error.Invalid` を返すようにする。攻撃者が制御可能な入力データから無制限に（青天井に）メモリを確保してはならない。
- **P3 (external lib dependency)**: libz / libzstd はリンク可能でなければならず、ドライバのリンクステップにおいて `-lz` / `-lzstd` が必要になる。存在しない場合はビルドに失敗する。この依存関係を明確に文書化すること。ライブラリが存在しなくてもビルドが通るようにフィーチャーゲート化する（単に compress モジュールが使えなくなるだけにする）ことも検討する。
- **P4 (view → FFI ptr)**: `bytes` の入力はデータポインタのみに切り詰められる（draft §15）。長さは別途渡す必要がある。ビュー自体は C 言語の raw ポインタとなるため、FFI の戻り値の型としては使用できない。出力はビューではなく、所有権を持つ buffer でなければならない。

## Test checklist

- 空 / 小さいデータ / 1MB のランダムデータ / 高圧縮率のデータ に対する gzip / zstd の往復処理 → `decompress(compress(x)) == x`
- 壊れた入力データ → `Error.Invalid`
- 展開爆弾（小さく細工した入力 → 巨大な出力） → 上限に達して `Error.Invalid`（P2）
- level 指定の境界値チェック
- buffer が所有権を持ち、Drop 時に正しく解放されること
- モジュールの使用に import が必須であること
- （テストの実行には libz / libzstd の存在が必要となるため、利用可能かどうかに応じてテストの実行をゲートすること）
