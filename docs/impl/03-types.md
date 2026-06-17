# 型システム・推論・安全性検査 (draft)

`align_sema` の設計たたき台。担当は3パス: (2) 型推論・型検査、(3) move 検査・arena escape 検査・効果検査。((1) 名前解決は `01-pipeline.md` 参照、ここでは解決済みを前提)。

設計原則(`draft.md` §3.3 / `design-notes.md`):

```text
lifetime を表に出さない    move も arena 寿命もフロー解析で推論し、誤りだけをエラーにする
推論前提                  局所推論 + 双方向型付け。global HM のような複雑さは持たない
予測可能                  同じコードは常に同じ型に決まる。曖昧なら明示を要求
コンパイラに情報を渡す      no-alias / non-null / region / cold path を HIR に載せ MIR/codegen が再計算しない
```

この文書は **draft**。未決は末尾「未決事項」+ 本文 `// OPEN:`。

---

## 1. 型の表現 (Ty)

`align_sema` 内部の型表現。

```text
Ty =
  Bool
  Int(width, signed)        // i8..i64 / u8..u64
  Float(width)              // f32 / f64
  Char
  Unit                      // ()
  Str | String | Bytes | Buffer | Builder
  Array(Ty)                 // 所有・連続メモリ
  Slice(Ty, Region)         // view。Region を持つ
  Vec(n, Ty) | Mask(Ty) | Bitset
  Option(Ty)
  Result(Ty, Ty)
  Named(DefId, [Ty])        // struct / sum type。ジェネリック実引数
  Fn([Ty], Ty, Effect)     // ラムダ・関数値
  Var(id)                   // 推論変数 (推論中のみ)
```

`Named` は **nominal**(名前で同一性が決まる)。struct も sum type も `Named` で表し、`DefId` 経由で定義(フィールド/variant)を引く。

### Region (寿命タグ)
view 系(`Slice` / `Str` 等の参照的な型)だけが持つ。ユーザは書かない。エラーメッセージにのみ現れる。

```text
Region =
  Static        // 文字列リテラル / const pool
  Heap          // 明示 heap 由来
  Value         // 所有値の内部 (その値と寿命を共有)
  Arena(id)     // 特定の arena ブロック由来
```

---

## 2. 数値リテラルの既定型

文脈(注釈・推論)で型が決まる。**最後まで未制約のときだけ**既定型を適用する。

```text
整数リテラル 既定 = i64    (modern/64bit 既定。id 等の溢れに安全)
浮動リテラル 既定 = f64
```

接尾辞で明示可: `10i32` / `2.0f32`。`// OPEN:` 大配列で i64 既定が無駄な場合の lint(i32 で足りる旨)。

### 整数オーバーフロー (確定, draft.md §5)

整数演算は **UB にしない**。既定は2の補数 wrap(全ビルド同一・分岐なし・ベクトル化を妨げない)。codegen は通常の `add`/`mul` 等をそのまま出す。`checked_*`(→Option) / `saturating_*` / `wrapping_*` を明示 op としてライブラリで提供。開発時のみ overflow チェック付きビルドと lint でバグ検出するが意味論は変えない。ゼロ除算等は溢れと別で、silent にせず必ずエラー(trap か Result)。

```align
x := 10;            // 未制約 → i64
y: i32 := 10;       // 注釈 → i32
z := 10i32;         // 接尾辞 → i32
s := xs.sum();      // xs: array<i32> → i32 (文脈で決まる)
```

---

## 3. 推論と検査 (双方向)

局所推論 + 双方向型付け。2モードを使い分ける。

```text
check(expr, expected)   期待型がある場合 (注釈 / 引数位置 / return / if 両腕の統一)
infer(expr) -> Ty       期待型がない場合 (:= の右辺など)
```

- `x := e` → `infer(e)` の結果を `x` の型に。
- `x: T := e` → `check(e, T)`。
- 関数本体は `check(body, ret)`。`= expr;` 形も同様。
- 引数は宣言型で `check`。

統一(unify)は推論変数 `Var` の解決のみに使い、nominal 型は構造で勝手に同一化しない。曖昧(`Var` が残る)なら型注釈を要求するエラー。

