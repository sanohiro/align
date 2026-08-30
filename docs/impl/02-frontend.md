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

This document records the implemented frontend model. Inline `// OPEN:` notes are narrow syntax or
tooling refinements; §11 summarizes the current boundaries.

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

### Keywords
```text
reserved:
fn  mut  return  if  else  match  arena  unsafe
module  import  pub
true  false

contextual:
borrow  out  resource
```

Type names (`i32` etc.) and built-ins (`array` `slice` `vec` `mask` `Option` `Result` etc.) are **not reserved words**. They are treated as ordinary identifiers defined by the standard library, keeping the language core small. `template` / `html` / `json` / `raw` are string prefixes (see below): weak, context-limited keywords.

`borrow`, `out`, and `resource` lex as identifiers. The parser recognizes them only in the exact
declaration positions below. They therefore remain legal path segments and member names, including
the required `resource.from_raw` and `resource.borrow` intrinsics.

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

Numeric literals in principle **have no type**; the type is fixed by context — a binding annotation
or inference (`03-types.md`). When an explicit type is wanted in expression position, the `as`
operator provides it (`10 as i64`); there is **no literal suffix** (`10i64`), which would only
duplicate `as` (a third way to type a literal — against "one way"). The lexer accepts the radix
prefixes `0x` / `0o` / `0b` and `_` digit separators, but no trailing type suffix.

```align
x := 10        // type determined by context
y := 10 as i64 // explicit (no `10i64` suffix)
```

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
  is inserted (multi-line method chains). This includes `!=`; the lexer distinguishes it from a bare
  unary `!`, which does start a new statement after a completed previous line.
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

