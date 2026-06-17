# 実装ロードマップ

マイルストーン。原則は `00-overview.md` の通り——**設計は全体を先に、実装は縦に貫く骨格を先に通し、各機能は全段へ差し込む**。各 M は「端から端まで動く(`.align` → 実行 → 出力検証)」ことを完了条件とする。段を縦に貫かないタスク(例: 型システムだけ先に全部)は作らない。

## M0 — 骨格貫通 (walking skeleton)

目的: パイプライン全段がクレート境界含めて繋がること。機能は最小。

```align
fn main() -> i32 {
  x := 1
  return x
}
```

完了条件:
- lexer / parser / sema(整数型のみ) / MIR / LLVM codegen / driver の6クレートが存在し連結。
- `alignc run` で実行ファイルが出て終了コードが返る。
- 端から端までの統合テストが1本緑。

この時点で `i64`/`i32` 整数、`:=`、`fn`、`return`、四則演算だけ。型推論も move 検査も最小。

## M1 — 言語の骨(関数・制御・struct・bool)

- `fn`(通常形 + `= expr` 短縮形)、複数引数、関数呼び出し。
- `if` / 比較演算 / `bool`。
- `mut` と再代入。
- `struct` 定義と値リテラル、フィールドアクセス(まず AoS)。
- プリミティブ型一式(`i8..u64` / `f32` `f64` / `char`)。
- `std.io` の `print` 相当を runtime 直結で1つ(出力検証のため)。

完了条件: フィボナッチ等、制御フロー + struct を使う小プログラムが動く。

## M2 — エラーと存在(Result / Option / ?)

- `Option<T>`(null 無し)、`else` による取り出し。
- `Result<T,E>` と `?` 演算子 → MIR で早期 return + cold path に脱糖。
- `pub fn main(...) -> Result<(), Error>` 形を通す。
- 単一 `Error` 型から開始(`open-questions.md` の error type 設計は M2 内で確定させる)。

完了条件: ファイル読み込み失敗を `?` で伝播する例が動く。

## M3 — メモリモデル(move / value / arena)

- 所有型の move と move 後使用エラー、明示 `clone()`。
- 小 struct の値渡し、大 struct copy の lint。
- `arena {}` ブロック → `align_runtime` の arena allocator 呼び出しと一括解放。
- arena view の escape 検査。

完了条件: `arena {}` 内で確保したデータが正しくブロック終了時に解放され、escape がエラーになる。

## M4 — 配列処理コア(Align の主役)

- `array<T>` / `slice<T>`、`out` 引数。
- `map` / `filter` / `where` / `reduce` / `sum` 等のチェーン。
- MIR での **loop fusion**(`map().where().sum()` を単一ループへ)。
- `.score` のようなフィールド射影。

完了条件: `draft.md` §19 の example(JSON 抜きの配列集計部分)が fusion されたコードで動く。

## M5 — 文字列と JSON

- `str` / `string` / `bytes` / `buffer` / `builder`。
- 文字列リテラル meta と const string pool。
- `template` / `html` / `json` 文字列の脱糖(`write_static`/`write_value`)。
- `json.decode<T>` / `encode<T>`、struct からの field table 生成、zero-copy view、SIMD structural scan。

完了条件: `draft.md` §19 の example が**丸ごと**動く(JSON 読み込み → 集計 → builder 出力)。

## M6 — SIMD / vec / mask

- `vec2/4/8/16<T>`、`mask<T>`、`bitset`。
- 配列式 `a = (b+c)*d - e` の一時配列なし fusion。
- MIR の mask を LLVM vector select へ決定論的に lowering。
- `sum_where` / `dot` / `select`。

完了条件: ベクトル化されたコードが LLVM IR レベルで vector 命令を含むことを確認。

## M7 — 並列

- `par_map`(並列単位 = chunk)、`chunks`。
- `par_map` の副作用検査(M3 の解析を流用)。
- `task_group` / `spawn` / `wait`(I/O 並行)。
- async/await は入れない(`non-goals.md`)。

## M8 — ツールと品質

- 公式 formatter(必須、`draft.md` §16)。
- 標準 lint 一式(loop 内 allocation / 巨大 struct copy / 不要 clone / 不要 heap / 未処理 Result / hot loop 内 branch / string 再 scan / 暗黙 copy)。
- `unsafe` ブロックと `raw.*`。

## 並行して詰める設計課題

`open-questions.md` の各項目を、関連 M に紐付けて決着させる(後回しにしない)。

```text
error type 設計        → M2 で確定
ownership 構文          → M3 で確定
arena API(明示 allocator) → M3 で確定
generics 最小システム    → M4 着手前に確定(配列操作が generic を要求するため)
purity 推論             → M7 で確定(par_map 検査と一体)
SIMD intrinsics の有無   → M6 で確定
reflection / FFI        → v1 範囲外。M8 後に再検討
```

## v1 範囲外(意図的)

`non-goals.md` / `open-questions.md` の通り。GPU バックエンド、分散実行、インクリメンタルコンパイル、セルフホストは v1 の外。ただし MIR をバックエンド非依存に保つことで将来の追加を阻害しない(`00-overview.md`)。
