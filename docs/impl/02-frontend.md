# Frontend: Lexing, Parsing, AST (draft)

Working draft for `align_lexer` / `align_parser` / `align_ast`. Reflects the syntax in `draft.md` and the decisions settled so far.

Settled premises:

```text
Statement termination   Go style (newline is an implicit terminator; ; is an optional separator for cramming onto one line)
Block value             trailing expression with no ; = the block's value
Expression-oriented     if / match / else-unwrap / arena are expressions
Type declarations       keyword-less (struct / sum type disambiguated by content)
Separators              , (fields, args, variants). Since newlines are meaningless, , is required
Normalization           the official formatter converges to a single normal form (One Way)
```

This document is a **draft**. Open items are collected under "Open items" at the end of the document; in the body they are flagged with `// OPEN:`.

---

## 1. Lexical

### Encoding
Source is UTF-8. Identifiers are basically ASCII; non-ASCII appears only inside strings, comments, and char literals.

### Comments
C/Rust style.

```align
// line comment
/* block comment /* nesting allowed */ */
/// doc comment (immediately before a declaration; for tooling / future doc generation)
```

### Identifiers
```text
ident   = (letter | "_") (letter | digit | "_")*
```

### Keywords (reserved words)
```text
fn  mut  return  if  else  match  arena  unsafe
module  import  pub
true  false
```

Type names (`i32` etc.) and built-ins (`array` `slice` `vec` `mask` `Option` `Result` etc.) are **not reserved words**. They are treated as ordinary identifiers defined by the standard library, keeping the language core small. `template` / `html` / `json` / `raw` are string prefixes (see below): weak, context-limited keywords.

### Literals

Integers:
```align
42        // decimal
1_000_000 // _ is a digit separator (ignored)
0xFF      // hex
0o755     // octal
0b1010    // binary
```

Floats:
```align
3.14
1.5e-10
```

Char / string / bool:
```align
'a'   '\n'   '\u{1F600}'
"hello\tworld"
true   false
```

Numeric literals in principle **have no type**; the type is fixed by context (annotation / inference) (`03-types.md`). A suffix makes it explicit only when ambiguous.

```align
x := 10        // type determined by context
y := 10i64     // explicit
```

`// OPEN:` the exact set of suffixes (`i8..u64`/`f32`/`f64`).

String literals get their **compile-time meta** (len / hash / ascii / utf8_valid / whether escaping is needed, `draft.md` §12) precomputed at the lexer stage and attached to the token.

### Operators and symbols
```text
+  -  *  /  %
==  !=  <  <=  >  >=
&&  ||  !
=        assignment
:=       declaration (immutable)
->       return type
?        Result propagation (postfix)
.        member / method / field projection
,  ;  :  ::
( )  { }  [ ]  < >
```

`< >` serves both comparison and generics (ambiguity resolved in §9).

### Statement termination (Go-style implicit semicolons)
The lexer generates a statement-terminator token `END`. Rules:

```text
- If the last token on a line is of a kind that "can end a statement" (ident / literal / ) / ] / } / ? etc.),
  an implicit END is inserted at the newline.
- However, if the next line starts with . or a binary operator, it is treated as a continuation and no END
  is inserted (multi-line method chains).
- An explicit ; is always an END. Used to cram multiple statements onto one line.
- If a line ends with a binary operator / , / ( / { / -> etc., it is also a continuation (no END inserted).
```

This lets you normally write without `;`, placing `;` only when you want to cram onto one line (`draft.md` §4). Since `{}` delimits blocks, indentation is meaningless (not Python). `// OPEN:` settling Go-style fine points such as newline right after `return`.

---

## 2. Grammar notation

EBNF. `A*` zero or more, `A+` one or more, `A?` optional, `A | B` choice, `( )` grouping, `","` literal. Trailing commas are allowed in principle (the formatter adds them).

---

## 3. Top level (Items)