item        = test_decl | vis? ( fn_decl | type_decl | resource_decl | const_decl )
vis         = "pub"
```

**Import resolution (slice A — `open-questions.md` module system).** `module_decl`/`import_decl` are parsed into `File.module`/`File.imports`. sema's `collect_imports` validates each import against `BUILTIN_MODULES` (the `core.*`/`std.*` table from `draft.md` §18; unknown → error, duplicate → error) and threads the imported set into every per-function `Checker`. At a prefix-accessed builtin dispatch (`json.encode`/`json.encode_bounded`/`json.decode` → `core.json`, `fs.read_file` → `std.fs`, `io.stdout.write` → `std.io`), `require_import` errors if the module is not imported (checked once per *source* function — skipped for monomorph instances). `json.encode_bounded` is the shipped Request 12 bounded owned-result addition. The language-syntactic core (the array pipeline, `Option`/`Result`, `arena`, numeric methods, `template`) is dispatched by method/keyword and needs no import.

**Multi-file user modules (slice B1 — DONE).** The driver (`align_driver::check`) resolves user-module imports (any import not under `core`/`std`) by **filename convention**: `import geom` → `geom.align` in the entry file's directory, loaded transitively (BFS, dedup, cycle-safe), each verified to declare `module geom`. sema's `check_file` is now a one-module wrapper over `check_program(&[Module])`, which checks all modules together: functions are **per-module mangled** (`module$fn`; the entry module is unmangled, so single-file output is byte-identical and two modules may share a name), a bare call resolves in the caller's module (`resolve_local_fn`), and `mod.fn(...)` resolves cross-module with `pub` visibility (`resolve_qualified_fn`). **Nested paths (B2 — DONE):** `import util.math` → `util/math.align` (declaring `module util.math`); the driver joins import segments into a directory path, and sema's `flatten_module_path` collapses the dotted call receiver (`util.math.fn`) to resolve it. **Cross-module type export (DONE):** types are **per-module namespaced** like functions — a non-entry module's type `T` has canonical name `module$T` (entry unmangled), recorded in a `type_table` (module → bare → canonical + `pub`). `pub` exports a struct/enum; an importer names it qualified (`geom.Point`); a bare type resolves in the current module (`canonical_type_name`), so an imported type must be qualified. `StructLit.name` is a `Path` (the parser detects a dotted `Path { ident :`). **Builtin nominal aliases (Q2):** non-entry local declarations precede the closed bare alias table; its exact explicit spellings are `core.Error` (always in scope), `crypto.argon2_params` (`import std.crypto`), and `regex.regex_match` (`import std.regex`). Entry canonical collisions still reject, and `error(c)` binds directly to the builtin. Semantic interface import uses the same local-first order and no longer rejects a dependency merely for one of these local names. **Completed follow-ons:** qualified imported variant construction (`geom.Color.Red` / `geom.Color.Code(40)`), imported `pub` types in struct fields and enum payloads, and the unused-import warning are all shipped and test-pinned.

**Nominal-alias extension (implemented 2026-08-30).** The asymmetric
signature suite adds six `crypto.{rs256,es256,ed25519}_{private,public}_key` explicit spellings to
the same data-driven Q2 rule. Their bare forms are no-import fallbacks; qualified forms require
`std.crypto`. The implementation expands the parameterized sema/interface owner rather than
adding another lookup rule.

**Qualified cross-module function values (DONE 2026-07-15; mode extension required by L2).**
`mod.fn` / `a.b.fn` may be used wherever a named function is accepted, not only called directly:
every pipeline/reducer callable and a normal binding (`f := util.dbl`) resolves to the imported
`pub` function's mangled target. `NamedFnRef` retains the optional dotted module prefix; checked
resolution reuses the direct-call import/visibility contract, while quiet signature peeks constrain
literal element and fold-accumulator types without duplicate diagnostics. A local that shadows the
leftmost module segment remains a value receiver. Whole-program and per-unit paths share this sema
code, including imported effect summaries at `par_map`. L2 extends each function-value parameter
entry with `ParamMode` and both return-provenance summaries; binding and indirectly calling a
borrow/out-mode function never erases its ABI, alias contract, or result lifetime.

### Functions

```ebnf
fn_decl   = "fn" ident generics? "(" params? ")" ret? fn_body
params    = param ("," param)* ","?
param     = param_mode? ident ":" type
param_mode= "out" | "borrow" | "borrow" "mut"
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
fn inspect(borrow c: Conn) -> i64 = native_id(c)
fn advance(borrow mut rows: Rows) -> Option<Row> = internal_next(rows)
```

Parameter-mode lookahead is deterministic:

```text
out ident ":"          -> Out mode, then name
borrow ident ":"       -> Borrow mode, then name
borrow mut ident ":"   -> BorrowMut mode, then name
contextual ":"         -> contextual word is the parameter name
```

Thus `out dst: slice<u8>` is an out parameter but `out: region` names a region capability.
Likewise, `borrow: T` is a legal by-value parameter name. No whitespace heuristic is involved.

`out`, `borrow`, and `borrow mut` are mutually exclusive parameter modes. They are preserved in
the AST, exported function signature, and function-value parameter entries; a call has no mode
marker. Concrete function values also retain inferred `ReturnBorrowSummary` and
`ReturnRegionSummary` parameter/capture roots plus `ReturnCleanupAbi`. Joins union compatible
parameter roots while preserving selected target-relative capture metadata. `borrow mut` is parsed
as one mode, not as a mutable local declaration. The checking and return-provenance rules are in
`03-types.md` and `17-library-boundary-prerequisites.md` §2. Call checking compares recursive
provenance for every argument mode, including distinct Copy/Move aggregate holders, so a
`BorrowMut` operand cannot invalidate a peer argument delivered to the same call.

### Test declarations (designed 2026-08-30; implementation pending)

```ebnf
test_decl = "test" string block
```

`test` remains an identifier token. At item position, only the two-token lookahead `test` followed
by a string token commits to `test_decl`; after that commitment a missing block and every
function-like near-shape receive test-specific recovery before the next top-level item. A `pub`
followed by that same `test` + string lookahead commits to the explicitly rejected visible-test
form so its diagnostic is stable. Bare `test {}` and `pub test {}` remain valid keyword-less type
declarations, while `test` followed by any other non-string token follows the ordinary item/type
grammar rather than test recovery. The AST gains `Item::Test(TestDecl { name, body, span })`; it
does not reuse `FnDecl`, because it has no source name, visibility, parameters, generics, return
annotation, or expression body.

The decoded name is 1..=256 UTF-8 bytes and rejects exactly U+0000..U+001F and
U+007F..U+009F before canonical-id construction. The sema catalog owner pins both boundary
neighbors and the complete 1,024-byte id limit. Canonical identity retains the source's declared
module path; only an entry source without a module declaration defaults to `main`.

Depth capping visits the test block exactly as a function block. The formatter preserves the
ordinary string token and formats the block with the same block rules; the contextual word is
always spelled `test`. Sema supplies the compiler-private `Result<Unit, Error>` function shape and
the documented implicit Ok tail. With `import core.test`, qualified `test.expect` and
`test.expect_eq` remain ordinary call-shaped AST expressions; semantic context restricts them to
standalone statements in the lexical test body and ordinary nested blocks, never a lambda. The
equality form must produce exact `bool`; an ordinary vector/mask equality result is rejected rather
than reduced. Because ordinary parsing stores the final expression before `}` in `Block::tail`,
test-context sema normalizes an exact assertion there into the final statement only at root
completion or structural statement placement. Every Value edge rejects, including expected Unit.
Checked HIR therefore retains only `Stmt::Expr(TestAssert)` without changing ordinary block
parsing.

Normal commands parse and check tests after closing and freezing the complete ordinary-source HIR
prefix. Test roots and every artifact generated only while checking them append to a checked-HIR
overlay. Production lowering validates the partition but consumes only the prefix; `explain-opt`
therefore forms no overlay MIR/remark. A database Query/command constructor remains an ordinary
named top-level descriptor function formed in the prefix; test context cannot construct one, and a
malformed database-consumer overlay descriptor rejects. `db prepare` needs no test mode and tests
consume the same checked metadata offline. Catalog records retain canonical module/name identity and
source ordinal independently from overlay function symbols. Test artifact formation reserves literal
`main` for the harness, maps source `main` to the existing encoded identity, and maps catalog index
`n` to hidden `align_test$<n-as-eight-lowercase-hex>`. Production cache/codegen identity uses the complete
span-erased semantic prefix, but encodes the exact expression-ownership fact stream and semantic
static-descriptor fields rather than dropping span-keyed side-table meaning. The checked prefix still
retains current spans for diagnostics and located output. The exact grammar, name/catalog bounds,
mode split, cache identity, and closure
matrix are `core-design/test.md`; that document, not this representation summary, owns the public
contract.

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

Field names within one struct must be unique. Sema diagnoses the second occurrence before building
the field lookup/layout table; the rule applies equally to concrete and generic structs.

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

### Opaque resource declarations

```ebnf
resource_decl = "resource" ident generics? "=" path END
```

At item start, `resource ident ... "="` is a resource declaration; elsewhere `resource` is an
ordinary identifier. A recognized resource intrinsic therefore uses the normal dotted-call grammar.

```align
import pkg.db.internal.resource

