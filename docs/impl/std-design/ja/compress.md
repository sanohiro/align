このディレクトリには、ロードマップの本文を超えた std モジュールの Opus 実装可能な設計仕様が置かれている。
メインループ (Fable) が執筆したもので、各モジュールを実装する際の source of truth である。

# std.compress — implementation design (M11)

> 🌐 [English](../compress.md) · **日本語**

## Overview

gzip、zstd(draft §18.2)。要となるライブラリ戦略: **メモリのラッパーは自前で持ち、数学的なエンジンは
借用する**(draft §15) — 調整済みの DEFLATE/zstd を再実装するのではなく、`extern "C"
link("z"|"zstd")` 経由で `libz`/`libzstd` をラップする。出力の割り当ては Align 側(arena/buffer)が行
い、C のエンジンがそこへ書き込む。

## Signatures

```text
compress.gzip_compress(data: bytes, level: i64) -> Result<buffer, Error>    // owned output
compress.gzip_decompress(data: bytes) -> Result<buffer, Error>
compress.zstd_compress(data: bytes, level: i64) -> Result<buffer, Error>
compress.zstd_decompress(data: bytes) -> Result<buffer, Error>
```

## Type & ownership classification

純粋な byte→byte 処理である。入力の `bytes` は借用されたビューである(そのデータポインタが FFI を越
え、長さは別途渡される。draft §15 の FFI ルールに従う)。出力は所有権付きの `buffer` である(buffer の
機構を通じて Align が割り当て、C のエンジンがそこへ書き込む)。新しい Move 型は不要 — 既存の `buffer`
(#346)を再利用する。

## Effect classification

**Impure** である — `extern "C"` 呼び出しは非 Pure と推論される(draft §15: extern を呼ぶ関数はすべて
non-Pure)ため、compress は決して `par_map` の被呼び出し関数にはなれない。I/O 的な用途では問題ない。

## Error policy

C エンジンのエラーコード(Z_DATA_ERROR、ZSTD のエラーコード)→ `Error.Invalid`(壊れている/切り詰められ
た入力)または `Error.Code`(エンジンのカテゴリを写像する)。展開爆弾(decompress bomb)対策: 出力サイズに
上限を設ける(展開サイズの上限パラメータ、あるいはハードキャップ) — v1 の安全ノブとして記録する。

## New machinery required

FFI の `link()` 経路(既存。M8 #265-269)に加え、次を行う safe な unsafe ラッパー: 出力バッファを割り当
て、C の関数を (in_ptr, in_len, out_ptr, out_cap) で呼び出し、「出力が小さすぎる → 拡大してリトライ」
のループを処理し、所有権付きの buffer を返す。ビルドは `-lz`/`-lzstd` をリンクしなければならない
(ドライバのリンクステップ — 新しい外部依存であり、libz/libzstd が存在している必要があることを文書化す
る。ライブラリが存在しない場合はこのモジュールをオプトイン/フィーチャーゲート化することを検討する)。

## Slice breakdown

1. gzip(libz) — compress + decompress + サイズの上限。
2. zstd(libzstd) — 同様の形。

## Pitfalls

- **P1 (FFI memory safety — the align-self-review Gate 2 core)**: i64→usize の変換は(`as usize` では
  なく)`try_from` を使う、バッファサイジングには `checked_mul` を使う、`from_raw_parts` の前には
  null ガードを入れる、出力を拡大してリトライするループはオーバーフローしてはならない。最もリスクが
  高い — これはまさに、このレビュースキルが存在する理由である FFI/割り当てのバグの典型である。
- **P2 (decompress bomb)**: ごく小さな入力がギガバイト単位に展開されうる。出力に上限を設け
  (パラメータまたはハードリミット)、超過したら `Error.Invalid` にする。攻撃者が制御可能な入力から無制
  限に割り当てないこと。
- **P3 (external lib dependency)**: libz/libzstd はリンクされていなければならない — ドライバのリンク
  ステップに -lz/-lzstd が必要である。存在しない場合はビルドが失敗する — この依存関係を文書化するこ
  と。ライブラリなしでもビルドが成立するようフィーチャーゲート化する(compress が単純に利用不能になる
  だけにする)ことを検討する。
- **P4 (view → FFI ptr)**: `bytes` の入力はそのデータポインタのみに落ちる(draft §15)。長さは別途渡す。
  ビューは有効な FFI の戻り値型ではない(C のポインタ → raw)。出力はビューではなく所有権付きの buffer
  でなければならない。

## Test checklist

- 空 / 小さい / 1MB のランダムデータ / 高度に圧縮可能なデータ に対する gzip/zstd の往復 →
  `decompress(compress(x)) == x`
- 壊れた入力 → `Error.Invalid`
- 展開爆弾(小さく作られた入力→巨大な出力)→ 上限で `Error.Invalid`(P2)
- level の境界値
- buffer が所有権付きであり Drop で解放されること
- import が必須であること
- (テストには libz/libzstd の存在が必要 — 利用可能性でテストをゲートすること。)
