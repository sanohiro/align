# フロントエンド: 字句・構文・AST (draft)

`align_lexer` / `align_parser` / `align_ast` の設計たたき台。`draft.md` の構文と、これまでに確定した方針を反映する。

確定済みの前提:

```text
文の終端       Go スタイル (改行が暗黙の終端、; は1行に詰めるとき用の任意セパレータ)
ブロック値     末尾の ; なし式 = ブロックの値
式指向         if / match / else取り出し / arena は式
型宣言         キーワードなし (内容で struct / sum type を判別)
区切り         , (フィールド・引数・variant)。改行が無意味なので , が必要
正規化         公式 formatter が単一正規形へ (One Way)
```

この文書は **draft(たたき台)**。未決箇所は本文末尾「未決事項」に集約し、本文中は `// OPEN:` で印を付ける。

---

## 1. 字句 (Lexical)

### エンコーディング
ソースは UTF-8。識別子は ASCII を基本とし、非 ASCII は文字列・コメント・char リテラル内のみ。

### コメント
C/Rust 風。

```align
// 行コメント
/* ブロックコメント /* ネスト可 */ */
/// doc コメント (宣言の直前。ツール/将来のドキュメント生成用)
```

### 識別子
```text
ident   = (letter | "_") (letter | digit | "_")*
```

### キーワード (予約語)
```text
fn  mut  return  if  else  match  arena  unsafe
module  import  pub
true  false
```

型名 (`i32` 等)・組込み (`array` `slice` `vec` `mask` `Option` `Result` 等) は**予約語にしない**。標準ライブラリが定義する通常の識別子として扱い、言語コアを小さく保つ。`template` / `html` / `json` / `raw` は文字列前置子(後述)で、文脈限定の弱いキーワード。

### リテラル

整数:
```align
42        // 10進
1_000_000 // _ は桁区切り (無視)
0xFF      // 16進
0o755     // 8進
0b1010    // 2進
```

浮動小数:
```align
3.14
1.5e-10
```

文字・文字列・bool:
```align
'a'   '\n'   '\u{1F600}'
"hello\tworld"
true   false
```

数値リテラルは原則**型を持たず**、文脈(注釈・推論)で確定する(`03-types.md`)。曖昧な場合のみ接尾辞で明示する。

```align
x := 10        // 文脈から型が決まる
y := 10i64     // 明示
```

`// OPEN:` 接尾辞の正確な集合(`i8..u64`/`f32`/`f64`)。

文字列リテラルは lexer 段で**コンパイル時 meta**(len / hash / ascii / utf8_valid / escape 要否, `draft.md` §12)を一次計算してトークンに添える。

### 演算子・記号
```text
+  -  *  /  %
==  !=  <  <=  >  >=
&&  ||  !
=        代入
:=       宣言 (immutable)
->       戻り型
?        Result 伝播 (後置)
.        メンバ / メソッド / フィールド射影
,  ;  :  ::
( )  { }  [ ]  < >
```

`< >` は比較とジェネリクスの両用(§9 で曖昧性解決)。

### 文終端 (Go スタイルの暗黙セミコロン)
lexer が文の終端トークン `END` を生成する。規則:

```text
- 行末のトークンが「文を終え得る」種別(ident / literal / ) / ] / } / ? など)なら、
  改行で暗黙の END を挿入する。
- ただし行頭が . または二項演算子で始まる場合は継続とみなし END を挿入しない
  (複数行メソッドチェーン)。
- 明示 ; は常に END。1行に複数文を詰めるときに使う。
- 行末が二項演算子 / , / ( / { / -> などで終わる場合も継続(END を挿入しない)。
```

これにより普段は `;` なしで書け、1行に詰めたいときだけ `;` を置ける(`draft.md` §4)。`{}` でブロックを区切るためインデントは無意味(非 Python)。`// OPEN:` `return` 直後改行など Go 流の細則の確定。

---

## 2. 文法表記

EBNF。`A*` 0回以上、`A+` 1回以上、`A?` 省略可、`A | B` 選択、`( )` グループ、`","` リテラル。末尾カンマは原則許可(formatter が付与)。

---

## 3. トップレベル (Items)

```ebnf
file        = module_decl? import_decl* item*

module_decl = "module" path END
import_decl = "import" path END
path        = ident ("." ident)*
END         = newline-inserted ";" | explicit ";"   // lexer 生成 (演算子・記号 §)

item        = vis? ( fn_decl | type_decl | const_decl )
vis         = "pub"
```