pub resource conn = pkg.db.internal.resource.drop_conn
pub resource stmt<P, R> = pkg.db.internal.resource.drop_stmt
```

This is an opaque nominal Move type declaration, not a third data-type body syntax. The right-hand
path is the exactly-once Drop hook and must name a `pub` function in the declaring package's
`internal` subtree. The source hook is an ordinary function with an `unsafe {}` body; `unsafe fn`
syntax is not added. Resource resolution records a producer-owned hidden support thunk so imported
cleanup remains linkable without exposing the internal module. Resource representation operations
are compiler intrinsics restricted to the
declaring module's canonical descendant subtree and `unsafe`; they are not parsed as special
resource syntax. The Drop hook accepts only `raw` and its module need not import the declaring root,
so that privilege does not introduce a reverse module edge. Full type and lowering rules are in
`17-library-boundary-prerequisites.md` §3.

### Global constants

```ebnf
const_decl = ident (":" type)? ":=" expr END
```

A top-level `:=` is a compile-time constant (immutable). `mut` is not allowed. One of the sources feeding the const string pool (`draft.md` §12).

**Const-eval (Pass 0d).** Constants are collected and folded before the checker runs: `ConstEval` (in `align_sema`) evaluates each initializer to a `ConstVal` (memoized, order-independent, with cycle detection) and `ConstTable` maps each `module.NAME` to its `(Ty, ConstVal)`. A use site substitutes the folded value as a literal HIR node via `const_literal` (`check_path` / `check_field_access`), so a *scalar* constant never reaches MIR/codegen.

**Aggregate (array) constants (S1, 2026-07-17).** An initializer may be an array literal. `ConstEval::array` folds each element with the same evaluation as a scalar (element type inferred from the elements, or pushed down from a `slice<T>` annotation), yielding `ConstVal::Array(elems, elem)`. Unlike a scalar constant this *does* reach the backend: `const_literal` substitutes it as `hir::ExprKind::ConstArray { elems, elem, len }` typed **`slice<elem>` / `Region::Static`** (not a synthesized `ArrayLit` — that would reproduce the §8.4 alloca+stores), lowered to `mir::Rvalue::ConstArray` and then to a `[N x T]` (or `[N x {ptr,len}]` for `str`) `private unnamed_addr constant` global with a static `{ptr,len}` view. A **constant index folds to the element** in sema (`check_index`, no load); a dynamic index / `.len()` / pipeline flows through the existing borrowed-slice paths. The type gate accepts only a `slice<T>` annotation of a scalar / `str` element (an `array<T>` annotation, or a `slice<Struct>`, is rejected); struct constants / elements and non-scalar element positions (calls, `as`, nested arrays, aggregate-const refs) stay deferred. The new `ExprKind::ConstArray` / `Rvalue::ConstArray` are wired through every exhaustive HIR/MIR analysis arm (effect scan, `region_of`, escape/`slice_is_local`, `MoveCheck`, `finalize_expr`, `print`) as an inert, Copy, `Static` leaf.

Two soundness checks ride the read-only nature of the rodata view. (1) **Read-only enforcement:** a constant view (or a string literal's `.bytes()`) may not be written through — `TABLE[i] = v` or an `out slice<T>` argument would store into the `constant` global. A `readonly_locals` provenance set on the `Checker` is grown at each binding / slice reassignment whose initializer is a read-only view (`hir_is_readonly_view`, insert-only so a value read-only on any reaching path stays flagged) and checked at `check_place` (element assignment) and the `out`-argument site. (2) **Producer-side `pub`-constant surface (Pass 0d-2):** a `pub` constant's initializer may reference only `pub` constants — its value ships in the interface summary and is re-folded in importing units, so a private reference would type-check whole-program yet fail per-unit; the check (mirroring the generic `pub`-fn body rule) makes both build paths reach the same verdict.

### Registered static source inputs

A compiler-known static constructor remains an ordinary `Call` in the AST. After import/name
resolution proves the callee is the recognized constructor, the producer unit records a
`StaticInputRef` containing the constructor kind, literal argument or same-basename mode, defining
source file, and call span. A local named `db`, an unimported path, or a user function with the same
spelling does not register an input.

Sema accepts that call only when it is the complete single-expression body of a named
zero-argument, non-generic descriptor function with a static Query/command return type. The body
contains exactly one constructor; conditional, nested, repeated, helper-wrapped, block-bodied, and
ordinary expression uses fail before static-input registration. The enclosing module/function
identity is the unique Query/artifact identity.

The registered source is tagged. A file constructor records its root-relative SQL path. An inline
constructor records `Inline { query_id }`, hashes the exact decoded UTF-8 literal value, and keeps a
decoded-byte-to-defining-`.align` span map for diagnostics. Inline SQL never invents a filesystem
path; its defining `.align` file already participates in the ordinary unit source identity.

On a cold source/import identity, the driver runs import/name resolution first, resolves only those
proven `StaticInputRef` paths under the project/package root, reads exact UTF-8 bytes, and adds their
logical paths and hashes to the producer identity. Successful resolution may persist a versioned
`StaticInputManifest` keyed by that exact source/import-resolution digest. Only a matching manifest
may supply the paths before a later frontend-cache lookup; a source/import/schema mismatch discards
it and resolves again. The manifest also records each descriptor/permitted-driver checked-metadata
logical path with `Missing` or `Present(content_hash, format_version)`. A matching manifest
revalidates those exact paths before a cache hit, so creation, deletion, or change invalidates the
action without a directory scan. Thus no lexical candidate can cause a false file read or cache
hit. The compiler does not scan sibling files. SQL diagnostics use the registered `SourceMap` file
ID and byte spans. The artifact/cache split is specified in
`17-library-boundary-prerequisites.md` §§5–6.

---

## 4. Types (Type)

```ebnf
type      = path generic_args?
          | "(" ")"                       // unit
          | "(" type ("," type)+ ")"      // tuple (arity >= 2); "(" type ")" is grouping
          | fn_type
