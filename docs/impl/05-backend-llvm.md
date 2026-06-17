# バックエンド: MIR → LLVM (draft)

`align_codegen_llvm` の設計たたき台。**純粋な lowering** に徹する——Align の意味論判断(脱糖・fusion・SIMD化・region)は MIR で済んでおり(`04-mir.md`)、ここは MIR を機械的に LLVM IR へ落とすだけ。型・Region・並列単位は MIR に載っているので**再計算しない**(anti-rewrite, `00-overview.md`)。

```text
MIR (optimized)  →  LLVM IR  →  object (.o)  →  [driver がリンク] → 実行ファイル
                                                  + align_runtime (06)
```

実装は Rust の LLVM バインディング(`inkwell`)を基本線とする。`// OPEN:` 版固定戦略(LLVM のバージョン依存をどう吸収するか)。

この文書は **draft**。未決は末尾 + 本文 `// OPEN:`。

---

## 1. 型の対応 (Ty → LLVM type)

MIR の `Ty`(`03 §1`)を LLVM 型へ一対一で写す。

```text
Bool              i1 (格納時は i8)
Int(w, signed)    iW            (符号は演算側で区別)
Float(32|64)      float | double
Char              i32 (Unicode scalar)
Unit              {} (空) / void (戻り)
Vec(n, T)         <n x T'>      ← LLVM vector type に直結
Mask(T)           <n x i1>
Bitset            iN / [iW]
Array(T)          { T* ptr, i64 len, i64 cap }   所有・連続
Slice(T, _)       { T* ptr, i64 len }            view (Region は型に出さない)
Str               { i8* ptr, i64 len }           (+ meta は別途, §6)
String/Buffer/Builder  所有ヘッダ構造体
Named(struct)     %struct.S = type { 各フィールド }   (レイアウトは §2)
Named(sum)        { iT tag, [payload bytes] }    タグ付き共用体
Option(T)         非null化できる型は null 表現、他は { i1, T }   // OPEN: 表現確定
Result(T,E)       { i1 is_ok, union{T,E} }
Fn(..)            関数ポインタ (+ キャプチャがあれば環境ポインタ)
```

`Region` は **LLVM に出ない**。安全性は HIR で検証済み(`03 §7`)で、codegen には実体(arena ポインタ等)だけ渡る。これが「lifetime を表に出さない」の最終地点。

---

## 2. struct レイアウト

既定は **AoS**(宣言順、自然アライン、`draft.md` の値型中心)。データ並列で効く **SoA** は配列に対する変換として扱う。

```text
AoS   array<User> = User の連続 → { User* , len, cap }
SoA   ホット処理向けに array<User> を {id[], name[], active[], score[]} へ
```

`// OPEN:` SoA 変換のトリガ(自動判定 vs 注釈)。MIR の射影/fusion 解析(`04 §3`)が SoA の方が有利と判断した配列を SoA に置けるようにする方向。フィールドは SIMD のため自然アラインし、必要なら明示アライン属性を付ける(`draft.md` §3.4 alignment)。

---

## 3. 関数・CFG・cold path

- MIR の `Function` → LLVM function。`Block` → LLVM basic block(ほぼ一対一)。
- ターミネータ対応:

```text
Goto         br
Branch       条件 br
Switch       switch
Return       ret
TryEdge      条件 br。err_bb に cold メタ付与
Loop         ヘッダ/ボディ/エグジットの br 構造 (§5 でベクトル化)
ParLoop      runtime の並列 API 呼び出しへ (§7)
```

### cold path (error)
`?` の失敗辺(`04 §2.1`)は cold。LLVM では:

```text
- err_bb に分岐する br へ llvm.expect / branch weights を付け、正常側を fall-through に
- err_bb の中身を関数末尾(またはコールド section)へ配置
- 失敗パスの関数呼び出しは noinline 寄りに
```

これで正常パスの I-cache を汚さない(`draft.md` §10)。

---

## 4. allocation の lowering

MIR の明示 `Alloc`(`04 §5`)を実体化する。

```text
Alloc(Arena(id), layout)   → align_rt_arena_alloc(arena_ptr, size, align) を返すポインタ
arena ブロック出口          → align_rt_arena_reset(arena_ptr)   (一括, 個別 free なし)
Alloc(Heap, layout)        → align_rt_heap_alloc(...)  / Drop 点で align_rt_heap_free
Alloc(Stack, layout)       → alloca
```

arena ポインタは arena ブロック入口で `align_rt_arena_begin()` 相当を確保し、ブロックスコープの値として持ち回る(関数引数/ローカル)。詳細な runtime ABI は `06-runtime-std.md`。

---

## 5. ループとベクトル化 (Align の性能の要)