```ebnf
file        = module_decl? import_decl* item*

module_decl = "module" path END
import_decl = "import" path END
path        = ident ("." ident)*
END         = newline-inserted ";" | explicit ";"   // lexer-generated (operators & symbols §)

item        = vis? ( fn_decl | type_decl | const_decl )
vis         = "pub"
```

### Functions

```ebnf
fn_decl   = "fn" ident generics? "(" params? ")" ret? fn_body
params    = param ("," param)* ","?
param     = "out"? ident ":" type
ret       = "->" type
fn_body   = block | "=" expr END          // single expression uses the = expr form (the only form)
generics  = "<" generic_param ("," generic_param)* ">"
```

```align
fn add(a: i32, b: i32) -> i32 = a + b

fn classify(u: User) -> str {
  s := score(u)
  if s > 80 { "high" } else { "low" }     // trailing expression = return value
}

fn fill(out dst: slice<f32>, v: f32) { dst = v }
```

### Type declarations (keyword-less)

struct and sum type are written in the **same syntactic position** and disambiguated by content.

```ebnf
type_decl  = ident generics? "{" type_body? "}"
type_body  = struct_body | enum_body
struct_body= field ("," field)* ","?
field      = ident ":" type
enum_body  = variant ("," variant)* ","?
variant    = ident ( "(" type ("," type)* ")" )?
```

Disambiguation rule (parser): if the first element inside the block is `ident ":" type` it is a **struct**; if it is `ident` or `ident "(" ... ")"` it is a **sum type**. Mixing the two is not allowed (error). An empty block `Name {}` is an empty struct.

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

`// OPEN:` whether to allow named fields in a variant (`Rect { w: f32, h: f32 }`). If allowed, extend the variant body to also accept struct_body.

### Global constants

```ebnf
const_decl = ident (":" type)? ":=" expr END
```

A top-level `:=` is a compile-time constant (immutable). `mut` is not allowed. One of the sources feeding the const string pool (`draft.md` §12).

---

## 4. Types (Type)

```ebnf
type      = path generic_args?
          | "(" ")"                       // unit
          | "(" type ("," type)+ ")"      // tuple (arity >= 2); "(" type ")" is grouping
generic_args = "<" type_arg ("," type_arg)* ">"
type_arg  = type | int_literal            // the N in vec<4, f32>
```

Tuple values mirror the type: a literal `(a, b, ...)` (arity ≥ 2; `()` is unit, `(e)` is
grouping), positional access `t.0` / `t.1`, and a destructuring binding `(a, b) := expr`
(parens required, `_` ignores an element). Multi-value return is returning a tuple — there is
no separate multiple-return form (`design-notes.md` "One way").

Built-in type names are also treated as ordinary paths:

```align
i64   bool   str
Option<User>
Result<T, Error>
array<User>   slice<f32>
vec<4, f32>   mask<f32>
```

`// OPEN:` function types (type notation when holding a closure in a variable). For now this depends on how it is handled at generics bounds.

---

## 5. Statements (Statement)

A block is a sequence of statements plus an optional trailing expression.

```ebnf
block     = "{" stmt* tail_expr? "}"
tail_expr = expr                          // no END. The block's value
stmt      = let_stmt
          | assign_stmt
          | return_stmt
          | expr END                      // expression statement
let_stmt  = "mut"? ident (":" type)? ":=" expr END
assign_stmt = place "=" expr END
return_stmt = "return" expr? END
place     = expr                          // an assignable lvalue (ident / field / index)
```

`END` is the implicit terminator inserted at a newline, or an explicit `;` (§1 statement termination). Normally you write with newlines only.

```align
x := 10
mut count := 0
count = count + 1
return x

a := 1; b := 2          // use ; only when cramming onto one line
```

Assignment `=` applies only to a declared `mut` variable (or a mutable place). `=` to an undeclared name is an error (declaration is `:=`).

---

## 6. Expressions (Expression)

Expression-oriented. `if` / `match` / `block` / `arena` / `unsafe` are all expressions.