### if / match は式 → 両腕を統一
フロントエンドの宿題を回収。

```text
if c { a } else { b }   : check(c, Bool); T = unify(type(a), type(b)); 結果 T
match s { p1 => e1, ... }: 各 ei を unify。結果は共通型
else 無しの if          : 値を持たない (Unit 文としてのみ可)
match は網羅でなければエラー (// OPEN: 網羅性判定の詳細)
```

```align
label := if s > 80 { "high" } else { "low" };   // 両腕 str → label: str
```

---

## 4. フィールドアクセスと射影 (`.field` の二義を解決)

`recv.field` の型は **受け手の型で決まる**。

```text
recv: Named(struct S)        → S.field の型 (通常アクセス)
recv: Array(Named S) / Slice → Array(field の型) (射影)
```

```align
u.score              // u: User        → i32
users.score          // users: array<User> → array<i32> (射影)
users.where(.active).score.sum()
//    ^ Slice<User>   ^ Array<i32>   ^ i32
```

射影は HIR 上で `Project(field)` ノードに確定し、MIR で fusion 対象になる(`04-mir.md`)。通常アクセスは `FieldAccess`。

### フィールドセレクタ `.ident`
引数位置の `.ident` は受け手要素型 `E` から関数値 `Fn([E], type_of(E.ident), Pure)` として型付け。

```align
users.where(.active)   // .active : Fn([User], bool, Pure)
```

---

## 5. Option / Result / ? / else

```text
?         expr: Result(T, E) で、囲む関数の戻りが Result(_, E') かつ E が E' に変換可能 → 値は T
          Result 以外に ? はエラー (draft.md §5)
else      lhs: Option(T) または Result(T, _)。
          rhs は (a) 発散する (return 等) か (b) T を与える。結果は T
```

```align
data := fs.read_file(path)?;             // Result(String,E) → String, 失敗は伝播
user := find_user(id) else return ...;   // Option(User) → User
port := get_env("PORT") else { 8080 };   // else 腕が i64 を供給
```

`?` / `else` は HIR では専用ノードのまま保持し、MIR で early-return + cold path に脱糖(`04-mir.md`)。`E → E'` 変換規則は `// OPEN:`(error type 設計, M2)。

---

## 6. 所有権と move 検査 (パス3, lifetime なし)

### Copy 型と Move 型
```text
Copy (値, ビット複製で安全)
  bool / 整数 / 浮動 / char / Unit
  Vec / Mask / Bitset
  全フィールドが Copy かつ小さい struct
  Slice (view の複製。指す先は複製しない。Region 制約は別途)

Move (所有, 線形)
  Array / String / Buffer / Builder
  Heap box
  Move 型を含む struct / 大きい struct
```

`// OPEN:` 「小さい」の閾値(レイアウトサイズ)。大 struct の値渡しは **lint**(エラーではない, `draft.md` §6.2)。

### 検査
CFG 上のフロー解析。Move 型の値が consume(値として代入/値引数に渡す/値で返す)されると、元の束縛は dead。dead な束縛の使用は **コンパイルエラー**。

```align
data := fs.read_file(path)?;
other := data;        // data を move
print(data);          // error: data は move 済み
```

複製は明示 `clone()`。`Copy` 型にはこの制約はかからない。

### out 引数と no-alias
`out dst: slice<T>` は「`dst` は他の入力と別領域」を意味する。検査(呼出側で `dst` が他引数と alias しないこと)+ 最適化情報(no-alias)として HIR に記録し MIR/codegen へ渡す(`draft.md` §7)。

---

## 7. arena escape 検査 (パス3, region でlifetimeを隠す)

`arena {}` はブロックに `Arena(id)` region を導入する。ブロック内の allocation 由来の view はこの region を帯びる。

**escape 規則**: `Arena(id)` を帯びた値は、その arena ブロックより長く生きてはならない。具体的に次を**コンパイルエラー**にする。

