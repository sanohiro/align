# MIR: 中間表現と脱糖・最適化 (draft)

`align_mir` の設計たたき台。MIR は **バックエンド非依存の核**(`00-overview.md`)。Align の意味論——脱糖・loop fusion・SIMD 化・arena/region のコード化——はすべてここで確定し、`MIR → LLVM`(`05-backend-llvm.md`)は純粋な lowering に限定する。

役割の境界:

```text
typed HIR (03)  型・Region・move・Effect が付いた、書かれた形のままの木
   │  ① 脱糖 (lowering)        ? / else / template / セレクタ / 射影チェーン を展開
   │  ② MIR 構築              CFG + 明示的な alloc / error-edge / 並列ノード
   │  ③ 最適化                fusion / mask branchless / 不要 clone・heap 除去 / const pool
   ▼
MIR (optimized)  → codegen へ
```

設計原則: **隠さない**(`draft.md` §3.2)。allocation / error path / 並列単位(chunk) は MIR 上の**明示ノード**として残し、lint(`draft.md` §16)と codegen の両方がそれを読む。

この文書は **draft**。未決は末尾 + 本文 `// OPEN:`。

---

## 1. MIR の形

関数ごとに CFG(基本ブロックの集合)。各ブロックは文の列 + 末尾ターミネータ。SSA に近い形(値は一度だけ定義、再代入は新しい値)を採るが、`mut` place への代入は明示的な store にする。

```text
Function { params, ret, regions, blocks[] }
Block    { stmts[], term }

stmt =
  Let(v, rvalue)               // v = rvalue (純粋計算)
  Store(place, operand)        // mut place への書き込み
  Alloc(v, kind, layout)       // ★明示 allocation ノード (kind: Arena(id) | Heap | Stack)
  Drop(place)                  // 所有値の解放点 (move 検査の結果から)
  Call(v?, callee, args, eff)  // eff: Pure/Impure。並列・I/O 解析に使う

term =
  Goto(bb)
  Branch(cond, bb_then, bb_else)
  Switch(operand, [(val,bb)...], default)   // match から
  TryEdge(ok_bb, err_bb)       // ★? の正常/失敗の分岐 (err_bb は cold)
  Return(operand?)
  Loop(header, body, exit)     // 構造化ループ (fusion 解析の単位)
  ParLoop(chunk, body, reduce?)// ★並列ループ (par_map/reduce)。単位は chunk

rvalue =
  Use(operand) | Bin(op,a,b) | Un(op,a)
  Field(place, idx)            // 単一フィールド
  Index(place, i)
  MakeStruct / MakeVariant
  VecOp(...)                   // SIMD レーン演算
  Mask(...)                    // 比較→mask
  Project(src, field)          // ★コレクションのフィールド射影 (fusion で消える)
```

`★` が「隠さない」ための明示ノード。最適化後も種別は保持され、codegen と lint が参照する。

各値・place は HIR 由来の `Ty` と(view なら)`Region` を持ち続ける。codegen は型を**再計算しない**(anti-rewrite)。

---

## 2. 脱糖 (lowering)

HIR の糖衣をここで CFG へ展開する。フロントエンド/型検査では展開しなかったもの(`02`/`03`)。

### 2.1 `?` (Result 伝播)
`expr?` を正常値の取り出し + 失敗時 early-return に展開し、失敗辺を **cold** に印付け(`draft.md` §10)。

```align
data := fs.read_file(path)?;
```
```text
t0 = call fs.read_file(path)        : Result(String, E)
TryEdge(ok, err)                    // err は cold
ok:  data = t0.ok_value
err: r = make Err(convert(t0.err))  // E -> 関数の E'
     Return(r)
```
codegen は cold 辺を別セクション/低優先で配置できる。

### 2.2 `else` 取り出し
`lhs else rhs` を Option/Result の分岐に。`rhs` が発散(`return`)ならそのまま、値供給なら then 側と合流。