### Precedence (low → high)

```text
1  else unwrap          expr else <block|stmt>
2  ||
3  &&
4  comparison  == != < <= > >=
5  + -
6  * / %
7  unary  - !
8  postfix  f(args)  .method(args)  .field  [index]  ?
9  primary  literal / path / (expr) / struct_lit / block / if / match / arena / unsafe / lambda
```

### Primary expressions

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
          | field_selector                // .ident (projection shortcut at argument position)
```

### struct literals
```ebnf
struct_lit = path "{" (field_init ("," field_init)* ","?)? "}"
field_init = ident ":" expr | ident       // a bare ident is shorthand for ident: ident
```
```align
p := Point{ x: 1, y: 2 }
u := User{ id, name, active: true }       // id, name use same-name shorthand
```

### if / match (expressions)
```ebnf
if_expr   = "if" expr block ("else" (if_expr | block))?
match_expr= "match" expr "{" arm+ "}"
arm       = pattern "=>" (expr "," | block) 
```
When `if` is used as an expression, both arms must have the same type (`03-types.md`). An `if` with no `else` is used as a statement that has no value.

```align
label := if s > 80 { "high" } else { "low" }

kind := match shape {
  Circle(_)  => "round",
  Rect(_, _) => "boxy",
}
```

### else unwrap (unwrap-or-else for Option/Result)
```ebnf
else_expr = expr "else" (block | stmt)
```
The right-hand block/stmt either diverges (`return` etc.) or supplies a value of the same type.

```align
user := find_user(id) else return Error.NotFound
port := get_env("PORT") else { 8080 }
```

### ? propagation
```ebnf
try_expr  = expr "?"
```
`?` applies to `Result` only (enforced by type checking, `draft.md` §5). Desugared to early return + cold path in MIR (`04-mir.md`).

```align
data := fs.read_file(path)?
user: User := json.decode(data)?
```

### Method chains, field projection
```align
total := users
  .where(.active)     // .active = field selector
  .score              // .score over array<User> = field projection
  .sum()