```text
- arena ブロックの外で宣言された束縛への代入
- arena ブロックからの return / ブロック値として外へ返す
- 非 arena コンテナ(外の array 等)への格納
- arena 外へ脱出するクロージャへのキャプチャ
```

```align
mut saved: slice<User> := empty;
arena {
  data := fs.read_file(path)?;
  users := json.decode<array<User>>(data)?;   // users は Arena(a) region
  total := users.where(.active).score.sum();  // OK: 値(i64)は region を持たない
  saved = users;                              // error: arena view が外へ escape
}
```

region の伝播はフロー解析で推論し、ユーザは一切書かない。違反時のみエラーメッセージに region を出す(例: 「この view は arena ブロックに束縛されている」)。`// OPEN:` arena のネスト時の region 順序、明示 allocator (`arena a {}`, open-questions) との統合。

---

## 8. 効果検査 (par_map の純粋性, パス3)

並列・データ処理に渡す関数は副作用を持てない(`draft.md` §11)。効果は**推論**する(注釈しない, open-questions の purity は推論方針で決着)。

```text
Effect = Pure | Impure(理由)
関数/ラムダは本体から効果を推論:
  外部 mut 束縛の変更        → Impure
  I/O など副作用 std 呼出し  → Impure
  上記が無ければ             → Pure
```

`par_map` / `map` / `where` / `reduce` のクロージャ引数は `Pure` を要求。違反はエラーで、`reduce` への誘導を示す。

```align
mut total := 0;
users.par_map(fn u { total = total + u.score });  // error: 外部 mut を変更 (Impure)
total := users.reduce(0, fn acc, u { acc + u.score });  // OK: Pure
```

`Fn` 型は効果を持つ(`Fn([Ty], Ty, Effect)`)ので、関数値経由でも検査できる。

---

## 9. ジェネリクス (最小, M4 前に確定)

monomorphization(使用箇所ごとに具象化)。Rust/C++ のトレイト/テンプレート複雑性は持たない(`non-goals.md`)。

```text
- 型パラメータは Named / Fn / array<T> 等に持てる
- 制約は当面「使われた演算から推論」する構造的方針を基本線とする
- 明示的な境界(trait 風)を入れるかは M4 着手前に決定
```

`// OPEN:` 制約の表現(構造的 vs 明示境界)、`vec<N,T>` の N(値ジェネリクス)の扱い、単相化の実装単位。これは `04-mir.md`(単相化を MIR 生成前に行うか後か)と連動。

---

## 10. typed HIR (パス出力)

検査を通った AST は **typed HIR** になる。AST とほぼ同形だが、後段が型情報を再計算しないよう次を確定済みで載せる(anti-rewrite, `00-overview.md`)。

```text
- 全 Expr に解決済み Ty
- Path は DefId に解決済み
- .field は FieldAccess / Project(field) のどちらかに確定
- フィールドセレクタは具体化したクロージャに
- view 型の Region
- move 点(consume 位置)と dead 束縛のマーキング
- out 引数の no-alias フラグ
- 各関数/クロージャの Effect
```

`?` / `else` / `template` / arena は**まだ脱糖しない**(HIR では専用ノード)。脱糖は MIR(`04-mir.md`)。

---

## 11. エラー報告

- 双方向検査で「期待 vs 実際」を持つので、型不一致は期待型の出所(注釈/引数/return/if 腕)を併記する。
- move エラーは move した位置を指す。
- arena escape はエラー本文に region(どの arena に束縛されているか)を出す。lifetime 構文は出さない。
- 1関数内で複数の型エラーを集約(`align_diag`)。推論変数が残った箇所は「型注釈が必要」で停止。

---

## 12. 未決事項 (要決着)

```text
- E → E' (error 型変換) の規則           → M2 (error type 設計)
- match 網羅性判定の正確なアルゴリズム
- Copy/Move を分ける struct サイズ閾値
- arena ネスト時の region 順序 / 明示 allocator との統合  → M3
- ジェネリクス制約: 構造的推論 vs 明示境界 / 単相化単位  → M4
- 数値既定型の lint (大配列で i64 が過剰な場合)
```

決着次第 `draft.md` と本書へ反映する。