### 関数

```ebnf
fn_decl   = "fn" ident generics? "(" params? ")" ret? fn_body
params    = param ("," param)* ","?
param     = "out"? ident ":" type
ret       = "->" type
fn_body   = block | "=" expr END          // 単一式は = expr 形 (唯一の形)
generics  = "<" generic_param ("," generic_param)* ">"
```

```align
fn add(a: i32, b: i32) -> i32 = a + b

fn classify(u: User) -> str {
  s := score(u)
  if s > 80 { "high" } else { "low" }     // 末尾式 = 戻り値
}

fn fill(out dst: slice<f32>, v: f32) { dst = v }
```

### 型宣言 (キーワードなし)

struct と sum type を**同じ構文の場**で書き、中身で判別する。

```ebnf
type_decl  = ident generics? "{" type_body? "}"
type_body  = struct_body | enum_body
struct_body= field ("," field)* ","?
field      = ident ":" type
enum_body  = variant ("," variant)* ","?
variant    = ident ( "(" type ("," type)* ")" )?
```

判別規則(パーサ): ブロック内の最初の要素が `ident ":" type` なら **struct**、`ident` または `ident "(" ... ")"` なら **sum type**。両者の混在は不可(エラー)。空ブロック `Name {}` は空 struct。

```align
User {
  id: i64,
  name: str,
  active: bool,
}

Color { Red, Green, Blue }

Shape {
  Circle(f32),
  Rect(f32, f32),
}
```

`// OPEN:` variant に名前付きフィールド(`Rect { w: f32, h: f32 }`)を許すか。許すなら variant 本体も struct_body を取れるよう拡張。

### グローバル定数

```ebnf
const_decl = ident (":" type)? ":=" expr END
```

トップレベルの `:=` はコンパイル時定数(immutable)。`mut` は不可。const string pool(`draft.md` §12)の供給源の一つ。

---

## 4. 型 (Type)

```ebnf
type      = path generic_args?
          | "(" ")"                       // unit
generic_args = "<" type_arg ("," type_arg)* ">"
type_arg  = type | int_literal            // vec<4, f32> の N
```

組込みは型名も通常の path として扱う:

```align
i64   bool   str
Option<User>
Result<T, Error>
array<User>   slice<f32>
vec<4, f32>   mask<f32>
```

`// OPEN:` 関数型(クロージャを変数に持つ場合の型表記)。当面はジェネリクス境界での扱いに依存。

---

## 5. 文 (Statement)

ブロックは文の並び + 省略可能な末尾式。

```ebnf
block     = "{" stmt* tail_expr? "}"
tail_expr = expr                          // END なし。ブロックの値
stmt      = let_stmt
          | assign_stmt
          | return_stmt
          | expr END                      // 式文
let_stmt  = "mut"? ident (":" type)? ":=" expr END
assign_stmt = place "=" expr END
return_stmt = "return" expr? END
place     = expr                          // 代入可能な左辺 (ident / field / index)
```

`END` は改行で挿入される暗黙の終端、または明示 `;`(§1 文終端)。普段は改行のみで書く。

```align
x := 10
mut count := 0
count = count + 1
return x

a := 1; b := 2          // 1行に詰めるときだけ ; を使う
```

代入 `=` は宣言済み `mut` 変数(または可変な place)のみ。未宣言名への `=` はエラー(宣言は `:=`)。

---

## 6. 式 (Expression)

式指向。`if` / `match` / `block` / `arena` / `unsafe` はすべて式。

### 優先順位 (低 → 高)

```text
1  else 取り出し        expr else <block|stmt>
2  ||
3  &&
4  比較  == != < <= > >=
5  + -
6  * / %
7  単項  - !
8  後置  f(args)  .method(args)  .field  [index]  ?
9  一次  literal / path / (expr) / struct_lit / block / if / match / arena / unsafe / lambda
```

### 一次式

```ebnf
primary   = literal
          | path
          | "(" expr ")"
          | struct_lit
          | block
          | if_expr
          | match_expr
          | arena_expr
          | unsafe_expr
          | lambda
          | str_prefixed                  // template/html/json/raw
          | field_selector                // .ident (引数位置の射影ショートカット)
```