```
`.field` has two meanings depending on context (determined by type, `03-types.md`):
- single value `u.score` → ordinary field access
- collection `users.score` → projection over each element (`array<i32>`)

### Field selector shorthand
A `.ident` at argument position is sugar for `fn x { x.ident }`.

```align
active := users.where(.active)   // == users.where(fn u { u.active })
```

### Lambdas
Matching the notation in `draft.md`, arguments have no parentheses.

```ebnf
lambda    = "fn" lambda_params? block
lambda_params = ident ("," ident)*        // types are inferred
```
```align
total := users.reduce(0, fn acc, u { acc + u.score })
ys := xs.map(fn x { x * 2 })
zero := fn { 0 }                           // no arguments
```
Distinguished from named functions (`fn ident (`) by "name + presence/absence of parentheses".

### arena / unsafe (expressions)
```ebnf
arena_expr  = "arena" block
unsafe_expr = "unsafe" block
```
```align
arena {
  data := fs.read_file(path)?
  users: array<User> := json.decode(data)?
  process(users)?
}
```

### String prefixes (template / html / json / raw)
```ebnf
str_prefixed = ("template" | "html" | "json") string_lit
             | "raw" "(" expr ")"
```
Takes a string literal containing `{ident}` interpolation, and desugars in MIR into a `write_static` / `write_value` sequence (`draft.md` §13, `04-mir.md`).

```align
msg := template "Hello {name}, score={score}"
body := html "<p>{name}</p>"
```

---

## 7. Patterns (match)

```ebnf
pattern   = "_"                           // wildcard
          | literal
          | ident                         // binding
          | path ( "(" pattern ("," pattern)* ")" )?   // variant destructuring
```
```align
match shape {
  Circle(r)     => area_circle(r),
  Rect(w, h)    => w * h,
}
```
`// OPEN:` guards (`if`), multiple patterns via `|`, and exhaustiveness-checking details.

---

## 8. Ambiguities and resolution

### struct literal vs block
Allowing a bare struct literal at the `cond` position of `if cond { ... }` makes `Foo { ... }` collide with a block (the same problem as Rust). **Resolution**: at the scrutinee position of `if` / `match` / `while`, bare struct literals are forbidden; wrap in parentheses if needed.

```align
if (Point{x:1,y:2}) == p { ... }
```

### generics `<` vs comparison
Ambiguity between `a < b` at expression position and `f<T>(x)`. **Resolution policy**: at type positions (`: type`, `fn ret`, `type_decl`, etc.) `<>` is always generics. At expression positions there is **no type-argument syntax** — so `<` at expression position is unambiguously comparison, and no lookahead/backtrack is needed.

**Settled (2026-06-22): no expression-position type-argument syntax (no turbofish).** A call's type parameters are recovered by inference — from a value argument (`json.encode(u)`) or from the expected type propagated from context (`u: User := json.decode(d)?`, flowing back through `?`). When neither supplies the type, that is a hard error directing the user to annotate the binding; an explicit `f<T>(x)` / `f::<T>(x)` form is **not** adopted. This keeps "one way" (the binding annotation is the single place a type is written), avoids importing the `<>` parse ambiguity that pushed Go to `f[T](x)` and Rust to `::<>`, and is friendlier to generate. The one residual is a *schema-selector* builtin whose type appears in neither arguments nor result (`json.validate<T>`, `json.field_table<T>`); that narrow case stays open (and may fold into `decode`). This rule scales to general generics (before M4): a return-only type parameter is supplied by the binding annotation, never a turbofish.

### type declaration vs struct literal
A type declaration (`User { id: i64 }`) appears only at top-level item position. A struct literal (`User{ id: 1 }`) appears only at expression position. They are uniquely distinguished by where they occur.

---

## 9. AST (align_ast, Rust)

Every node carries a `Span` (`align_span`). No desugaring (the written form is preserved, for use by formatter / lint). Excerpt:

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
    Expr(Expr),              // the trailing expression is held in Block.tail, not made a Stmt
}
struct Block { stmts: Vec<Stmt>, tail: Option<Box<Expr>> }

enum Expr {
    Lit(Lit),
    Path(Path),
    Unary { op: UnOp, rhs: Box<Expr> },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    Method { recv: Box<Expr>, name: Ident, args: Vec<Expr> },
    Field { recv: Box<Expr>, name: Ident },     // single vs projection decided by type checking
    FieldSelector(Ident),                       // .ident at argument position
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

`// OPEN:` operator associativity details, holding generic actual arguments in `Path`, representing error nodes (for recovery).

---

## 10. Parser implementation policy

- Hand-written recursive descent + Pratt parsing for expressions (precedence per the table in §6). Easier to build out diagnostics and error recovery than an LALR generator, and easier to handle Align's weak keywords and context dependence (e.g. struct-literal suppression).
- **Error recovery**: report multiple errors in one file. Use statement boundaries (`;`) and block boundaries (`}`) as synchronization points.
- Statement termination is normalized by the lexer into the `END` token (Go-style implicit semicolons + line-head continuation, §1). The parser decides statement boundaries by looking only at `END`, without being aware of raw newlines.
- No desugaring. `?` / `template` / field selectors / `else`-unwrap are kept in the AST as-is and expanded at the MIR stage (`04-mir.md`).

---

## 11. Open items (to be settled)

```text
- The exact set of numeric suffixes and the default type (i32? i64?)
- Whether to allow named fields in a variant (Rect { w, h })
- match guards / | multiple patterns / exhaustiveness checking
- Explicit generic-type-argument syntax at expression position (need turbofish or not)
- Type notation for function types (closures)
- String-interpolation grammar details (allow {expr}, or {ident} only)
- The collection target and form for doc comments (///)
```

These will be settled at the related milestones (`07-roadmap.md`), and reflected into `draft.md` and this document as they are decided.
