# コンパイラパイプライン

`source.align` から実行ファイルまでの段と、段の間を流れる IR の境界を定義する。**IR 境界 = クレート境界**(`00-overview.md`)。各段は前段の出力だけに依存し、後段を知らない。

## 全体図

```text
source (.align)
  │  align_lexer
  ▼
Tokens                      位置付きトークン列
  │  align_parser
  ▼
AST                         構文木。意味解析前。span 付き
  │  align_sema (1) 名前解決 / モジュール解決
  ▼
Resolved AST                参照が定義に結びついた状態
  │  align_sema (2) 型推論 / 型検査
  ▼
Typed HIR                   全式に型が付いた高レベルIR
  │  align_sema (3) move 検査 / arena escape 検査
  ▼
Checked HIR                 安全性検証済み。ここで通れば安全
  │  align_mir  lowering(脱糖) + 解析
  ▼
MIR                         バックエンド非依存の核。SIMD/fusion/arena 確定
  │  align_mir  最適化パス
  ▼
MIR (optimized)
  │  align_codegen_llvm
  ▼
LLVM IR → object
  │  align_driver  リンク(+ align_runtime)
  ▼
実行ファイル
```

## 各段の責務

### Lexer (`align_lexer`)
- 入力バイト列 → トークン列。
- 文字列リテラルの **コンパイル時 meta** (`draft.md` §12: len / hash / ascii / utf8_valid / escape 要否) はここで一次計算し、トークンに添える。
- ブロックは `{}`、インデントは意味を持たない(非 Python)。文の終端は **Go スタイル**: 改行が暗黙の終端、`;` は1行に詰めるときの任意セパレータ。lexer が「行末トークンが文を終え得るなら改行で暗黙 `;` を挿入、ただし行頭が `.`/二項演算子なら前行の継続」を判定する。

### Parser (`align_parser`)
- トークン列 → AST。エラー回復付き(1ファイル内で複数エラーを報告)。
- `:=` / `mut` / `fn ... = expr` 短縮形 / struct リテラル / `?` / `else` / `arena {}` / `template`・`html`・`json` 文字列 などの構文をここで吸収。
- **脱糖はしない**。`?` や `template` の展開は MIR 段。AST は書かれたまま保つ(formatter/lint が AST を使うため)。

### Sema (1) 名前解決 (`align_sema`)
- `module` / `import` の解決、シンボルテーブル構築、参照→定義の結合。
- 可視性(`pub`)の検査。

### Sema (2) 型推論・型検査 (`align_sema`)
- 局所型推論(`x := 10` の型決定)と注釈との突合。
- `Option<T>` / `Result<T,E>` / `array<T>` / `slice<T>` / `vec<N,T>` / `mask<T>` の型付け。
- `?` 演算子が `Result` にのみ適用されることの検査。
- 配列操作チェーン(`map`/`where`/`sum` ...)の型付け。詳細 `03-types.md`。

### Sema (3) move 検査 / arena escape 検査 (`align_sema`)
- 所有型の move 後使用を**コンパイルエラー**にする(`draft.md` §6.3)。
- `arena {}` 内で確保した view が arena 外へ漏れることを検査(§6.4, §15)。
- `par_map` に渡す関数が外部 mutable state を変更しないことを検査(§11)。
- `out` 引数の no-alias 制約検査(§7)。
- **lifetime 注釈は要求しない**。フロー解析で寿命違反を検出する(`03-types.md`)。

### MIR 生成 (`align_mir`)
- ここで初めて**脱糖**する。詳細 `04-mir.md`。
  - `?` → 早期 return + cold error path 分岐。
  - `template` / `html` / `json` 文字列 → `write_static` / `write_value` 列(§13)。
  - 配列式 `a = (b+c)*d` → 一時配列を作らない fused loop(§9)。
  - `map`/`where`/`sum` チェーン → 単一ループへ fusion。
  - `arena {}` → arena allocator の確保/一括解放呼び出し。
  - struct → SoA/AoS レイアウト決定、field table 生成(§14)。
- allocation / error path / 並列単位(chunk)を MIR 上で**明示ノード**として持つ(隠さない)。

### MIR 最適化 (`align_mir`)
- loop fusion、mask の branchless 化、不要 clone/heap の除去、const string pool 化。
- lint(`draft.md` §16)の多くはこの解析結果を流用して診断する。

### Codegen (`align_codegen_llvm`)
- MIR → LLVM IR。`vec<N,T>`/`mask` を LLVM の vector type / select に対応付け、決定論的にベクトル命令を出す。
- arena 確保は runtime 呼び出しへ。詳細 `05-backend-llvm.md`。

### Driver (`align_driver`)
- CLI。各段を順に呼び、object を `align_runtime` とリンクして実行ファイルを出す。
- サブコマンド(予定): `alignc build` / `alignc run` / `alignc check`(sema まで) / `alignc emit-mir` / `alignc emit-llvm`。

## 横断クレート

- `align_span`: ファイル ID + バイトオフセット範囲。全 IR ノードが span を持ち、診断で元ソースを指す。
- `align_diag`: エラー/警告の型、表示、複数エラー集約。各段は失敗しても可能な限り続行し診断を貯める。

## 骨格(walking skeleton)で最初に通す経路

M0(`07-roadmap.md`)で通す最小経路。各段の「自明な実装」だけを繋ぐ。

```align
fn main() -> i32 {
  x := 1
  return x
}
```

この1本が lexer → parser → sema(型のみ) → MIR → LLVM → 実行ファイル(終了コード1)まで通れば骨格完成。以降の機能は全段へ少しずつ差し込む。