### struct リテラル
```ebnf
struct_lit = path "{" (field_init ("," field_init)* ","?)? "}"
field_init = ident ":" expr | ident       // ident 単独は ident: ident の短縮
```
```align
p := Point{ x: 1, y: 2 }
u := User{ id, name, active: true }       // id, name は同名短縮
```

### if / match (式)
```ebnf
if_expr   = "if" expr block ("else" (if_expr | block))?
match_expr= "match" expr "{" arm+ "}"
arm       = pattern "=>" (expr "," | block) 
```
`if` を式に使う場合、両腕の型は一致する(`03-types.md`)。`else` 無しの `if` は値を持たない文として使う。

```align
label := if s > 80 { "high" } else { "low" }

kind := match shape {
  Circle(_)  => "round",
  Rect(_, _) => "boxy",
}
```

### else 取り出し (Option/Result の unwrap-or-else)
```ebnf
else_expr = expr "else" (block | stmt)
```
右辺の block/stmt は脱出(`return` 等)するか、同型の値を与える。

```align
user := find_user(id) else return Error.NotFound
port := get_env("PORT") else { 8080 }
```

### ? 伝播
```ebnf
try_expr  = expr "?"
```
`?` は `Result` のみ(型検査で強制, `draft.md` §5)。MIR で早期 return + cold path に脱糖(`04-mir.md`)。

```align
data := fs.read_file(path)?
user := json.decode<User>(data)?
```

### メソッドチェーン・フィールド射影
```align
total := users
  .where(.active)     // .active = フィールドセレクタ
  .score              // array<User> に対する .score = フィールド射影
  .sum()
```
`.field` は文脈で2つの意味を持つ(型で決まる、`03-types.md`):
- 単一値 `u.score` → 通常のフィールドアクセス
- コレクション `users.score` → 各要素の射影(`array<i32>`)

### フィールドセレクタ短縮
引数位置の `.ident` は `fn x { x.ident }` の糖衣。

```align
active := users.where(.active)   // == users.where(fn u { u.active })
```

### ラムダ
`draft.md` の記法に合わせ、引数は括弧なし。

```ebnf
lambda    = "fn" lambda_params? block
lambda_params = ident ("," ident)*        // 型は推論
```
```align
total := users.reduce(0, fn acc, u { acc + u.score })
ys := xs.map(fn x { x * 2 })
zero := fn { 0 }                           // 引数なし
```
名前付き関数(`fn ident (`)とは「名前＋括弧の有無」で区別する。

### arena / unsafe (式)
```ebnf
arena_expr  = "arena" block
unsafe_expr = "unsafe" block
```
```align
arena {
  data := fs.read_file(path)?
  users := json.decode<array<User>>(data)?
  process(users)?
}
```

### 前置文字列 (template / html / json / raw)
```ebnf
str_prefixed = ("template" | "html" | "json") string_lit
             | "raw" "(" expr ")"
```
`{ident}` 補間を含む文字列リテラルを取り、MIR で `write_static` / `write_value` 列に脱糖(`draft.md` §13, `04-mir.md`)。

```align
msg := template "Hello {name}, score={score}"
body := html "<p>{name}</p>"
```

---

## 7. パターン (match)

```ebnf
pattern   = "_"                           // ワイルドカード
          | literal
          | ident                         // 束縛
          | path ( "(" pattern ("," pattern)* ")" )?   // variant 分解
```
```align
match shape {
  Circle(r)     => area_circle(r),
  Rect(w, h)    => w * h,
}
```
`// OPEN:` ガード(`if`)、`|` による複数パターン、網羅性検査の詳細。

---

## 8. 曖昧性と解決

### struct リテラル vs ブロック
`if cond { ... }` の `cond` 位置に裸の struct リテラルを許すと `Foo { ... }` がブロックと衝突する(Rust と同じ問題)。**解決**: `if` / `match` / `while` のスクルチニ位置では裸の struct リテラルを禁止し、必要なら括弧で囲む。

```align
if (Point{x:1,y:2}) == p { ... }
```

### ジェネリクスの `<` vs 比較
式位置の `a < b` と `f<T>(x)` の曖昧性。**解決方針**: 型位置(`: type`、`fn ret`、`type_decl` 等)では `<>` は常にジェネリクス。式位置では原則ジェネリック実引数を**書かせない**(推論で確定)。どうしても必要な箇所のみ明示構文を用意する。