MIR は fusion 済みで、要素独立ループを「vector幅 W + remainder」に整形済み(`04 §4`)。codegen はこれを**決定論的に** vector 命令へ落とす——LLVM の自動ベクトル化に「期待する」のではなく、こちらが vector 型で IR を組む。

```text
vector body   <W x T> の load → VecOp/Mask → store。ポインタは W ずつ進む
remainder     端数をスカラで処理
```

```text
total := scores.sum_where(scores > 80);   (MIR: VecCmp + MaskedReduceAdd)
=>
loop:
  v   = load <W x f32>, p
  m   = fcmp ogt <W x f32> v, splat 80.0     ; <W x i1>
  sel = select <W x i1> m, v, zeroinitializer
  acc = fadd <W x f32> acc, sel
  p  += W
; reduce: llvm.vector.reduce.fadd(acc) + remainder
```

- **mask** → LLVM `<W x i1>` と `select`(branchless, `04 §4`)。
- **dot / sum / min / max** → `llvm.vector.reduce.*`。
- **no-alias**(`out`, `03 §6`)→ ポインタ引数に `noalias` 属性。依存なしでベクトル化が成立する根拠を LLVM に明示。
- アライン済みなら aligned load/store。

### ターゲット幅 W
```text
vec<N,T> 由来   N をそのまま LLVM vector 幅に
推論ループ      ターゲット ISA のネイティブ幅 (例 AVX2: 256bit) を既定に
```
`// OPEN:` W の最終決定方針(`04 §9` と同一論点)。複数 ISA 対応(feature 別 codegen / 実行時ディスパッチ)は v1 範囲外候補。

---

## 6. 文字列・builder・const pool

- **文字列リテラル**: バイト列を LLVM global constant に。`str` 値は `{ptr,len}`。コンパイル時 meta(len/hash/ascii, `draft.md` §12, `03`)は定数として埋め込み、`write_static` の長さや hash 比較に使う。
- **const string pool**(`draft.md` §12): 同一リテラル/JSON フィールド名/HTTP ヘッダ名は単一の global に集約(重複排除)。
- **builder**: runtime の可変バッファ。`template` 脱糖(`04 §2.5`)の `write_static` は memcpy + 既知長、`write_value` は型別の書式化呼び出しに。

---

## 7. 並列 (ParLoop → runtime)

MIR の `ParLoop`(`04 §6`)を runtime の並列 API へ。

```text
ParLoop(chunk, body)          → align_rt_par_for(items, chunk, body_fn, ctx)
ParLoop(.., reduce)           → 部分結果配列を確保 → 並列実行 → 結合 reduce を直列/木状に
task_group spawn/wait         → align_rt_task_spawn / align_rt_task_wait
```

`body` は MIR で fusion 済みの本体を**別関数として切り出し**、関数ポインタ + キャプチャ環境(`ctx`)を runtime に渡す。並列単位が MIR から渡るので codegen に並列判断は無い。ABI は `06`。

---

## 8. ターゲット・最適化・出力

```text
- TargetMachine をホスト(または指定トリプル)で構築。data layout を取得し §2 のレイアウトに反映
- LLVM 最適化: Align 側で fusion/vectorize 済みなので、LLVM には
  下位最適化(instcombine, regalloc, ピープホール等)を任せる。高位変換は二重にしない
- 出力: object (.o)。driver が align_runtime とリンクして実行ファイルへ (01/06)
- alignc emit-llvm で IR をテキスト出力 (検証/デバッグ用, 01)
```

`// OPEN:` LLVM パスパイプラインをどこまで使うか(O2 相当一括 vs 必要パス選別)。Align の最適化と衝突しない範囲を実測で決める。

---

## 9. デバッグ情報・パニック

```text
- DWARF/CodeView 行情報を Span(align_span) から生成。最低限ステップ実行できる水準を M で段階導入
- ゼロ除算等のトラップ(03/draft §5): runtime のアボート(align_rt_panic)へ。メッセージ + 位置
- overflow は既定 wrap なのでチェックを出さない (開発ビルドのみ任意でチェック挿入)
```

---

## 10. 未決事項 (要決着)

```text
- inkwell / LLVM バージョン固定とアップグレード戦略
- Option/Result の LLVM 表現確定 (null化 vs タグ付き、ニッチ最適化)
- SoA 変換のトリガ (自動 vs 注釈) と array<T> ABI への影響
- ベクトル幅 W の決定と複数 ISA 対応の範囲 (04 §9 と共通)
- LLVM 最適化パイプラインの採用範囲 (Align 最適化との非重複)
- デバッグ情報の精度をどの M でどこまで上げるか
- リンク: 静的 runtime か、libc 依存をどこまで持つか (06 と連動)
```

決着次第 `draft.md`(該当機能)と本書へ反映する。