generic_args = "<" type_arg ("," type_arg)* ">"
type_arg  = type | int_literal            // the N in vec<4, f32>
fn_type   = "fn" "(" fn_type_params? ")" "->" type
fn_type_params = fn_type_param ("," fn_type_param)* ","?
fn_type_param  = param_mode? type
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

Function types preserve parameter modes:

```align
fn(i64) -> i64
fn(borrow Conn) -> i64
fn(borrow mut State, Row) -> Result<(), Error>
fn(out slice<u8>, str) -> ()
```

Within a function-type parameter list, a contextual mode is recognized only when another type
follows it. `fn(borrow) -> T` therefore uses a type named `borrow`; `fn(borrow Conn) -> T` has a
shared-borrow parameter. Mode equality is exact. Effects and return-provenance summaries remain
inferred and are not written.

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
arena_expr  = "arena" ident? block
unsafe_expr = "unsafe" block
```
```align
arena {
  data := fs.read_file(path)?
  users: array<User> := json.decode(data)?
  process(users)?
}

arena out {
  result := build(input, out)?
  use(result)
}
```

The optional identifier binds a scope-limited value of builtin type `region`. It is not a user
allocator object and is not part of the result of the arena expression.

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
pattern   = "_"                                      // wildcard
          | ident ( "(" ident ("," ident)* ")" )?    // variant + positional payload bindings
          | ident ("|" ident)+                       // bare-variant or-pattern; binds no payload
```
```align
match shape {
  Circle(r)     => area_circle(r),
  Rect(w, h)    => w * h,
}
```
Bare-variant or-patterns (`A | B`) are implemented and participate in exhaustiveness checking.
They bind no payload; write separate arms when payload names are needed. Match guards remain
outside the current grammar.

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