```align
// OPEN: 式位置で型引数を明示する構文 (turbofish 風 `f::<T>()` を採るか)
```
Align は推論前提(`design-notes.md`)なので、式位置の明示型引数は最小化する。

### 型宣言 vs struct リテラル
型宣言(`User { id: i64 }`)はトップレベル item 位置のみ。struct リテラル(`User{ id: 1 }`)は式位置のみ。出現位置で一意に分かれる。

---

## 9. AST (align_ast, Rust)

全ノードは `Span`(`align_span`)を持つ。脱糖はしない(書かれた形を保持し、formatter / lint が使う)。抜粋:

```rust
struct File { module: Option<Path>, imports: Vec<Path>, items: Vec<Item> }

enum Item {
    Fn(FnDecl),
    Type(TypeDecl),
    Const(ConstDecl),
}

struct FnDecl {
    vis: Vis,
    name: Ident,
    generics: Vec<GenericParam>,
    params: Vec<Param>,
    ret: Option<Type>,
    body: FnBody,            // Block | ExprEq
    span: Span,
}
struct Param { is_out: bool, name: Ident, ty: Type }

struct TypeDecl { vis: Vis, name: Ident, generics: Vec<GenericParam>, kind: TypeKind }
enum TypeKind { Struct(Vec<Field>), Sum(Vec<Variant>) }
struct Field { name: Ident, ty: Type }
struct Variant { name: Ident, payload: Vec<Type> }

enum Stmt {
    Let { is_mut: bool, name: Ident, ty: Option<Type>, init: Expr },
    Assign { place: Expr, value: Expr },
    Return(Option<Expr>),
    Expr(Expr),              // 末尾式は Block.tail に保持し Stmt にはしない
}
struct Block { stmts: Vec<Stmt>, tail: Option<Box<Expr>> }

enum Expr {
    Lit(Lit),
    Path(Path),
    Unary { op: UnOp, rhs: Box<Expr> },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    Method { recv: Box<Expr>, name: Ident, args: Vec<Expr> },
    Field { recv: Box<Expr>, name: Ident },     // 単一/射影は型検査で決定
    FieldSelector(Ident),                       // 引数位置の .ident
    Index { recv: Box<Expr>, index: Box<Expr> },
    Try(Box<Expr>),                             // expr?
    StructLit { path: Path, fields: Vec<(Ident, Option<Expr>)> },
    If { cond: Box<Expr>, then: Block, els: Option<Box<Expr>> },
    Match { scrut: Box<Expr>, arms: Vec<Arm> },
    Else { lhs: Box<Expr>, rhs: ElseBody },     // unwrap-or-else
    Block(Block),
    Arena(Block),
    Unsafe(Block),
    Lambda { params: Vec<Ident>, body: Block },
    StrPrefixed { kind: StrKind, lit: StringLit },  // template/html/json
    Raw(Box<Expr>),
}
```

`// OPEN:` 演算子の結合性詳細、`Path` のジェネリック実引数保持、エラーノード(回復用)の表現。

---

## 10. パーサ実装方針

- 手書き再帰下降 + 式は Pratt parsing(優先順位は §6 の表)。LALR 生成器より診断とエラー回復を作り込みやすく、Align の弱キーワードや文脈依存(struct リテラル抑制等)を扱いやすい。
- **エラー回復**: 1ファイルで複数エラーを報告。文境界(`;`)とブロック境界(`}`)を同期点にする。
- 文終端は lexer が `END` トークンに正規化する(Go スタイルの暗黙セミコロン + 行頭継続、§1)。パーサは `END` だけを見て文境界を決め、生改行を意識しない。
- 脱糖しない。`?` / `template` / フィールドセレクタ / `else`取り出し は AST にそのまま保持し、MIR 段で展開(`04-mir.md`)。

---

## 11. 未決事項 (要決着)

```text
- 数値接尾辞の正確な集合と既定型 (i32? i64?)
- variant に名前付きフィールドを許すか (Rect { w, h })
- match のガード / | 複数パターン / 網羅性検査
- 式位置でのジェネリック型引数明示構文 (turbofish の要否)
- 関数型(クロージャ)の型表記
- 文字列補間の文法詳細 ({expr} を許すか、{ident} のみか)
- doc コメント(///)の収集対象と形式
```

これらは関連マイルストーン(`07-roadmap.md`)で決着させ、決まり次第 `draft.md` と本書へ反映する。