```align
user := find_user(id) else return Error.NotFound;
port := get_env("PORT") else { 8080 };
```
```text
Switch(tag(lhs), some=>bind, none=>rhs_block)
```

### 2.3 フィールドセレクタ `.ident`
`xs.where(.active)` の `.active` は HIR で具体化済みクロージャ(`03 §4`)。MIR ではインライン展開し、呼び出しを残さない(fusion 前提)。

### 2.4 射影チェーン
`users.where(.active).score.sum()` を **単一ループへ脱糖**(fusion 本体は §3)。中間 `array<i32>`(`.score` の結果)を**作らない**。

### 2.5 template / html / json
コンパイル時に静的部と値部へ分解(`draft.md` §13)。

```align
msg := template "Hello {name}, score={score}";
```
```text
b = builder()
b.write_static("Hello ")     // 長さ既知 (文字列 meta, 03)
b.write_value(name)
b.write_static(", score=")
b.write_int(score)
msg = b.to_string()
```
`html`/`json` は値部に文脈別エスケープ(`write_html_escaped` 等)を挿す。静的部の長さ合計が既知なら builder の初期容量を事前確保(`Alloc` 1回)。

### 2.6 match
`Switch` ターミネータ + variant 分解 (`Field`/payload 取り出し)へ。網羅は型検査で保証済み(`03`)。

---

## 3. Loop Fusion (Align の看板)

「普通に書くとコンパイラが最適化しやすい」(`draft.md` §1)の本体。`map`/`where`/`filter`/`scan`/reduction のチェーンを**一つのループ**にまとめ、中間配列を消す。

### 対象と規則
```text
map(f)       要素ごと変換。次段へ素通し
where(p)/filter(p)  述語成立要素のみ次段へ (mask 化可能, §4)
Project(field)      要素のフィールドを取り出す (中間配列を作らない)
reduce/sum/min/max/count/dot/any/all  末端。アキュムレータへ畳み込み
```

連続する map/where/project は **producer-consumer 融合**して一本のループ本体に連結し、末端の reduction でループを閉じる。`Effect=Pure`(`03 §8`)が融合の前提。

```text
total := users.where(.active).score.sum();
=>
acc = 0
Loop over i in users:
  u = users[i]
  if u.active:                 // where → 分岐 or mask
    acc += u.score             // .score 射影は load に融合、配列を作らない
total = acc
```

### 配列式 (一時配列なし)
`a = (b + c) * d - e`(`draft.md` §9)は要素ごとの式木として、出力配列へ一発で書くループに。中間 `b+c` 等の一時配列を持たない。

```text
Loop over i:
  a[i] = (b[i] + c[i]) * d[i] - e[i]
```

`out` 引数(no-alias, `03 §6`)があると、入出力の別領域が保証され、依存チェックなしでベクトル化できる。

### 融合の境界 (`// OPEN:` 詳細)
```text
融合する     連続 map/where/project + 末端 reduction、要素独立、Pure
融合しない   sort / group_by / partition (全体再配置を伴う)、副作用、要素間依存(scan の一部)
```
`sort` 等は融合点を切り、その前後で別ループにする。

---

## 4. SIMD / mask lowering

vec/mask を**第一級**で MIR に持ち(`draft.md` §9)、codegen が決定論的にベクトル命令へ落とせる形にする。

### mask は branchless
`where`/比較は可能なら分岐でなく `mask` + 述語付き演算に落とす(SIMD/GPU 向き)。

```align
m := scores > 80;
total := scores.sum_where(m);
```
```text
m   = VecCmp(gt, scores, splat(80))   // mask<...>
acc = MaskedReduceAdd(scores, m)       // 分岐なし
```

### ベクトル化されたループの形
fusion 後のループ本体が要素独立なら、MIR で **vector幅 + 末尾(remainder)** の二相に整形しておく。codegen はこれをそのまま vector 命令 + スカラ末尾に落とす(`05`)。