Every node carries a `Span` (`align_span`). No desugaring (the written form is preserved, for the lint and for the formatter's AST *assist* — the formatter is token-driven, re-emitting original token text and recovering trivia from source spans, and consults the AST only to disambiguate `<>`/unary spacing; see `open-questions.md` "Formatter"). Excerpt:

```rust
struct File { module: Option<Path>, imports: Vec<Path>, items: Vec<Item> }

enum Item {
    Fn(FnDecl),
    Test(TestDecl),
    Type(TypeDecl),
    Resource(ResourceDecl),
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
struct TestDecl { name: String, body: Block, span: Span }
enum ParamMode { ByValue, Out, Borrow, BorrowMut }
struct Param { mode: ParamMode, name: Ident, ty: Type }
struct FnTypeParam { mode: ParamMode, ty: Type }
// Type::Fn stores Vec<FnTypeParam> plus its return Type.

struct TypeDecl { vis: Vis, name: Ident, generics: Vec<GenericParam>, kind: TypeKind }
enum TypeKind { Struct(Vec<Field>), Sum(Vec<Variant>) }
struct Field { name: Ident, ty: Type }
struct Variant { name: Ident, payload: Vec<Type> }
struct ResourceDecl {
    vis: Vis,
    name: Ident,
    generics: Vec<GenericParam>,
    drop_hook: Path,
}

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
    Loop(Block),
    Arena { binding: Option<Ident>, body: Block },
    Unsafe(Block),
    TaskGroup(Block),
    Lambda { params: Vec<Ident>, body: Block },
    Template(Vec<TemplatePart>),
    Raw(Box<Expr>),
}
```

This is a compact architectural sketch, not the exhaustive current Rust enum. The implemented
`align_ast::ExprKind` also carries arrays/slices, tuples, casts, field shorthand, and other concrete
forms; `Stmt` includes `break`. Generic actual arguments at expression position were settled away.

---

## 10. Parser implementation policy

- Hand-written recursive descent + Pratt parsing for expressions (precedence per the table in §6). Easier to build out diagnostics and error recovery than an LALR generator, and easier to handle Align's weak keywords and context dependence (e.g. struct-literal suppression).
- **Error recovery**: report multiple errors in one file. Use statement boundaries (`;`) and block boundaries (`}`) as synchronization points.
- Statement termination is normalized by the lexer into the `END` token (Go-style implicit semicolons + line-head continuation, §1). The parser decides statement boundaries by looking only at `END`, without being aware of raw newlines.
- No desugaring. `?` / `template` / field selectors / `else`-unwrap are kept in the AST as-is and expanded at the MIR stage (`04-mir.md`).

---

## 11. Settled boundaries and remaining tooling work

```text
- Sum variants use the one positional payload model; structs carry named fields.
- match is exhaustive; `A | B` or-pattern alternatives are implemented and bind nothing. Guards
  and recursive/nested patterns are not part of the current surface.
- There are no expression-position generic arguments (no turbofish); context/annotations infer them.
- Lambda parameter types are inferred at a typed use site and written when a lambda is a value.
- Named-function parameters preserve `ByValue`/`Out`/`Borrow`/`BorrowMut` in interfaces.
- Resource declarations are nominal opaque owners; they do not reuse the struct/sum disambiguation.
- Plain `template` holes contain full expressions. html/raw/JSON-template variants remain deferred.
- `///` collection for generated API documentation remains future tooling work.
```

The normative syntax remains `draft.md`; this file records the frontend representation choices.
