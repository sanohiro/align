このディレクトリには、ロードマップの本文には収まりきらない std モジュールの、Opus が実装できる粒度の
設計仕様を置いている。執筆はメインループ (Fable)。各モジュールを実装するときの source of truth である。

# std.compress — implementation design (M11)

> 🌐 [English](../compress.md) · **日本語**

## Overview

gzip、zstd(draft §18.2)。要となるライブラリ戦略は **メモリのラッパーは自前で持ち、数学的なエンジンは
借りる**(draft §15)。チューニング済みの DEFLATE/zstd を自前で書き直すのではなく、`extern "C"
link("z"|"zstd")` 経由で `libz`/`libzstd` をラップする。出力の確保は Align 側(arena/buffer)が担い、C の
エンジンがそこへ書き込む。

## Signatures

```text
compress.gzip_compress(data: bytes, level: i64) -> Result<buffer, Error>    // owned output
compress.gzip_decompress(data: bytes) -> Result<buffer, Error>
compress.zstd_compress(data: bytes, level: i64) -> Result<buffer, Error>
compress.zstd_decompress(data: bytes) -> Result<buffer, Error>
```

## Type & ownership classification

純粋な byte→byte 処理である。入力の `bytes` は借用ビューで、そのデータポインタが FFI を越え、長さは別途
渡される(draft §15 の FFI ルールに従う)。出力は所有権付きの `buffer` で、Align が buffer 機構を通じて
確保し、C のエンジンがそこへ書き込む。新しい Move 型は要らない — 既存の `buffer`(#346)を再利用する。

## Effect classification

**Impure** である。`extern "C"` 呼び出しは非 Pure と推論される(draft §15: extern を呼ぶ関数はすべて
non-Pure)ため、compress を `par_map` の被呼び出し関数にすることはできない。I/O 的な用途では問題にならない。

## Error policy

C エンジンのエラーコード(Z_DATA_ERROR、ZSTD のエラーコード)は、`Error.Invalid`(壊れている/切り詰め
られた入力)または `Error.Code`(エンジンのカテゴリを写す)へマップする。展開爆弾(decompress bomb)対策
として、出力サイズには上限を設ける(展開サイズの上限パラメータ、あるいはハードキャップ) — v1 の安全ノブ
として記録しておく。

## New machinery required

FFI の `link()` 経路(既存。M8 #265-269)に加えて、次を行う safe な unsafe ラッパーが要る。出力バッファを
確保し、C の関数を (in_ptr, in_len, out_ptr, out_cap) で呼び出し、「出力が足りない → 拡大してリトライ」の
ループをさばき、所有権付きの buffer を返す。ビルドでは `-lz`/`-lzstd` をリンクする必要がある(ドライバの
リンクステップ)。これは新しい外部依存なので、libz/libzstd が存在している必要があることを文書化する。
ライブラリが無い環境向けに、このモジュールをオプトイン/フィーチャーゲート化することも検討する。

## Slice breakdown

1. gzip(libz) — compress + decompress + サイズ上限。
2. zstd(libzstd) — 同じ形。

## Pitfalls

- **P1 (FFI memory safety — the align-self-review Gate 2 core)**: i64→usize の変換は `as usize` ではなく
  `try_from` を使う。バッファサイズの計算には `checked_mul` を使う。`from_raw_parts` の前には null ガード
  を入れる。出力を拡大してリトライするループはオーバーフローさせない。最もリスクの高い箇所であり — レビュ
  ースキルがそもそも存在する理由である FFI/確保のバグ、そのものだ。
- **P2 (decompress bomb)**: ごく小さな入力がギガバイト規模に展開されうる。出力に上限を設け(パラメータ
  またはハードリミット)、超えたら `Error.Invalid` にする。攻撃者が制御できる入力から青天井に確保しないこと。
- **P3 (external lib dependency)**: libz/libzstd はリンクされていなければならず、ドライバのリンクステップに
  -lz/-lzstd が必要になる。無ければビルドは失敗する。この依存関係を文書化すること。ライブラリが無くても
  ビルドが通るようフィーチャーゲート化する(compress が単に使えなくなるだけにする)ことも検討する。
- **P4 (view → FFI ptr)**: `bytes` の入力はデータポインタだけに落ちる(draft §15)。長さは別途渡す。ビュー
  は FFI の戻り値型としては使えない(C のポインタ → raw)。出力はビューではなく、所有権付きの buffer で
  なければならない。

## Test checklist

- 空 / 小さい / 1MB のランダムデータ / 高圧縮率のデータ に対する gzip/zstd の往復 →
  `decompress(compress(x)) == x`
- 壊れた入力 → `Error.Invalid`
- 展開爆弾(小さく作った入力 → 巨大な出力)→ 上限で `Error.Invalid`(P2)
- level の境界値
- buffer が所有権付きで、Drop で解放されること
- import が必須であること
- (テストには libz/libzstd の存在が要る — 利用可能かどうかでテストをゲートすること。)