```text
Loop:
  vector body   (幅 W)        // VecOp / Mask
  remainder body(< W)         // スカラ
```

`select(mask, a, b)` / `dot` / `sum_where` は専用 rvalue として保持。`// OPEN:` ターゲット幅 W の決め方(固定 vec<N> 由来 vs ターゲット ISA 由来)。

---

## 5. arena / region のコード化

`03 §7` で検査済みの region を、ここで実際の確保/解放に変換する。

```text
arena {}        →  Alloc(.., Arena(id))  群 + ブロック出口での一括解放
                   個々の Drop は出さない (arena は bump + 一括 reset)
Heap            →  Alloc(.., Heap)。Drop は move 検査由来の解放点で
Stack/Value     →  スタック上。Drop はスコープ末
```

```text
arena {
  data := fs.read_file(path)?;     // Alloc(Arena(a))
  users := json.decode(...)?;      // Alloc(Arena(a))  (zero-copy view は data を指す)
  process(users)?;
}
// 出口: arena(a) を一括 reset (個別 free なし)
```

`Alloc` ノードに region が付くので、lint「loop 内 allocation」「不要 heap」(`draft.md` §16)はこの MIR を走査して検出する。escape は HIR で既に弾いてある(`03 §7`)ため MIR では安全前提。

---

## 6. 並列ノード (par_map / task_group)

データ並列は `ParLoop`、I/O 並行は Call として残す。

```text
par_map(f)   ParLoop(chunk=既定/指定, body=f を fusion した本体, reduce=なし)
reduce 並列   ParLoop(.., reduce=結合的アキュムレータ)   // 部分和を結合
chunks(n)    ParLoop の chunk サイズを n に
task_group   spawn=非同期 Call、wait=合流点。? は各 spawn 結果に適用
```

`ParLoop` の body は `Effect=Pure` を要求済み(`03 §8`)。並列単位が MIR に明示されるので、codegen はランタイム(`06`)の並列 API へ素直に渡せる。`// OPEN:` reduce の結合性をどう保証/表現するか(モノイド指定 vs 既知 reduction 限定)。

---

## 7. 最適化パス (順序案)

```text
1. inline      小関数 / セレクタクロージャ展開 (fusion の前提を作る)
2. fuse        map/where/project + reduction の融合、配列式の融合 (§3)
3. mask        where→mask 化、branchless 化 (§4)
4. vectorize-shape  ループを vector幅+remainder に整形 (§4)
5. mem         不要 clone 除去 / 不要 heap → stack / arena 昇格
6. const       const string pool 化、定数畳み込み、文字列 meta 活用
7. simplify    到達不能(cold)整理、共通部分式
```

各パスは MIR→MIR。lint 診断は 2/3/5/6 の解析結果を流用する(別実装しない)。`// OPEN:` パス順の確定とフィックスポイント反復の要否。

---

## 8. デバッグ出力

`alignc emit-mir`(`01`)で MIR をテキスト表示。fusion 前後を比較できるよう、パス間スナップショットを出せるようにする。これは「最適化が効いているか」を人/AI が確認する手段であり、Align の予測可能性(`design-notes.md`)を支える。

---

## 9. 未決事項 (要決着)

```text
- fusion 境界の正確な規則 (scan / group_by の部分融合をどこまでやるか)
- SIMD ターゲット幅 W の決定方針 (vec<N> 固定 vs ターゲット ISA)
- par reduce の結合性表現 (モノイド指定 vs 既知 reduction 限定)
- 最適化パスの順序確定と反復(フィックスポイント)の要否
- MIR を SSA にどこまで寄せるか / mut place の扱い
- 単相化を MIR 構築の前後どちらで行うか (03 §9 と連動)
```

決着次第 `draft.md`(該当機能)と本書へ反映する。
