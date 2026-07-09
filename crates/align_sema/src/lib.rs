//! Semantic analysis: name resolution + type inference/checking -> typed HIR
//! (`docs/impl/03-types.md`).
//!
//! M1 scope: integer types, `bool`, functions with parameters + calls, `if`,
//! comparison/logical operators, and `mut` reassignment. Local inference +
//! bidirectional typing. Integer literals are unconstrained inference variables fixed
//! to a concrete width by context; if still unconstrained at the end, default to `i64`
//! (`03-types.md` §2). Move/arena/effect checking is M3+.

use std::collections::HashMap;

use align_ast::{self as ast, BinOp, UnOp};
use align_diag::Diagnostics;
use align_span::Span;

pub mod hir;
pub use hir::*;

/// Integer width and sign. `i32` = `IntTy { bits: 32, signed: true }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntTy {
    pub bits: u8,
    pub signed: bool,
}

impl IntTy {
    pub fn name(&self) -> String {
        format!("{}{}", if self.signed { 'i' } else { 'u' }, self.bits)
    }
}

/// Floating-point width. `f64` = `FloatTy { bits: 64 }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloatTy {
    pub bits: u8,
}

impl FloatTy {
    pub fn name(&self) -> String {
        format!("f{}", self.bits)
    }
}

/// A variable-free scalar type — the only payloads M2 allows inside `Option`/`Result`.
/// Keeping it `Copy` and non-recursive lets [`Ty`] stay `Copy` (no boxing/interning).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scalar {
    Int(IntTy),
    Float(FloatTy),
    Bool,
    Char,
    Unit,
    /// A struct payload (the struct's id). Lets `Option`/`Result` carry a whole struct
    /// (e.g. `Result<User, Error>` from `json.decode`). No recursion — just the id.
    Struct(u32),
    /// An owned `string` payload (MMv2 slice 8a). Unlike the other scalars this is a **Move**
    /// type with a heap buffer, so an `Option<string>` / `Result<string, E>` that holds it owns
    /// that buffer: it is dropped (freed) when the aggregate local is dropped, and moved out on
    /// `?` / `else` unwrap. Lets a fallible function return an owned string
    /// (`fn f() -> Result<string, Error>`). Kept var-free (`Scalar: Copy`) — it carries no inner.
    String,
    /// An owned `array<T>` payload (MMv2 slice 8b), the owned-collection dual of [`Scalar::String`]
    /// — same `{ptr,len}` layout, Move, dropped/moved as a unit. Lets a fallible function return an
    /// owned array (`fn f() -> Result<array<i64>, Error>`). The element is a [`PrimScalar`] (not a
    /// full [`Scalar`]) so the variant stays non-recursive and `Copy`; owned arrays only ever hold
    /// primitive elements today (struct/dynamic-array elements are a later capability).
    DynArray(PrimScalar),
    /// An owned, dynamic-length array of structs (AoS), the struct dual of [`Scalar::DynArray`]
    /// (MMv2 slice 8d). Same `{ptr,len}` layout, Move, dropped/freed as a unit. Carries the struct
    /// id (non-recursive, so `Scalar` stays `Copy`). Produced by `json.decode<array<Struct>>`,
    /// whose decoded `str` fields are zero-copy views into the input — so unlike a scalar
    /// `array<T>`, a struct array is region-tied to that input and cannot escape it.
    DynStructArray(u32),
    /// A `str` view payload (`array<str>` / `slice<str>` element, `Option<str>` / `Result<str,E>`
    /// payload). A `{ptr,len}` borrow — **Copy, not Move** (no heap buffer of its own), but
    /// **region-tracked**: a composite carrying a `str` lives only as long as that `str`'s source
    /// (`tracks_region`), exactly the struct-with-`str`-field rule extended to scalars. Unlike
    /// `String`, it is never dropped (it borrows). A `box<str>` is rejected (a view is not boxable).
    Str,
    /// A sum-type payload (the enum's id) — a Copy tagged struct, like [`Scalar::Struct`]. Lets
    /// `Option`/`Result` carry an enum, notably `Result<T, MyError>` (4b). Non-recursive (just the
    /// id), so `Scalar` stays `Copy`.
    Enum(u32),
    /// A `soa<Struct>` view payload (the struct's id) — a `{ptr,len}` borrow over a column-major
    /// buffer, **Copy, not Move** but **region-tracked** (like [`Scalar::Str`]: it borrows arena
    /// storage, never dropped). Lets `Result<soa<T>, Error>` carry a decoded soa — the result type
    /// of `s: soa<User> := json.decode(d)?`. Non-recursive (just the id), so `Scalar` stays `Copy`.
    Soa(u32),
    /// A generic type parameter as a composite payload — `Option<T>` / `Result<T, E>` inside a
    /// generic template (4c-3). Carries the parameter index (like [`Ty::Param`]). Present **only**
    /// while a generic template is type-checked abstractly; monomorphization re-resolves every type
    /// with the concrete arguments, so a `Scalar::Param` never reaches MoveCheck / MIR / codegen.
    Param(u32),
    /// A `reader` payload (`Result<reader, Error>` from `fs.open`). An owned **Move** handle (an fd);
    /// the enclosing `Result` owns it and its `Drop` closes it. Opaque pointer, like [`Scalar::Str`]'s
    /// counterpart but owned.
    Reader,
    /// A `writer` payload (`Result<writer, Error>` from `fs.create`). An owned **Move** handle (an fd
    /// + buffer); the enclosing `Result`'s `Drop` flushes + closes it. Opaque pointer.
    Writer,
    /// A `buffer` payload (`Result<buffer, Error>` from `encoding.*_decode`). An owned **Move**
    /// handle (a growable byte container); the enclosing `Result`'s `Drop` frees it. Opaque pointer,
    /// like [`Scalar::Reader`]/[`Scalar::Writer`] — owned, never region-tracked (it borrows nothing).
    Buffer,
    /// A `cli parsed` payload (`Result<parsed, Error>` from `cli.command(...).parse(args)`). An owned
    /// **Move** handle (the resolved flag map); the enclosing `Result`'s `Drop` frees it. Opaque
    /// pointer, like [`Scalar::Reader`]/[`Scalar::Writer`]/[`Scalar::Buffer`] — owned, never
    /// region-tracked. (There is no `Scalar::CliCommand`: a `command` never rides an aggregate.)
    CliParsed,
    /// A `tcp_conn` payload (`Result<tcp_conn, Error>` from `tcp.connect`). An owned **Move** handle
    /// (a connected socket fd); the enclosing `Result`'s `Drop` closes it. Opaque pointer, like
    /// [`Scalar::Reader`]/[`Scalar::Writer`]/[`Scalar::Buffer`] — owned, never region-tracked.
    TcpConn,
    /// A `tcp_listener` payload (`Result<tcp_listener, Error>` from `tcp.listen`). An owned **Move**
    /// handle (a listening socket fd); the enclosing `Result`'s `Drop` closes it. Opaque pointer,
    /// like [`Scalar::TcpConn`] — owned, never region-tracked.
    TcpListener,
    /// A `udp_socket` payload (`Result<udp_socket, Error>` from `udp.bind`). An owned **Move** handle
    /// (a bound `SOCK_DGRAM` socket fd); the enclosing `Result`'s `Drop` closes it. Opaque pointer,
    /// like [`Scalar::TcpConn`]/[`Scalar::TcpListener`] — owned, never region-tracked.
    UdpSocket,
    /// A `child` payload (`Result<child, Error>` from `process.spawn`). An owned **Move** handle
    /// (a child process pid + a reaped flag); the enclosing `Result`'s `Drop` reaps it (a blocking
    /// `waitpid`, discarding the code, so it can't zombie). Opaque pointer, like [`Scalar::TcpConn`]
    /// — owned, never region-tracked. The one Move scalar backed by a pid, not an fd.
    Child,
    /// A `response` payload (`Result<response, Error>` from `http.parse`). An owned **Move** handle
    /// (one raw byte buffer + an offset table); the enclosing `Result`'s `Drop` frees it. Opaque
    /// pointer, like [`Scalar::Buffer`] — owned, never region-tracked. (There is no
    /// `Scalar::HttpRequest`: a `request` builder never rides an aggregate — it has no `Scalar`.)
    HttpResponse,
}

impl Scalar {
    /// Whether this payload scalar is an owned **Move** type (a heap buffer that the enclosing
    /// `Option`/`Result` owns and must drop / move out). Today: `string` (8a), `array<T>` (8b),
    /// the I/O handles `reader`/`writer`, a decoded `buffer`, a `cli parsed`, a `tcp_conn`, a
    /// `tcp_listener`, and a `udp_socket`.
    pub fn is_move(self) -> bool {
        matches!(self, Scalar::String | Scalar::DynArray(_) | Scalar::DynStructArray(_) | Scalar::Reader | Scalar::Writer | Scalar::Buffer | Scalar::CliParsed | Scalar::TcpConn | Scalar::TcpListener | Scalar::UdpSocket | Scalar::Child | Scalar::HttpResponse)
    }
}

/// The element of an owned-`array<T>` payload ([`Scalar::DynArray`]). A primitive scalar only —
/// a deliberately small, `Copy`, **non-recursive** subset of [`Scalar`] so an `array` can sit
/// inside an `Option`/`Result` payload without making [`Scalar`]/[`Ty`] recursive (MMv2 slice 8b).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimScalar {
    Int(IntTy),
    Float(FloatTy),
    Bool,
    Char,
    /// A `str` **view** (`{ptr,len}`) — Copy and non-recursive (it borrows, owns nothing), so it
    /// satisfies the `PrimScalar` contract even though it is not a number. Lets an owned
    /// `array<str>` (`Scalar::DynArray(Str)`) be a payload / tuple element: a buffer of `str` views,
    /// freed as a unit while its elements borrow their source. Produced by `group_by(.str_key)`.
    Str,
    /// An owned `string` (`{ptr,len}`) element — the only **Move** `PrimScalar` (non-recursive: a
    /// tag, no inner, so `PrimScalar` stays `Copy`). Lets an owned `array<string>`
    /// (`Scalar::DynArray(String)`) be a `Result` payload — the result of `fs.read_dir`. Each element
    /// owns its own buffer, so the array's `Drop` is a **deep** free (each element, then the header),
    /// distinct from every other `array<T>` (one buffer). Move-element *indexing* is still deferred
    /// project-wide (`check_index`), so today the array is used whole (`.len()`, move/return).
    String,
}

/// A [`PrimScalar`] as a full [`Scalar`] (the array element type).
pub fn prim_to_scalar(p: PrimScalar) -> Scalar {
    match p {
        PrimScalar::Int(it) => Scalar::Int(it),
        PrimScalar::Float(ft) => Scalar::Float(ft),
        PrimScalar::Bool => Scalar::Bool,
        PrimScalar::Char => Scalar::Char,
        PrimScalar::Str => Scalar::Str,
        PrimScalar::String => Scalar::String,
    }
}

/// A [`Scalar`] as a [`PrimScalar`] if it is a primitive (or a `str` view); `None` for
/// struct / string / array / soa / unit / error elements.
pub fn scalar_to_prim(s: Scalar) -> Option<PrimScalar> {
    match s {
        Scalar::Int(it) => Some(PrimScalar::Int(it)),
        Scalar::Float(ft) => Some(PrimScalar::Float(ft)),
        Scalar::Bool => Some(PrimScalar::Bool),
        Scalar::Char => Some(PrimScalar::Char),
        Scalar::Str => Some(PrimScalar::Str),
        // Owned `string` is a Move `PrimScalar` (the only one) — `array<string>` (`fs.read_dir`).
        Scalar::String => Some(PrimScalar::String),
        _ => None,
    }
}

/// Memory layout of a struct array — a property of the array *type*, so AoS-vs-SoA is decided
/// once (at the type) and threaded into field-access lowering, not re-derived per use site
/// (`open-questions.md` Open "SoA layout"). Only [`Layout::Aos`] exists today; `Layout::Soa`
/// (column-oriented, `soa array<T>`) joins at M6. Keeping it in the type **now** means adding
/// `Soa` later turns every place that must handle the new layout into a compile error — the
/// layout decision can never be silently forgotten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Array-of-structs: elements are contiguous whole structs (`[... %Struct ...]`). Field access
    /// GEPs `element, field`.
    Aos,
    /// Struct-of-arrays: one contiguous column per field in a single column-major buffer. Field
    /// access addresses `column_base(field) + index` — a `soa<T>` pipeline source. A field scan
    /// touches only the columns it reads (the cache lever).
    Soa,
}

/// sema-internal type representation (`03-types.md` §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Int(IntTy),
    /// A generic type parameter, by its index in the enclosing function's `<...>` list
    /// (`fn f<T, U>` → `T` = `Param(0)`, `U` = `Param(1)`). Present only inside a generic-function
    /// template; monomorphization substitutes each `Param(i)` with a concrete type *before* the
    /// flow analyses / MIR run, so MIR and codegen never see a `Param`.
    Param(u32),
    /// Unresolved integer (inference variable). Eventually fixed to a concrete [`IntTy`].
    IntVar(u32),
    Float(FloatTy),
    /// Unresolved float (inference variable). Eventually fixed to a concrete [`FloatTy`].
    FloatVar(u32),
    Bool,
    /// A Unicode scalar value (32-bit).
    Char,
    /// `Option<T>`; the payload is a concrete scalar (M2 restriction).
    Option(Scalar),
    /// `Result<T, E>`; both payloads are concrete scalars (M2 restriction).
    Result(Scalar, Scalar),
    /// `box<T>` — an owning heap pointer to a scalar (a Move type). M3.
    Box(Scalar),
    /// `array<T>` of a fixed length — contiguous scalars. M4 (length known from the
    /// literal; dynamic-length arrays/slices come later).
    Array(Scalar, u32),
    /// `vecN<T>` — a fixed-width SIMD vector (`vec2`/`vec4`/`vec8`/`vec16` of a numeric scalar).
    /// M6 slice 1. A **Copy**, `Static` register value (no heap, borrows nothing) laid out as the
    /// LLVM vector `<N x T>`. The element is a numeric [`Scalar`] (int or float); the width `N` is
    /// part of the type (like [`Ty::Array`]'s length). Elementwise `+`/`-`/`*`/`/` map to LLVM
    /// vector arithmetic; `dot(a, b)` reduces to the element scalar. Constructed from an array
    /// literal under a `vecN<T>` annotation. `mask`/comparisons/`select`/lanes are later slices.
    Vec(Scalar, u32),
    /// A SIMD comparison mask — `<N x i1>`, one bool lane per vector lane (M6). Produced by a
    /// comparison of two `vecN<T>` (`a > b`), consumed by `select(mask, a, b)` / `v.sum_where(mask)`.
    /// Carries the source element scalar + the width `N` — i.e. it is tied to a `vecN<T>`, so the
    /// written annotation `maskN<T>` mirrors `vecN<T>`. Copy/`Static`. The repr (`<N x i1>`) is
    /// element-independent; the element is part of the *type* for `select`/`sum_where` matching.
    Mask(Scalar, u32),
    /// A fixed-length array of structs (AoS); `(struct_id, length)`. M4.
    StructArray(u32, u32),
    /// An *owned*, dynamic-length array of structs, laid out like a slice
    /// (`{ Struct* ptr, i64 len }`) but Move and region-tracked — the dynamic struct dual of
    /// [`Ty::DynArray`] (MMv2 slice 8d). Produced by `json.decode<array<Struct>>`. Its `str`
    /// fields are zero-copy views into the decode input, so the array is region-tied to that
    /// input and dropped (buffer freed) at scope exit. Carries its [`Layout`] (AoS today; SoA at
    /// M6) — the memory layout is a property of the type, threaded into field-access lowering.
    DynStructArray(u32, Layout),
    /// `slice<T>` — a borrowed view `{ T* ptr, i64 len }` of scalar elements. Copy. M4.
    Slice(Scalar),
    /// `soa<Struct>` — a struct-of-arrays **view**: one contiguous column per field, all in a
    /// single column-major buffer addressed as `{ ptr, i64 len }` (same ABI as a slice). Copy and
    /// borrowed (M6 first cut — primitive-scalar fields only, no ownership/drop). A field
    /// projection `s.field` yields the `slice<FieldTy>` for that column, so a scan touches only the
    /// fields it reads — the cache lever that beats an array-of-structs (`draft.md` §3.4/§9). The
    /// id indexes `Program::structs`.
    Soa(u32),
    /// `array<slice<T>>` — an *owned*, dynamic-length array whose elements are `slice<T>` views
    /// (each `{ T* ptr, i64 len }`). Laid out like a slice (`{ slice* ptr, i64 count }`), Move
    /// (owns the buffer of slice headers, freed at scope exit), and region-tracked (the element
    /// slices borrow a source array, so the whole thing cannot outlive it). Produced by
    /// `chunks(n)` — the unit of chunk parallelism (`draft.md` §11). `T` is a primitive scalar.
    DynSliceArray(PrimScalar),
    /// `array<T>` — an *owned*, dynamic-length array of scalars, laid out like a slice
    /// (`{ T* ptr, i64 len }`) but Move and region-tracked. MMv2 slice 3: produced by a
    /// materializing terminal (`.to_array()`) and (this slice) arena-bump-allocated.
    DynArray(Scalar),
    /// `str` — an immutable string view `{ u8* ptr, i64 len }`. Copy. M5.
    Str,
    /// `string` — an *owned* string `{ u8* ptr, i64 len }`, laid out like `str` but Move and
    /// region-tracked (MMv2 slice 7). Produced by `str.clone()`; free-standing values own a
    /// heap buffer freed by `Drop` (the same machinery as owned `array<T>`). A `string` is
    /// readable as a `str` (a borrow of itself).
    String,
    /// An arena handle (internal; produced by `arena {}`, never written by the user).
    ArenaHandle,
    /// `raw` — an opaque, untyped raw byte pointer, the unsafe escape hatch (`raw.alloc` yields one;
    /// `raw.free` consumes one). Copy, `Static` region (not arena/region-tracked), never auto-dropped
    /// — the memory is manually managed, which is why `raw.*` ops are confined to an `unsafe {}` block.
    /// Lowers to an LLVM `ptr`, like the other opaque-pointer types.
    Raw,
    /// `builder` — an append-oriented string writer (draft.md §12), the canonical way to
    /// construct a `string` (over `a + b` concat). An opaque owned handle to a heap builder
    /// object (a Move type): `builder()` opens it, `.write(...)` appends, `.to_string()` consumes
    /// it into an owned `string`. An unfinished builder is `Drop`-freed at scope exit (MMv2 7c).
    Builder,
    /// A `writer` (`std.io`) — the one concrete write-sink Move type: `io.stdout`/`io.stderr`
    /// (unbuffered), `io.stdout.buffered()` (buffered), `fs.create` (a file). An opaque owned handle
    /// to a heap writer object owning an fd (like [`Ty::Builder`]): `.write(x)` appends, `.flush()`
    /// drains to the OS. `Drop`-freed (after a best-effort flush; a file fd is also closed). Its
    /// writes are I/O-effecting (Impure). Polymorphism lives in the constructors, not the type
    /// ("one way").
    Writer,
    /// A `reader` (`std.io`) — the one concrete read-source Move type: `io.stdin`, `fs.open` (a
    /// file). An opaque owned handle to a heap reader object owning an fd. `r.read(b: mut buffer)`
    /// fills a caller-owned buffer. `Drop`-freed (a file fd is also closed). Its reads are Impure.
    Reader,
    /// A `buffer` (`core.buffer`) — an owned, growable byte container (the byte analog of a
    /// `Vec<u8>`), the caller-owned sink `reader.read` fills. An opaque owned handle to a heap
    /// buffer object (a Move type). `buffer(cap)` opens it, `.bytes()` views its contents (a
    /// `slice<u8>` borrow), `.len()` is its byte count; `Drop`-freed. Constructing / reading it is
    /// pure (no I/O).
    Buffer,
    /// `rng` (`std.rand`) — a non-cryptographic random generator. A **Copy** state-only value (the
    /// 256-bit Xoshiro256++ state, four `i64`s) — deliberately unlike the Move `reader`/`writer`
    /// handles: it owns no external resource (no fd), so Copy is the right default. `Static` region
    /// (borrows nothing, never dropped, never Move). `rand.seed()`/`rand.seed_with(s)` produce one;
    /// `r.next()`/`r.range(lo, hi)`/`r.shuffle(out xs)`/`r.sample(xs, k)` take a **mut** receiver and
    /// advance the state in place. Laid out in LLVM as `[4 x i64]`, passed/returned by value.
    Rng,
    /// A `cli command` (`std.cli`) — the flag-registration builder from `cli.command(name)`. An
    /// owned **Move** handle (like `reader`/`writer`/`buffer`) owning its heap flag table (each entry
    /// holds an owned `string` name / default), `Drop`-freed. `c.flag_bool/str/i64(...)` register a
    /// flag (mutate in place through the handle, not consumed); `c.parse(args)` **borrows** it and
    /// yields `Result<parsed, Error>`; `c.usage()` renders it. Opaque pointer.
    CliCommand,
    /// A `cli parsed` (`std.cli`) — the outcome of `c.parse(args)`, the `Ok` payload of its
    /// `Result<parsed, Error>`. An owned **Move** handle owning the resolved name→value map (with
    /// owned `string` values), `Drop`-freed. `p.get_bool/i64/str(name)` read a flag total-ly (abort
    /// on unregistered / wrong-kind); `get_str` returns a `str` **view** into this handle's storage
    /// (region-bound to `p`). Opaque pointer.
    CliParsed,
    /// A `tcp_conn` (`std.net`) — a connected TCP socket, the `Ok` payload of `tcp.connect`'s
    /// `Result<tcp_conn, Error>`. An owned **Move** handle owning one socket fd (like `reader`/
    /// `writer`), `Drop`-freed (the fd is `close`d). Its `c.reader()`/`c.writer()` return **borrowed**
    /// M9 `reader`/`writer` over the same fd (`owns_fd: false` — only the conn closes it), region-
    /// bound to `c` so a stream cannot outlive the connection. Impure (its I/O hits the network).
    /// Opaque pointer. Polymorphism lives in the constructor, not the byte path (reuses reader/writer).
    TcpConn,
    /// A `tcp_listener` (`std.net`) — a listening TCP socket, the `Ok` payload of `tcp.listen`'s
    /// `Result<tcp_listener, Error>`. An owned **Move** handle owning one listening socket fd (like
    /// `tcp_conn`), `Drop`-freed (the fd is `close`d). Its `l.accept()` returns a new **owned**
    /// `tcp_conn` (never a borrow of the listener), so — unlike `c.reader()`/`c.writer()` — accept's
    /// result is not region-bound to the listener. Impure (its I/O hits the network). Opaque pointer.
    TcpListener,
    /// A `udp_socket` (`std.net`) — a bound `SOCK_DGRAM` (UDP) socket, the `Ok` payload of
    /// `udp.bind`'s `Result<udp_socket, Error>`. An owned **Move** handle owning one socket fd (like
    /// `tcp_conn`/`tcp_listener`), `Drop`-freed (the fd is `close`d). Its `u.send_to(...)` /
    /// `u.recv_from(...)` datagram ops each return a `Result<i64, Error>` (a byte count) — connectionless,
    /// so there is no borrowed reader/writer and no region binding. Impure. Opaque pointer.
    UdpSocket,
    /// A `child` (`std.process`) — a spawned child process, the `Ok` payload of `process.spawn`'s
    /// `Result<child, Error>`. An owned **Move** handle owning the child's pid (plus a reaped flag),
    /// `Drop`-reaped (a blocking `waitpid` discarding the code — no zombie; the documented tradeoff is
    /// that dropping a still-running child blocks). Its `ch.wait()` returns `Result<i64, Error>` (the
    /// exit code) and flips the reaped flag through the borrow so the later `Drop` is a no-op — the
    /// receiver is read, not consumed (mirrors `l.accept()`). Impure. Opaque pointer.
    Child,
    /// An `http request` (`std.http`) — the request builder from `http.request(method, url)`. An
    /// owned **Move** handle (like `reader`/`writer`/`buffer`/`cli command`) owning its method / url /
    /// header list / body buffer, `Drop`-freed. `r.header(name, value)` / `r.body(data)` mutate it in
    /// place through the handle (not consumed). Pure in this slice (no I/O — serialization is an
    /// internal codec, the network client is Slice 2). Opaque pointer. Never rides an aggregate (no
    /// `Scalar::HttpRequest`).
    HttpRequest,
    /// An `http response` (`std.http`) — a parsed HTTP/1.1 response, the `Ok` payload of `http.parse`'s
    /// `Result<response, Error>`. An owned **Move** handle owning ONE raw byte buffer + an offset table
    /// (zero-copy, http.md R1), `Drop`-freed. `resp.status()` reads the code; `resp.header(name)`
    /// (case-insensitive) returns an `Option<str>` **view** and `resp.body()` a `slice<u8>` **view**,
    /// both region-bound to `resp` (an escape past its `Drop` is a compile error, #297). Pure. Opaque
    /// pointer.
    HttpResponse,
    /// A `client` (`std.http`) — the HTTP/1.1 client handle from `http.client()`. An owned **Move**
    /// handle (like `reader`/`writer`/`tcp_conn`), `Drop`-freed. `cl.get(url)` / `cl.post(url, body)` /
    /// `cl.request(req)` each perform ONE request over one fresh `tcp_conn` (connect → send → read →
    /// parse → close) and return `Result<response, Error>` — a 4xx/5xx is `Ok` (status is data, #P2),
    /// only transport/parse failures are `Err`. `cl` is borrowed by its methods (not consumed);
    /// `request` **consumes** its `req` argument. **Impure** (network). In Slice 2 it owns no pooled
    /// conns (one connection per request); Slice 3 adds a keepalive pool behind the same surface.
    /// Opaque pointer; never rides an aggregate (no `Scalar::HttpClient`).
    HttpClient,
    /// A struct type; the id indexes `Program::structs`.
    Struct(u32),
    /// An anonymous tuple type `(T, U, ...)`; the id indexes `Program::tuples`. PR1 elements
    /// are primitive scalars (Copy, `Static`) — a tuple is Copy and never dropped/region-tied
    /// yet; owned/`str` elements are a later, additive slice.
    Tuple(u32),
    /// A first-class function value type (`fn(params) -> ret`), indexed into `Program.fn_types`.
    /// A function pointer — Copy, `Static`, no environment (non-capturing functions, slice ①).
    Fn(u32),
    /// A sum type, indexed into `Program.enums`. S1a: tag-only variants — a Copy/`Static` value
    /// represented as the variant tag (`i32`); constructed `Type.Variant`, consumed by `match`.
    Enum(u32),
    /// `Task<R>` — a handle to a spawned task's result (`task_group`, slice ④). The payload is a
    /// scalar. ④a represents it identically to `R` (eager execution); ④b makes it a real future.
    Task(Scalar),
    /// A **dictionary-encoded** AoS struct array (`s.dict_encode(.key)`, the A2 reuse rail). Carries
    /// `(struct_id, key_field)`: the source struct and the interned `str` key field. A Move,
    /// region-tracked value laid out as **three `{ptr,len}` slices** — `{ source_aos (borrowed),
    /// ids (owned i64 dense-id column), dict (owned `str` dictionary) }`. `dict_encode` pays the
    /// string interning once; a later `e.group_by(.key).<agg>(.value)` reuses the precomputed `ids`
    /// (the dense-id `align_rt_group_*_i64` path) and labels results back through `dict` — so repeated
    /// group-bys on the same key are integer-column work. Its `dict`/`source` slices borrow the source
    /// AoS, so it is region-tied to it; `Drop` frees `ids` + `dict` (not the borrowed `source`).
    DictEncoded(u32, u32),
    Unit,
    /// Type-checking error sentinel (bottom). Distinct from the `Error` *type*
    Error,
}

/// Convert a concrete scalar [`Ty`] to a [`Scalar`]; `None` for vars/composites/structs.
/// A primitive scalar type (int/float/bool/char) — the only values `raw.load`/`raw.store` move
/// through raw memory soundly in the first cut (no `str` views, no structs/aggregates).
fn is_raw_scalar(ty: Ty) -> bool {
    matches!(ty, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char)
}

pub fn ty_to_scalar(ty: Ty) -> Option<Scalar> {
    match ty {
        Ty::Int(it) => Some(Scalar::Int(it)),
        Ty::Float(ft) => Some(Scalar::Float(ft)),
        Ty::Bool => Some(Scalar::Bool),
        Ty::Char => Some(Scalar::Char),
        Ty::Unit => Some(Scalar::Unit),
        Ty::Struct(id) => Some(Scalar::Struct(id)),
        Ty::String => Some(Scalar::String),
        // An owned `array<T>` is a payload only when its element is primitive (slice 8b).
        Ty::DynArray(elem) => scalar_to_prim(elem).map(Scalar::DynArray),
        // Only an AoS array is payload-able today; an SoA array as an Option/Result payload is a
        // later concern (so `Scalar::DynStructArray` stays layout-free — always AoS).
        Ty::DynStructArray(id, Layout::Aos) => Some(Scalar::DynStructArray(id)),
        Ty::Str => Some(Scalar::Str),
        // A `reader`/`writer` owned handle as a `Result` Ok payload (`fs.open`/`fs.create`).
        Ty::Reader => Some(Scalar::Reader),
        Ty::Writer => Some(Scalar::Writer),
        // A `buffer` owned handle as a `Result` Ok payload (`encoding.*_decode`).
        Ty::Buffer => Some(Scalar::Buffer),
        // A `cli parsed` owned handle as the `Result` Ok payload of `c.parse(args)`. (A `cli command`
        // is never a payload — it has no `Scalar` and maps to `None` here.)
        Ty::CliParsed => Some(Scalar::CliParsed),
        // A `tcp_conn` owned handle as the `Result` Ok payload of `tcp.connect` / `l.accept`.
        Ty::TcpConn => Some(Scalar::TcpConn),
        // A `tcp_listener` owned handle as the `Result` Ok payload of `tcp.listen`.
        Ty::TcpListener => Some(Scalar::TcpListener),
        // A `udp_socket` owned handle as the `Result` Ok payload of `udp.bind`.
        Ty::UdpSocket => Some(Scalar::UdpSocket),
        // A `child` owned handle as the `Result` Ok payload of `process.spawn`.
        Ty::Child => Some(Scalar::Child),
        // An `http response` owned handle as the `Result` Ok payload of `http.parse`. (An `http
        // request` builder is never a payload — it has no `Scalar` and maps to `None` here.)
        Ty::HttpResponse => Some(Scalar::HttpResponse),
        // A `soa<Struct>` borrowed view can be a `Result`/`Option` payload (the `json.decode →
        // soa` result). Region-tracked, never dropped — like `Str`.
        Ty::Soa(id) => Some(Scalar::Soa(id)),
        // A sum type is a Copy value (a tagged struct of Copy fields), so it can be an
        // Option/Result payload — notably `Result<T, MyError>` with a user error enum (4b).
        Ty::Enum(id) => Some(Scalar::Enum(id)),
        // A generic type parameter as an Option/Result payload (4c-3, template mode only).
        Ty::Param(i) => Some(Scalar::Param(i)),
        _ => None,
    }
}

pub fn scalar_to_ty(s: Scalar) -> Ty {
    match s {
        Scalar::Int(it) => Ty::Int(it),
        Scalar::Float(ft) => Ty::Float(ft),
        Scalar::Bool => Ty::Bool,
        Scalar::Char => Ty::Char,
        Scalar::Unit => Ty::Unit,
        Scalar::Struct(id) => Ty::Struct(id),
        Scalar::String => Ty::String,
        Scalar::DynArray(elem) => Ty::DynArray(prim_to_scalar(elem)),
        Scalar::DynStructArray(id) => Ty::DynStructArray(id, Layout::Aos),
        Scalar::Str => Ty::Str,
        Scalar::Enum(id) => Ty::Enum(id),
        Scalar::Soa(id) => Ty::Soa(id),
        Scalar::Param(i) => Ty::Param(i),
        Scalar::Reader => Ty::Reader,
        Scalar::Writer => Ty::Writer,
        Scalar::Buffer => Ty::Buffer,
        Scalar::CliParsed => Ty::CliParsed,
        Scalar::TcpConn => Ty::TcpConn,
        Scalar::TcpListener => Ty::TcpListener,
        Scalar::UdpSocket => Ty::UdpSocket,
        Scalar::Child => Ty::Child,
        Scalar::HttpResponse => Ty::HttpResponse,
    }
}

fn scalar_name(s: Scalar) -> String {
    ty_name(scalar_to_ty(s))
}

/// Whether an `Option`/`Result` type carries an owned (Move) payload that the aggregate owns
/// — so the aggregate is itself a Move type and its drop must free that payload (MMv2 slice 8a).
pub fn payload_is_move(ty: Ty) -> bool {
    match ty {
        Ty::Option(s) => s.is_move(),
        Ty::Result(o, e) => o.is_move() || e.is_move(),
        _ => false,
    }
}

/// Whether `ty` is a tuple with at least one owned (Move) element — i.e. a Move tuple. Needs the
/// tuple table to read the element scalars. (Such tuples are restricted to temporaries in this
/// cut — returned or destructured — so they never occupy a drop slot; see `check`/`check_fn`.)
fn ty_tuple_is_move(ty: Ty, tuples: &[hir::TupleDef]) -> bool {
    matches!(ty, Ty::Tuple(id) if tuples[id as usize].elems.iter().any(|s| s.is_move()))
}

/// Parse an explicit-overflow arithmetic method name into its op and overflow mode (`core.math`).
/// `None` mode = `wrapping_*` (the default wrapping arithmetic — lowered to a plain `Binary`);
/// `Some(_)` = `saturating_*` / `checked_*`. Returns `None` for any other method name.
fn parse_int_arith(method: &str) -> Option<(BinOp, Option<hir::ArithMode>)> {
    let (prefix, opname) = method.rsplit_once('_')?;
    let op = match opname {
        "add" => BinOp::Add,
        "sub" => BinOp::Sub,
        "mul" => BinOp::Mul,
        _ => return None,
    };
    let mode = match prefix {
        "wrapping" => None,
        "saturating" => Some(hir::ArithMode::Saturating),
        "checked" => Some(hir::ArithMode::Checked),
        _ => return None,
    };
    Some((op, mode))
}

/// Whether `ty` is a Move (owned) type — used to reject capturing an owned value into a lambda
/// (slice ③ supports copy-value captures only; an owned capture needs move/region handling).
fn ty_capture_is_move(ty: Ty, structs: &[StructDef], tuples: &[hir::TupleDef]) -> bool {
    // `Task<R>` (④b) is a box in the task_group region — Move, like `box<T>`.
    matches!(ty, Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::DynSliceArray(_) | Ty::String | Ty::Builder | Ty::Box(_) | Ty::Task(_) | Ty::Writer | Ty::Reader | Ty::Buffer | Ty::CliCommand | Ty::CliParsed | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child | Ty::HttpRequest | Ty::HttpResponse | Ty::HttpClient | Ty::DictEncoded(..))
        || payload_is_move(ty)
        || ty_tuple_is_move(ty, tuples)
        || matches!(ty, Ty::Struct(id) if struct_is_move(id, structs))
}

/// Whether struct `id` (transitively) owns a heap buffer — a `string`/owned field, or a nested
/// struct that does — which makes it a **Move** type with a recursive `Drop` (Slice 3). The struct
/// graph is *meant* to be acyclic (pass 0b-2 / `struct_acyclic` reports any cycle as an error), but
/// the compiler keeps running later passes on the erroneous program, which then call this on a
/// cyclic struct — so the walk is **cycle-safe** (a `visiting` set, like `struct_acyclic`) to report
/// the error gracefully instead of overflowing the stack.
pub fn struct_is_move(id: u32, structs: &[StructDef]) -> bool {
    struct_is_move_rec(id, structs, &mut Vec::new())
}

fn struct_is_move_rec(id: u32, structs: &[StructDef], visiting: &mut Vec<u32>) -> bool {
    if visiting.contains(&id) {
        return false; // a cycle (already reported by `struct_acyclic`) — not a Move type here
    }
    visiting.push(id);
    let res = structs.get(id as usize).is_some_and(|def| def.fields.iter().any(|f| ty_owns_buffer_rec(f.ty, structs, visiting)));
    visiting.pop();
    res
}

/// Whether a value of `ty` owns a heap buffer that a `Drop` must free — used to decide a struct
/// field makes its enclosing struct a Move type. A free-standing owned collection/string/builder, an
/// `Option`/`Result` with a Move payload, or a nested Move struct. (Tuples can't be struct fields, so
/// they are not considered here; `str` is a borrow, not owned.) `visiting` carries the struct ids on
/// the current recursion path so a cyclic struct graph terminates instead of overflowing the stack.
fn ty_owns_buffer_rec(ty: Ty, structs: &[StructDef], visiting: &mut Vec<u32>) -> bool {
    matches!(ty, Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::DynSliceArray(_) | Ty::String | Ty::Builder | Ty::Writer | Ty::Reader | Ty::Buffer | Ty::CliCommand | Ty::CliParsed | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child | Ty::HttpRequest | Ty::HttpResponse | Ty::HttpClient)
        || payload_is_move(ty)
        || matches!(ty, Ty::Struct(id) if struct_is_move_rec(id, structs, visiting))
}

/// Byte threshold for the **huge struct copy** lint (`draft.md` §16): a struct passed/returned **by
/// value** above this many bytes is flagged. Two cache lines — deliberately conservative so the lint
/// prefers silence to noise and fires only on genuinely large records (≈16+ `i64` fields, or 8+
/// `str` fields). The cost it warns about is structural (a fixed-size copy at every call boundary),
/// not frequency-dependent, so unlike the allocation/clone perf lints it needs no `--profile` data.
const HUGE_STRUCT_BYTES: u64 = 128;

/// Element-count threshold for the **wasteful default element type** lint (`draft.md` §16): a
/// literal array whose element type is left to the unconstrained default (`i64` / `f64`) and has at
/// least this many elements is flagged. Rationale for 64: a narrower element (`i8`) would occupy one
/// 64-byte cache line for 64 elements, whereas the `i64` default spends 8 lines (512 bytes) — an 8×
/// memory/bandwidth cost. Below ~64 the wasted width is at most a line or two (negligible), so the
/// lint prefers silence to noise and fires only where the default plausibly costs real bandwidth.
const DEFAULT_ELEM_LITERAL_ARRAY_LEN: u32 = 64;

/// Round `n` up to the next multiple of alignment `a` (`a` a power of two, ≥ 1). The bitwise form
/// (vs `div_ceil`) is the standard branch-free align-up; the `a <= 1` guard also avoids the `a - 1`
/// underflow a stray `a == 0` would cause.
fn align_up(n: u64, a: u64) -> u64 {
    if a <= 1 { n } else { (n + a - 1) & !(a - 1) }
}

/// Natural-alignment `(size, align)` in bytes of `ty`, matching the layout codegen emits via LLVM.
/// Scalars are their machine width; a `{ptr, len}` view or owned handle
/// (`str`/`string`/array/slice/soa/box/builder/…) is two 64-bit words. Used by the huge-struct-copy
/// lint and by [`struct_size_align`], which consults it per field; the `_ => (16, 8)` fallback is a
/// safe default for any composite that is not a current struct-field type. Cycle-safe (`visiting`),
/// like [`struct_is_move`].
///
/// **Alignment parity with codegen's `field_abi_align`.** The per-field *alignment* this returns is
/// the sort key both here and in `align_codegen_llvm::logical_to_physical` use to order fields by
/// descending alignment, so the two must agree on the alignment of every **valid struct-field type**
/// (`is_field_ok`: `Int`/`Float`/`Bool`/`Char`/`Str`/`String`/nested `Struct`). They do — for that
/// domain both give width-or-8-for-a-pointer, and both take a nested struct's alignment as the max of
/// its members. The branches where they *differ* (`Unit` → here 1, there 4; `Array` → here 8, there
/// `scalar_bytes.min(8)`) are all types `is_field_ok` **rejects**, so they never reach struct
/// ordering; the divergence is unreachable, not a bug. `tests/…`/`layout_parity` (in the codegen
/// crate) pins this against the real LLVM ABI size/align so any future drift — or a new wider-aligned
/// field type (e.g. a `vecN<T>` field, 16-byte aligned) added to `is_field_ok` without updating
/// **both** functions — fails loudly. Scalars top out at 64-bit (no `i128`/`f128`), so no field is
/// wider than 8-byte aligned today.
fn ty_size_align(ty: Ty, structs: &[StructDef], visiting: &mut Vec<u32>) -> (u64, u64) {
    match ty {
        Ty::Int(it) => {
            let b = (it.bits / 8).max(1) as u64;
            (b, b)
        }
        Ty::Float(ft) => {
            let b = (ft.bits / 8).max(1) as u64;
            (b, b)
        }
        Ty::Bool => (1, 1),
        Ty::Char => (4, 4),
        Ty::Unit => (0, 1),
        Ty::Struct(id) => struct_size_align(id, structs, visiting),
        // Two 64-bit words: a `{ptr, len}` view/owned-handle, an opaque heap handle, or a fn pointer.
        // (A struct can hold only scalar / `str` fields today; the rest are a defensive default.)
        _ => (16, 8),
    }
}

/// Natural-alignment `(size, align)` of a struct as codegen lays it out (the dual of
/// [`struct_is_move`]). A non-`layout(C)` struct's fields are **reordered by descending alignment** to
/// eliminate padding (matching `logical_to_physical` in `align_codegen_llvm`); a `layout(C)` struct
/// keeps declaration order. An `align(N)` over-alignment pads the reported *size* up to `N` (a tight
/// array stride), but the reported *alignment* stays natural — the over-alignment lives at the
/// storage seam (`type_align`), not in the aggregate type. Cycle-safe.
fn struct_size_align(id: u32, structs: &[StructDef], visiting: &mut Vec<u32>) -> (u64, u64) {
    if visiting.contains(&id) {
        return (0, 1); // a cycle (already reported by `struct_acyclic`) — stop the recursion
    }
    let Some(def) = structs.get(id as usize) else {
        return (0, 1);
    };
    visiting.push(id);
    // Per-field `(size, align)` in declaration order.
    let mut fields: Vec<(u64, u64)> = def
        .fields
        .iter()
        .map(|f| {
            let (fsz, fal) = ty_size_align(f.ty, structs, visiting);
            (fsz, fal.max(1))
        })
        .collect();
    // A non-`layout(C)` struct is laid out in descending alignment (stable → declaration order on
    // ties), the same padding-eliminating order codegen emits. `layout(C)` keeps declaration order.
    if !def.c_repr {
        fields.sort_by_key(|&(_, fal)| std::cmp::Reverse(fal));
    }
    let mut size = 0u64;
    let mut align = 1u64;
    for (fsz, fal) in fields {
        size = align_up(size, fal) + fsz;
        align = align.max(fal);
    }
    // Pad the type's *size* up to its **effective** alignment — the natural aggregate alignment,
    // raised by any `align(N)` over-alignment. This is what C does, and matches codegen: an `align(N)`
    // struct gets an `[K x i8]` size-padding tail so a tight `[N x %S]` array has an over-aligned
    // element stride. Crucially the returned *alignment* stays the **natural** aggregate alignment
    // (`align`, not `effective`): the `align(N)` over-alignment is applied at the storage seam
    // (`type_align`, the alloca/global), never baked into the aggregate type, so the padding field is
    // `align 1` and the LLVM type's ABI alignment is unchanged. Reporting the natural alignment here
    // keeps this the exact dual of the LLVM type (pinned by `layout_parity`).
    let effective = def.align.map_or(align, |a| align.max(a as u64));
    visiting.pop();
    (align_up(size, effective), align)
}

/// The `(size, align)` of struct `id` as codegen lays it out (descending-alignment field order for a
/// non-`layout(C)` struct; declaration order for `layout(C)`). Public wrapper over
/// [`struct_size_align`] for the cross-crate layout-parity test in `align_codegen_llvm`, which checks
/// this against the real LLVM ABI size/alignment so the two hand-written layout computations
/// (`ty_size_align` here, `field_abi_align` there) can never silently drift.
pub fn struct_abi_layout(id: u32, structs: &[StructDef]) -> (u64, u64) {
    struct_size_align(id, structs, &mut Vec::new())
}

/// Whether `ty` is a Move (owned) type — owns a heap buffer consumed on move. Includes Move structs
/// and Move tuples; needs the struct/tuple tables to inspect composite members. The free-function
/// form (vs `MoveCheck::is_move_ty`) is shared by the field-access checker.
/// Whether a type may cross the C ABI boundary in an `extern "C"` signature (first FFI slice). A
/// plain primitive scalar (integer or float) or the opaque `raw` byte pointer — the types with an
/// obvious, stable C representation. `bool`/`char`, aggregates, and the owning collection types
/// (`str`/`string`/`array`/…) are deferred: their C mapping needs a settled layout/marshaling rule
/// (`bool` ABI width, `str`→pointer+len split, struct `layout(C)`), a later slice.
fn is_ffi_safe(ty: Ty) -> bool {
    matches!(ty, Ty::Int(_) | Ty::Float(_) | Ty::Raw)
}

/// FFI-safe as an extern **parameter**: everything [`is_ffi_safe`] accepts, plus a `str` and a
/// `slice<T>` **whose element is an FFI-safe scalar** (int/float — so `bytes` = `slice<u8>` qualifies,
/// but `slice<str>` / `slice<Struct>` do not: their element layout has no settled C representation,
/// and handing C a pointer to such a buffer would misinterpret it). A view is passed to C as its
/// **data pointer** (a `char*`/`void*`); the length is passed separately by the caller (`s.len()`)
/// when the C function needs it — matching the C idiom of adjacent `(ptr, len)` arguments, without
/// hiding an argument. A view is *not* FFI-safe as a **return** type (a bare C pointer carries no
/// length), so returns stay scalar-only.
fn is_ffi_safe_param(ty: Ty) -> bool {
    is_ffi_safe(ty)
        || ty == Ty::Str
        || matches!(ty, Ty::Slice(elem) if matches!(elem, Scalar::Int(_) | Scalar::Float(_)))
}

fn ty_is_move(ty: Ty, structs: &[StructDef], tuples: &[hir::TupleDef]) -> bool {
    matches!(ty, Ty::Box(_) | Ty::Task(_) | Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::DynSliceArray(_) | Ty::String | Ty::Builder | Ty::Writer | Ty::Reader | Ty::Buffer | Ty::CliCommand | Ty::CliParsed | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child | Ty::HttpRequest | Ty::HttpResponse | Ty::HttpClient | Ty::DictEncoded(..))
        || payload_is_move(ty)
        || ty_tuple_is_move(ty, tuples)
        || matches!(ty, Ty::Struct(id) if struct_is_move(id, structs))
}

/// The pipeline stages of a stage-bearing pipeline node (else `None`). Lets the flow analyses
/// (`MoveCheck`/`EscapeCheck`) walk stage captures — a lifted lambda's captured enclosing locals,
/// which are reads of enclosing locals and must be analyzed like any other use.
fn pipeline_stages(kind: &ExprKind) -> Option<&[Stage]> {
    match kind {
        ExprKind::ArraySum { stages, .. }
        | ExprKind::ArrayCount { stages, .. }
        | ExprKind::ArrayAnyAll { stages, .. }
        | ExprKind::ArrayMinMax { stages, .. }
        | ExprKind::ArrayReduce { stages, .. }
        | ExprKind::ArrayScan { stages, .. }
        | ExprKind::ArraySort { stages, .. }
        | ExprKind::ArraySortBy { stages, .. }
        | ExprKind::ArrayToArray { stages, .. }
        | ExprKind::ArrayMapInto { stages, .. }
        | ExprKind::ArrayPartition { stages, .. }
        | ExprKind::ArrayParMap { stages, .. } => Some(stages),
        _ => None,
    }
}

/// The capture operands carried by a pipeline's stages (a lifted lambda's captured values).
fn stage_capture_exprs(stages: &[Stage]) -> impl Iterator<Item = &Expr> {
    stages.iter().flat_map(|s| match &s.kind {
        StageKind::Map { captures, .. } | StageKind::Where { captures, .. } => captures.as_slice(),
        StageKind::Project { .. } | StageKind::WhereField { .. } => &[][..],
    })
}

/// The capture operands carried by a reducer/terminal node's own function (a lifted lambda's
/// captured values for `reduce`/`scan`/`partition`/`par_map`/`any`/`all`). The flow analyses walk
/// these like stage captures.
/// Peel a `arr[i].f0.f1…` chain (an AST `FieldAccess` spine bottoming out at an `Index`) into the
/// array expression, the index expression, and the field-name path in order. `None` if the receiver
/// is not rooted at an index (ordinary local/field-path access then handles it). Used to route a
/// nested struct-array element field access (`arr[i].a.x`) to `check_index_field`.
fn peel_index_field_chain(e: &ast::Expr) -> Option<(&ast::Expr, &ast::Expr, Vec<&ast::Ident>)> {
    match &e.kind {
        ast::ExprKind::Index { recv, index } => Some((recv, index, Vec::new())),
        ast::ExprKind::FieldAccess { recv, field } => {
            let (arr, index, mut fields) = peel_index_field_chain(recv)?;
            fields.push(field);
            Some((arr, index, fields))
        }
        _ => None,
    }
}

fn node_captures(kind: &ExprKind) -> &[Expr] {
    match kind {
        ExprKind::ArrayReduce { captures, .. }
        | ExprKind::ArrayScan { captures, .. }
        | ExprKind::ArrayPartition { captures, .. }
        | ExprKind::ArrayParMap { captures, .. }
        | ExprKind::ArraySortBy { captures, .. }
        | ExprKind::ArrayAnyAll { captures, .. } => captures,
        _ => &[],
    }
}

/// Whether a local of `ty` owns a heap buffer that must be freed by a per-binding `Drop` (when its
/// region is `Static`) — the predicate the drop set is built from. A free-standing owned
/// collection/string/builder, or an `Option`/`Result` carrying a Move payload.
fn is_owned_droppable(ty: Ty, structs: &[StructDef]) -> bool {
    // `Task<R>` (④b) is a box in the task_group region — bulk-freed with the region, never an
    // individually-dropped owned value (like `box<T>`).
    matches!(ty, Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::DynSliceArray(_) | Ty::String | Ty::Builder | Ty::Writer | Ty::Reader | Ty::Buffer | Ty::CliCommand | Ty::CliParsed | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child | Ty::HttpRequest | Ty::HttpResponse | Ty::HttpClient | Ty::DictEncoded(..))
        || payload_is_move(ty)
        // A Move struct (owns a `string`/owned field, transitively) — its `Drop` recursively frees
        // each owned field (Slice 3).
        || matches!(ty, Ty::Struct(id) if struct_is_move(id, structs))
        // A fixed array of a Move struct — dropped element-by-element (Slice 4a).
        || matches!(ty, Ty::StructArray(id, _) if struct_is_move(id, structs))
}

impl Ty {
    fn is_int_like(self) -> bool {
        matches!(self, Ty::Int(_) | Ty::IntVar(_))
    }

    fn is_float_like(self) -> bool {
        matches!(self, Ty::Float(_) | Ty::FloatVar(_))
    }

    fn is_numeric(self) -> bool {
        self.is_int_like() || self.is_float_like()
    }
}

/// A builtin generic bound — the only constraints (no user-defined trait bounds). A capability
/// hierarchy: `Num` ⊃ `Ord` ⊃ `Eq`. `Num` grants arithmetic + ordering + equality, `Ord` grants
/// ordering + equality, `Eq` grants equality, `Unconstrained` grants nothing (the parameter is
/// opaque — pass / return / store only). A concrete type argument is checked against the bound at
/// instantiation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bound {
    Unconstrained,
    Eq,
    Ord,
    Num,
}

impl Bound {
    fn grants_arith(self) -> bool {
        self == Bound::Num
    }
    fn grants_ord(self) -> bool {
        matches!(self, Bound::Ord | Bound::Num)
    }
    fn grants_eq(self) -> bool {
        matches!(self, Bound::Eq | Bound::Ord | Bound::Num)
    }
    fn name(self) -> &'static str {
        match self {
            Bound::Unconstrained => "(none)",
            Bound::Eq => "Eq",
            Bound::Ord => "Ord",
            Bound::Num => "Num",
        }
    }
    fn from_name(s: &str) -> Option<Bound> {
        match s {
            "Eq" => Some(Bound::Eq),
            "Ord" => Some(Bound::Ord),
            "Num" => Some(Bound::Num),
            _ => None,
        }
    }
    /// Whether a concrete type satisfies this bound (checked at instantiation). `Eq` = anything with
    /// `==` (int/float/char/bool/str); `Ord` = the ordered scalars (int/float/char — `str` has only
    /// `==`); `Num` = the numerics (int/float).
    fn satisfied_by(self, ty: Ty) -> bool {
        match self {
            Bound::Unconstrained => true,
            Bound::Eq => ty.is_numeric() || matches!(ty, Ty::Char | Ty::Bool | Ty::Str),
            Bound::Ord => ty.is_numeric() || ty == Ty::Char,
            Bound::Num => ty.is_numeric(),
        }
    }
}

#[derive(Clone)]
struct FnSig {
    params: Vec<Ty>,
    /// `out[i]` — whether parameter `i` is an `out` (writable, no-alias) output buffer.
    out: Vec<bool>,
    ret: Ty,
    /// Generic type-parameter names (`fn f<T, U>` → `["T", "U"]`); empty for a non-generic fn.
    /// The `params`/`ret` types may contain `Ty::Param(i)` indexing into this list.
    type_params: Vec<String>,
    /// The builtin bound declared for each type parameter (parallel to `type_params`).
    bounds: Vec<Bound>,
    /// Whether this is a foreign (`extern "C"`) declaration. A call to an extern is only valid
    /// inside an `unsafe {}` block (foreign code can violate every safe-core invariant), so
    /// `check_named_call` gates it on `unsafe_depth`.
    is_extern: bool,
}

/// A pipeline stage as collected from the AST (before type checking).
/// A stage's function argument: either a reference to a named top-level function, or an inline
/// lambda (which sema lifts to a synthetic top-level function — see [`Checker::lift_lambda`]).
enum StageFn {
    Named(ast::Ident),
    Lambda { params: Vec<ast::LambdaParam>, body: ast::Block, span: Span },
}

enum RawStage {
    Map(StageFn),
    Where(StageFn),
    WhereField(ast::Ident),
    Project(ast::Ident),
}

/// An assignable location resolved by [`Checker::check_place`].
enum Place {
    Local { id: LocalId, ty: Ty },
    Field { root: LocalId, path: Vec<u32>, ty: Ty },
    /// `base[index] = value` — an element store into a `mut` array local or an `out` slice
    /// parameter. `index` is the checked (`i64`) subscript; `elem` is the element type.
    Index { base: LocalId, index: Expr, elem: Ty },
    /// `v[lane] = value` — write one lane of a `mut vecN<T>` local (M6). `lane` is a constant in
    /// `0..N`; `elem` is the element scalar. Lowers to `v = insertelement(v, value, lane)`.
    VecLane { local: LocalId, lane: u32, elem: Ty },
    /// `base[index].f0.f1.… = value` — store the leaf field reached by `path` (length ≥ 1) of
    /// element `index` of a `mut` struct-array or soa local. `soa` selects the lowering
    /// (`StoreColumn` vs `StoreElemField`/`StoreElemFieldPtr`); `ty` is the leaf field type the value
    /// is checked against.
    ElemField { base: LocalId, index: Expr, path: Vec<u32>, struct_id: u32, soa: bool, ty: Ty },
    /// `base[index] = value` — store a whole struct value into element `index` of a `mut`
    /// struct-array or soa local. `soa` selects the lowering (per-column scatter vs aggregate slot
    /// store); the value is checked against `Ty::Struct(struct_id)`.
    Elem { base: LocalId, index: Expr, struct_id: u32, soa: bool },
    Err,
}

/// The tag of the builtin `Error` enum's `Code(i32)` variant (the generic error-code category).
/// Must match the variant order registered in `check_file`.
pub const ERROR_VARIANT_CODE: u32 = 3;

/// One source module compiled together with the others (multi-file, slice B1). The entry module is
/// the file passed on the command line; the rest are reached transitively through `import`.
pub struct Module<'f> {
    /// The module's path (`main` for the entry, else the imported name, e.g. `geom`).
    pub path: String,
    pub file: &'f ast::File,
    /// The entry module (holds `main`). Its functions keep their plain names; every other module's
    /// functions are mangled `module$fn`, so single-file programs are byte-identical and two modules
    /// may share a function name.
    pub is_entry: bool,
}

/// The codegen name of a function: plain in the entry module (so `main` stays `main` and single-file
/// programs are unchanged), `module$fn` elsewhere. Per-module mangling lets two modules define a
/// function with the same name and is what `pub`/private visibility resolution rewrites calls to.
fn mangle_fn(module: &str, is_entry: bool, name: &str) -> String {
    if is_entry {
        name.to_string()
    } else {
        format!("{module}${name}")
    }
}

/// Per-module resolution facts: each module's functions (bare name → mangled name + whether `pub`)
/// and the user modules it `import`s (so a `mod.fn()` call is resolved + visibility-checked).
#[derive(Default)]
struct ModuleInfo {
    fns: HashMap<String, (String, bool)>,
    user_imports: std::collections::HashSet<String>,
}
type ModuleTable = HashMap<String, ModuleInfo>;

/// A type declared in some module: its canonical (codegen / interner) name and whether it is `pub`
/// (visible to other modules). Types are per-module namespaced like functions.
#[derive(Clone)]
struct TypeEntry {
    canonical: String,
    is_pub: bool,
}
/// module path → (bare type name → entry). Used to resolve a bare type name in its own module and a
/// qualified `mod.Type` from an importer (with import + `pub` checks).
type ModTypes = HashMap<String, HashMap<String, TypeEntry>>;

/// Resolve a type-name path to its canonical key, applying module visibility. A single segment is a
/// type in `cur_module` (or the builtin `Error`); a dotted `mod.Type` must name an imported module
/// and a `pub` type. `None` (with a diagnostic, unless the single-segment miss is left to the
/// caller) if it does not resolve. `emit_unknown` controls whether a single-segment miss reports an
/// error here (callers that still want to try other interpretations pass `false`).
fn canonical_type_name(
    path: &ast::Path,
    cur_module: &str,
    imports: &std::collections::HashSet<String>,
    table: &ModTypes,
    emit_unknown: bool,
    span: Span,
    diags: &mut Diagnostics,
) -> Option<String> {
    let segs = &path.segments;
    let bare = segs.last().map(|s| s.name.as_str()).unwrap_or("");
    if segs.len() <= 1 {
        // `Error` is the builtin error sum type — visible everywhere, no import.
        if bare == "Error" {
            return Some("Error".to_string());
        }
        // `argon2_params` is the builtin std.crypto Argon2 parameters struct — likewise visible
        // everywhere (a reserved type name), so `argon2_params{...}` is an ordinary struct literal.
        if bare == "argon2_params" {
            return Some("argon2_params".to_string());
        }
        match table.get(cur_module).and_then(|m| m.get(bare)) {
            Some(e) => Some(e.canonical.clone()),
            None => {
                if emit_unknown {
                    diags.error(format!("unknown type: '{bare}'"), span);
                }
                None
            }
        }
    } else {
        let mut module = String::new();
        for (i, s) in segs[..segs.len() - 1].iter().enumerate() {
            if i > 0 {
                module.push('.');
            }
            module.push_str(&s.name);
        }
        if module != cur_module && !imports.contains(&module) {
            diags.error(format!("module `{module}` is not imported (add `import {module}`)"), span);
            return None;
        }
        let Some(entry) = table.get(&module).and_then(|m| m.get(bare)) else {
            diags.error(format!("no type `{bare}` in module `{module}`"), span);
            return None;
        };
        if module != cur_module && !entry.is_pub {
            diags.error(format!("type `{bare}` is private to module `{module}` (mark it `pub` to export it)"), span);
            return None;
        }
        Some(entry.canonical.clone())
    }
}

// --- top-level constants ---------------------------------------------------------------------
//
// A top-level `NAME := expr` is a **compile-time constant**: it is evaluated to a scalar / string
// value here in sema and substituted as a literal at every use, so it never reaches MIR/codegen.
// Constants are per-module namespaced like functions/types (`pub` exports; a qualified `mod.NAME`
// reaches an imported module's `pub` constant), and a constant initializer may reference other
// constants *in the same module* (cross-module references inside an initializer are deferred).

/// A folded compile-time constant value (the `Ty` travels alongside it in [`ConstTable::values`]).
#[derive(Clone, Debug)]
enum ConstVal {
    Int(i128),
    Float(f64),
    Bool(bool),
    Char(u32),
    Str(String),
}

/// A constant's declaration facts, collected before evaluation (keyed by canonical name).
struct ConstDeclInfo<'a> {
    module: &'a str,
    /// The resolved type annotation, if one was written (`NAME: i32 := …`); else `None` (inferred).
    ann_ty: Option<Ty>,
    value: &'a ast::Expr,
    span: Span,
}

/// The program's evaluated constants: a per-module bare→(canonical, pub?) lookup and the folded
/// value of each canonical constant. One `&ConstTable` is threaded into every [`Checker`].
#[derive(Default)]
struct ConstTable {
    /// module path → (bare const name → (canonical key, is_pub)).
    by_module: HashMap<String, HashMap<String, (String, bool)>>,
    /// canonical name → (type, folded value).
    values: HashMap<String, (Ty, ConstVal)>,
}

impl ConstTable {
    /// Resolve a constant reference to its folded `(ty, value)`, applying module visibility: a bare
    /// name (`module == cur`) resolves in its own module; a qualified `mod.NAME` requires `mod`
    /// imported (checked by the caller) and the constant `pub`. `Err(msg)` is a reported-style
    /// resolution failure; `Ok(None)` means "no such constant here" (let the caller try other
    /// interpretations).
    fn resolve(&self, module: &str, name: &str, cur_module: &str) -> Result<Option<(Ty, ConstVal)>, String> {
        let Some((canonical, is_pub)) = self.by_module.get(module).and_then(|m| m.get(name)) else {
            return Ok(None);
        };
        if module != cur_module && !is_pub {
            return Err(format!("constant `{name}` is private to module `{module}` (mark it `pub` to export it)"));
        }
        Ok(self.values.get(canonical).cloned())
    }
}

/// Wrap an `i128` arithmetic result into a concrete integer width (defined two's-complement wrap,
/// the same rule as runtime integer overflow — `draft.md` §5).
fn wrap_to_int(v: i128, it: IntTy) -> i128 {
    let bits = it.bits as u32;
    if bits >= 128 {
        return v;
    }
    let mask = (1i128 << bits) - 1;
    let m = v & mask;
    if it.signed && (m & (1i128 << (bits - 1))) != 0 {
        m - (1i128 << bits) // sign-extend
    } else {
        m
    }
}

/// The inclusive `[min, max]` value range of an integer type, as `i128` (wide enough to hold `u64`'s
/// max and `i64`'s min). Widths are 8/16/32/64, so the shifts stay well within `i128`.
fn int_range(it: IntTy) -> (i128, i128) {
    let bits = it.bits as u32;
    if it.signed {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    }
}

/// If `operand` (the operand of a unary `-`) is an integer literal reached through a chain of
/// further unary negations (`--128` → `Neg(Neg(Int))`; parentheses create no node, so `-(-128)`
/// peels the same way), return `(count, value)` where `count` is the total number of `-` applied
/// (including the outer one whose operand this is) and `value` is the literal's stored magnitude.
/// The literal's *effective* value is `value` sign-flipped once per `-`, i.e. negated iff `count`
/// is odd. Returns `None` when the chain bottoms out in a non-literal (a variable, a call, …).
fn peel_neg_literal(operand: &Expr) -> Option<(u32, i128)> {
    let mut count = 1u32; // the outer `-` whose operand `operand` is
    let mut node = operand;
    loop {
        match &node.kind {
            ExprKind::Unary { op: UnOp::Neg, expr } => {
                count += 1;
                node = expr;
            }
            ExprKind::Int(v) => return Some((count, *v)),
            _ => return None,
        }
    }
}

/// Evaluates top-level constant initializers to folded values, resolving same-module references
/// on demand (memoized) with cycle detection.
struct ConstEval<'a, 'd> {
    decls: &'a HashMap<String, ConstDeclInfo<'a>>,
    /// module → bare → canonical (for resolving a bare reference inside an initializer).
    by_module: &'a HashMap<String, HashMap<String, (String, bool)>>,
    values: HashMap<String, (Ty, ConstVal)>,
    in_progress: std::collections::HashSet<String>,
    diags: &'d mut Diagnostics,
}

impl<'a, 'd> ConstEval<'a, 'd> {
    /// Fold the constant named by `canonical` (memoized; recurses into referenced constants).
    fn value(&mut self, canonical: &str) -> Option<(Ty, ConstVal)> {
        if let Some(v) = self.values.get(canonical) {
            return Some(v.clone());
        }
        let decls = self.decls; // copy the `&'a` so the borrow is independent of `&mut self`
        let info = decls.get(canonical)?;
        let error = || (Ty::Error, ConstVal::Int(0));
        if !self.in_progress.insert(canonical.to_string()) {
            self.diags.error(
                format!("constant `{canonical}` is defined in terms of itself (cyclic constant)"),
                info.span,
            );
            return Some(error());
        }
        let result = self.expr(info.value, info.ann_ty, info.module);
        self.in_progress.remove(canonical);
        // Enforce the annotation type if one was written and evaluation produced a concrete type.
        let result = match (result, info.ann_ty) {
            (Some((ty, val)), Some(ann)) if ty != ann && ty != Ty::Error => {
                self.diags.error(
                    format!("constant has type {} but its value is {}", ty_name(ann), ty_name(ty)),
                    info.value.span,
                );
                Some((ann, val))
            }
            (r, _) => r,
        };
        // Memoize — folding to an `Error` sentinel on failure, returned consistently (not `None`),
        // so every reference to a bad constant resolves silently to `Ty::Error` rather than
        // cascading (the first vs. later references must behave identically).
        let result = result.unwrap_or_else(error);
        self.values.insert(canonical.to_string(), result.clone());
        Some(result)
    }

    /// Evaluate one constant-expression node. `expected` carries a numeric literal's type down from
    /// context (annotation / enclosing operand); `module` is the constant's home module (for
    /// resolving a bare reference).
    fn expr(&mut self, e: &ast::Expr, expected: Option<Ty>, module: &str) -> Option<(Ty, ConstVal)> {
        use ast::ExprKind as K;
        match &e.kind {
            K::Int(v) => {
                let ty = match expected {
                    Some(Ty::Int(it)) => Ty::Int(it),
                    Some(other) if other != Ty::Error => {
                        self.diags.error(format!("expected {}, found an integer literal", ty_name(other)), e.span);
                        return None;
                    }
                    _ => Ty::Int(IntTy { bits: 64, signed: true }), // unconstrained int defaults to i64
                };
                Some((ty, ConstVal::Int(*v)))
            }
            K::Float(v) => {
                let ty = match expected {
                    Some(Ty::Float(ft)) => Ty::Float(ft),
                    Some(other) if other != Ty::Error => {
                        self.diags.error(format!("expected {}, found a float literal", ty_name(other)), e.span);
                        return None;
                    }
                    _ => Ty::Float(FloatTy { bits: 64 }), // unconstrained float defaults to f64
                };
                Some((ty, ConstVal::Float(*v)))
            }
            K::Bool(b) => self.expect_scalar(Ty::Bool, ConstVal::Bool(*b), expected, e.span),
            K::Char(c) => self.expect_scalar(Ty::Char, ConstVal::Char(*c), expected, e.span),
            K::Str(s) => self.expect_scalar(Ty::Str, ConstVal::Str(s.clone()), expected, e.span),
            K::Unary { op, expr } => self.unary(*op, expr, expected, module, e.span),
            K::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, expected, module, e.span),
            K::Path(p) => {
                let Some(name) = single_name(p) else {
                    self.diags.error("a constant initializer may not be a qualified reference yet".to_string(), e.span);
                    return None;
                };
                let Some((canonical, _)) = self.by_module.get(module).and_then(|m| m.get(name)).cloned() else {
                    self.diags.error(format!("`{name}` is not a constant (a constant initializer may reference only literals and other constants)"), e.span);
                    return None;
                };
                let (ty, val) = self.value(&canonical)?;
                self.check_const_type(ty, expected, e.span)?;
                Some((ty, val))
            }
            _ => {
                self.diags.error(
                    "a constant initializer must be a literal, a unary/binary expression, or another constant".to_string(),
                    e.span,
                );
                None
            }
        }
    }

    fn expect_scalar(&mut self, ty: Ty, val: ConstVal, expected: Option<Ty>, span: Span) -> Option<(Ty, ConstVal)> {
        self.check_const_type(ty, expected, span)?;
        Some((ty, val))
    }

    /// A constant has a fixed type; `Some(())` if it matches `expected` (or there is none), else a
    /// reported error and `None` (no implicit numeric coercion — `draft.md` §3).
    fn check_const_type(&mut self, ty: Ty, expected: Option<Ty>, span: Span) -> Option<()> {
        if ty == Ty::Error {
            return Some(()); // a constant that failed to fold already reported; do not cascade
        }
        match expected {
            Some(exp) if exp != ty && exp != Ty::Error => {
                self.diags.error(format!("expected {}, found {}", ty_name(exp), ty_name(ty)), span);
                None
            }
            _ => Some(()),
        }
    }

    fn unary(&mut self, op: UnOp, expr: &ast::Expr, expected: Option<Ty>, module: &str, span: Span) -> Option<(Ty, ConstVal)> {
        match op {
            UnOp::Neg => {
                let (ty, val) = self.expr(expr, expected, module)?;
                match (ty, val) {
                    // Unary negation is signed; negating an unsigned type (`X: u32 := -5`) would
                    // silently two's-complement wrap and lose the sign. Reject it here too — the
                    // const-fold path is separate from the runtime `finalize_expr` check.
                    (Ty::Int(it), _) if !it.signed => {
                        self.diags.error(
                            format!("cannot apply unary `-` to the unsigned type `{}`: a negative value cannot have an unsigned type (it would silently wrap). Use a signed type, or convert explicitly with `as {}`.", ty_name(ty), ty_name(ty)),
                            span,
                        );
                        None
                    }
                    (Ty::Int(it), ConstVal::Int(v)) => Some((ty, ConstVal::Int(wrap_to_int(v.wrapping_neg(), it)))),
                    (Ty::Float(_), ConstVal::Float(v)) => Some((ty, ConstVal::Float(-v))),
                    (Ty::Error, _) => None, // operand failed to fold; do not cascade
                    _ => {
                        self.diags.error("unary '-' expects a number".to_string(), span);
                        None
                    }
                }
            }
            UnOp::Not => {
                let (ty, val) = self.expr(expr, Some(Ty::Bool), module)?;
                match (ty, val) {
                    (_, ConstVal::Bool(b)) => Some((Ty::Bool, ConstVal::Bool(!b))),
                    (Ty::Error, _) => None,
                    _ => {
                        self.diags.error("unary '!' expects a bool".to_string(), span);
                        None
                    }
                }
            }
            UnOp::BitNot => {
                let (ty, val) = self.expr(expr, expected, module)?;
                match (ty, val) {
                    (Ty::Int(it), ConstVal::Int(v)) => Some((ty, ConstVal::Int(wrap_to_int(!v, it)))),
                    (Ty::Error, _) => None,
                    _ => {
                        self.diags.error("unary '~' expects an integer".to_string(), span);
                        None
                    }
                }
            }
        }
    }

    fn binary(&mut self, op: BinOp, lhs: &ast::Expr, rhs: &ast::Expr, expected: Option<Ty>, module: &str, span: Span) -> Option<(Ty, ConstVal)> {
        use BinOp::*;
        let is_cmp = matches!(op, Eq | Ne | Lt | Le | Gt | Ge);
        let is_logic = matches!(op, And | Or);
        if is_logic {
            let (_, l) = self.expr(lhs, Some(Ty::Bool), module)?;
            let (_, r) = self.expr(rhs, Some(Ty::Bool), module)?;
            let (ConstVal::Bool(a), ConstVal::Bool(b)) = (l, r) else {
                self.diags.error("logical operators expect bools".to_string(), span);
                return None;
            };
            let v = if op == And { a && b } else { a || b };
            return self.expect_scalar(Ty::Bool, ConstVal::Bool(v), expected, span);
        }
        // Arithmetic / comparison: both operands share one numeric (or, for `==`/`!=`, scalar) type.
        // The result is that type for arithmetic, or `bool` for a comparison.
        let arith_expected = if is_cmp { None } else { expected };
        let (lty, lval) = self.expr(lhs, arith_expected, module)?;
        let (rty, rval) = self.expr(rhs, Some(lty), module)?;
        if lty == Ty::Error || rty == Ty::Error {
            return None; // an operand failed to fold; do not cascade a mismatch error
        }
        if lty != rty {
            self.diags.error(format!("operands have mismatched types: {} vs {}", ty_name(lty), ty_name(rty)), span);
            return None;
        }
        if is_cmp {
            let v = self.compare(op, &lval, &rval, span)?;
            return self.expect_scalar(Ty::Bool, ConstVal::Bool(v), expected, span);
        }
        // Arithmetic + bitwise/shift: numeric operands only (bitwise/shift integer-only).
        match (lval, rval, lty) {
            (ConstVal::Int(a), ConstVal::Int(b), Ty::Int(it)) => {
                let r = match op {
                    Add => a.wrapping_add(b),
                    Sub => a.wrapping_sub(b),
                    Mul => a.wrapping_mul(b),
                    Div | Rem => {
                        if b == 0 {
                            self.diags.error("division by zero in a constant expression".to_string(), span);
                            return None;
                        }
                        if op == Div { a.wrapping_div(b) } else { a.wrapping_rem(b) }
                    }
                    BitAnd => a & b,
                    BitOr => a | b,
                    BitXor => a ^ b,
                    // The shift amount is masked mod the bit width (the same defined behavior as
                    // codegen), so a too-large shift wraps rather than being undefined.
                    Shl => a << (b & (it.bits as i128 - 1)),
                    Shr => a >> (b & (it.bits as i128 - 1)),
                    _ => unreachable!("comparison/logic handled above"),
                };
                Some((lty, ConstVal::Int(wrap_to_int(r, it))))
            }
            (ConstVal::Float(a), ConstVal::Float(b), Ty::Float(_)) => {
                let r = match op {
                    Add => a + b,
                    Sub => a - b,
                    Mul => a * b,
                    Div => a / b,
                    Rem => a % b,
                    _ => {
                        self.diags.error("bitwise and shift operators expect integers".to_string(), span);
                        return None;
                    }
                };
                Some((lty, ConstVal::Float(r)))
            }
            _ => {
                self.diags.error(format!("arithmetic expects numbers, found {}", ty_name(lty)), span);
                None
            }
        }
    }

    fn compare(&mut self, op: BinOp, l: &ConstVal, r: &ConstVal, span: Span) -> Option<bool> {
        use std::cmp::Ordering;
        let ord: Option<Ordering> = match (l, r) {
            (ConstVal::Int(a), ConstVal::Int(b)) => Some(a.cmp(b)),
            (ConstVal::Float(a), ConstVal::Float(b)) => a.partial_cmp(b),
            (ConstVal::Char(a), ConstVal::Char(b)) => Some(a.cmp(b)),
            (ConstVal::Bool(a), ConstVal::Bool(b)) if matches!(op, BinOp::Eq | BinOp::Ne) => Some(a.cmp(b)),
            _ => {
                self.diags.error("these values are not comparable in a constant expression".to_string(), span);
                return None;
            }
        };
        let Some(ord) = ord else {
            // NaN comparison: `==`/`!=` are defined, ordering is always false.
            return Some(op == BinOp::Ne);
        };
        Some(match op {
            BinOp::Eq => ord == Ordering::Equal,
            BinOp::Ne => ord != Ordering::Equal,
            BinOp::Lt => ord == Ordering::Less,
            BinOp::Le => ord != Ordering::Greater,
            BinOp::Gt => ord == Ordering::Greater,
            BinOp::Ge => ord != Ordering::Less,
            _ => unreachable!(),
        })
    }
}

/// Analyze a single file into a typed program (the single-module entry point; tests and the
/// single-file driver path use this). Multi-file compilation goes through [`check_program`].
pub fn check_file(file: &ast::File, diags: &mut Diagnostics) -> Program {
    check_program(&[Module { path: "main".to_string(), file, is_entry: true }], diags)
}

/// Analyze a set of modules (the entry file plus its transitively-imported user modules) into one
/// typed program. Errors are pushed to `diags`.
pub fn check_program(modules: &[Module], diags: &mut Diagnostics) -> Program {
    // Pass 0a: assign a canonical id to every type (so field/sig types can refer to them regardless
    // of order). Types are **per-module namespaced** like functions: a non-entry module's type `T`
    // has canonical name `module$T` (the entry module keeps the bare `T`, so single-file programs
    // are byte-identical), and two modules may define the same type name. `type_table` records each
    // module's bare→(canonical, pub?) for name resolution (`canonical_type_name`); a bare name
    // resolves in its own module, a qualified `mod.T` requires `mod` imported and `T` `pub`.
    let mut struct_ids: HashMap<String, u32> = HashMap::new();
    let mut struct_decls: Vec<(&str, bool, &ast::StructDecl)> = Vec::new();
    let mut enum_ids: HashMap<String, u32> = HashMap::new();
    let mut enum_decls: Vec<(&str, bool, &ast::EnumDecl)> = Vec::new();
    // Generic templates (`Pair<T>` / `Opt<T>`) — kept separate from concrete structs / enums.
    let mut generic_struct_decls: Vec<(&str, bool, &ast::StructDecl)> = Vec::new();
    let mut generic_enum_decls: Vec<(&str, bool, &ast::EnumDecl)> = Vec::new();
    let mut type_table: ModTypes = HashMap::new();
    for m in modules {
        // Ensure the module has an entry even if it declares no types (so a bare lookup in it is a
        // clean miss, not a panic).
        let tt = type_table.entry(m.path.clone()).or_default();
        for item in &m.file.items {
            let (bare, vis, type_params, span) = match item {
                ast::Item::Struct(s) => (&s.name.name, s.vis, s.type_params.len(), s.span),
                ast::Item::Enum(e) => (&e.name.name, e.vis, e.type_params.len(), e.span),
                ast::Item::Fn(_) | ast::Item::Const(_) | ast::Item::Extern(_) => continue,
            };
            if bare == "Error" {
                diags.error("'Error' is a reserved type name (the builtin error sum type)".to_string(), span);
            }
            if bare == "argon2_params" {
                diags.error(
                    "'argon2_params' is a reserved type name (the builtin std.crypto Argon2 parameters struct)".to_string(),
                    span,
                );
            }
            if tt.contains_key(bare) {
                // Keep the first declaration; ignore this one so it cannot overwrite the valid
                // `type_table` / `*_ids` entry and cascade into confusing secondary errors.
                diags.error(format!("duplicate type declaration: '{bare}' in module '{}'", m.path), span);
                continue;
            }
            let canonical = mangle_fn(&m.path, m.is_entry, bare);
            tt.insert(bare.clone(), TypeEntry { canonical: canonical.clone(), is_pub: matches!(vis, ast::Vis::Pub) });
            match item {
                ast::Item::Struct(s) if type_params == 0 => {
                    struct_ids.insert(canonical, struct_decls.len() as u32);
                    struct_decls.push((m.path.as_str(), m.is_entry, s));
                }
                // A generic struct/enum is a template, monomorphized on demand by `resolve_type`; it
                // is not in `struct_ids`/`structs` (its fields carry `Ty::Param`).
                ast::Item::Struct(s) => generic_struct_decls.push((m.path.as_str(), m.is_entry, s)),
                ast::Item::Enum(e) if type_params == 0 => {
                    enum_ids.insert(canonical, enum_decls.len() as u32);
                    enum_decls.push((m.path.as_str(), m.is_entry, e));
                }
                ast::Item::Enum(e) => generic_enum_decls.push((m.path.as_str(), m.is_entry, e)),
                ast::Item::Fn(_) | ast::Item::Const(_) | ast::Item::Extern(_) => unreachable!(),
            }
        }
    }

    // The shared type-resolution interners, grown on demand as types resolve. `structs`/`enums`
    // grow with monomorph instances of generic structs / sum types; `*_mono` dedup them by name.
    let mut tuples: Vec<hir::TupleDef> = Vec::new();
    let mut fn_types: Vec<hir::FnTy> = Vec::new();
    // Reserve a fixed slot per concrete struct / enum (its `*_ids` index), filled in Pass 0b/0c.
    // Monomorph instances are appended *after* these, so a concrete def's id stays valid as the
    // tables grow.
    let mut structs: Vec<StructDef> = struct_decls
        .iter()
        .map(|(m, e, s)| StructDef { name: mangle_fn(m, *e, &s.name.name), fields: Vec::new(), align: None, c_repr: false })
        .collect();
    let mut struct_mono: HashMap<String, u32> = HashMap::new();
    let mut enums: Vec<hir::EnumDef> = enum_decls
        .iter()
        .map(|(m, e, ed)| hir::EnumDef { name: mangle_fn(m, *e, &ed.name.name), variants: Vec::new() })
        .collect();
    let mut enum_mono: HashMap<String, u32> = HashMap::new();
    // Resolution context for type-declaration passes (0b/0c, templates): a bare field/payload type
    // resolves in the declaring module; a qualified `field: other.Type` resolves against that
    // module's imports. `no_imports` is the fallback for a module with none / not in the map.
    let no_imports: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Per-module user imports (module path → imported user-module paths), so a struct field / enum
    // payload / generic-template member can name a `pub` type from another module (`field: other.T`).
    // Resolution-only: the authoritative import validation (duplicates / unknown / builtins) is the
    // module-resolution-table pass below — a bare set built here keeps it available before pass 0b.
    let known_user_modules: std::collections::HashSet<&str> = modules.iter().map(|m| m.path.as_str()).collect();
    // Keyed by `&str` borrowed from `modules` (which outlives this function) — no per-module clone.
    let imports_by_module: HashMap<&str, std::collections::HashSet<String>> = modules
        .iter()
        .map(|m| {
            let set = m
                .file
                .imports
                .iter()
                .map(path_str)
                .filter(|p| known_user_modules.contains(p.as_str()))
                .collect();
            (m.path.as_str(), set)
        })
        .collect();

    // The canonical builtin `Error` sum type (4b-2): universal categories + a generic `Code(i32)`
    // (variant order must match `ERROR_VARIANT_CODE`). Registered right after the concrete user
    // enums' reserved slots; `Error` is a reserved type name (rejected in pass 0a).
    let error_enum_id = enums.len() as u32;
    enum_ids.insert("Error".to_string(), error_enum_id);
    enums.push(hir::EnumDef {
        name: "Error".to_string(),
        variants: vec![
            hir::EnumVariant { name: "NotFound".to_string(), payload: Vec::new(), field_base: 1 },
            hir::EnumVariant { name: "Invalid".to_string(), payload: Vec::new(), field_base: 1 },
            hir::EnumVariant { name: "Denied".to_string(), payload: Vec::new(), field_base: 1 },
            hir::EnumVariant {
                name: "Code".to_string(),
                payload: vec![Scalar::Int(IntTy { bits: 32, signed: true })],
                field_base: 1,
            },
        ],
    });

    // The builtin `argon2_params` struct (M11 std.crypto Slice 5) — a plain **Copy** struct of four
    // `i64` tuning knobs for `crypto.argon2id` (`m_cost` KiB, `t_cost` iterations, `parallelism`
    // lanes, `len` output bytes). Registered like the `Error` enum above: a reserved type name
    // (rejected as a user declaration in pass 0a), visible everywhere, so `argon2_params{...}` is an
    // ordinary struct literal — the security-tuning knobs are named, never positional. It occupies a
    // reserved concrete-struct slot right after the user structs (`structs` currently holds exactly
    // the `struct_decls.len()` reserved slots — pass 0b fills only those indices, leaving this one
    // intact; monomorphs append after it, exactly as they do after the `Error` enum).
    {
        let i64_field = Ty::Int(IntTy { bits: 64, signed: true });
        struct_ids.insert("argon2_params".to_string(), structs.len() as u32);
        structs.push(StructDef {
            name: "argon2_params".to_string(),
            fields: vec![
                FieldDef { name: "m_cost".to_string(), ty: i64_field },
                FieldDef { name: "t_cost".to_string(), ty: i64_field },
                FieldDef { name: "parallelism".to_string(), ty: i64_field },
                FieldDef { name: "len".to_string(), ty: i64_field },
            ],
            align: None,
            c_repr: false,
        });
    }

    // Build the generic templates: resolve each template's fields / payloads with its type
    // parameters in scope (so `T` becomes `Ty::Param`). A template may not (yet) reference another
    // generic def, so empty template maps suffice while building them.
    let mut struct_templates: HashMap<String, StructTemplate> = HashMap::new();
    let mut enum_templates: HashMap<String, EnumTemplate> = HashMap::new();
    {
        let est: HashMap<String, StructTemplate> = HashMap::new();
        let eet: HashMap<String, EnumTemplate> = HashMap::new();
        macro_rules! build_cx {
            ($module:expr) => {
                &mut TyCx {
                    cur_module: $module,
                    imports: imports_by_module.get(*$module).unwrap_or(&no_imports),
                    type_table: &type_table,
                    struct_ids: &struct_ids,
                    enum_ids: &enum_ids,
                    struct_templates: &est,
                    structs: &mut structs,
                    struct_mono: &mut struct_mono,
                    enum_templates: &eet,
                    enums: &mut enums,
                    enum_mono: &mut enum_mono,
                    tuples: &mut tuples,
                    fn_types: &mut fn_types,
                }
            };
        }
        for (module, is_entry, s) in &generic_struct_decls {
            // `layout(C)` on a generic struct is meaningless — each monomorph is a distinct C type,
            // there is no generic C struct — and it would bypass the concrete-struct FFI-safe field
            // check (which never runs for a template), letting e.g. `Pair<str>` become a `layout(C)`
            // struct. Reject it here rather than validating per-monomorph.
            if s.c_repr {
                diags.error("a generic struct cannot be marked `layout(C)`".to_string(), s.span);
            }
            let tparams: Vec<String> = s.type_params.iter().map(|t| t.name.name.clone()).collect();
            let mut fields = Vec::with_capacity(s.fields.len());
            for f in &s.fields {
                let ty = resolve_type(&f.ty, build_cx!(module), &tparams, diags);
                fields.push(hir::FieldDef { name: f.name.name.clone(), ty });
            }
            struct_templates.insert(mangle_fn(module, *is_entry, &s.name.name), StructTemplate { type_params: tparams, fields, align: s.align, c_repr: false });
        }
        for (module, is_entry, e) in &generic_enum_decls {
            let tparams: Vec<String> = e.type_params.iter().map(|t| t.name.name.clone()).collect();
            let mut variants = Vec::with_capacity(e.variants.len());
            let mut field_base = 1u32;
            for v in &e.variants {
                let mut payload = Vec::with_capacity(v.payload.len());
                for t in &v.payload {
                    let ty = resolve_type(t, build_cx!(module), &tparams, diags);
                    match ty_to_scalar(ty) {
                        Some(s) => payload.push(s),
                        None if ty != Ty::Error => diags.error(
                            format!("variant payloads must be a scalar or a type parameter for now, got {}", ty_name(ty)),
                            t.span(),
                        ),
                        None => {}
                    }
                }
                let n = payload.len() as u32;
                variants.push(hir::EnumVariant { name: v.name.name.clone(), payload, field_base });
                field_base += n;
            }
            enum_templates.insert(mangle_fn(module, *is_entry, &e.name.name), EnumTemplate { type_params: tparams, variants });
        }
    }

    // A fresh `TyCx` borrowing the resolution interners — built per `resolve_type` call so each
    // borrow is released immediately (the macro keeps the many call sites readable). `$module` is
    // the module the type is resolved in; `$imports` are the modules it may qualify against.
    macro_rules! tcx {
        ($module:expr, $imports:expr) => {
            &mut TyCx {
                cur_module: $module,
                imports: $imports,
                type_table: &type_table,
                struct_ids: &struct_ids,
                enum_ids: &enum_ids,
                struct_templates: &struct_templates,
                structs: &mut structs,
                struct_mono: &mut struct_mono,
                enum_templates: &enum_templates,
                enums: &mut enums,
                enum_mono: &mut enum_mono,
                tuples: &mut tuples,
                fn_types: &mut fn_types,
            }
        };
    }

    // Pass 0b: resolve concrete struct field types (before enum payloads, which may be structs).
    // Each concrete struct fills its reserved slot `i`; monomorphs created while resolving a field
    // append after the reserved slots, so `structs[i]` stays the struct `struct_ids` named.
    for (i, (module, _is_entry, s)) in struct_decls.iter().enumerate() {
        let mut fields = Vec::with_capacity(s.fields.len());
        for f in &s.fields {
            let ty = resolve_type(&f.ty, tcx!(module, imports_by_module.get(*module).unwrap_or(&no_imports)), &[], diags);
            // A field is a primitive scalar (int/float/bool/char), `str`, or a nested struct (the
            // nested-struct shape is checked structurally here; that it is scalar-only + acyclic is
            // validated in pass 0b-2, once all struct fields are populated). Slice/option/box/Move
            // fields are still rejected.
            if !is_field_ok(ty) {
                diags.error(
                    format!("struct fields must be a primitive scalar, str, or a plain struct for now, got {}", ty_name(ty)),
                    f.span,
                );
            }
            // A `layout(C)` struct promises a C-compatible flat layout, so its fields must be the
            // FFI-mappable scalars (integers/floats). `bool`/`char`, `str`, and nested structs are
            // deferred (their C representation is a later slice).
            if s.c_repr && !matches!(ty, Ty::Int(_) | Ty::Float(_) | Ty::Error) {
                diags.error(
                    format!("a `layout(C)` struct field must be an integer or float (got {}) — other field types are a later FFI slice", ty_name(ty)),
                    f.span,
                );
            }
            fields.push(FieldDef { name: f.name.name.clone(), ty });
        }
        // `align(N)` over-alignment (M6): honored at the one `type_align` codegen seam (the slot
        // alloca / struct-array element). `None` = the type's natural alignment.
        structs[i] = StructDef { name: mangle_fn(module, *_is_entry, &s.name.name), fields, align: s.align, c_repr: s.c_repr };
    }

    // Pass 0b-2: now that every struct's fields are populated, validate **nested** struct fields. A
    // struct-typed field must reference an **acyclic** struct — self/mutual recursion without a `box`
    // indirection is infinite layout (Slice 3 lifts the Slice-1 scalar-only restriction: a nested
    // struct may now own a `string`, making the outer struct a Move type with a recursive `Drop`).
    // Done in a separate pass because the referenced struct may be declared after the referencing one.
    for (i, (_m, _e, s)) in struct_decls.iter().enumerate() {
        for (fi, f) in s.fields.iter().enumerate() {
            let Ty::Struct(nid) = structs[i].fields[fi].ty else { continue };
            // An `align(N)` struct embedded as a field is not honored yet — embedding needs the
            // struct's size padded up to its alignment (deferred), so the over-alignment would be
            // silently dropped. Reject it cleanly rather than mislead (only a standalone value is
            // over-aligned today).
            if structs[nid as usize].align.is_some() {
                diags.error(
                    format!("an `align(N)` struct ('{}') cannot be a struct field yet — its over-alignment is only honored for a standalone value", structs[nid as usize].name),
                    f.span,
                );
            }
            // Seed the visiting path with the containing struct `i`, so a cycle back to it (even at
            // depth 1, `Node { next: Node }`) is detected.
            if !struct_acyclic(nid, &structs, &mut vec![i as u32]) {
                diags.error(
                    format!("struct field '{}' is recursive — a struct cannot contain itself without a `box` indirection", f.name.name),
                    f.span,
                );
            }
        }
    }

    // Pass 0c: resolve concrete enum variant payloads (structs are now known) into the reserved
    // slots. The enum lowers to a non-union struct `{ i32 tag, <flattened payloads> }`; payloads are
    // primitive scalars (S1b) or a plain-data struct (S2). Monomorph instances of generic sum types
    // append after the reserved slots, so a concrete enum's id stays valid.
    for (i, (module, _is_entry, e)) in enum_decls.iter().enumerate() {
        let mut seen = std::collections::HashSet::new();
        let mut variants = Vec::with_capacity(e.variants.len());
        let mut field_base = 1u32;
        for v in &e.variants {
            if !seen.insert(v.name.name.clone()) {
                diags.error(format!("duplicate variant '{}' in '{}'", v.name.name, e.name.name), v.span);
            }
            let mut payload = Vec::with_capacity(v.payload.len());
            for t in &v.payload {
                let ty = resolve_type(t, tcx!(module, imports_by_module.get(*module).unwrap_or(&no_imports)), &[], diags);
                match ty {
                    Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char => {
                        payload.push(ty_to_scalar(ty).expect("primitive scalar"));
                    }
                    Ty::Struct(id) if struct_is_move(id, &structs) => diags.error(
                        format!("a sum-type payload may not be the Move struct '{}' yet (its owned fields would not be dropped)", structs[id as usize].name),
                        t.span(),
                    ),
                    Ty::Struct(id) if structs[id as usize].fields.iter().all(|f| f.ty != Ty::Str) => {
                        payload.push(Scalar::Struct(id));
                    }
                    Ty::Struct(_) => diags.error(
                        "a sum-type payload struct may not contain a `str` field yet (region tracking pending)".to_string(),
                        t.span(),
                    ),
                    Ty::Error => {}
                    other => diags.error(
                        format!("variant payloads must be a primitive scalar or plain struct for now, got {}", ty_name(other)),
                        t.span(),
                    ),
                }
            }
            let n = payload.len() as u32;
            variants.push(hir::EnumVariant { name: v.name.name.clone(), payload, field_base });
            field_base += n;
        }
        enums[i] = hir::EnumDef { name: mangle_fn(module, *_is_entry, &e.name.name), variants };
    }

    // Every function across all modules, tagged with its module path + whether that is the entry
    // module (so its name is unmangled). Used by passes 1 / 2 and the module-resolution table.
    let all_fns: Vec<(&str, bool, &ast::FnDecl)> = modules
        .iter()
        .flat_map(|m| {
            m.file.items.iter().filter_map(move |it| match it {
                ast::Item::Fn(f) => Some((m.path.as_str(), m.is_entry, f)),
                _ => None,
            })
        })
        .collect();

    // The module-resolution table: each module's functions (bare name → mangled name + `pub`?) and
    // the user modules it imports. A bare call resolves in the caller's own module; a `mod.fn()`
    // call resolves in `mod` (which must be imported) and requires `fn` to be `pub`. Each module's
    // imported builtin namespaces (`core.json`/`std.fs`/…) are also collected here — the
    // capability-header check needs them per file.
    let known_modules: std::collections::HashSet<&str> = modules.iter().map(|m| m.path.as_str()).collect();
    let mut mod_table: ModuleTable = HashMap::new();
    let mut mod_builtin_imports: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
    // Populate each module's function set in one linear pass over all functions.
    for &(module, is_entry, f) in &all_fns {
        let info = mod_table.entry(module.to_string()).or_default();
        let mangled = mangle_fn(module, is_entry, &f.name.name);
        let is_pub = matches!(f.vis, ast::Vis::Pub);
        if info.fns.insert(f.name.name.clone(), (mangled, is_pub)).is_some() {
            diags.error(format!("duplicate function '{}' in module '{}'", f.name.name, module), f.span);
        }
    }
    for m in modules {
        let info = mod_table.entry(m.path.clone()).or_default();
        // Validate each `import`: a builtin `core.*`/`std.*` module (recorded for the capability
        // check), a known user module (recorded for `mod.fn` resolution), or an error.
        let builtins = mod_builtin_imports.entry(m.path.clone()).or_default();
        let mut seen = std::collections::HashSet::new();
        for imp in &m.file.imports {
            let p = path_str(imp);
            if !seen.insert(p.clone()) {
                diags.error(format!("duplicate import `{p}`"), imp.span);
                continue;
            }
            if BUILTIN_MODULES.contains(&p.as_str()) {
                builtins.insert(p);
            } else if known_modules.contains(p.as_str()) {
                info.user_imports.insert(p);
            } else {
                diags.error(
                    format!("unknown module `{p}`: not a builtin `core.*` / `std.*` module, and no source file provides it"),
                    imp.span,
                );
            }
        }
    }

    // Unused-import lint (warning): an `import` whose module is never referenced in the file. A
    // builtin (`core.json`) is matched by its `json.*` namespace; a user module by its dotted path.
    for m in modules {
        let mut refs = std::collections::HashSet::new();
        collect_refs(&m.file.items, &mut refs);
        // Used iff some collected prefix equals `needle` or is `needle.` + more (allocation-free).
        let used = |needle: &str| {
            refs.iter().any(|r| r == needle || (r.starts_with(needle) && r.as_bytes().get(needle.len()) == Some(&b'.')))
        };
        let mut seen = std::collections::HashSet::new();
        for imp in &m.file.imports {
            let p = path_str(imp);
            if !seen.insert(p.clone()) {
                continue; // a duplicate import already errored above
            }
            // `std.net` is reached through its sub-namespaces (`dns`/`tcp`/`udp`/`socket`), not a
            // bare `net.*`, so it is "used" if any of those is referenced. Only the namespaces
            // that have actually shipped are listed — a user local named `tcp` must not suppress
            // the unused-import warning. Extend this list as net slices 2-4 land.
            let is_used = if p == "std.net" {
                ["dns", "tcp", "udp"].iter().any(|ns| used(ns))
            } else {
                let namespace: &str = if BUILTIN_MODULES.contains(&p.as_str()) {
                    // The accessed namespace is the prefix after the last `.` (`core.json` → `json`).
                    p.rsplit('.').next().unwrap_or(p.as_str())
                } else if known_modules.contains(p.as_str()) {
                    p.as_str() // a user module is referenced by its full dotted path
                } else {
                    continue; // an unknown import already errored above
                };
                used(namespace)
            };
            if !is_used {
                diags.push(align_diag::Diagnostic::warning(format!("unused import `{p}`"), imp.span));
            }
        }
    }

    // Pass 0d: collect top-level constants and fold them to compile-time values. A constant is
    // per-module namespaced like a function (`module$NAME` canonical, unmangled in the entry so a
    // single-file program is byte-identical); a bare name resolves in its own module, a qualified
    // `mod.NAME` in an imported module's `pub` constant. The folded value is substituted as a literal
    // at every use (`check_path` / `check_field_access`), so constants never reach MIR/codegen.
    let mut const_table = ConstTable::default();
    {
        let mut decls: HashMap<String, ConstDeclInfo> = HashMap::new();
        for m in modules {
            let by = const_table.by_module.entry(m.path.clone()).or_default();
            for item in &m.file.items {
                let ast::Item::Const(c) = item else { continue };
                let bare = &c.name.name;
                // A constant shares the value namespace with functions; a clash is ambiguous.
                if mod_table.get(&m.path).is_some_and(|mi| mi.fns.contains_key(bare)) {
                    diags.error(format!("`{bare}` is declared as both a function and a constant in module `{}`", m.path), c.span);
                    continue;
                }
                if by.contains_key(bare) {
                    diags.error(format!("duplicate constant `{bare}` in module `{}`", m.path), c.span);
                    continue;
                }
                let canonical = mangle_fn(&m.path, m.is_entry, bare);
                let ann_ty = c.ty.as_ref().map(|t| {
                    let ty = resolve_type(t, tcx!(m.path.as_str(), &no_imports), &[], diags);
                    if matches!(ty, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Str | Ty::Error) {
                        ty
                    } else {
                        diags.error(format!("a constant's type must be a scalar or `str`, got {}", ty_name(ty)), t.span());
                        Ty::Error // suppress a cascading mismatch when folding the initializer
                    }
                });
                by.insert(bare.clone(), (canonical.clone(), matches!(c.vis, ast::Vis::Pub)));
                decls.insert(canonical, ConstDeclInfo { module: m.path.as_str(), ann_ty, value: &c.value, span: c.span });
            }
        }
        let mut eval = ConstEval {
            decls: &decls,
            by_module: &const_table.by_module,
            values: HashMap::new(),
            in_progress: std::collections::HashSet::new(),
            diags,
        };
        // Evaluate every constant (memoized; order-independent — a reference folds its target first).
        let canonicals: Vec<String> = decls.keys().cloned().collect();
        for canonical in &canonicals {
            eval.value(canonical);
        }
        const_table.values = eval.values;
    }

    // Pass 1: collect function signatures so calls can resolve regardless of order. `sigs` is keyed
    // by the **mangled** name (module-qualified for non-entry modules), so every function across the
    // program has a distinct key even when two modules share a bare name.
    let mut sigs: HashMap<String, FnSig> = HashMap::new();
    for &(module, is_entry, f) in &all_fns {
        let mangled = mangle_fn(module, is_entry, &f.name.name);
        let imports = mod_table.get(module).map(|i| &i.user_imports).unwrap_or(&no_imports);
        // Generic type-parameter names (`fn f<T, U: Ord>`). Reject duplicates and names that collide
        // with a declared type, so `Param` resolution is unambiguous; resolve each builtin bound.
        let tparams: Vec<String> = f.type_params.iter().map(|t| t.name.name.clone()).collect();
        let mut bounds: Vec<Bound> = Vec::with_capacity(f.type_params.len());
        for (i, t) in f.type_params.iter().enumerate() {
            if tparams[..i].contains(&t.name.name) {
                diags.error(format!("duplicate type parameter '{}'", t.name.name), t.name.span);
            }
            if type_table.get(module).is_some_and(|m| m.contains_key(&t.name.name)) {
                diags.error(format!("type parameter '{}' shadows a declared type", t.name.name), t.name.span);
            }
            let bound = match &t.bound {
                None => Bound::Unconstrained,
                Some(id) => Bound::from_name(&id.name).unwrap_or_else(|| {
                    diags.error(format!("unknown bound '{}' (expected `Eq`, `Ord`, or `Num`)", id.name), id.span);
                    Bound::Unconstrained
                }),
            };
            bounds.push(bound);
        }
        // `main` is the program entry; it cannot be a generic template (no concrete instance would
        // be generated, so the entry point would vanish).
        if f.name.name == "main" && !tparams.is_empty() {
            diags.error("main cannot be generic".to_string(), f.span);
        }
        let mut params: Vec<Ty> = Vec::with_capacity(f.params.len());
        for p in &f.params {
            params.push(resolve_type(&p.ty, tcx!(module, imports), &tparams, diags));
        }
        // A box across a call boundary would escape its arena, so M3 forbids box
        // parameters and returns (boxes are arena-local). This also closes escape
        // holes via call results.
        for (p, ty) in f.params.iter().zip(&params) {
            if matches!(ty, Ty::Box(_)) {
                diags.error(
                    "a box cannot be a function parameter (boxes are arena-local in M3)".to_string(),
                    p.ty.span(),
                );
            }
        }
        let ret = match &f.ret {
            Some(t) => {
                let r = resolve_type(t, tcx!(module, imports), &tparams, diags);
                if matches!(r, Ty::Box(_)) {
                    diags.error(
                        "a box cannot be a function return type (it would escape its arena)".to_string(),
                        t.span(),
                    );
                }
                // A returned function value would carry a frame-local closure environment out of
                // the frame (use-after-free); deferred until closures can own a region-backed env.
                if matches!(r, Ty::Fn(_)) {
                    diags.error(
                        "returning a function value is not supported yet (a closure's environment is frame-local)".to_string(),
                        t.span(),
                    );
                }
                r
            }
            None => Ty::Unit,
        };
        let out = f.params.iter().map(|p| p.is_out).collect();
        sigs.insert(mangled, FnSig { params, out, ret, type_params: tparams, bounds, is_extern: false });
    }

    // Extern (`extern "C"`) declarations: resolve + FFI-validate each foreign signature, register it
    // in `sigs` under its bare C symbol (never mangled — a C symbol is global), make it resolvable
    // from every module (like a builtin), and collect it for codegen's external-declaration pass.
    let mut externs: Vec<hir::ExternFn> = Vec::new();
    let mut link_libs: Vec<String> = Vec::new();
    for m in modules {
        for it in &m.file.items {
            let ast::Item::Extern(blk) = it else { continue };
            if blk.abi != "C" {
                diags.error(
                    format!("unsupported ABI '{}' — only `extern \"C\"` is supported", blk.abi),
                    blk.span,
                );
            }
            // A `link("name")` clause names an external library to link (`-lname`). Validate the
            // name (a linker gets it verbatim) and dedupe into `link_libs`.
            if let Some(lib) = &blk.link {
                // A leading `-` is never a real library name; reject it so a name can never look
                // like a linker flag (defense in depth — it is already passed as a single `-l<name>`
                // argv, so it cannot inject a separate flag).
                if lib.is_empty()
                    || lib.starts_with('-')
                    || !lib.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-'))
                {
                    diags.error(
                        format!("invalid library name '{lib}' in `link(...)` — use letters, digits, and `._+-` (not starting with `-`)"),
                        blk.span,
                    );
                } else if !link_libs.contains(lib) {
                    link_libs.push(lib.clone());
                }
            }
            // Extern signatures see the top-level type namespace with no module imports (an FFI type
            // is a primitive/`raw`, never an imported user type); the entry module's context suffices.
            let imports = &no_imports;
            for sig in &blk.fns {
                let cname = sig.name.name.clone();
                let mut params: Vec<Ty> = Vec::with_capacity(sig.params.len());
                for p in &sig.params {
                    let ty = resolve_type(&p.ty, tcx!(m.path.as_str(), imports), &[], diags);
                    // `Ty::Error` already produced a diagnostic in `resolve_type` — don't pile a
                    // second "not FFI-safe" error on the same root cause. A parameter also accepts a
                    // `str`/`slice`/`bytes` view — passed to C as its data pointer (see
                    // `is_ffi_safe_param`) — and a `layout(C)` struct **by value** (SysV register
                    // passing; the ABI/target/size limits are enforced in codegen).
                    if ty != Ty::Error && !is_ffi_safe_param(ty) {
                        if let Ty::Struct(id) = ty {
                            // A struct crosses the C ABI by value only with a stable C layout, and an
                            // empty struct has no defined C representation at all.
                            if !structs[id as usize].c_repr {
                                diags.error(
                                    format!("an extern struct parameter must be `layout(C)` ('{}' is not) — mark the struct `layout(C)` so it has a stable C byte layout", ty_name(ty)),
                                    p.ty.span(),
                                );
                            } else if structs[id as usize].fields.is_empty() {
                                diags.error(
                                    format!("an empty struct ('{}') has no C ABI representation, so it cannot cross an extern boundary", ty_name(ty)),
                                    p.ty.span(),
                                );
                            }
                        } else {
                            diags.error(
                                format!("'{}' is not an FFI-safe type for an extern parameter (use an integer, float, `raw`, a `str`/`slice` view, or a `layout(C)` struct)", ty_name(ty)),
                                p.ty.span(),
                            );
                        }
                    }
                    params.push(ty);
                }
                let ret = match &sig.ret {
                    Some(t) => {
                        let r = resolve_type(t, tcx!(m.path.as_str(), imports), &[], diags);
                        // A `()` (void) return is allowed; otherwise an FFI-safe scalar or a
                        // `layout(C)` struct returned **by value** (SysV register return; codegen
                        // enforces the ABI/target/size limits). (`Ty::Error` already reported.)
                        if r != Ty::Unit && r != Ty::Error && !is_ffi_safe(r) {
                            if let Ty::Struct(id) = r {
                                if !structs[id as usize].c_repr {
                                    diags.error(
                                        format!("an extern struct return type must be `layout(C)` ('{}' is not) — mark the struct `layout(C)` so it has a stable C byte layout", ty_name(r)),
                                        t.span(),
                                    );
                                } else if structs[id as usize].fields.is_empty() {
                                    diags.error(
                                        format!("an empty struct ('{}') has no C ABI representation, so it cannot cross an extern boundary", ty_name(r)),
                                        t.span(),
                                    );
                                }
                            } else {
                                diags.error(
                                    format!("'{}' is not an FFI-safe return type for an extern (use an integer, float, `raw`, a `layout(C)` struct, or `()`)", ty_name(r)),
                                    t.span(),
                                );
                            }
                        }
                        r
                    }
                    None => Ty::Unit,
                };
                // Re-declaring the same C symbol is fine as long as the signature agrees — a C
                // symbol is global, so two modules may each declare the extern they use (like
                // repeating a C header). It is registered/collected exactly once; a *conflicting*
                // re-declaration (different signature, or a clash with a non-extern user function of
                // the same name) is an error.
                if let Some(existing) = sigs.get(&cname) {
                    if !existing.is_extern {
                        diags.error(format!("extern '{cname}' conflicts with a function of the same name"), sig.span);
                    } else if existing.params != params || existing.ret != ret {
                        diags.error(format!("extern '{cname}' re-declared with a different signature"), sig.span);
                    }
                } else {
                    sigs.insert(cname.clone(), FnSig { params: params.clone(), out: vec![false; params.len()], ret, type_params: Vec::new(), bounds: Vec::new(), is_extern: true });
                    // Make the C symbol resolvable as a bare call from every module.
                    for info in mod_table.values_mut() {
                        info.fns.entry(cname.clone()).or_insert((cname.clone(), true));
                    }
                    externs.push(hir::ExternFn { name: cname, params, ret });
                }
            }
        }
    }

    // Pass 2: check each function body. A function's inline lambdas are lifted to synthetic
    // top-level functions (`cx.lifted`) and appended to the program, so all later passes treat
    // them like ordinary named functions. A **generic** function (`fn f<T>`) is a template: it is
    // not checked here but instantiated on demand below (its body is checked per monomorph, like a
    // C++ template — an uninstantiated generic is not type-checked).
    let mut fns: Vec<hir::Fn> = Vec::new();
    // Generic-function templates by **mangled** name, for monomorphization (the worklist + every
    // call target use mangled names, so the template lookup must too). The value carries the
    // template's module so a monomorph's body resolves its own bare calls in that module.
    let generic_decls: HashMap<String, (&str, &ast::FnDecl)> = all_fns
        .iter()
        .filter(|(_, _, f)| !f.type_params.is_empty())
        .map(|&(module, is_entry, f)| (mangle_fn(module, is_entry, &f.name.name), (module, f)))
        .collect();
    let empty_imports: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Monomorphization worklist: `(generic_fn_mangled_name, concrete_type_args)`, collected from
    // every generic call and processed to a fixpoint below.
    let mut worklist: std::collections::VecDeque<(String, Vec<Ty>)> = std::collections::VecDeque::new();
    for &(module, is_entry, f) in &all_fns {
        let mangled = mangle_fn(module, is_entry, &f.name.name);
        let is_template = !f.type_params.is_empty();
        let tparams = f.type_params.iter().map(|t| t.name.name.clone()).collect();
        let bounds = sigs[&mangled].bounds.clone();
        let imported = mod_builtin_imports.get(module).unwrap_or(&empty_imports);
        let mut cx = Checker::new(diags, &sigs, &struct_ids, &enum_ids, &mut enums, &enum_templates, &mut enum_mono, error_enum_id, &mut structs, &struct_templates, &mut struct_mono, &mut tuples, &mut fn_types, tparams, bounds, Vec::new(), imported, module.to_string(), &mod_table, &type_table, mod_table.get(module).map(|i| &i.user_imports).unwrap_or(&empty_imports), &const_table);
        let mut checked = cx.check_fn(f);
        checked.name = mangled;
        let lifted = std::mem::take(&mut cx.lifted);
        if is_template {
            // A generic template is checked here only to validate its body abstractly (`T` is the
            // opaque `Ty::Param`, so an operation needing a capability — arithmetic, a field, … —
            // is rejected; the constraint model is a later slice). Its HIR carries `Param` and is
            // discarded; concrete instances are generated on demand below. Instantiations it
            // *would* trigger are rediscovered when it is itself monomorphized, so they are not
            // collected here (a never-instantiated template emits no code).
            if !lifted.is_empty() {
                diags.error(
                    format!("a generic function ('{}') may not contain a lambda or pipeline yet", f.name.name),
                    f.span,
                );
            }
        } else {
            worklist.extend(cx.instantiations);
            fns.push(checked);
            fns.extend(lifted);
        }
    }
    // Monomorphization: generate one concrete instance per distinct `(template, type_args)`, then
    // scan each instance for further generic calls (transitive). `check_generic_call` already
    // rewrote every call target to the mangled name, so nothing else needs renaming.
    let mut generated: std::collections::HashSet<String> = std::collections::HashSet::new();
    while let Some((oname, targs)) = worklist.pop_front() {
        let mangled = mangle_mono(&oname, &targs);
        if !generated.insert(mangled.clone()) {
            continue; // already instantiated
        }
        let Some(&(tmpl_module, decl)) = generic_decls.get(oname.as_str()) else { continue };
        let tparams = decl.type_params.iter().map(|t| t.name.name.clone()).collect();
        let bounds = sigs[oname.as_str()].bounds.clone();
        let imported = mod_builtin_imports.get(tmpl_module).unwrap_or(&empty_imports);
        let user_imported = mod_table.get(tmpl_module).map(|i| &i.user_imports).unwrap_or(&empty_imports);
        let mut cx = Checker::new(diags, &sigs, &struct_ids, &enum_ids, &mut enums, &enum_templates, &mut enum_mono, error_enum_id, &mut structs, &struct_templates, &mut struct_mono, &mut tuples, &mut fn_types, tparams, bounds, targs, imported, tmpl_module.to_string(), &mod_table, &type_table, user_imported, &const_table);
        let mut checked = cx.check_fn(decl);
        checked.name = mangled;
        worklist.extend(cx.instantiations);
        // A lambda / pipeline inside a generic function is already rejected when the template is
        // checked in Pass 2, so a monomorph never has lifted helpers here.
        fns.push(checked);
    }
    let mut program = Program { fns, externs, link_libs, structs, enums, tuples, fn_types };
    // Pass 3 (partial): move / use-after-move checking + arena escape checking
    // (`03-types.md` §6–§7), then derive the per-function drop set (MMv2 slice 4).
    // Destructure so the flow analyses can read `tuples` (a tuple may be region-tracked when it
    // holds a `str` element) while iterating `&mut fns`.
    let Program { fns, tuples, structs, .. } = &mut program;
    let tuples: &[hir::TupleDef] = tuples;
    let structs: &[StructDef] = structs;
    for f in fns.iter_mut() {
        MoveCheck { f, diags, tuples, structs }.check();
        let region = {
            let mut ec = EscapeCheck {
                f,
                diags,
                tuples,
                structs,
                region: std::collections::HashMap::new(),
                decl_depth: std::collections::HashMap::new(),
                local_backed_slice: std::collections::HashSet::new(),
            };
            ec.check();
            ec.region
        };
        // Every free-standing owned `array<T>` (region `Static`) is dropped at every function
        // exit. Arena-allocated ones (region `Arena(k)`) are bulk-freed by the arena, so they
        // are excluded. A moved-out local stays in this set, but MIR nulls its slot at the move
        // site (null-on-move drop flag), so its exit `Drop` is a no-op `free(null)` — no
        // double-free, and the path where it is *not* moved is still freed (no leak).
        let drops: Vec<LocalId> = f
            .locals
            .iter()
            .filter(|l| is_owned_droppable(l.ty, structs) || ty_tuple_is_move(l.ty, tuples))
            // Drop at function exit unless the value lives in an arena (region `Arena(k)`), which
            // bulk-frees it. A `Static` (heap-owned) *or* `Frame`-region owned local (a Move-struct
            // array, whose owned buffers die at this frame's exit — Slice 4b) is dropped here.
            // Exception: a `chunks` result (`DynSliceArray`) is *always* heap-`malloc`'d (never
            // arena memory — `align_rt_chunks` uses the general allocator), so it must be dropped
            // even when its **region** is `Arena(k)`. Its region tracks the *borrowed source* array
            // (so it can't escape that arena), not where its own header buffer lives — decoupled
            // here so a chunks bound inside an arena is freed rather than leaked.
            .filter(|l| {
                matches!(l.ty, Ty::DynSliceArray(_))
                    || !matches!(region.get(&l.id).copied().unwrap_or(Region::Static), Region::Arena(_))
            })
            .map(|l| l.id)
            .collect();
        f.drop_locals = drops;
    }
    // Pass 4: effect/purity inference + the `par_map` Pure requirement (`draft.md` §11).
    check_parallelism(&program, diags);
    program
}

/// Effect/purity inference + the rule that a `par_map` function must be **Pure** (`draft.md` §11,
/// a Settled decision). A function is **Impure** iff it (transitively) performs an observable
/// side effect — calling `print` / `io.stdout.write` / `fs.read_file`, or calling an Impure
/// function. Everything else (arithmetic, field/array reads, builder/arena/heap use, owned-value
/// moves) is Pure. A `par_map(f)` whose `f` is Impure is rejected. (`f` is `(T) -> R` with no `out`
/// parameter, so reaching a side effect is the only way it can be Impure — sound for the language
/// as it stands.)
fn check_parallelism(program: &Program, diags: &mut Diagnostics) {
    use std::collections::HashMap;
    // Per function: directly observable effect + the set of functions it calls (incl. pipeline
    // stage/reducer functions) + the `par_map` callees to verify.
    let mut direct: HashMap<&str, bool> = HashMap::new();
    let mut calls: HashMap<&str, Vec<String>> = HashMap::new();
    let mut parmaps: Vec<(String, Span)> = Vec::new();
    for f in &program.fns {
        let mut scan = EffectScan { impure_direct: false, calls: Vec::new(), parmaps: Vec::new() };
        scan.block(&f.body);
        direct.insert(f.name.as_str(), scan.impure_direct);
        calls.insert(f.name.as_str(), scan.calls);
        parmaps.extend(scan.parmaps);
    }
    // Transitive propagation: build a reverse call graph (callee -> callers)
    // and propagate impurity starting from directly impure functions using a worklist.
    let mut reverse_calls: HashMap<&str, Vec<&str>> = HashMap::new();
    for f in &program.fns {
        if let Some(callees) = calls.get(f.name.as_str()) {
            for callee in callees {
                reverse_calls.entry(callee.as_str()).or_default().push(f.name.as_str());
            }
        }
    }

    let mut impure = std::collections::HashSet::new();
    let mut worklist = Vec::new();

    for (name, &is_direct_impure) in &direct {
        if is_direct_impure {
            let n = name.to_string();
            impure.insert(n.clone());
            worklist.push(n);
        }
    }

    while let Some(callee) = worklist.pop() {
        if let Some(callers) = reverse_calls.get(callee.as_str()) {
            for caller in callers {
                if impure.insert(caller.to_string()) {
                    worklist.push(caller.to_string());
                }
            }
        }
    }
    // The `par_map` function must be Pure.
    for (func, span) in parmaps {
        if impure.contains(&func) {
            diags.error(
                format!("'par_map' requires a Pure function, but '{func}' has a side effect (it reads/writes I/O); use `reduce` for an accumulation"),
                span,
            );
        }
    }
}

/// Walks a function body to collect its directly-observable effect, the functions it calls (incl.
/// pipeline stage/reducer functions), and any `par_map` callees. The match is exhaustive, so no
/// call edge or effect node can be silently missed.
struct EffectScan {
    impure_direct: bool,
    calls: Vec<String>,
    parmaps: Vec<(String, Span)>,
}

impl EffectScan {
    fn stage_funcs(&mut self, stages: &[Stage]) {
        for s in stages {
            match &s.kind {
                StageKind::Map { func, captures } | StageKind::Where { func, captures } => {
                    self.calls.push(func.clone());
                    // Capture operands are reads of enclosing locals — walk them so no call edge /
                    // effect they might contain is missed (exhaustiveness).
                    for c in captures {
                        self.expr(c);
                    }
                }
                StageKind::Project { .. } | StageKind::WhereField { .. } => {}
            }
        }
    }

    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            match s {
                Stmt::Let { init, .. } | Stmt::Assign { value: init, .. } | Stmt::AssignField { value: init, .. } | Stmt::LetTuple { init, .. } => self.expr(init),
                Stmt::AssignIndex { index, value, .. }
                | Stmt::AssignElemField { index, value, .. }
                | Stmt::AssignElem { index, value, .. } => {
                    self.expr(index);
                    self.expr(value);
                }
                Stmt::AssignVecLane { value, .. } => self.expr(value),
                Stmt::Return(Some(e)) | Stmt::Expr(e) => self.expr(e),
                Stmt::Return(None) => {}
            }
        }
        if let Some(v) = &b.value {
            self.expr(v);
        }
    }

    fn expr(&mut self, e: &Expr) {
        // A reducer node may carry capture operands (a lifted lambda's captured enclosing locals);
        // walk them so no call edge / effect they contain is missed. (Stage captures are walked by
        // `stage_funcs`.)
        for c in node_captures(&e.kind) {
            self.expr(c);
        }
        match &e.kind {
            // Observable side effects.
            ExprKind::Call { func, args, .. } => {
                if func == "print" {
                    self.impure_direct = true;
                } else {
                    self.calls.push(func.clone());
                }
                for a in args {
                    self.expr(a);
                }
            }
            // Taking a function's address (`g := loud`) is not itself a call, but it exposes that
            // function's effects to any later indirect call (`g(x)`) — which reaches `CallFnValue`
            // with a `callee` whose target is not statically known. So record a call edge to the
            // referenced function here: its impurity then propagates through the call graph, closing
            // the `par_map`-purity bypass where an impure function is laundered through a fn value.
            ExprKind::FnValue(name) => self.calls.push(name.clone()),
            ExprKind::Closure { captures, .. } => {
                for c in captures {
                    self.expr(c);
                }
            }
            ExprKind::CallFnValue { callee, args } => {
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
            }
            // Constructing a `writer`/`reader`/`buffer` is allocation only (no I/O → pure, like
            // `BuilderNew`); the reads/writes below reach the OS, so those are impure.
            ExprKind::WriterStd { .. } | ExprKind::ReaderStdin | ExprKind::BufferNew { .. } => {}
            ExprKind::WriterWrite { writer, arg, .. } => {
                self.impure_direct = true;
                self.expr(writer);
                self.expr(arg);
            }
            ExprKind::WriterFlush { writer } => {
                self.impure_direct = true;
                self.expr(writer);
            }
            ExprKind::ReaderRead { reader, buffer } => {
                self.impure_direct = true;
                self.expr(reader);
                self.expr(buffer);
            }
            ExprKind::IoCopy { reader, writer } => {
                self.impure_direct = true;
                self.expr(reader);
                self.expr(writer);
            }
            // `.bytes()` / `.len()` on a buffer read owned memory — pure (no I/O), like a field read.
            ExprKind::BufferBytes { buffer } | ExprKind::BufferLen { buffer } => self.expr(buffer),
            ExprKind::FsReadFile { path } | ExprKind::ReaderOpen { path } | ExprKind::WriterCreate { path }
            | ExprKind::FsExists { path } | ExprKind::FsRemove { path } | ExprKind::FsReadDir { path }
            | ExprKind::FsReadFileView { path } => {
                self.impure_direct = true;
                self.expr(path);
            }
            // `dns.resolve(host)` is impure (a name-resolution syscall) — excluded from `par_map`.
            ExprKind::DnsResolve { host } => {
                self.impure_direct = true;
                self.expr(host);
            }
            // `tcp.connect(host, port)` is impure (DNS + connect syscalls) — excluded from `par_map`.
            ExprKind::TcpConnect { host, port } => {
                self.impure_direct = true;
                self.expr(host);
                self.expr(port);
            }
            // `c.reader()` / `c.writer()` just wrap the conn's fd (no syscall — the I/O happens on the
            // returned reader/writer's `read`/`write`, already impure), like `io.stdout`: walk `conn`.
            ExprKind::ConnReader { conn } | ExprKind::ConnWriter { conn } => self.expr(conn),
            // `tcp.listen(host, port)` is impure (DNS + bind/listen syscalls) — excluded from `par_map`.
            ExprKind::TcpListen { host, port } => {
                self.impure_direct = true;
                self.expr(host);
                self.expr(port);
            }
            // `l.accept()` is impure (a blocking accept syscall) — excluded from `par_map`.
            ExprKind::TcpAccept { listener } => {
                self.impure_direct = true;
                self.expr(listener);
            }
            // `udp.bind` / `u.send_to` / `u.recv_from` are impure (bind/sendto/recvfrom syscalls) —
            // excluded from `par_map`.
            ExprKind::UdpBind { host, port } => {
                self.impure_direct = true;
                self.expr(host);
                self.expr(port);
            }
            ExprKind::UdpSendTo { sock, data, host, port } => {
                self.impure_direct = true;
                self.expr(sock);
                self.expr(data);
                self.expr(host);
                self.expr(port);
            }
            ExprKind::UdpRecvFrom { sock, buffer } => {
                self.impure_direct = true;
                self.expr(sock);
                self.expr(buffer);
            }
            ExprKind::FsWriteFile { path, data, .. } => {
                self.impure_direct = true;
                self.expr(path);
                self.expr(data);
            }
            // `std.path` ops are pure lexical string manipulation (no OS access) — like a field read.
            ExprKind::PathComponent { path, .. } | ExprKind::PathNormalize { path } => self.expr(path),
            ExprKind::PathJoin { a, b } => {
                self.expr(a);
                self.expr(b);
            }
            // `std.env` / `std.time` observe/mutate external state — Impure.
            ExprKind::EnvGet { name } => {
                self.impure_direct = true;
                self.expr(name);
            }
            ExprKind::EnvSet { name, value } => {
                self.impure_direct = true;
                self.expr(name);
                self.expr(value);
            }
            ExprKind::TimeNow | ExprKind::TimeInstant => self.impure_direct = true,
            ExprKind::TimeSleep { ns } => {
                self.impure_direct = true;
                self.expr(ns);
            }
            // `std.process` — `exit`/`abort` terminate the process (observable external effect), so
            // both are Impure: an `exit`/`abort` inside a `par_map` closure is rejected (not Pure).
            ExprKind::ProcessExit { code } => {
                self.impure_direct = true;
                self.expr(code);
            }
            ExprKind::ProcessAbort => self.impure_direct = true,
            // `process.spawn` (fork+exec) / `ch.wait()` (waitpid) are impure — excluded from `par_map`.
            ExprKind::ProcessSpawn { cmd, args } => {
                self.impure_direct = true;
                self.expr(cmd);
                self.expr(args);
            }
            ExprKind::ChildWait { child } => {
                self.impure_direct = true;
                self.expr(child);
            }
            // `ch.kill(sig)` (signal delivery) / `process.exec` (execvp) are impure — excluded from
            // `par_map`.
            ExprKind::ChildKill { child, sig } => {
                self.impure_direct = true;
                self.expr(child);
                self.expr(sig);
            }
            ExprKind::ProcessExec { cmd, args } => {
                self.impure_direct = true;
                self.expr(cmd);
                self.expr(args);
            }
            // `std.encoding` transforms are pure byte computations (no I/O) — recurse into the view.
            ExprKind::EncodingEncode { data, .. } | ExprKind::Utf8Valid { data } => self.expr(data),
            ExprKind::EncodingDecode { input, .. } => self.expr(input),
            // `std.compress` — a C-engine (libz) call, inferred **Impure** (draft §15: any
            // extern-calling fn is non-Pure), so a compress/decompress-using closure is rejected by
            // `par_map`. Recurse into the operands.
            ExprKind::Compress { data, level, .. } => {
                self.impure_direct = true;
                self.expr(data);
                self.expr(level);
            }
            ExprKind::Decompress { data, .. } => {
                self.impure_direct = true;
                self.expr(data);
            }
            // `std.rand` — **all impure**: `seed()` reads OS entropy; `seed_with`/`next`/`range`/
            // `shuffle`/`sample` produce or advance mutable RNG state. So an rng-using closure is
            // never `Pure` and is excluded from `par_map` (each thread would need its own generator).
            ExprKind::RandSeed => self.impure_direct = true,
            ExprKind::RandSeedWith { seed } => {
                self.impure_direct = true;
                self.expr(seed);
            }
            ExprKind::RandNext { rng } => {
                self.impure_direct = true;
                self.expr(rng);
            }
            ExprKind::RandRange { rng, lo, hi } => {
                self.impure_direct = true;
                self.expr(rng);
                self.expr(lo);
                self.expr(hi);
            }
            ExprKind::RandShuffle { rng, xs, .. } => {
                self.impure_direct = true;
                self.expr(rng);
                self.expr(xs);
            }
            ExprKind::RandSample { rng, xs, k, .. } => {
                self.impure_direct = true;
                self.expr(rng);
                self.expr(xs);
                self.expr(k);
            }
            // `std.cli` — **all pure** (no I/O; argv is already captured by `main(args)`): just
            // recurse into the operands so any effect *inside* them is still counted.
            ExprKind::CliCommand { name } => self.expr(name),
            ExprKind::CliFlag { cmd, name, default, .. } => {
                self.expr(cmd);
                self.expr(name);
                if let Some(d) = default {
                    self.expr(d);
                }
            }
            ExprKind::CliParse { cmd, args } => {
                self.expr(cmd);
                self.expr(args);
            }
            ExprKind::CliGetBool { parsed, name } | ExprKind::CliGetI64 { parsed, name } | ExprKind::CliGetStr { parsed, name } => {
                self.expr(parsed);
                self.expr(name);
            }
            ExprKind::CliUsage { cmd } => self.expr(cmd),
            // `std.http` (Slice 1) — **all pure** (no I/O; serialize/parse operate on owned/borrowed
            // memory — the network client is Slice 2): recurse so an effect *inside* the operands is
            // still counted.
            ExprKind::HttpRequest { method, url } => {
                self.expr(method);
                self.expr(url);
            }
            ExprKind::HttpHeader { req, name, value } => {
                self.expr(req);
                self.expr(name);
                self.expr(value);
            }
            ExprKind::HttpBody { req, data } => {
                self.expr(req);
                self.expr(data);
            }
            ExprKind::HttpParse { data } => self.expr(data),
            ExprKind::HttpRespStatus { resp } | ExprKind::HttpRespBody { resp } => self.expr(resp),
            ExprKind::HttpRespHeader { resp, name } => {
                self.expr(resp);
                self.expr(name);
            }
            // `std.http` (Slice 2) — `http.client()` allocates a handle (no I/O — **Pure**), but every
            // request op (`get`/`post`/`request`) hits the network (connect/write/read syscalls), so it
            // is **Impure** — excluded from `par_map`, like `tcp.connect` / `io`.
            ExprKind::HttpClient => {}
            ExprKind::HttpClientGet { client, url } => {
                self.impure_direct = true;
                self.expr(client);
                self.expr(url);
            }
            ExprKind::HttpClientPost { client, url, body } => {
                self.impure_direct = true;
                self.expr(client);
                self.expr(url);
                self.expr(body);
            }
            ExprKind::HttpClientRequest { client, req } => {
                self.impure_direct = true;
                self.expr(client);
                self.expr(req);
            }
            // `std.crypto` — `constant_time_equal` is **Pure** (a branchless self-hosted computation,
            // no I/O), so it may run inside a `par_map` closure: recurse into the operands only.
            ExprKind::CryptoCtEqual { a, b } => {
                self.expr(a);
                self.expr(b);
            }
            // `crypto.random` reads OS entropy → **Impure**: an rng-filling closure is never `Pure`,
            // so it is excluded from `par_map`.
            ExprKind::CryptoRandom { out } => {
                self.impure_direct = true;
                self.expr(out);
            }
            // `crypto.sha256`/`sha512` — a C-engine (libcrypto) call, inferred **Impure** (draft §15:
            // any extern-calling fn is non-Pure), so a hashing closure is rejected by `par_map`
            // (matching `std.compress`; hashing's determinism does not make it pure). Recurse into
            // the byte view.
            ExprKind::CryptoHash { data, .. } => {
                self.impure_direct = true;
                self.expr(data);
            }
            // `crypto.hmac_sha256` / `crypto.hkdf_sha256` — libcrypto calls, inferred **Impure**
            // (never `Pure`, so excluded from `par_map`, matching `crypto.sha256`). Recurse operands.
            ExprKind::CryptoHmac { key, data } => {
                self.impure_direct = true;
                self.expr(key);
                self.expr(data);
            }
            ExprKind::CryptoHkdf { salt, ikm, info, len } => {
                self.impure_direct = true;
                self.expr(salt);
                self.expr(ikm);
                self.expr(info);
                self.expr(len);
            }
            // `crypto.{aes_gcm,chacha20_poly1305}_{seal,open}` — libcrypto AEAD, inferred **Impure**
            // (never `Pure`, so excluded from `par_map`). Recurse the four byte-view operands.
            ExprKind::CryptoAead { key, nonce, input, aad, .. } => {
                self.impure_direct = true;
                self.expr(key);
                self.expr(nonce);
                self.expr(input);
                self.expr(aad);
            }
            // `crypto.argon2id` — a libcrypto call, inferred **Impure** (never `Pure`, so excluded
            // from `par_map`). Recurse into all three operands (`params` is a Copy struct literal).
            ExprKind::CryptoArgon2 { password, salt, params } => {
                self.impure_direct = true;
                self.expr(password);
                self.expr(salt);
                self.expr(params);
            }
            // Pipeline nodes carry a `source` (+ a stage/reducer function that is a call).
            ExprKind::ArraySum { source, stages } | ExprKind::ArrayCount { source, stages } => {
                self.stage_funcs(stages);
                self.expr(source);
            }
            ExprKind::ArrayMinMax { source, stages, .. } | ExprKind::ArraySort { source, stages, .. }
            | ExprKind::ArrayToArray { source, stages, .. } => {
                self.stage_funcs(stages);
                self.expr(source);
            }
            // `map_into` writes each post-stage element into `dst` — a stage-carrying pipeline plus
            // the destination place (a read of the `out`/`mut` slice), both walked for effects.
            ExprKind::ArrayMapInto { source, stages, dst, .. } => {
                self.stage_funcs(stages);
                self.expr(source);
                self.expr(dst);
            }
            ExprKind::ArrayAnyAll { source, stages, func, .. }
            | ExprKind::ArrayReduce { source, stages, func, .. }
            | ExprKind::ArrayScan { source, stages, func, .. }
            | ExprKind::ArraySortBy { source, stages, key_func: func, .. }
            | ExprKind::ArrayPartition { source, stages, func, .. } => {
                self.stage_funcs(stages);
                self.calls.push(func.clone());
                self.expr(source);
            }
            ExprKind::ArrayParMap { source, stages, func, .. } => {
                self.stage_funcs(stages);
                self.calls.push(func.clone());
                self.parmaps.push((func.clone(), e.span));
                self.expr(source);
            }
            // `to_soa` transposes its source array — no stage/reducer functions of its own.
            ExprKind::ArrayToSoa { source, .. } => self.expr(source),
            ExprKind::ArrayDot { a, b, .. } => {
                self.expr(a);
                self.expr(b);
            }
            ExprKind::ArrayChunks { source, n, .. } => {
                self.expr(source);
                self.expr(n);
            }
            // Structural recursion (no effect of their own).
            ExprKind::Unary { expr, .. } | ExprKind::Cast(expr) => self.expr(expr),
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::IntArith { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::If { cond, then, els } => {
                self.expr(cond);
                self.block(then);
                self.block(els);
            }
            ExprKind::StructLit { fields, .. } => {
                for f in fields {
                    self.expr(f);
                }
            }
            ExprKind::Tuple { elems, .. } => {
                for el in elems {
                    self.expr(el);
                }
            }
            ExprKind::MathOp { operands, .. } => {
                for o in operands {
                    self.expr(o);
                }
            }
            ExprKind::ArrayLit { elems, .. } | ExprKind::VecLit { elems, .. } => {
                for el in elems {
                    self.expr(el);
                }
            }
            ExprKind::Select { mask, a, b } => {
                self.expr(mask);
                self.expr(a);
                self.expr(b);
            }
            ExprKind::VecSumWhere { vec, mask } => {
                self.expr(vec);
                self.expr(mask);
            }
            ExprKind::VecDot { a, b } => {
                self.expr(a);
                self.expr(b);
            }
            ExprKind::VecMinMax { vec, .. } => self.expr(vec),
            ExprKind::VecSum { vec } => self.expr(vec),
            ExprKind::VecLoad { src, index, .. } => {
                self.expr(src);
                self.expr(index);
            }
            ExprKind::VecStore { dst, index, value, .. } => {
                self.expr(dst);
                self.expr(index);
                self.expr(value);
            }
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.expr(opt);
                self.expr(fallback);
            }
            ExprKind::BuilderWrite { builder, arg, .. } => {
                self.expr(builder);
                self.expr(arg);
            }
            ExprKind::Block(b) | ExprKind::Arena(b) | ExprKind::TaskGroup(b) => self.block(b),
            // An `unsafe {}` block (and any `raw.*` op) makes the function impure — it can never be a
            // Pure `par_map` callee. This is the "unsafe must be visible/traceable" rule, reusing the
            // existing binary purity flag (unsafe is conflated with I/O-impure for now).
            ExprKind::Unsafe(b) => {
                self.impure_direct = true;
                self.block(b);
            }
            ExprKind::RawAlloc(e) | ExprKind::RawFree(e) => {
                self.impure_direct = true;
                self.expr(e);
            }
            ExprKind::RawLoad { ptr, offset, .. } | ExprKind::RawOffset { ptr, offset } => {
                self.impure_direct = true;
                self.expr(ptr);
                self.expr(offset);
            }
            ExprKind::RawStore { ptr, offset, value } => {
                self.impure_direct = true;
                self.expr(ptr);
                self.expr(offset);
                self.expr(value);
            }
            // Spawning / joining concurrent work is an observable effect (the enclosing function
            // is not pure); the spawned closure's own effects live in its lifted function.
            ExprKind::Spawn { closure, .. } => {
                self.impure_direct = true;
                self.expr(closure);
            }
            ExprKind::TaskGet(inner) => self.expr(inner),
            ExprKind::Wait => self.impure_direct = true,
            ExprKind::EnumValue { payload, .. } => {
                for p in payload {
                    self.expr(p);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for a in arms {
                    self.expr(&a.body);
                }
            }
            ExprKind::ResultMapErr { result, f } => {
                self.expr(result);
                self.expr(f);
            }
            ExprKind::TupleIndex { recv, .. } => self.expr(recv),
            ExprKind::Index { recv, index } => {
                self.expr(recv);
                self.expr(index);
            }
            ExprKind::SliceRange { recv, start, end } => {
                self.expr(recv);
                if let Some(s) = start { self.expr(s); }
                if let Some(e) = end { self.expr(e); }
            }
            ExprKind::ElemField { recv, index, .. } => {
                self.expr(recv);
                self.expr(index);
            }
            ExprKind::OptionSome(i) | ExprKind::ResultOk(i) | ExprKind::ResultErr(i) | ExprKind::Try(i)
            | ExprKind::HeapNew(i) | ExprKind::BoxGet(i) | ExprKind::BoxClone(i) | ExprKind::StrClone(i)
            | ExprKind::StrBorrow(i) | ExprKind::BuilderToString(i) | ExprKind::Len(i)
            | ExprKind::ArrayToSlice(i) => self.expr(i),
            ExprKind::StrPredicate { haystack, needle, .. } => {
                self.expr(haystack);
                self.expr(needle);
            }
            ExprKind::StrTrim { recv, .. } => self.expr(recv),
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Hole(h) | TemplatePart::JsonStr(h) = p {
                        self.expr(h);
                    }
                }
            }
            ExprKind::JsonDecode { input, .. } | ExprKind::JsonDecodeArray { input, .. }
            | ExprKind::JsonDecodeStructArray { input, .. } | ExprKind::JsonDecodeSoa { input, .. } => self.expr(input),
            // `builder(capacity)` — the capacity expr may itself have effects.
            ExprKind::BuilderNew { capacity } => {
                if let Some(c) = capacity {
                    self.expr(c);
                }
            }
            // Leaves.
            ExprKind::Unit | ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Char(_)
            | ExprKind::Str(_) | ExprKind::Bool(_) | ExprKind::Local(_) | ExprKind::OptionNone
            | ExprKind::Field { .. } | ExprKind::SoaColumn { .. } | ExprKind::ArrayGroupAgg { .. }
            | ExprKind::ArrayGroupAggMulti { .. }
            | ExprKind::ArrayDictEncode { .. } | ExprKind::IndexField { .. } => {}
        }
    }
}

/// A value's inferred lifetime region (Memory Model v2, `impl/08-memory-model-v2.md`).
/// Total order, longest-lived first: `Static ⊐ Frame ⊐ Arena(1) ⊐ … ⊐ Arena(d)`. Regions are
/// inferred, never written, and live only in this analysis — they are not part of `Ty`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Region {
    /// Process / program lifetime: literals, leaked allocations, owned-from-scalar values.
    Static,
    /// The current function's frame: a view created in-frame over frame-local storage. Cannot
    /// be returned. (A view *parameter* borrows the caller and is `Static` here — returnable.)
    /// Not yet produced — frame-local slices still use the `local_backed_slice` set; folding
    /// them onto this variant is a later MMv2 slice.
    #[allow(dead_code)]
    Frame,
    /// The k-th enclosing `arena {}` (1 = outermost). Freed at that block's end.
    Arena(u32),
}

impl Region {
    /// Ordinal in the lattice; smaller = longer-lived.
    fn ord(self) -> u32 {
        match self {
            Region::Static => 0,
            Region::Frame => 1,
            Region::Arena(k) => 1 + k,
        }
    }

    /// Whether a value of `self` may be stored into / returned to a location of region `dst`
    /// — i.e. `self` lives at least as long as `dst`. This is the single escape rule.
    fn outlives(self, dst: Region) -> bool {
        self.ord() <= dst.ord()
    }

    /// The region of a value allocated at arena nesting `depth` (0 = outside any arena, where
    /// the result is leaked / process-lifetime → `Static`).
    fn arena(depth: u32) -> Region {
        if depth == 0 {
            Region::Static
        } else {
            Region::Arena(depth)
        }
    }

    /// The shorter-lived (higher-ordinal) of two regions — a view over both lives only as
    /// long as the shorter source.
    fn shorter(self, other: Region) -> Region {
        if self.ord() >= other.ord() {
            self
        } else {
            other
        }
    }
}

/// Arena escape checking (`03-types.md` §7, generalized per `impl/08-memory-model-v2.md`):
/// every view / arena-allocated value carries an inferred [`Region`], and the one escape rule
/// ([`Region::outlives`]) forbids it being returned to / stored into a longer-lived location.
/// A `box<T>` / arena-backed `str` is `Arena(k)`; a frame-local-backed `slice` is `Frame`.
/// Regions are inferred — never written.
struct EscapeCheck<'a> {
    f: &'a Fn,
    diags: &'a mut Diagnostics,
    /// Tuple defs (to decide whether a `Ty::Tuple` is region-tracked — true iff an element is).
    tuples: &'a [hir::TupleDef],
    /// Struct defs (to decide whether a `soa<Struct>` has a `str` column — see `struct_has_str`).
    structs: &'a [StructDef],
    /// For each box/str local, the region at which its current value was allocated.
    region: std::collections::HashMap<LocalId, Region>,
    /// For each local, the arena depth at which it was declared.
    decl_depth: std::collections::HashMap<LocalId, u32>,
    /// Slice locals bound to a view of *function-local* array storage (an array literal or
    /// local array materialized in this frame). Such a slice borrows the stack frame and so
    /// must not be returned. A slice *parameter* borrows the caller and is never in this set.
    local_backed_slice: std::collections::HashSet<LocalId>,
}

impl<'a> EscapeCheck<'a> {
    fn check(&mut self) {
        self.block(&self.f.body, 0);
        // The body's trailing value is the function's return value (single-expression
        // bodies and fall-through blocks), so apply the same escape check there.
        if let Some(v) = &self.f.body.value {
            self.check_return_escape(v, 0);
        }
    }

    /// Escape check for a returned value `e` (an explicit `return` or a body's trailing value):
    /// a region-tracked value must be `Static` (returnable), and a `slice` must not view a local
    /// array. The region-tracked diagnostic distinguishes a `Frame` borrow of local storage (use
    /// `.clone()`) from an arena allocation.
    fn check_return_escape(&mut self, e: &Expr, depth: u32) {
        let r = self.region_of(e, depth);
        if self.tracks_region(e.ty) && !r.outlives(Region::Static) {
            let msg = if r == Region::Frame {
                "cannot return a view that borrows local storage (it is freed when the function returns); use `.clone()` to return an owned value"
            } else {
                "cannot return a value allocated in an arena (it is freed at block end)"
            };
            self.diags.error(msg.to_string(), e.span);
        }
        if matches!(e.ty, Ty::Slice(_)) && self.slice_is_local(e) {
            self.diags.error(
                "cannot return a slice that views a local array (it is freed when the function returns)".to_string(),
                e.span,
            );
        }
    }

    /// Types whose values carry an inferred region and so must be escape-checked: `box<T>`
    /// (M3), arena-backed `str` (M5 — `template`/concat allocate in the arena), and a struct
    /// (MMv2 slice 2 — a struct's region is the max of its fields, so a struct holding an
    /// arena-backed `str` field carries that arena region). A scalar-only struct is `Static`.
    fn tracks_region(&self, ty: Ty) -> bool {
        match ty {
            Ty::Box(_) | Ty::Str | Ty::String | Ty::Struct(_) | Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::DynSliceArray(_) => true,
            // A `dict_encoded` value's `dict`/`source` slices borrow the source AoS, so it is
            // region-tracked — it must not outlive the array it encodes.
            Ty::DictEncoded(..) => true,
            // A `soa<Struct>` view borrows its column buffer (arena-allocated by `to_soa`), so it is
            // region-tracked — it must not outlive the arena that owns the buffer.
            Ty::Soa(_) => true,
            // A tuple is region-tracked iff any element is (today: a `str` element — a view tied to
            // its source). A tuple of plain scalars is Copy / `Static`, freely returnable.
            Ty::Tuple(id) => self.tuples[id as usize].elems.iter().any(|s| self.tracks_region(scalar_to_ty(*s))),
            // A *fixed* `array<T>` (a stack value) is region-tracked iff its element is — an
            // `array<str>` holds `str` views (so an array of arena strs is arena-regioned and must
            // not escape), while an `array<i64>` is plain Copy data (Static, freely returnable).
            // A `slice<T>` likewise tracks iff its element does (its own backing is handled
            // separately by the local-backed-slice check). A fixed `array<Struct>` (AoS) always
            // tracks, like `Struct` itself — a struct may hold a region-tracked `str` field, so an
            // element / element-field read must inherit the array's region.
            Ty::Array(s, _) | Ty::Slice(s) => self.tracks_region(scalar_to_ty(s)),
            Ty::StructArray(..) => true,
            // An `Option`/`Result` is region-tracked iff its payload is. A `Struct` payload (e.g. a
            // `json.decode`-d struct) and now a `str` payload (a view) both track; scalars do not.
            Ty::Option(s) => self.tracks_region(scalar_to_ty(s)),
            Ty::Result(o, e) => self.tracks_region(scalar_to_ty(o)) || self.tracks_region(scalar_to_ty(e)),
            // `Task<R>` (④b) is a box in the task_group region — region-tracked like `box<T>`, so
            // a task handle cannot escape its `task_group` scope.
            Ty::Task(_) => true,
            // A `reader`/`writer` is region-tracked *because* it can be a **borrow**: `c.reader()` /
            // `c.writer()` on a `tcp_conn` hand back a reader/writer over the conn's fd
            // (`owns_fd: false`), region-bound to `c` (see `region_of` — `ConnReader`/`ConnWriter`),
            // so a stream used past the conn's `close(fd)` is a use-after-close (net.md P2, #297).
            // An *owned* reader/writer constructed **directly** by a builtin (`io.stdin`/`fs.open`/
            // `io.stdout`/`fs.create`) has region `Static` (its `region_of` producer — `ReaderOpen` /
            // `WriterCreate` — falls through to the `Static` wildcard), so it stays freely returnable.
            // But a reader/writer threaded through a **user** function call is not so lucky:
            // `region_of(Call)` conservatively folds in *every* argument's region (it has no per-fn
            // "does this actually borrow arg i" fact), so calling that user fn with a Frame/Arena-
            // region argument taints the whole result — even when the callee's own reader is an
            // unrelated direct `fs.open`. Returning that call's result past the tainted region is
            // then rejected, even though nothing is actually borrowed. This is sound (never
            // miscompiles) but imprecise; the precise fix belongs to the escape-check → MIR-dataflow
            // structural follow-up (`docs/open-questions.md` "External soundness audit"). This arm
            // only makes the escape check *consult* the region either way. (A `tcp_conn` itself is
            // always owned, never a borrow, so it is deliberately NOT here.)
            Ty::Reader | Ty::Writer => true,
            _ => false,
        }
    }

    /// Whether struct `id` has any `str` column — the field that turns a `soa<Struct>` from a
    /// self-contained, arena-only value into one whose columns hold zero-copy `str` views borrowing
    /// the decode input (or `to_soa` source). A str-bearing soa must be region-tied to that borrow;
    /// a primitive-only one is free to escape the arena (`s[i]` gather returns a Copy POD value).
    fn struct_has_str(&self, id: u32) -> bool {
        self.structs[id as usize].fields.iter().any(|f| f.ty == Ty::Str)
    }

    /// The [`Region`] a region-bearing (`box`/`str`) value is bound to. `Static` = no region
    /// (a leaked/static str, a box param — none exist — etc.). Recurses through value forms so
    /// it can't slip out via an `if`/block value.
    fn region_of(&self, e: &Expr, depth: u32) -> Region {
        match &e.kind {
            // Allocating producers are bound to the enclosing arena (Static outside any arena,
            // where the result is leaked / process-lifetime and safe to return).
            ExprKind::HeapNew(_) | ExprKind::BoxClone(_) | ExprKind::Template(_) => Region::arena(depth),
            // `fs.read_file_view(p)` returns a `str` viewing an mmap `munmap`ped at arena end, so it is
            // bound to the enclosing arena exactly like `heap.new` (sema requires an arena). The view
            // must not escape it — `.clone()` copies out. `fs.read_dir` / `fs.write_file` / `fs.exists`
            // / `fs.remove` return owned / non-region values and stay `Static` (the wildcard below).
            ExprKind::FsReadFileView { .. } => Region::arena(depth),
            // A spawned task's handle is a box in the enclosing `task_group` region.
            ExprKind::Spawn { .. } => Region::arena(depth),
            // `.to_array()` bump-allocates the owned array in the enclosing arena. `reduce` folds
            // its accumulator there too — when that accumulator is region-tracked (a `str` built by
            // concatenation, a struct), the result lives in the enclosing arena and must not escape
            // it. `arena(depth)` is the shortest-lived (most restrictive) region anything allocated
            // at this depth can have, so it conservatively covers an accumulator that instead just
            // forwards `init` or borrows a source element (both outlive `arena(depth)`).
            // These allocating producers bump-allocate in the enclosing arena; the returned value
            // borrows it, so it is arena-regioned and cannot escape (like `to_array`'s buffer).
            // (`to_soa` and `json.decode → soa` are handled separately below — a `str`-bearing soa
            // also borrows its source/input, so it needs the shorter of the two regions.)
            ExprKind::ArrayToArray { .. }
            | ExprKind::ArrayPartition { .. }
            | ExprKind::ArrayParMap { .. }
            | ExprKind::ArrayScan { .. }
            | ExprKind::ArraySort { .. }
            | ExprKind::ArraySortBy { .. }
            | ExprKind::ArrayReduce { .. } => Region::arena(depth),
            // `str + str` concatenation is also built in the enclosing arena.
            ExprKind::Binary { op: BinOp::Add, .. } if e.ty == Ty::Str => Region::arena(depth),
            // A decoded struct's `str`/array fields are zero-copy views into the input buffer
            // (MMv2 slice 6), so the struct is region-tied to that input — it cannot outlive it.
            // Conservative: even a scalar-only decoded struct is bound to the input region (no
            // struct-field lookup here); use `.clone()` to escape. `?` preserves the region.
            ExprKind::JsonDecode { input, .. } => self.region_of(input, depth),
            // A decoded `array<Struct>` (slice 8d) likewise carries the input's region — its
            // elements' `str` fields are zero-copy views into the input; `.clone()` to escape.
            ExprKind::JsonDecodeStructArray { input, .. } => self.region_of(input, depth),
            // `json.decode → soa`: the column buffer is arena-allocated, and a `str` column holds
            // zero-copy views into the JSON input (like the AoS decode). So a str-bearing soa is
            // bound to BOTH — the arena buffer and the input — i.e. the shorter of the two regions.
            // (A primitive-only soa borrows nothing and stays purely arena-regioned via the group
            // arm above, so it is self-contained and free to escape the input.)
            ExprKind::JsonDecodeSoa { input, struct_id } if self.struct_has_str(*struct_id) => {
                self.region_of(input, depth).shorter(Region::arena(depth))
            }
            ExprKind::JsonDecodeSoa { .. } => Region::arena(depth),
            // `to_soa` transposes an AoS `array<Struct>` into an arena-allocated column buffer. A
            // `str` column copies the source elements' `str` views into the column, so a str-bearing
            // soa borrows the source's string storage — it is bound to BOTH the arena buffer and the
            // source (the shorter of the two). A primitive-only `to_soa` borrows nothing → the group
            // arm below binds it purely to the arena (self-contained, like `to_array`'s buffer).
            ExprKind::ArrayToSoa { source, struct_id } if self.struct_has_str(*struct_id) => {
                self.region_of(source, depth).shorter(Region::arena(depth))
            }
            ExprKind::ArrayToSoa { .. } => Region::arena(depth),
            // `arr[i].field` reads a field of a struct-array element; a `str` field is a view into
            // the array's storage, so it inherits the array's region (it must not outlive it). A
            // scalar field is Copy → the default `Static` (handled below), but tying to the array
            // is conservatively correct for both.
            ExprKind::ElemField { recv, .. } => self.region_of(recv, depth),
            // `s[i]` on a `soa` *gathers* a whole struct. A primitive-only struct is copied
            // column-by-column into a fresh POD value that borrows nothing → `Static` (returnable).
            // A struct with a `str` column gathers `str` views that borrow the soa's buffer/input,
            // so the gathered value inherits the soa's region (it must not outlive it).
            ExprKind::Index { recv, .. } if matches!(recv.ty, Ty::Soa(_)) => match recv.ty {
                Ty::Soa(sid) if self.struct_has_str(sid) => self.region_of(recv, depth),
                _ => Region::Static,
            },
            // `arr[i]` reads an element; a `str` element is a view into the array's storage, so it
            // inherits the array's region (it must not outlive it). A scalar element is Copy and
            // not region-tracked, so inheriting the array's region is harmless (never checked).
            ExprKind::Index { recv, .. } => self.region_of(recv, depth),
            // A range slice is a borrowed view into the receiver's storage (a sub-`str` or a
            // sub-`slice`), so it lives exactly as long as the receiver — inherit its region (the
            // same rule as `Index` / `StrTrim`; the bounds are scalar `i64`, never region-tracked).
            ExprKind::SliceRange { recv, .. } => self.region_of(recv, depth),
            // An array literal lives as long as its shortest-lived element — a `[str]` of arena
            // `str` views is arena-regioned (the same rule as a struct literal over its fields). A
            // Move-struct array literal, however, *owns* its elements' heap buffers (its `.clone()`
            // strings): a view into it (`[User{…}][i].name`, indexed directly without binding) is
            // frame-local — the temporary's buffers die within the frame — so cap it at `Frame`, the
            // same bound a bound Move-struct array gets at its `let` (else the view could be returned).
            ExprKind::ArrayLit { elems, .. } => {
                let r = elems
                    .iter()
                    .fold(Region::Static, |acc, el| acc.shorter(self.region_of(el, depth)));
                if matches!(e.ty, Ty::StructArray(sid, _) if struct_is_move(sid, self.structs)) {
                    r.shorter(Region::Frame)
                } else {
                    r
                }
            }
            // A tuple lives as long as its shortest-lived element (same rule as an array literal):
            // a tuple holding an arena `str` view is arena-regioned and cannot escape.
            ExprKind::Tuple { elems, .. } => elems
                .iter()
                .fold(Region::Static, |acc, el| acc.shorter(self.region_of(el, depth))),
            // `t.N` reads an element; a `str` element is a view into the tuple, so it inherits the
            // tuple's region (a scalar element is Copy → harmless to inherit, never checked).
            ExprKind::TupleIndex { recv, .. } => self.region_of(recv, depth),
            // `chunks` makes an `array<slice<T>>` (`DynSliceArray`) whose slice headers borrow the
            // source's **backing storage**. That storage region is distinct from the *element*
            // region `region_of(source)` returns for reads: `arr[i]` of an `array<str>` yields a
            // `str` view whose region is the element's (a literal's is `Static`, safely returnable),
            // yet the array *slot* it was read from is frame-local. A str/struct-array's element
            // region would otherwise hide the frame storage and let the chunks result escape (a
            // use-after-free of the frame slot). So bind the chunks result to its source's storage
            // region (see `chunks_source_storage_region`), and also to the element region (`shorter`)
            // so an `array<str>` of arena strings is bounded by both its slot and that arena.
            ExprKind::ArrayChunks { source, .. } => self
                .chunks_source_storage_region(source, depth)
                .shorter(self.region_of(source, depth)),
            // Borrowing an array as a slice preserves the array's region — a `slice<str>` coerced
            // from an arena str-array must not outlive that arena.
            ExprKind::ArrayToSlice(inner) => self.region_of(inner, depth),
            // Wrapping/unwrapping preserves the payload's region: `Ok(decoded)` is as short-lived
            // as `decoded`, and `res?` re-exposes whatever region `res` carried. Without this a
            // region-tied struct could escape through a `Result`-typed local (use-after-free).
            ExprKind::Try(inner)
            | ExprKind::OptionSome(inner)
            | ExprKind::ResultOk(inner)
            | ExprKind::ResultErr(inner) => self.region_of(inner, depth),
            // `map_err` passes the `Ok` payload through unchanged, so its region is the source's
            // (a region-tied Ok payload must not escape via the converted result).
            ExprKind::ResultMapErr { result, .. } => self.region_of(result, depth),
            // `opt else fb` yields one of two values, so it lives only as long as the shorter.
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.region_of(opt, depth).shorter(self.region_of(fallback, depth))
            }
            // A `str` borrow of an owned `string` (slice 7b) views storage owned by *this* frame
            // (the `string` is `Drop`-freed at frame exit), so the view is `Frame`-regioned — it
            // must not escape the frame. This feeds `region_of(Call)`: passing a borrowed string
            // to a function that returns a borrow of its argument correctly blocks the escape.
            // We cap at the shorter of `Frame` and the borrowed value's own region: today every
            // `string` is heap-owned (`Static`), so this is exactly `Frame`; but if a later slice
            // arena-allocates a `string` (`Arena(k)`, shorter than `Frame`), the borrow must not
            // outlive that arena — taking the shorter keeps it sound for free.
            ExprKind::StrBorrow(inner) => Region::Frame.shorter(self.region_of(inner, depth)),
            // A trim yields a sub-`str` of its receiver (same bytes), so the view lives exactly as
            // long as the receiver — inherit its region directly. (The receiver is already a `str`:
            // an owned `string` was auto-borrowed to a `Frame` view first, so this stays sound.)
            ExprKind::StrTrim { recv, .. } => self.region_of(recv, depth),
            // `path.base`/`dir`/`ext(p)` return a zero-copy substring `str` view of `p`, so the view
            // lives exactly as long as `p` — inherit its region directly (like `StrTrim`). Without this
            // explicit arm the wildcard below would mis-infer `Static`, letting a view of an arena/frame
            // `str` escape (the #297-class bug). `path.join`/`normalize` allocate owned strings and stay
            // `Static` (the wildcard).
            ExprKind::PathComponent { path, .. } => self.region_of(path, depth),
            // `buf.bytes()` is a `slice<u8>` view of the `buffer` local's heap storage (freed at
            // frame exit), so — like `StrBorrow` — it is `Frame`-regioned and cannot escape the frame.
            ExprKind::BufferBytes { buffer } => Region::Frame.shorter(self.region_of(buffer, depth)),
            // `p.get_str(name)` is a `str` view into the `cli parsed` handle's owned storage (freed at
            // frame exit), so — like `BufferBytes` — it is `Frame`-regioned and cannot escape the
            // frame. Without this explicit arm the wildcard below mis-infers `Static`, letting the view
            // of a dropped `parsed` escape (the #297-class bug); `.clone()` copies out.
            ExprKind::CliGetStr { parsed, .. } => Region::Frame.shorter(self.region_of(parsed, depth)),
            // `resp.header(name)` returns `Option<str>` and `resp.body()` a `slice<u8>`, both **views**
            // into the `http response` handle's owned buffer (freed at frame exit). Like `CliGetStr` /
            // `BufferBytes`, they are `Frame`-regioned and bound to `resp` (or shorter if `resp` is
            // arena-scoped) — an escape past `resp`'s `Drop` reads freed memory (#297). Without these
            // arms the wildcard mis-infers `Static`; `.clone()` (header) / a copy-out copies past `resp`.
            ExprKind::HttpRespHeader { resp, .. } | ExprKind::HttpRespBody { resp } => {
                Region::Frame.shorter(self.region_of(resp, depth))
            }
            // `c.reader()` / `c.writer()` borrow the `tcp_conn`'s fd (`owns_fd: false` — only `c`'s
            // `Drop` closes it), so — like `BufferBytes` / `CliGetStr` — the returned stream is
            // region-bound to `c`: `Frame` (or shorter if `c` lives in an arena). It must not escape
            // `c`'s scope, else it would read/write a `close`d fd (net.md P2, #297). Without this
            // explicit arm the wildcard below would mis-infer `Static`, letting the stream outlive
            // the connection. (`c` is a bound local — the receiver gate in `check_conn_stream`.)
            ExprKind::ConnReader { conn } | ExprKind::ConnWriter { conn } => {
                Region::Frame.shorter(self.region_of(conn, depth))
            }
            ExprKind::Local(p) => self.region.get(p).copied().unwrap_or(Region::Static),
            // A struct's region is the shortest-lived of its fields (a view over it lives only
            // as long as the shortest source); a scalar/literal-only struct stays `Static`.
            ExprKind::StructLit { fields, .. } => fields
                .iter()
                .fold(Region::Static, |acc, f| acc.shorter(self.region_of(f, depth))),
            // A field read inherits its base struct's region (the field may be a view into it).
            ExprKind::Field { root, .. } => self.region.get(root).copied().unwrap_or(Region::Static),
            // A str-key (or dict-encoded) `group_by` yields `(array<str>, array<i64>)` whose key views
            // borrow `base`'s string storage, so the tuple inherits `base`'s region — it must not
            // outlive the source. (An i64-key group_by yields owned arrays that borrow nothing → the
            // default `Static`.) `dict_encode` itself likewise borrows its source AoS (its `dict`/
            // `source` slices view it), so the encoded value inherits the source's region.
            ExprKind::ArrayGroupAgg { base, source: hir::GroupSource::AosStr | hir::GroupSource::Encoded | hir::GroupSource::SoaStr, .. }
            | ExprKind::ArrayGroupAggMulti { base, source: hir::GroupSource::AosStr | hir::GroupSource::Encoded, .. }
            | ExprKind::ArrayDictEncode { base, .. }
            // A `soa` column projection `s.field` is a `{ptr,len}` slice into the soa's buffer; for a
            // `str` column it is a `slice<str>` whose views borrow the soa's buffer/input, so it
            // inherits the soa local's region. (A primitive column `slice<i64>` is not region-tracked,
            // so inheriting is harmless — never checked.)
            | ExprKind::SoaColumn { base, .. } => self.region.get(base).copied().unwrap_or(Region::Static),
            ExprKind::Block(b) => self.region_of_block(b, depth),
            // `unsafe {}` is a plain marker block — its value's region is the tail value's region (no
            // new region opened, unlike `arena {}`). Explicit so an arena value returned from an
            // unsafe block isn't silently inferred `Static` via the wildcard below and allowed to escape.
            ExprKind::Unsafe(b) => self.region_of_block(b, depth),
            // An `arena {}` *expression* yields its block value, evaluated one level deeper.
            // Without this, a binding that captures an arena's value (`p := arena { … }`) would
            // be inferred `Static` and could then escape undetected (a use-after-free across
            // nested arenas); the per-block walk only checks the immediate boundary, not a
            // later escape of the binding.
            ExprKind::Arena(b) => self.region_of_block(b, depth + 1),
            ExprKind::If { then, els, .. } => {
                self.region_of_block(then, depth).shorter(self.region_of_block(els, depth))
            }
            // A `match` yields one of its arms' values, so it lives only as long as the
            // shortest-lived arm (the same rule as `if`/`else`). Without this, an arena value
            // returned through a match arm is inferred `Static` and escapes undetected (a
            // use-after-free — `region_of` otherwise falls to the `Static` wildcard below).
            ExprKind::Match { arms, .. } => arms
                .iter()
                .fold(Region::Static, |acc, a| acc.shorter(self.region_of(&a.body, depth))),
            // An indirect call's result may borrow one of its arguments (`g := id; g(s)`), so it
            // lives no longer than the shortest-lived argument — exactly like a direct `Call`.
            // Without this, returning `g(arena_str)` out of an arena slips the escape check.
            ExprKind::CallFnValue { args, .. } => args
                .iter()
                .fold(Region::Static, |acc, a| acc.shorter(self.region_of(a, depth))),
            // `arr[const].field` reads a field of a struct-array element; a `str` field is a view
            // into the array's storage, so it inherits the array's region (like `ElemField`).
            ExprKind::IndexField { base, .. } => self.region.get(base).copied().unwrap_or(Region::Static),
            // `t.get()` exposes the task's result; a region-tracked result borrows whatever the
            // task closure did, so it inherits the inner value's region (conservative, never longer).
            ExprKind::TaskGet(inner) => self.region_of(inner, depth),
            // A `task_group {}` opens a region for its spawned tasks (like `arena {}`), so its value
            // is evaluated one level deeper — else a binding capturing a task_group's value (a `Task`
            // handle / box) is inferred `Static` and escapes the group undetected (use-after-free).
            ExprKind::TaskGroup(b) => self.region_of_block(b, depth + 1),
            // A call's result may be a view borrowing one of its arguments (`fn id(s: str) -> str
            // = s`), so conservatively it lives no longer than the shortest-lived argument — the
            // region analogue of `slice_is_local`'s arg propagation. Without this, returning
            // `f(arena_str)` out of the arena slips the escape check → use-after-free of the
            // freed buffer. A function that does *not* return a borrow of its args is
            // over-restricted here; precise per-fn "returns a borrow of arg i" inference is a
            // later slice. Non-tracked args (ints, literals) are `Static` and don't shorten.
            ExprKind::Call { args, .. } => args
                .iter()
                .fold(Region::Static, |acc, a| acc.shorter(self.region_of(a, depth))),
            _ => Region::Static,
        }
    }

    fn region_of_block(&self, b: &Block, depth: u32) -> Region {
        b.value.as_ref().map(|v| self.region_of(v, depth)).unwrap_or(Region::Static)
    }

    /// The region of the **backing storage** a `chunks` source borrows — deliberately distinct from
    /// the source's *element/value* region ([`Self::region_of`]). A fixed stack `array<T>` / AoS
    /// `array<Struct>` bound as a `Let`-local owns a **frame slot**, scoped to the arena it was
    /// declared in (`Frame.shorter(arena(decl_depth))`); a fixed-array *parameter* borrows the caller
    /// (never in `decl_depth` → `Static`, so chunking a param array stays returnable). An array
    /// literal materializes a frame temporary at the current depth. A `slice` that itself borrows a
    /// frame-local array (`local_backed_slice`) re-borrows that frame storage. Any other source (an
    /// owned `array<T>`/`slice` producer, a slice param, a nested value expression) borrows storage
    /// that `region_of` already places correctly — use it as-is. `chunks` restricts a fixed-`array`
    /// source to a literal or a bare local (`check_array_chunks`), so no nested-expression fixed
    /// array reaches here.
    fn chunks_source_storage_region(&self, source: &Expr, depth: u32) -> Region {
        match &source.kind {
            ExprKind::Local(p) if matches!(source.ty, Ty::Array(..) | Ty::StructArray(..)) => self
                .decl_depth
                .get(p)
                .map_or(Region::Static, |d| Region::Frame.shorter(Region::arena(*d))),
            ExprKind::Local(p) if self.local_backed_slice.contains(p) => Region::Frame,
            ExprKind::ArrayLit { .. } => Region::Frame.shorter(Region::arena(depth)),
            _ => self.region_of(source, depth),
        }
    }

    /// Whether a `slice<T>`-typed expression borrows *function-local* array storage (and so
    /// cannot be returned). An array literal / local array materializes in this frame; a
    /// slice parameter borrows the caller (safe). A call returns a local-backed slice iff any
    /// argument it borrows is itself local-backed (the callee can only re-borrow its args).
    fn slice_is_local(&self, e: &Expr) -> bool {
        match &e.kind {
            // `buf.bytes()` views storage owned by the `buffer` local (`Drop`-freed at frame exit),
            // so the `slice<u8>` is frame-local and must not be returned — like a slice of a local array.
            // `resp.body()` is a `slice<u8>` view into the `http response` handle's frame-local buffer
            // — local-backed like `buf.bytes()`, so returning it is rejected (its region arm above also
            // binds it to `resp`, but a `slice<u8>` of a numeric element is not `tracks_region`, so this
            // local-backed check is the one that catches its escape).
            ExprKind::ArrayToSlice(_) | ExprKind::ArrayLit { .. } | ExprKind::BufferBytes { .. } | ExprKind::HttpRespBody { .. } => true,
            ExprKind::Local(p) => self.local_backed_slice.contains(p),
            ExprKind::Call { args, .. } => args.iter().any(|a| self.slice_is_local(a)),
            ExprKind::Block(b) => b.value.as_ref().is_some_and(|v| self.slice_is_local(v)),
            ExprKind::If { then, els, .. } => {
                then.value.as_ref().is_some_and(|v| self.slice_is_local(v))
                    || els.value.as_ref().is_some_and(|v| self.slice_is_local(v))
            }
            // A range slice `recv[a..b]` borrows the receiver's storage, so it is frame-local iff
            // the receiver is (a sub-slice of a local array is still a view of that stack array).
            // Without this, `return xs[0..2]` over a local array returns a dangling slice.
            ExprKind::SliceRange { recv, .. } => self.slice_is_local(recv),
            // A `match`/`else` yields one of its arms, so it is frame-local if any arm is (like the
            // `if`/`else` arm above — a local-backed slice must not escape through either).
            ExprKind::Match { arms, .. } => arms.iter().any(|a| self.slice_is_local(&a.body)),
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.slice_is_local(opt) || self.slice_is_local(fallback)
            }
            // An `arena` / `unsafe` / `task_group` block yields its block value, which is frame-local
            // if the inner value is (like the plain `Block` arm above). Without these a local-backed
            // slice returned through such a block escapes the function undetected (dangling slice).
            ExprKind::Arena(b) | ExprKind::Unsafe(b) | ExprKind::TaskGroup(b) => {
                b.value.as_ref().is_some_and(|v| self.slice_is_local(v))
            }
            _ => false,
        }
    }

    fn block(&mut self, b: &Block, depth: u32) {
        for s in &b.stmts {
            self.stmt(s, depth);
        }
        if let Some(v) = &b.value {
            self.walk(v, depth);
        }
    }

    fn stmt(&mut self, s: &Stmt, depth: u32) {
        match s {
            Stmt::Let { local, init } => {
                self.walk(init, depth);
                self.decl_depth.insert(*local, depth);
                if self.tracks_region(init.ty) {
                    let mut r = self.region_of(init, depth);
                    // A Move-struct array is a *frame* slot whose elements' heap buffers are freed
                    // when the array is dropped — at **function exit** (`drop_locals`), not by an
                    // enclosing arena's bulk free (the buffers are `malloc`'d by `.clone()`, not arena
                    // memory). So a `str` view read out of it (`us[i].name`, Slice 4b) is `Frame`-
                    // region — it can't be *returned* (freed when the function returns), but it stays
                    // valid for the rest of the frame. `region_of` would infer `Static` from the
                    // individually heap-owned strings; cap it at `Frame` so the return check fires.
                    if matches!(init.ty, Ty::StructArray(sid, _) if struct_is_move(sid, self.structs)) {
                        r = r.shorter(Region::Frame);
                    }
                    self.region.insert(*local, r);
                }
                if matches!(init.ty, Ty::Slice(_)) && self.slice_is_local(init) {
                    self.local_backed_slice.insert(*local);
                }
                // A local bound to a *fixed* array (`xs := [1, 2, 3]`) owns frame storage, so any
                // slice viewing it (`xs[0..2]`, `xs` coerced) is frame-local and must not escape.
                // Array *parameters* borrow the caller and are never `Let`-bound, so they stay out
                // of this set (returnable), matching the slice-parameter convention above.
                if matches!(init.ty, Ty::Array(..) | Ty::StructArray(..)) {
                    self.local_backed_slice.insert(*local);
                }
            }
            // `base[index] = value` / `base[index].field = value`. The store itself targets the
            // element slot; recurse into the index/value for nested escapes, and — when the element
            // is region-tracked (a `str` element) — reject storing a shorter-lived value into the
            // longer-lived array (the `Assign`/`AssignField` region rule, extended to elements).
            Stmt::AssignIndex { base, index, value }
            | Stmt::AssignElemField { base, index, value, .. }
            | Stmt::AssignElem { base, index, value, .. } => {
                self.walk(index, depth);
                self.walk(value, depth);
                if self.tracks_region(value.ty) {
                    let target = self.region.get(base).copied().unwrap_or(Region::Static);
                    if !self.region_of(value, depth).outlives(target) {
                        self.diags.error(
                            "this value cannot be stored into a longer-lived array element (it would escape its region)".to_string(),
                            value.span,
                        );
                    }
                }
            }
            Stmt::AssignVecLane { value, .. } => self.walk(value, depth),
            Stmt::Assign { local, value, drop_old } => {
                self.walk(value, depth);
                // The value being *overwritten* is dropped here (the move pass set `drop_old`). But
                // if it lived in an arena, its buffer is bump-allocated and bulk-freed by the arena
                // — never individually. Freeing it here would corrupt the allocator (an interior
                // arena pointer passed to `free`, then freed again by the arena's bulk reset — the
                // observed double-free). The move pass has no region facts, so clear `drop_old` here
                // (this pass owns them). The *new* value's own drop is still handled correctly by
                // its region: a `Static` heap array keeps its exit drop; another arena value stays
                // bulk-freed. Checked before the region map is updated below, so it reads the region
                // of the value being replaced, not the replacement.
                if matches!(self.region.get(local).copied(), Some(Region::Arena(_))) {
                    drop_old.set(false);
                }
                // Conservative without a dataflow join: a binding that is *ever* assigned a
                // local-backed slice stays local-backed (a later branch could reach `return`
                // while the binding still holds the local array). We never clear the flag.
                if matches!(value.ty, Ty::Slice(_)) && self.slice_is_local(value) {
                    self.local_backed_slice.insert(*local);
                }
                if self.tracks_region(value.ty) {
                    let r = self.region_of(value, depth);
                    // The binding's scope: at least the frame (a depth-0 binding lives the whole
                    // frame, region `Frame`), or the enclosing arena if declared inside one. Using
                    // `Frame` rather than `Static` here lets a `Frame`-region borrow (a `str` view
                    // of a local `string`, slice 7e) be held by a frame binding — escape past the
                    // frame is still caught by the return / struct-field-store checks. A deeper
                    // arena value assigned to a shallower binding stays rejected.
                    let target = Region::Frame.shorter(Region::arena(*self.decl_depth.get(local).unwrap_or(&0)));
                    if !r.outlives(target) {
                        self.diags.error(
                            "this value is bound to an arena block and cannot escape it".to_string(),
                            value.span,
                        );
                    }
                    // Track the reassigned binding's region for later uses.
                    self.region.insert(*local, r);
                }
            }
            Stmt::AssignField { root, value, .. } => {
                self.walk(value, depth);
                // The base struct lives at its own (fixed) region; a stored value must outlive
                // it, else the value would escape its region via the longer-lived struct.
                if self.tracks_region(value.ty) {
                    let target = self.region.get(root).copied().unwrap_or(Region::Static);
                    if !self.region_of(value, depth).outlives(target) {
                        self.diags.error(
                            "this value cannot be stored into a longer-lived struct field (it would escape its region)".to_string(),
                            value.span,
                        );
                    }
                }
            }
            Stmt::Return(Some(e)) => {
                self.walk(e, depth);
                // A returned value escapes to the caller (`Static`): only a `Static`-region
                // value may be returned (an arena/frame view cannot).
                self.check_return_escape(e, depth);
            }
            Stmt::Return(None) => {}
            Stmt::Expr(e) => self.walk(e, depth),
            // A tuple destructure binds each element to a local. If the tuple is region-tracked
            // (holds a `str` view, or owned arrays allocated in an arena), each bound local inherits
            // the tuple's region — else an arena-allocated destructured array would default to
            // `Static`, land in the drop set, and be freed both here and by the arena (double-free).
            // (The current producers — `partition`, owned-tuple returns — give all elements the same
            // region, so the tuple's region is exact; per-element regions are a later refinement.)
            Stmt::LetTuple { locals, init, .. } => {
                self.walk(init, depth);
                if self.tracks_region(init.ty) {
                    let r = self.region_of(init, depth);
                    for l in locals.iter().flatten() {
                        self.decl_depth.insert(*l, depth);
                        self.region.insert(*l, r);
                    }
                }
            }
        }
    }

    /// Recurse to find nested arenas and value positions that let a box escape.
    fn walk(&mut self, e: &Expr, depth: u32) {
        // A pipeline stage or reducer may carry capture operands (a lifted lambda's captured
        // enclosing locals); walk them so a captured value escaping its region is caught.
        if let Some(stages) = pipeline_stages(&e.kind) {
            for c in stage_capture_exprs(stages) {
                self.walk(c, depth);
            }
        }
        for c in node_captures(&e.kind) {
            self.walk(c, depth);
        }
        match &e.kind {
            ExprKind::Tuple { elems, .. } => {
                for el in elems {
                    self.walk(el, depth);
                }
            }
            ExprKind::MathOp { operands, .. } => {
                for o in operands {
                    self.walk(o, depth);
                }
            }
            ExprKind::TupleIndex { recv, .. } => self.walk(recv, depth),
            ExprKind::Arena(b) => {
                let inner = depth + 1;
                self.block(b, inner);
                if let Some(v) = &b.value {
                    // The block's value escapes to the enclosing region (`Region::arena(depth)`);
                    // a value bound to this inner arena cannot outlive it.
                    if self.tracks_region(v.ty) && !self.region_of(v, inner).outlives(Region::arena(depth)) {
                        self.diags.error(
                            "a value allocated in this arena cannot escape as the block's value".to_string(),
                            v.span,
                        );
                    }
                }
            }
            ExprKind::Block(b) => self.block(b, depth),
            // `unsafe {}` is a plain marker block for escape purposes — walk it at the same depth.
            ExprKind::Unsafe(b) => self.block(b, depth),
            // ④b: `task_group` opens a region (its task boxes live there), like `arena {}` — so a
            // region value (e.g. a `Task` handle) cannot escape as the block's value.
            ExprKind::TaskGroup(b) => {
                let inner = depth + 1;
                self.block(b, inner);
                if let Some(v) = &b.value
                    && self.tracks_region(v.ty) && !self.region_of(v, inner).outlives(Region::arena(depth)) {
                        self.diags.error(
                            "a value from this task_group cannot escape as the block's value".to_string(),
                            v.span,
                        );
                    }
            }
            ExprKind::Spawn { closure, .. } => self.walk(closure, depth),
            ExprKind::EnumValue { payload, .. } => {
                for p in payload {
                    self.walk(p, depth);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.walk(scrutinee, depth);
                // A payload bound by an arm pattern (`Some(v)` / `Ok(v)` / `Variant(v)`) is extracted
                // *out of* the scrutinee — for a region-tracked scrutinee it is a view into / part of
                // the same storage, so the binding inherits the scrutinee's region. This mirrors
                // `LetTuple` destructuring and the `OptionSome`/`Try` region pass-through in reverse.
                // Without it, unwrapping an `Option<view>` / `Result<view>` through a `match` arm loses
                // the region (`region_of(Local)` defaults to `Static`) and the view escapes — the
                // general #297-class use-after-free that first bit `resp.header()`'s `Option<str>` view
                // (env.get's `Option<string>` is owned/`Static`, so it never exposed this gap). A
                // non-tracked (scalar) payload binding needs no region — the guard skips it, and a
                // Copy binding's region is never consulted anyway.
                if self.tracks_region(scrutinee.ty) {
                    let sr = self.region_of(scrutinee, depth);
                    for a in arms {
                        for b in &a.bindings {
                            self.decl_depth.insert(*b, depth);
                            self.region.insert(*b, sr);
                        }
                    }
                }
                for a in arms {
                    self.walk(&a.body, depth);
                }
            }
            ExprKind::ResultMapErr { result, f } => {
                self.walk(result, depth);
                self.walk(f, depth);
            }
            ExprKind::TaskGet(inner) => self.walk(inner, depth),
            ExprKind::Wait => {}
            ExprKind::If { cond, then, els } => {
                self.walk(cond, depth);
                self.block(then, depth);
                self.block(els, depth);
            }
            ExprKind::Unary { expr, .. } | ExprKind::Cast(expr) => self.walk(expr, depth),
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::IntArith { lhs, rhs, .. } => {
                self.walk(lhs, depth);
                self.walk(rhs, depth);
            }
            ExprKind::Call { args, .. } => {
                for a in args {
                    self.walk(a, depth);
                }
            }
            // A fn value is a `Static` pointer (no region); an indirect call recurses its parts.
            ExprKind::FnValue(_) => {}
            // A capturing closure's env is frame-local and the closure cannot leave the frame
            // (no fn-typed returns/fields/parameters), so there is nothing to escape-check; just
            // recurse the captured values.
            ExprKind::Closure { captures, .. } => {
                for c in captures {
                    self.walk(c, depth);
                }
            }
            ExprKind::CallFnValue { callee, args } => {
                self.walk(callee, depth);
                for a in args {
                    self.walk(a, depth);
                }
            }
            ExprKind::StructLit { fields, .. } => {
                // No per-field rejection: the struct *carries* the region of its fields
                // (`region_of`), and escape is checked when the whole struct is returned /
                // stored / used as an arena block value. Just recurse for nested escapes.
                for f in fields {
                    self.walk(f, depth);
                }
            }
            ExprKind::OptionSome(i) | ExprKind::ResultOk(i) | ExprKind::ResultErr(i)
            | ExprKind::Try(i) | ExprKind::HeapNew(i) | ExprKind::RawAlloc(i) | ExprKind::RawFree(i) | ExprKind::BoxGet(i)
            | ExprKind::BoxClone(i) | ExprKind::StrClone(i) | ExprKind::StrBorrow(i) | ExprKind::BuilderToString(i) | ExprKind::ArraySum { source: i, .. } | ExprKind::ArrayCount { source: i, .. } | ExprKind::ArrayAnyAll { source: i, .. } | ExprKind::ArrayMinMax { source: i, .. } | ExprKind::ArrayToArray { source: i, .. } | ExprKind::ArrayToSoa { source: i, .. } | ExprKind::ArrayPartition { source: i, .. } | ExprKind::ArrayParMap { source: i, .. } | ExprKind::ArraySort { source: i, .. } | ExprKind::ArraySortBy { source: i, .. } | ExprKind::ArrayToSlice(i)
            | ExprKind::Len(i) => self.walk(i, depth),
            ExprKind::Index { recv, index } | ExprKind::ElemField { recv, index, .. } => {
                self.walk(recv, depth);
                self.walk(index, depth);
            }
            ExprKind::RawLoad { ptr, offset, .. } | ExprKind::RawOffset { ptr, offset } => {
                self.walk(ptr, depth);
                self.walk(offset, depth);
            }
            ExprKind::RawStore { ptr, offset, value } => {
                self.walk(ptr, depth);
                self.walk(offset, depth);
                self.walk(value, depth);
            }
            ExprKind::SliceRange { recv, start, end } => {
                self.walk(recv, depth);
                if let Some(s) = start { self.walk(s, depth); }
                if let Some(e) = end { self.walk(e, depth); }
            }
            ExprKind::BuilderWrite { builder, arg, .. } => {
                self.walk(builder, depth);
                self.walk(arg, depth);
            }
            ExprKind::StrPredicate { haystack, needle, .. } => {
                self.walk(haystack, depth);
                self.walk(needle, depth);
            }
            ExprKind::StrTrim { recv, .. } => self.walk(recv, depth),
            ExprKind::ArrayReduce { source, init, .. } | ExprKind::ArrayScan { source, init, .. } => {
                self.walk(source, depth);
                self.walk(init, depth);
            }
            // `map_into` reads its source and writes `dst` — recurse into both (the destination
            // is a place read of the `out`/`mut` slice; nothing is moved out of it).
            ExprKind::ArrayMapInto { source, dst, .. } => {
                self.walk(source, depth);
                self.walk(dst, depth);
            }
            ExprKind::ArrayDot { a, b, .. } => {
                self.walk(a, depth);
                self.walk(b, depth);
            }
            ExprKind::ArrayChunks { source, n, .. } => {
                self.walk(source, depth);
                self.walk(n, depth);
            }
            ExprKind::ArrayLit { elems, .. } | ExprKind::VecLit { elems, .. } => {
                for e in elems {
                    self.walk(e, depth);
                }
            }
            ExprKind::Select { mask, a, b } => {
                self.walk(mask, depth);
                self.walk(a, depth);
                self.walk(b, depth);
            }
            ExprKind::VecSumWhere { vec, mask } => {
                self.walk(vec, depth);
                self.walk(mask, depth);
            }
            ExprKind::VecDot { a, b } => {
                self.walk(a, depth);
                self.walk(b, depth);
            }
            ExprKind::VecMinMax { vec, .. } => self.walk(vec, depth),
            ExprKind::VecSum { vec } => self.walk(vec, depth),
            ExprKind::VecLoad { src, index, .. } => {
                self.walk(src, depth);
                self.walk(index, depth);
            }
            ExprKind::VecStore { dst, index, value, .. } => {
                self.walk(dst, depth);
                self.walk(index, depth);
                self.walk(value, depth);
            }
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.walk(opt, depth);
                self.walk(fallback, depth);
            }
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Hole(h) | TemplatePart::JsonStr(h) = p {
                        self.walk(h, depth);
                    }
                }
            }
            ExprKind::JsonDecode { input, .. } | ExprKind::JsonDecodeArray { input, .. } | ExprKind::JsonDecodeStructArray { input, .. } | ExprKind::JsonDecodeSoa { input, .. } => self.walk(input, depth),
            ExprKind::FsReadFile { path } | ExprKind::ReaderOpen { path } | ExprKind::WriterCreate { path }
            | ExprKind::FsExists { path } | ExprKind::FsRemove { path } | ExprKind::FsReadDir { path }
            | ExprKind::FsReadFileView { path } => self.walk(path, depth),
            ExprKind::DnsResolve { host } => self.walk(host, depth),
            ExprKind::TcpConnect { host, port } => {
                self.walk(host, depth);
                self.walk(port, depth);
            }
            ExprKind::ConnReader { conn } | ExprKind::ConnWriter { conn } => self.walk(conn, depth),
            ExprKind::TcpListen { host, port } => {
                self.walk(host, depth);
                self.walk(port, depth);
            }
            ExprKind::TcpAccept { listener } => self.walk(listener, depth),
            ExprKind::UdpBind { host, port } => {
                self.walk(host, depth);
                self.walk(port, depth);
            }
            ExprKind::UdpSendTo { sock, data, host, port } => {
                self.walk(sock, depth);
                self.walk(data, depth);
                self.walk(host, depth);
                self.walk(port, depth);
            }
            ExprKind::UdpRecvFrom { sock, buffer } => {
                self.walk(sock, depth);
                self.walk(buffer, depth);
            }
            ExprKind::FsWriteFile { path, data, .. } => {
                self.walk(path, depth);
                self.walk(data, depth);
            }
            ExprKind::PathComponent { path, .. } | ExprKind::PathNormalize { path } => self.walk(path, depth),
            ExprKind::PathJoin { a, b } => {
                self.walk(a, depth);
                self.walk(b, depth);
            }
            ExprKind::EnvGet { name } => self.walk(name, depth),
            ExprKind::EnvSet { name, value } => {
                self.walk(name, depth);
                self.walk(value, depth);
            }
            ExprKind::TimeNow | ExprKind::TimeInstant => {}
            ExprKind::TimeSleep { ns } => self.walk(ns, depth),
            // `process.exit` diverges and its `code` is a scalar `i64` (nothing escapes); `abort`
            // has no operand.
            ExprKind::ProcessExit { code } => self.walk(code, depth),
            ExprKind::ProcessAbort => {}
            // `spawn`'s `cmd`/`args` are borrowed views (nothing escapes); `wait`'s `child` is a
            // borrowed handle — recurse only.
            ExprKind::ProcessSpawn { cmd, args } => {
                self.walk(cmd, depth);
                self.walk(args, depth);
            }
            ExprKind::ChildWait { child } => self.walk(child, depth),
            // `kill`'s `child` is a borrowed handle + `sig` a scalar; `exec`'s `cmd`/`args` are borrowed
            // views (nothing escapes) — recurse only.
            ExprKind::ChildKill { child, sig } => {
                self.walk(child, depth);
                self.walk(sig, depth);
            }
            ExprKind::ProcessExec { cmd, args } => {
                self.walk(cmd, depth);
                self.walk(args, depth);
            }
            ExprKind::EncodingEncode { data, .. } | ExprKind::Utf8Valid { data } => self.walk(data, depth),
            ExprKind::EncodingDecode { input, .. } => self.walk(input, depth),
            // `std.compress` — the owned `buffer` result borrows nothing from `data`; just recurse
            // into the operands so any escape inside them is still checked.
            ExprKind::Compress { data, level, .. } => {
                self.walk(data, depth);
                self.walk(level, depth);
            }
            ExprKind::Decompress { data, .. } => self.walk(data, depth),
            // `std.rand`: an `rng` is Copy/`Static` (borrows nothing); `sample` returns a fresh owned
            // `array<T>` that borrows nothing from `xs`. Nothing escapes — just recurse into the
            // subexpressions so any escape *inside* them (a captured local, etc.) is still checked.
            ExprKind::RandSeed => {}
            ExprKind::RandSeedWith { seed } => self.walk(seed, depth),
            ExprKind::RandNext { rng } => self.walk(rng, depth),
            ExprKind::RandRange { rng, lo, hi } => {
                self.walk(rng, depth);
                self.walk(lo, depth);
                self.walk(hi, depth);
            }
            ExprKind::RandShuffle { rng, xs, .. } => {
                self.walk(rng, depth);
                self.walk(xs, depth);
            }
            ExprKind::RandSample { rng, xs, k, .. } => {
                self.walk(rng, depth);
                self.walk(xs, depth);
                self.walk(k, depth);
            }
            // `std.cli`: the command / parsed handles are owned Move (never region-borrows); a
            // `get_str` view borrows `parsed` but its escape is caught by `region_of` (a `Frame` view),
            // not here — just recurse so an escape *inside* the operands is still checked.
            ExprKind::CliCommand { name } => self.walk(name, depth),
            ExprKind::CliFlag { cmd, name, default, .. } => {
                self.walk(cmd, depth);
                self.walk(name, depth);
                if let Some(d) = default {
                    self.walk(d, depth);
                }
            }
            ExprKind::CliParse { cmd, args } => {
                self.walk(cmd, depth);
                self.walk(args, depth);
            }
            ExprKind::CliGetBool { parsed, name } | ExprKind::CliGetI64 { parsed, name } | ExprKind::CliGetStr { parsed, name } => {
                self.walk(parsed, depth);
                self.walk(name, depth);
            }
            ExprKind::CliUsage { cmd } => self.walk(cmd, depth),
            // `std.http`: the request / response handles are owned Move (never region-borrows); a
            // `resp.header`/`resp.body` view borrows `resp` but its escape is caught by `region_of` /
            // `slice_is_local`, not here — just recurse to check an escape *inside* the operands.
            ExprKind::HttpRequest { method, url } => {
                self.walk(method, depth);
                self.walk(url, depth);
            }
            ExprKind::HttpHeader { req, name, value } => {
                self.walk(req, depth);
                self.walk(name, depth);
                self.walk(value, depth);
            }
            ExprKind::HttpBody { req, data } => {
                self.walk(req, depth);
                self.walk(data, depth);
            }
            ExprKind::HttpParse { data } => self.walk(data, depth),
            ExprKind::HttpRespStatus { resp } | ExprKind::HttpRespBody { resp } => self.walk(resp, depth),
            ExprKind::HttpRespHeader { resp, name } => {
                self.walk(resp, depth);
                self.walk(name, depth);
            }
            // `std.http` (Slice 2): `get`/`post`/`request` return `Result<response, Error>` whose
            // `response` is **owned** (not a view) — it escapes fine, like `http.parse`'s result — so
            // there is no `region_of` arm. `client`/`req` are owned handles (never region-borrows).
            // Just recurse to catch an escape *inside* the operands.
            ExprKind::HttpClient => {}
            ExprKind::HttpClientGet { client, url } => {
                self.walk(client, depth);
                self.walk(url, depth);
            }
            ExprKind::HttpClientPost { client, url, body } => {
                self.walk(client, depth);
                self.walk(url, depth);
                self.walk(body, depth);
            }
            ExprKind::HttpClientRequest { client, req } => {
                self.walk(client, depth);
                self.walk(req, depth);
            }
            // `std.crypto` — `constant_time_equal` returns a Copy `bool` (borrows nothing); `random`
            // fills the `buffer` in place (returns `()`, nothing escapes). Just recurse into the
            // operands so any escape *inside* them is still checked.
            ExprKind::CryptoCtEqual { a, b } => {
                self.walk(a, depth);
                self.walk(b, depth);
            }
            ExprKind::CryptoRandom { out } => self.walk(out, depth),
            // `crypto.sha256`/`sha512` return a fresh *owned* `array<u8>` that borrows nothing (it
            // owns its heap buffer, `Drop`-freed) — freely returnable, like `rand.sample`. Just
            // recurse into the byte view so any escape *inside* it is still checked.
            ExprKind::CryptoHash { data, .. } => self.walk(data, depth),
            // `crypto.hmac_sha256` returns a fresh owned `array<u8>` (borrows nothing);
            // `crypto.hkdf_sha256` a fresh owned `buffer` inside a `Result` — both freely returnable.
            // Recurse into the operands so any escape inside them is still checked.
            ExprKind::CryptoHmac { key, data } => {
                self.walk(key, depth);
                self.walk(data, depth);
            }
            ExprKind::CryptoHkdf { salt, ikm, info, len } => {
                self.walk(salt, depth);
                self.walk(ikm, depth);
                self.walk(info, depth);
                self.walk(len, depth);
            }
            // AEAD seal/open return a fresh owned `buffer` inside a `Result` (borrows nothing) — freely
            // returnable. Recurse into the operands so any escape inside them is still checked.
            ExprKind::CryptoAead { key, nonce, input, aad, .. } => {
                self.walk(key, depth);
                self.walk(nonce, depth);
                self.walk(input, depth);
                self.walk(aad, depth);
            }
            // `crypto.argon2id` returns a fresh owned `buffer` inside a `Result` (borrows nothing) —
            // freely returnable. Recurse into the operands so any escape inside them is still checked.
            ExprKind::CryptoArgon2 { password, salt, params } => {
                self.walk(password, depth);
                self.walk(salt, depth);
                self.walk(params, depth);
            }
            ExprKind::WriterWrite { writer, arg, .. } => {
                self.walk(writer, depth);
                self.walk(arg, depth);
            }
            ExprKind::WriterFlush { writer } => self.walk(writer, depth),
            ExprKind::ReaderRead { reader, buffer } => {
                self.walk(reader, depth);
                self.walk(buffer, depth);
            }
            ExprKind::IoCopy { reader, writer } => {
                self.walk(reader, depth);
                self.walk(writer, depth);
            }
            ExprKind::BufferBytes { buffer } | ExprKind::BufferLen { buffer } => self.walk(buffer, depth),
            ExprKind::BufferNew { capacity } => self.walk(capacity, depth),
            ExprKind::BuilderNew { capacity } => {
                if let Some(c) = capacity {
                    self.walk(c, depth);
                }
            }
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Local(_)
            | ExprKind::OptionNone
            | ExprKind::WriterStd { .. }
            | ExprKind::ReaderStdin
            | ExprKind::Field { .. }
            | ExprKind::SoaColumn { .. }
            | ExprKind::ArrayGroupAgg { .. }
            | ExprKind::ArrayGroupAggMulti { .. }
            | ExprKind::ArrayDictEncode { .. }
            | ExprKind::IndexField { .. } => {}
        }
    }
}

/// The "unnecessary heap" lint, broad form (`draft.md` §16, `open-questions.md` M8 lint candidates).
///
/// The narrow slice (in `finalize_expr`) flags the inline `heap.new(x).get()`. This is the common
/// shape it deliberately left out: a box bound to a local that is *only* ever read back with `.get()`
/// and never escapes — `p := heap.new(x); … p.get()`. A `box<T>` payload is a scalar in M3, so `.get()`
/// is a plain copy-out; a box that is only copied-out and never moved / stored / returned / cloned /
/// captured serves no purpose (a stack value suffices), so it is a **warning** (the code still
/// type-checks and runs).
///
/// It is a whole-function box-use scan — the escape pass keeps no reusable per-box "escaped?" fact to
/// piggyback on. One linear pass over the body classifies every *occurrence* of every box local: a
/// `BoxGet` whose receiver is the local is a "get" occurrence, any *other* appearance (a move, a store,
/// a return, a `.clone()`, a function-call argument, a closure/pipeline capture, a reassignment target)
/// is an "other" occurrence. The lint fires for a box local iff it has at least one get and **no** other
/// occurrence — sound and conservative: any occurrence the walk does not recognize as a get suppresses
/// the warning. The `match` on `ExprKind` is exhaustive (no wildcard) so a future IR variant that could
/// carry a box use forces a compile error here rather than silently escaping the classification.
struct UnnecessaryHeapScan {
    /// Box locals (a `Let` whose init is `HeapNew`) → the span of the allocation (`heap.new`).
    candidates: std::collections::HashMap<LocalId, Span>,
    /// Per local: how many times it is the direct receiver of a `.get()`.
    get_uses: std::collections::HashMap<LocalId, u32>,
    /// Per local: how many times it appears in any *other* position (a use that needs the heap box).
    other_uses: std::collections::HashMap<LocalId, u32>,
}

impl UnnecessaryHeapScan {
    fn record_get(&mut self, l: LocalId) {
        *self.get_uses.entry(l).or_insert(0) += 1;
    }

    fn record_other(&mut self, l: LocalId) {
        *self.other_uses.entry(l).or_insert(0) += 1;
    }

    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            self.stmt(s);
        }
        if let Some(v) = &b.value {
            self.visit(v);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            // The defining binding: `local` is a definition, not a use, so it is never recorded. A
            // `heap.new` init makes `local` a candidate; visit the init either way (its boxed value
            // may itself reference locals).
            Stmt::Let { local, init } => {
                if matches!(init.kind, ExprKind::HeapNew(_)) {
                    self.candidates.insert(*local, init.span);
                }
                self.visit(init);
            }
            // `local = value` — a reassignment target is a use of the local (it needs a live binding);
            // for a box that is an "other" occurrence that suppresses the lint.
            Stmt::Assign { local, value, .. } => {
                self.record_other(*local);
                self.visit(value);
            }
            Stmt::AssignVecLane { local, value, .. } => {
                self.record_other(*local);
                self.visit(value);
            }
            // The remaining assignment targets (a struct field root / array or soa base) are never a
            // box local — a box has no fields, no indexing — but recording them costs nothing and keeps
            // the "every LocalId target counts as other" rule uniform.
            Stmt::AssignField { root, value, .. } => {
                self.record_other(*root);
                self.visit(value);
            }
            Stmt::AssignIndex { base, index, value }
            | Stmt::AssignElemField { base, index, value, .. }
            | Stmt::AssignElem { base, index, value, .. } => {
                self.record_other(*base);
                self.visit(index);
                self.visit(value);
            }
            // Destructure binds fresh locals (definitions, and never `box` — no producer yields a
            // tuple of boxes); just visit the init.
            Stmt::LetTuple { init, .. } => self.visit(init),
            Stmt::Return(Some(e)) | Stmt::Expr(e) => self.visit(e),
            Stmt::Return(None) => {}
        }
    }

    fn visit(&mut self, e: &Expr) {
        // Capture operands (a lifted lambda's captured enclosing locals) live outside the normal child
        // recursion — a captured box is an "other" use, so classify them too.
        if let Some(stages) = pipeline_stages(&e.kind) {
            for c in stage_capture_exprs(stages) {
                self.visit(c);
            }
        }
        for c in node_captures(&e.kind) {
            self.visit(c);
        }
        match &e.kind {
            // `p.get()` — the one occurrence that does *not* need a heap box. When the receiver is a
            // bare local, record it as a get and do NOT recurse into the receiver (so the inner
            // `Local` is not double-counted as an "other" use). Any other receiver (e.g. the inline
            // `heap.new(x).get()`, handled by the narrow lint) recurses normally.
            ExprKind::BoxGet(inner) => {
                if let ExprKind::Local(l) = inner.kind {
                    self.record_get(l);
                } else {
                    self.visit(inner);
                }
            }
            // Any bare appearance of a local is an "other" use of it.
            ExprKind::Local(l) => self.record_other(*l),
            ExprKind::Tuple { elems, .. } => {
                for el in elems {
                    self.visit(el);
                }
            }
            ExprKind::MathOp { operands, .. } => {
                for o in operands {
                    self.visit(o);
                }
            }
            ExprKind::TupleIndex { recv, .. } => self.visit(recv),
            ExprKind::Arena(b) | ExprKind::Block(b) | ExprKind::Unsafe(b) | ExprKind::TaskGroup(b) => {
                self.block(b);
            }
            ExprKind::Spawn { closure, .. } => self.visit(closure),
            ExprKind::EnumValue { payload, .. } => {
                for p in payload {
                    self.visit(p);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.visit(scrutinee);
                for a in arms {
                    self.visit(&a.body);
                }
            }
            ExprKind::ResultMapErr { result, f } => {
                self.visit(result);
                self.visit(f);
            }
            ExprKind::TaskGet(inner) => self.visit(inner),
            ExprKind::Wait => {}
            ExprKind::If { cond, then, els } => {
                self.visit(cond);
                self.block(then);
                self.block(els);
            }
            ExprKind::Unary { expr, .. } | ExprKind::Cast(expr) => self.visit(expr),
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::IntArith { lhs, rhs, .. } => {
                self.visit(lhs);
                self.visit(rhs);
            }
            ExprKind::Call { args, .. } => {
                for a in args {
                    self.visit(a);
                }
            }
            ExprKind::FnValue(_) => {}
            ExprKind::Closure { captures, .. } => {
                for c in captures {
                    self.visit(c);
                }
            }
            ExprKind::CallFnValue { callee, args } => {
                self.visit(callee);
                for a in args {
                    self.visit(a);
                }
            }
            ExprKind::StructLit { fields, .. } => {
                for f in fields {
                    self.visit(f);
                }
            }
            ExprKind::OptionSome(i) | ExprKind::ResultOk(i) | ExprKind::ResultErr(i)
            | ExprKind::Try(i) | ExprKind::HeapNew(i) | ExprKind::RawAlloc(i) | ExprKind::RawFree(i)
            | ExprKind::BoxClone(i) | ExprKind::StrClone(i) | ExprKind::StrBorrow(i) | ExprKind::BuilderToString(i) | ExprKind::ArraySum { source: i, .. } | ExprKind::ArrayCount { source: i, .. } | ExprKind::ArrayAnyAll { source: i, .. } | ExprKind::ArrayMinMax { source: i, .. } | ExprKind::ArrayToArray { source: i, .. } | ExprKind::ArrayToSoa { source: i, .. } | ExprKind::ArrayPartition { source: i, .. } | ExprKind::ArrayParMap { source: i, .. } | ExprKind::ArraySort { source: i, .. } | ExprKind::ArraySortBy { source: i, .. } | ExprKind::ArrayToSlice(i)
            | ExprKind::Len(i) => self.visit(i),
            ExprKind::Index { recv, index } | ExprKind::ElemField { recv, index, .. } => {
                self.visit(recv);
                self.visit(index);
            }
            ExprKind::RawLoad { ptr, offset, .. } | ExprKind::RawOffset { ptr, offset } => {
                self.visit(ptr);
                self.visit(offset);
            }
            ExprKind::RawStore { ptr, offset, value } => {
                self.visit(ptr);
                self.visit(offset);
                self.visit(value);
            }
            ExprKind::SliceRange { recv, start, end } => {
                self.visit(recv);
                if let Some(s) = start { self.visit(s); }
                if let Some(en) = end { self.visit(en); }
            }
            ExprKind::BuilderWrite { builder, arg, .. } => {
                self.visit(builder);
                self.visit(arg);
            }
            ExprKind::StrPredicate { haystack, needle, .. } => {
                self.visit(haystack);
                self.visit(needle);
            }
            ExprKind::StrTrim { recv, .. } => self.visit(recv),
            ExprKind::ArrayReduce { source, init, .. } | ExprKind::ArrayScan { source, init, .. } => {
                self.visit(source);
                self.visit(init);
            }
            ExprKind::ArrayMapInto { source, dst, .. } => {
                self.visit(source);
                self.visit(dst);
            }
            ExprKind::ArrayDot { a, b, .. } => {
                self.visit(a);
                self.visit(b);
            }
            ExprKind::ArrayChunks { source, n, .. } => {
                self.visit(source);
                self.visit(n);
            }
            ExprKind::ArrayLit { elems, .. } | ExprKind::VecLit { elems, .. } => {
                for el in elems {
                    self.visit(el);
                }
            }
            ExprKind::Select { mask, a, b } => {
                self.visit(mask);
                self.visit(a);
                self.visit(b);
            }
            ExprKind::VecSumWhere { vec, mask } => {
                self.visit(vec);
                self.visit(mask);
            }
            ExprKind::VecDot { a, b } => {
                self.visit(a);
                self.visit(b);
            }
            ExprKind::VecMinMax { vec, .. } => self.visit(vec),
            ExprKind::VecSum { vec } => self.visit(vec),
            ExprKind::VecLoad { src, index, .. } => {
                self.visit(src);
                self.visit(index);
            }
            ExprKind::VecStore { dst, index, value, .. } => {
                self.visit(dst);
                self.visit(index);
                self.visit(value);
            }
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.visit(opt);
                self.visit(fallback);
            }
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Hole(h) | TemplatePart::JsonStr(h) = p {
                        self.visit(h);
                    }
                }
            }
            ExprKind::JsonDecode { input, .. } | ExprKind::JsonDecodeArray { input, .. } | ExprKind::JsonDecodeStructArray { input, .. } | ExprKind::JsonDecodeSoa { input, .. } => self.visit(input),
            ExprKind::FsReadFile { path } | ExprKind::ReaderOpen { path } | ExprKind::WriterCreate { path }
            | ExprKind::FsExists { path } | ExprKind::FsRemove { path } | ExprKind::FsReadDir { path }
            | ExprKind::FsReadFileView { path } => self.visit(path),
            ExprKind::DnsResolve { host } => self.visit(host),
            ExprKind::TcpConnect { host, port } => {
                self.visit(host);
                self.visit(port);
            }
            ExprKind::ConnReader { conn } | ExprKind::ConnWriter { conn } => self.visit(conn),
            ExprKind::TcpListen { host, port } => {
                self.visit(host);
                self.visit(port);
            }
            ExprKind::TcpAccept { listener } => self.visit(listener),
            ExprKind::UdpBind { host, port } => {
                self.visit(host);
                self.visit(port);
            }
            ExprKind::UdpSendTo { sock, data, host, port } => {
                self.visit(sock);
                self.visit(data);
                self.visit(host);
                self.visit(port);
            }
            ExprKind::UdpRecvFrom { sock, buffer } => {
                self.visit(sock);
                self.visit(buffer);
            }
            ExprKind::FsWriteFile { path, data, .. } => {
                self.visit(path);
                self.visit(data);
            }
            ExprKind::PathComponent { path, .. } | ExprKind::PathNormalize { path } => self.visit(path),
            ExprKind::PathJoin { a, b } => {
                self.visit(a);
                self.visit(b);
            }
            ExprKind::EnvGet { name } => self.visit(name),
            ExprKind::EnvSet { name, value } => {
                self.visit(name);
                self.visit(value);
            }
            ExprKind::TimeNow | ExprKind::TimeInstant => {}
            ExprKind::TimeSleep { ns } => self.visit(ns),
            ExprKind::ProcessExit { code } => self.visit(code),
            ExprKind::ProcessAbort => {}
            ExprKind::ProcessSpawn { cmd, args } => {
                self.visit(cmd);
                self.visit(args);
            }
            ExprKind::ChildWait { child } => self.visit(child),
            ExprKind::ChildKill { child, sig } => {
                self.visit(child);
                self.visit(sig);
            }
            ExprKind::ProcessExec { cmd, args } => {
                self.visit(cmd);
                self.visit(args);
            }
            ExprKind::EncodingEncode { data, .. } | ExprKind::Utf8Valid { data } => self.visit(data),
            ExprKind::EncodingDecode { input, .. } => self.visit(input),
            // `std.compress` — recurse into the subexpressions (no heap-narrowing pattern of its own).
            ExprKind::Compress { data, level, .. } => {
                self.visit(data);
                self.visit(level);
            }
            ExprKind::Decompress { data, .. } => self.visit(data),
            // `std.rand` — recurse into the subexpressions (no heap-narrowing pattern of its own).
            ExprKind::RandSeed => {}
            ExprKind::RandSeedWith { seed } => self.visit(seed),
            ExprKind::RandNext { rng } => self.visit(rng),
            ExprKind::RandRange { rng, lo, hi } => {
                self.visit(rng);
                self.visit(lo);
                self.visit(hi);
            }
            ExprKind::RandShuffle { rng, xs, .. } => {
                self.visit(rng);
                self.visit(xs);
            }
            ExprKind::RandSample { rng, xs, k, .. } => {
                self.visit(rng);
                self.visit(xs);
                self.visit(k);
            }
            // `std.cli` — no heap-narrowing pattern of its own; recurse into the operands.
            ExprKind::CliCommand { name } => self.visit(name),
            ExprKind::CliFlag { cmd, name, default, .. } => {
                self.visit(cmd);
                self.visit(name);
                if let Some(d) = default {
                    self.visit(d);
                }
            }
            ExprKind::CliParse { cmd, args } => {
                self.visit(cmd);
                self.visit(args);
            }
            ExprKind::CliGetBool { parsed, name } | ExprKind::CliGetI64 { parsed, name } | ExprKind::CliGetStr { parsed, name } => {
                self.visit(parsed);
                self.visit(name);
            }
            ExprKind::CliUsage { cmd } => self.visit(cmd),
            // `std.http` — no heap-narrowing pattern of its own; recurse into the operands.
            ExprKind::HttpRequest { method, url } => {
                self.visit(method);
                self.visit(url);
            }
            ExprKind::HttpHeader { req, name, value } => {
                self.visit(req);
                self.visit(name);
                self.visit(value);
            }
            ExprKind::HttpBody { req, data } => {
                self.visit(req);
                self.visit(data);
            }
            ExprKind::HttpParse { data } => self.visit(data),
            ExprKind::HttpRespStatus { resp } | ExprKind::HttpRespBody { resp } => self.visit(resp),
            ExprKind::HttpRespHeader { resp, name } => {
                self.visit(resp);
                self.visit(name);
            }
            ExprKind::HttpClient => {}
            ExprKind::HttpClientGet { client, url } => {
                self.visit(client);
                self.visit(url);
            }
            ExprKind::HttpClientPost { client, url, body } => {
                self.visit(client);
                self.visit(url);
                self.visit(body);
            }
            ExprKind::HttpClientRequest { client, req } => {
                self.visit(client);
                self.visit(req);
            }
            // `std.crypto` — recurse into the subexpressions (no heap-narrowing pattern of its own).
            ExprKind::CryptoCtEqual { a, b } => {
                self.visit(a);
                self.visit(b);
            }
            ExprKind::CryptoRandom { out } => self.visit(out),
            ExprKind::CryptoHash { data, .. } => self.visit(data),
            ExprKind::CryptoHmac { key, data } => {
                self.visit(key);
                self.visit(data);
            }
            ExprKind::CryptoHkdf { salt, ikm, info, len } => {
                self.visit(salt);
                self.visit(ikm);
                self.visit(info);
                self.visit(len);
            }
            ExprKind::CryptoAead { key, nonce, input, aad, .. } => {
                self.visit(key);
                self.visit(nonce);
                self.visit(input);
                self.visit(aad);
            }
            ExprKind::CryptoArgon2 { password, salt, params } => {
                self.visit(password);
                self.visit(salt);
                self.visit(params);
            }
            ExprKind::WriterWrite { writer, arg, .. } => {
                self.visit(writer);
                self.visit(arg);
            }
            ExprKind::WriterFlush { writer } => self.visit(writer),
            ExprKind::ReaderRead { reader, buffer } => {
                self.visit(reader);
                self.visit(buffer);
            }
            ExprKind::IoCopy { reader, writer } => {
                self.visit(reader);
                self.visit(writer);
            }
            ExprKind::BufferBytes { buffer } | ExprKind::BufferLen { buffer } => self.visit(buffer),
            ExprKind::BufferNew { capacity } => self.visit(capacity),
            ExprKind::BuilderNew { capacity } => {
                if let Some(c) = capacity {
                    self.visit(c);
                }
            }
            // Leaves and nodes whose only local references (a `Field`/`SoaColumn`/`IndexField` /
            // group-agg base) are never a box local — a box has no fields, columns, or indexing.
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::OptionNone
            | ExprKind::WriterStd { .. }
            | ExprKind::ReaderStdin
            | ExprKind::Field { .. }
            | ExprKind::SoaColumn { .. }
            | ExprKind::ArrayGroupAgg { .. }
            | ExprKind::ArrayGroupAggMulti { .. }
            | ExprKind::ArrayDictEncode { .. }
            | ExprKind::IndexField { .. } => {}
        }
    }

    /// Run the scan over a function body and emit one warning per unnecessary box local.
    fn run(body: &Block, diags: &mut Diagnostics) {
        let mut scan = UnnecessaryHeapScan {
            candidates: std::collections::HashMap::new(),
            get_uses: std::collections::HashMap::new(),
            other_uses: std::collections::HashMap::new(),
        };
        scan.block(body);
        // Emit in a deterministic order (by span start) so diagnostics are stable.
        let mut fire: Vec<Span> = scan
            .candidates
            .iter()
            .filter(|(l, _)| {
                scan.get_uses.get(*l).copied().unwrap_or(0) >= 1
                    && scan.other_uses.get(*l).copied().unwrap_or(0) == 0
            })
            .map(|(_, span)| *span)
            .collect();
        fire.sort_by_key(|s| s.lo);
        for span in fire {
            diags.push(align_diag::Diagnostic::warning(
                "unnecessary heap allocation: this box is only ever read back with `.get()` and never escapes — use the value directly (a stack value suffices)".to_string(),
                span,
            ));
        }
    }
}

/// Whether a single HIR statement always diverges (control never proceeds to the next statement).
/// A `return` always diverges; a `let`/assignment/expression statement diverges iff the value it
/// evaluates does.
fn hir_stmt_diverges(s: &hir::Stmt) -> bool {
    match s {
        hir::Stmt::Return(_) => true,
        hir::Stmt::Let { init, .. } | hir::Stmt::LetTuple { init, .. } => hir_expr_diverges(init),
        hir::Stmt::Assign { value, .. } | hir::Stmt::AssignField { value, .. } => hir_expr_diverges(value),
        hir::Stmt::AssignIndex { index, value, .. }
        | hir::Stmt::AssignElemField { index, value, .. }
        | hir::Stmt::AssignElem { index, value, .. } => hir_expr_diverges(index) || hir_expr_diverges(value),
        hir::Stmt::AssignVecLane { value, .. } => hir_expr_diverges(value),
        hir::Stmt::Expr(e) => hir_expr_diverges(e),
    }
}

/// Whether a HIR block **always diverges** (never falls through to its end) — used by `MoveCheck`
/// to drop a diverging branch's moves at an `if` join (they happen on a path that never reaches
/// past the `if`). Conservative: only `true` when divergence is certain (any statement that always
/// diverges — including an intermediate one, after which the rest is dead — or a tail `if`/block
/// that itself diverges); anything else is `false`, falling back to the safe union.
fn hir_block_diverges(b: &hir::Block) -> bool {
    if b.stmts.iter().any(hir_stmt_diverges) {
        return true;
    }
    if let Some(v) = &b.value {
        return hir_expr_diverges(v);
    }
    false
}

/// Whether a HIR expression in tail position always diverges. An `if` diverges only when **both**
/// arms do; a block-wrapping expr (`{…}` / `arena {…}` / `task_group {…}`) defers to its block.
/// (A `match` / `?` may fall through, so they are conservatively non-diverging here.)
fn hir_expr_diverges(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::If { then, els, .. } => hir_block_diverges(then) && hir_block_diverges(els),
        ExprKind::Block(b) | ExprKind::Arena(b) | ExprKind::TaskGroup(b) => hir_block_diverges(b),
        _ => false,
    }
}

/// Flow analysis that flags use-after-move. A Move-typed value (M3: `box<T>`) is
/// consumed when bound/assigned/passed/returned by value; using it afterwards is an
/// error. Borrowing positions (`.get()`/`.clone()` receiver, operands) do not consume.
struct MoveCheck<'a> {
    f: &'a Fn,
    diags: &'a mut Diagnostics,
    /// Tuple defs — so a Move tuple (one with an owned element) is recognised as a Move type and
    /// its consumption (pass / destructure / return) is tracked for use-after-move.
    tuples: &'a [hir::TupleDef],
    /// Struct defs — so a Move struct (one that owns a `string`/owned field, transitively) is
    /// recognised as a Move type for use-after-move tracking (Slice 3).
    structs: &'a [StructDef],
}

/// What has been moved out of a local. A whole-local move (`a := xs`, `f(xs)`, destructure) and a
/// partial tuple-field move (`a := t.0`, moving one owned element) coexist: each owned tuple field
/// can be moved out independently, after which the tuple may no longer be used as a whole.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum MovedKey {
    Whole(LocalId),
    Field(LocalId, u32),
}

type MovedSet = std::collections::HashSet<MovedKey>;
/// A matchable type's variants: `(variant name, positional payload scalars)`, see
/// `Sema::match_variants`.
type VariantList = Vec<(String, Vec<Scalar>)>;

/// `id` is unusable as a whole if it was wholly moved or *any* of its fields was moved.
fn whole_moved(moved: &MovedSet, id: LocalId) -> bool {
    moved.contains(&MovedKey::Whole(id)) || moved.iter().any(|k| matches!(k, MovedKey::Field(l, _) if *l == id))
}

/// Field `n` of `id` is unusable if it (or the whole local) was moved.
fn field_moved(moved: &MovedSet, id: LocalId, n: u32) -> bool {
    moved.contains(&MovedKey::Field(id, n)) || moved.contains(&MovedKey::Whole(id))
}

/// Re-binding a local (`x := …`) clears every move record for it (whole and per-field).
fn clear_moved(moved: &mut MovedSet, id: LocalId) {
    moved.retain(|k| !matches!(k, MovedKey::Whole(l) | MovedKey::Field(l, _) if *l == id));
}

impl<'a> MoveCheck<'a> {
    fn check(mut self) {
        let mut moved: MovedSet = std::collections::HashSet::new();
        // If the function returns a Move type, its body's trailing expression is consumed by
        // the return: a bare owned local there (`fn make() -> array<i32> { ys := ...; ys }`) is
        // moved out to the caller (MIR nulls its slot so it is not also freed at exit).
        let ret_is_move = self.is_move_ty(self.f.ret);
        self.block(&self.f.body, &mut moved, ret_is_move, true);
    }

    /// Whether `ty` is a Move type (owns a heap buffer consumed on move) — including a Move tuple
    /// or Move struct.
    fn is_move_ty(&self, ty: Ty) -> bool {
        ty_is_move(ty, self.structs, self.tuples)
    }

    fn is_move(&self, id: LocalId) -> bool {
        match self.f.locals.get(id as usize).map(|l| l.ty) {
            Some(ty) => self.is_move_ty(ty),
            None => false,
        }
    }

    /// `tail_consuming` = whether the block's trailing value is consumed by its context;
    /// `tail_direct` = whether that consuming position is a "direct" move site (a statement /
    /// return / the function tail) rather than nested inside a branching expression (`if`).
    /// MIR nulls a moved owned local's slot only at direct sites, so a move of a *bound* owned
    /// local through an `if`/`else` arm is rejected here (deferred — bind it to a local first).
    fn block(
        &mut self,
        b: &Block,
        moved: &mut MovedSet,
        tail_consuming: bool,
        tail_direct: bool,
    ) {
        for s in &b.stmts {
            match s {
                Stmt::Let { local, init } => {
                    self.expr(init, moved, true, true);
                    clear_moved(moved, *local);
                }
                Stmt::Assign { local, value, drop_old } => {
                    let was_moved = whole_moved(moved, *local);
                    self.expr(value, moved, true, true);
                    // The RHS consumed the old value iff it just transitioned the local live→moved
                    // (it appeared in a consuming position). If so, ownership of the old buffer
                    // transferred away — MIR must NOT drop it here (double-free). Otherwise the
                    // overwritten owned value must be dropped before the store (else its buffer
                    // leaks); a no-op `free(null)` if the slot was already moved/null. Non-owned
                    // locals never drop. (`s = make(s.len())` borrows, not moves → still drops.)
                    let consumed_by_rhs = whole_moved(moved, *local) && !was_moved;
                    drop_old.set(self.is_move(*local) && !consumed_by_rhs);
                    clear_moved(moved, *local);
                }
                // `root.field = value` — writing a field is a use of `root` (an owned struct could
                // have been moved away), so flag use-after-move on it, mirroring the `AssignIndex`
                // check below (same diagnostic; the field write has no index expr to span, so it
                // points at the RHS instead).
                Stmt::AssignField { root, value, .. } => {
                    if whole_moved(moved, *root) {
                        let name = &self.f.locals[*root as usize].name;
                        self.diags.error(format!("use of moved value '{name}'"), value.span);
                    }
                    self.expr(value, moved, true, true);
                }
                // `base[index] = value` / `base[index].field = value` — writing an element is a use
                // of `base` (an owned array could have been moved away), so flag use-after-move on
                // it; index and value are read (not moved; Copy).
                Stmt::AssignIndex { base, index, value }
                | Stmt::AssignElemField { base, index, value, .. }
                | Stmt::AssignElem { base, index, value, .. } => {
                    if whole_moved(moved, *base) {
                        let name = &self.f.locals[*base as usize].name;
                        self.diags.error(format!("use of moved value '{name}'"), index.span);
                    }
                    self.expr(index, moved, false, false);
                    self.expr(value, moved, false, false);
                }
                Stmt::AssignVecLane { value, .. } => self.expr(value, moved, false, false),
                Stmt::Return(Some(e)) => self.expr(e, moved, true, true),
                Stmt::Return(None) => {}
                Stmt::Expr(e) => self.expr(e, moved, false, false),
                // Destructure consumes its tuple source whole (see the `Local` arm in `expr`).
                Stmt::LetTuple { locals, init, .. } => {
                    self.expr(init, moved, true, true);
                    for l in locals.iter().flatten() {
                        clear_moved(moved, *l);
                    }
                }
            }
        }
        if let Some(v) = &b.value {
            self.expr(v, moved, tail_consuming, tail_direct);
        }
    }

    /// `consuming` = this position takes a Move value by value (so it moves it). `direct` = the
    /// consuming position is a direct move site (see [`block`]); a non-direct owned-local move
    /// is a deferred-feature error.
    fn expr(
        &mut self,
        e: &Expr,
        moved: &mut MovedSet,
        consuming: bool,
        direct: bool,
    ) {
        // A pipeline stage or reducer may carry capture operands (a lifted lambda's captured
        // enclosing locals); walk them as borrows so use-after-move of a captured value is caught.
        if let Some(stages) = pipeline_stages(&e.kind) {
            for c in stage_capture_exprs(stages) {
                self.expr(c, moved, false, false);
            }
        }
        for c in node_captures(&e.kind) {
            self.expr(c, moved, false, false);
        }
        match &e.kind {
            ExprKind::Local(id) => {
                if whole_moved(moved, *id) {
                    let name = &self.f.locals[*id as usize].name;
                    self.diags.error(format!("use of moved value '{name}'"), e.span);
                } else if consuming && self.is_move(*id) {
                    if !direct {
                        let name = &self.f.locals[*id as usize].name;
                        self.diags.error(
                            format!(
                                "cannot move owned value '{name}' out through a conditional \
                                 expression yet; bind the `if`/`else` result to a local first"
                            ),
                            e.span,
                        );
                    }
                    moved.insert(MovedKey::Whole(*id));
                }
            }
            ExprKind::Field { root: base, path } => {
                if path.len() == 1 {
                    let fld = path[0];
                    if field_moved(moved, *base, fld) {
                        // The whole struct, or just this field, was already moved out — name the
                        // field in the latter case (the struct stays partially usable), like a tuple.
                        let name = &self.f.locals[*base as usize].name;
                        let msg = if moved.contains(&MovedKey::Whole(*base)) {
                            format!("use of moved value '{name}'")
                        } else {
                            let fld_name = match self.f.locals[*base as usize].ty {
                                Ty::Struct(sid) => self.structs[sid as usize].fields[fld as usize].name.as_str(),
                                _ => "field",
                            };
                            format!("use of moved field '{fld_name}' of '{name}'")
                        };
                        self.diags.error(msg, e.span);
                    } else if consuming && e.ty == Ty::String {
                        // A partial move of a depth-1 owned `string` field (`n := u.name`,
                        // `f(u.name)` by value, `return u.name`): mark just that field moved. The
                        // struct's recursive `Drop` frees null there (MIR nulls the field on move);
                        // the struct can no longer move as a whole, and the field can't be reused,
                        // but its other fields stay readable. A *borrow* (`u.name.len()`, a `str`
                        // argument) reaches here non-consuming (wrapped in `StrBorrow`/`Len`), so it
                        // is allowed and moves nothing.
                        moved.insert(MovedKey::Field(*base, fld));
                    } else if consuming && self.is_move_ty(e.ty) {
                        // A whole nested Move-struct field (`a := u.addr`) moved out is still
                        // deferred — it needs the whole sub-struct nulled, not a single `{ptr,len}`.
                        self.diags.error(
                            "moving a nested struct field out of a struct is not supported yet — clone it, or move the whole struct".to_string(),
                            e.span,
                        );
                    }
                } else {
                    // Depth ≥ 2 (`u.addr.name`): a borrow is fine; the read is invalid only if the
                    // root struct was moved (as a whole or in any field — conservative for deep
                    // reads). Moving a field out through a nested path is deferred.
                    if whole_moved(moved, *base) {
                        // A deep read is blocked by a whole-struct move or — conservatively — any
                        // partial field move; distinguish the two for a clearer message.
                        let name = &self.f.locals[*base as usize].name;
                        let msg = if moved.contains(&MovedKey::Whole(*base)) {
                            format!("use of moved value '{name}'")
                        } else {
                            format!("use of partially moved value '{name}'")
                        };
                        self.diags.error(msg, e.span);
                    } else if consuming && self.is_move_ty(e.ty) {
                        self.diags.error(
                            "moving an owned field out through a nested path is not supported yet — clone it".to_string(),
                            e.span,
                        );
                    }
                }
            }
            ExprKind::SoaColumn { base, .. } | ExprKind::ArrayGroupAgg { base, .. }
            | ExprKind::ArrayGroupAggMulti { base, .. }
            | ExprKind::ArrayDictEncode { base, .. } | ExprKind::IndexField { base, .. } => {
                if whole_moved(moved, *base) {
                    let name = &self.f.locals[*base as usize].name;
                    self.diags.error(format!("use of moved value '{name}'"), e.span);
                }
            }
            ExprKind::Unary { expr, .. } | ExprKind::Cast(expr) => self.expr(expr, moved, false, false),
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::IntArith { lhs, rhs, .. } => {
                self.expr(lhs, moved, false, false);
                self.expr(rhs, moved, false, false);
            }
            // Value arguments / wrapped payloads are consumed (a direct move site). `print` is a
            // read-only builtin, so it *borrows* its argument (a `string` printed once is still
            // usable — `print(s); s.len()`); it never takes ownership.
            ExprKind::Call { func, args, .. } => {
                let consuming = func != "print";
                for a in args {
                    self.expr(a, moved, consuming, consuming);
                }
            }
            // A fn value is Copy (a pointer); an indirect call's callee + args are reads.
            ExprKind::FnValue(_) => {}
            // A closure copies its captured (Copy) values into its env — reads, not moves.
            ExprKind::Closure { captures, .. } => {
                for c in captures {
                    self.expr(c, moved, false, false);
                }
            }
            ExprKind::CallFnValue { callee, args } => {
                self.expr(callee, moved, false, false);
                for a in args {
                    self.expr(a, moved, true, true);
                }
            }
            ExprKind::StructLit { fields, .. } => {
                for f in fields {
                    self.expr(f, moved, true, true);
                }
            }
            ExprKind::OptionSome(i) | ExprKind::ResultOk(i) | ExprKind::ResultErr(i)
            | ExprKind::Try(i) | ExprKind::HeapNew(i) => self.expr(i, moved, true, true),
            // `b.to_string()` consumes (moves) the builder; `b.write(...)` borrows it (and its
            // str/int arg). `builder()` is a leaf.
            ExprKind::BuilderToString(i) => self.expr(i, moved, true, true),
            ExprKind::BuilderWrite { builder, arg, .. } => {
                self.expr(builder, moved, false, false);
                self.expr(arg, moved, false, false);
            }
            ExprKind::BuilderNew { capacity } => {
                if let Some(c) = capacity {
                    self.expr(c, moved, false, false);
                }
            }
            // `w.write(x)` / `w.flush()` borrow the writer (and its arg); `r.read(b)` borrows both
            // reader and buffer (the buffer is filled in place, not consumed); `b.bytes()` / `b.len()`
            // borrow the buffer. The constructors (`io.stdout`, `buffer(cap)`) are leaves. None of
            // these Move handles is consumed — each is `Drop`-freed at scope exit.
            ExprKind::WriterWrite { writer, arg, .. } => {
                self.expr(writer, moved, false, false);
                self.expr(arg, moved, false, false);
            }
            ExprKind::WriterFlush { writer } => self.expr(writer, moved, false, false),
            ExprKind::ReaderRead { reader, buffer } => {
                self.expr(reader, moved, false, false);
                self.expr(buffer, moved, false, false);
            }
            // `io.copy(r, w)` borrows both handles (fd ownership does not move — neither is
            // consumed, so both stay usable after the call), like `print`'s argument. NOT a
            // consuming call, even though `reader`/`writer` are Move types.
            ExprKind::IoCopy { reader, writer } => {
                self.expr(reader, moved, false, false);
                self.expr(writer, moved, false, false);
            }
            ExprKind::BufferBytes { buffer } | ExprKind::BufferLen { buffer } => self.expr(buffer, moved, false, false),
            ExprKind::BufferNew { capacity } => self.expr(capacity, moved, false, false),
            ExprKind::WriterStd { .. } | ExprKind::ReaderStdin => {}
            // Both operands are borrowed (read for bytes), never consumed.
            ExprKind::StrPredicate { haystack, needle, .. } => {
                self.expr(haystack, moved, false, false);
                self.expr(needle, moved, false, false);
            }
            // The receiver is borrowed (the trimmed view aliases its bytes), never consumed.
            ExprKind::StrTrim { recv, .. } => self.expr(recv, moved, false, false),
            // The receiver is borrowed, not consumed.
            ExprKind::BoxGet(i) | ExprKind::BoxClone(i) | ExprKind::StrClone(i) | ExprKind::StrBorrow(i) | ExprKind::ArraySum { source: i, .. } | ExprKind::ArrayCount { source: i, .. } | ExprKind::ArrayAnyAll { source: i, .. } | ExprKind::ArrayMinMax { source: i, .. } | ExprKind::ArrayToArray { source: i, .. } | ExprKind::ArrayToSoa { source: i, .. } | ExprKind::ArrayPartition { source: i, .. } | ExprKind::ArrayParMap { source: i, .. } | ExprKind::ArraySort { source: i, .. } | ExprKind::ArraySortBy { source: i, .. } | ExprKind::ArrayToSlice(i)
            | ExprKind::Len(i) => {
                self.expr(i, moved, false, false)
            }
            // `recv[index]` / `recv[index].field` borrow the receiver (read an element) and read
            // the index.
            ExprKind::Index { recv, index } | ExprKind::ElemField { recv, index, .. } => {
                self.expr(recv, moved, false, false);
                self.expr(index, moved, false, false);
            }
            // A range slice borrows the receiver (a view, never consumed) and reads the bounds.
            ExprKind::SliceRange { recv, start, end } => {
                self.expr(recv, moved, false, false);
                if let Some(s) = start { self.expr(s, moved, false, false); }
                if let Some(e) = end { self.expr(e, moved, false, false); }
            }
            ExprKind::ArrayReduce { source, init, .. } | ExprKind::ArrayScan { source, init, .. } => {
                self.expr(source, moved, false, false);
                self.expr(init, moved, false, false);
            }
            // `map_into`: the source is borrowed (read per element, never consumed) and `dst` is a
            // borrowed writable slice place (a Copy `{ptr,len}` view — the buffer is written, but the
            // slice value itself is not moved).
            ExprKind::ArrayMapInto { source, dst, .. } => {
                self.expr(source, moved, false, false);
                self.expr(dst, moved, false, false);
            }
            ExprKind::ArrayDot { a, b, .. } => {
                self.expr(a, moved, false, false);
                self.expr(b, moved, false, false);
            }
            ExprKind::ArrayChunks { source, n, .. } => {
                self.expr(source, moved, false, false);
                self.expr(n, moved, false, false);
            }
            ExprKind::ArrayLit { elems, .. } | ExprKind::VecLit { elems, .. } => {
                for e in elems {
                    self.expr(e, moved, true, true);
                }
            }
            ExprKind::Select { mask, a, b } => {
                self.expr(mask, moved, true, true);
                self.expr(a, moved, true, true);
                self.expr(b, moved, true, true);
            }
            ExprKind::VecSumWhere { vec, mask } => {
                self.expr(vec, moved, true, true);
                self.expr(mask, moved, true, true);
            }
            ExprKind::VecDot { a, b } => {
                self.expr(a, moved, true, true);
                self.expr(b, moved, true, true);
            }
            ExprKind::VecMinMax { vec, .. } => self.expr(vec, moved, true, true),
            ExprKind::VecSum { vec } => self.expr(vec, moved, true, true),
            ExprKind::VecLoad { src, index, .. } => {
                self.expr(src, moved, true, true);
                self.expr(index, moved, true, true);
            }
            ExprKind::VecStore { dst, index, value, .. } => {
                self.expr(dst, moved, true, true);
                self.expr(index, moved, true, true);
                self.expr(value, moved, true, true);
            }
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.expr(opt, moved, true, true);
                // The fallback is an arm value: it inherits this position's `consuming` but is
                // not a direct move site (like an `if`/`else` arm). Today Option payloads are
                // scalar-only, so a Move-typed unwrap result is not constructible — but treating
                // the fallback consistently keeps the analysis sound if that ever changes.
                self.expr(fallback, moved, consuming, false);
            }
            // A plain block is transparent: its tail inherits this position's consuming/direct.
            ExprKind::Block(b) | ExprKind::Arena(b) | ExprKind::TaskGroup(b) | ExprKind::Unsafe(b) => self.block(b, moved, consuming, direct),
            // `raw.alloc`'s size / `raw.free`'s pointer are Copy operands (int / `raw`), never moved.
            ExprKind::RawAlloc(e) | ExprKind::RawFree(e) => self.expr(e, moved, false, false),
            // `raw.load`/`raw.store` operands are Copy (raw ptr + int offset + scalar value), never moved.
            ExprKind::RawLoad { ptr, offset, .. } | ExprKind::RawOffset { ptr, offset } => {
                self.expr(ptr, moved, false, false);
                self.expr(offset, moved, false, false);
            }
            ExprKind::RawStore { ptr, offset, value } => {
                self.expr(ptr, moved, false, false);
                self.expr(offset, moved, false, false);
                self.expr(value, moved, false, false);
            }
            ExprKind::Spawn { closure, .. } => self.expr(closure, moved, false, false),
            ExprKind::EnumValue { payload, .. } => {
                for p in payload {
                    self.expr(p, moved, false, false);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee, moved, false, false);
                // Arms are mutually exclusive, so each is checked in the *same* incoming state:
                // clone `moved` per arm and join, exactly like `if`/`else` generalised to N arms.
                // Without this the arms share one set, so a value consumed in arm A is wrongly seen
                // as already-moved in arm B (a false "use of moved value"). A diverging arm
                // (`=> { return … }`) contributes nothing to the fall-through, so its moves must not
                // poison the post-state; if every arm diverges the code after is unreachable.
                let mut joined: Option<MovedSet> = None;
                for a in arms {
                    let mut m = moved.clone();
                    self.expr(&a.body, &mut m, consuming, direct);
                    if hir_expr_diverges(&a.body) {
                        continue;
                    }
                    joined = Some(match joined {
                        None => m,
                        Some(j) => &j | &m,
                    });
                }
                if let Some(j) = joined {
                    *moved = j;
                }
            }
            ExprKind::ResultMapErr { result, f } => {
                // `map_err` unwraps/consumes the result (its Ok payload may be an owned Move type).
                self.expr(result, moved, true, true);
                self.expr(f, moved, false, false);
            }
            // `t.get()` moves the result out of the task when `R` is an owned/move type, so it
            // consumes the task (a second `get()` would double-free the buffer).
            ExprKind::TaskGet(inner) => {
                let consuming = is_owned_droppable(e.ty, self.structs);
                self.expr(inner, moved, consuming, consuming);
            }
            ExprKind::Wait => {}
            ExprKind::If { cond, then, els } => {
                self.expr(cond, moved, false, false);
                // An `if`/`else` arm value is a consuming-but-NOT-direct position: moving a
                // bound owned local out through it is rejected (the `direct = false`).
                let mut m1 = moved.clone();
                self.block(then, &mut m1, consuming, false);
                let mut m2 = moved.clone();
                self.block(els, &mut m2, consuming, false);
                // Join the branch states — but a branch that always diverges (`return`) contributes
                // nothing past the `if`, so its moves must not poison the fall-through. (Without this,
                // `if c { return x }; use(x)` wrongly reports `x` moved.) When both diverge the code
                // after is unreachable, so the post-state is immaterial.
                *moved = match (hir_block_diverges(then), hir_block_diverges(els)) {
                    (false, false) => &m1 | &m2,
                    (true, false) => m2,
                    (false, true) => m1,
                    (true, true) => moved.clone(),
                };
            }
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Hole(h) | TemplatePart::JsonStr(h) = p {
                        // A hole value is read (copied) into the builder, not moved out.
                        self.expr(h, moved, false, false);
                    }
                }
            }
            ExprKind::JsonDecode { input, .. } | ExprKind::JsonDecodeArray { input, .. } | ExprKind::JsonDecodeStructArray { input, .. } | ExprKind::JsonDecodeSoa { input, .. } => self.expr(input, moved, false, false),
            ExprKind::FsReadFile { path } | ExprKind::ReaderOpen { path } | ExprKind::WriterCreate { path }
            | ExprKind::FsExists { path } | ExprKind::FsRemove { path } | ExprKind::FsReadDir { path }
            | ExprKind::FsReadFileView { path } => self.expr(path, moved, false, false),
            // `dns.resolve(host)` borrows its `str` host (never consumed).
            ExprKind::DnsResolve { host } => self.expr(host, moved, false, false),
            // `tcp.connect(host, port)` borrows `host` (str, never consumed); `port` is a Copy i64.
            ExprKind::TcpConnect { host, port } => {
                self.expr(host, moved, false, false);
                self.expr(port, moved, false, false);
            }
            // `c.reader()` / `c.writer()` borrow the `tcp_conn` (the fd stays owned by `c` — the
            // returned stream is `owns_fd:false`), never consumed — like `io.copy`'s handles.
            ExprKind::ConnReader { conn } | ExprKind::ConnWriter { conn } => self.expr(conn, moved, false, false),
            // `tcp.listen(host, port)` borrows `host` (str, never consumed); `port` is a Copy i64.
            ExprKind::TcpListen { host, port } => {
                self.expr(host, moved, false, false);
                self.expr(port, moved, false, false);
            }
            // `l.accept()` borrows the `tcp_listener` (the listening fd stays owned by `l`); the
            // returned `tcp_conn` is freshly owned, never consumes the listener.
            ExprKind::TcpAccept { listener } => self.expr(listener, moved, false, false),
            // `udp.bind` borrows `host` (str, never consumed); `port` is a Copy i64. `u.send_to` /
            // `u.recv_from` borrow the `udp_socket` (the fd stays owned by `u`) plus their byte
            // view / buffer — none consumed.
            ExprKind::UdpBind { host, port } => {
                self.expr(host, moved, false, false);
                self.expr(port, moved, false, false);
            }
            ExprKind::UdpSendTo { sock, data, host, port } => {
                self.expr(sock, moved, false, false);
                self.expr(data, moved, false, false);
                self.expr(host, moved, false, false);
                self.expr(port, moved, false, false);
            }
            ExprKind::UdpRecvFrom { sock, buffer } => {
                self.expr(sock, moved, false, false);
                self.expr(buffer, moved, false, false);
            }
            // `fs.write_file(path, data)` borrows `data` (str/bytes/builder — not consumed), like
            // `writer.write`; neither `path` nor `data` is moved.
            ExprKind::FsWriteFile { path, data, .. } => {
                self.expr(path, moved, false, false);
                self.expr(data, moved, false, false);
            }
            // `std.path`/`std.env`/`std.time` builtins borrow their `str`/`i64` args (never consumed).
            ExprKind::PathComponent { path, .. } | ExprKind::PathNormalize { path } => self.expr(path, moved, false, false),
            ExprKind::PathJoin { a, b } => {
                self.expr(a, moved, false, false);
                self.expr(b, moved, false, false);
            }
            ExprKind::EnvGet { name } => self.expr(name, moved, false, false),
            ExprKind::EnvSet { name, value } => {
                self.expr(name, moved, false, false);
                self.expr(value, moved, false, false);
            }
            ExprKind::TimeNow | ExprKind::TimeInstant => {}
            ExprKind::TimeSleep { ns } => self.expr(ns, moved, false, false),
            // `process.exit(code)` reads a scalar `i64` (never consumed); `abort` reads nothing.
            ExprKind::ProcessExit { code } => self.expr(code, moved, false, false),
            ExprKind::ProcessAbort => {}
            // `spawn` borrows `cmd`/`args` (never consumed); `wait` borrows its `child` (NOT consumed —
            // it only flips the reaped flag through the borrow, mirroring `l.accept()`).
            ExprKind::ProcessSpawn { cmd, args } => {
                self.expr(cmd, moved, false, false);
                self.expr(args, moved, false, false);
            }
            ExprKind::ChildWait { child } => self.expr(child, moved, false, false),
            // `kill` borrows its `child` (only flips the reaped flag through the borrow, like `wait`) and
            // reads a scalar `sig`; `exec` borrows `cmd`/`args` (never consumed).
            ExprKind::ChildKill { child, sig } => {
                self.expr(child, moved, false, false);
                self.expr(sig, moved, false, false);
            }
            ExprKind::ProcessExec { cmd, args } => {
                self.expr(cmd, moved, false, false);
                self.expr(args, moved, false, false);
            }
            // `std.encoding` borrows its byte-view / `str` arg (never consumed) — like `hash64`.
            ExprKind::EncodingEncode { data, .. } | ExprKind::Utf8Valid { data } => self.expr(data, moved, false, false),
            ExprKind::EncodingDecode { input, .. } => self.expr(input, moved, false, false),
            // `std.compress` borrows its byte-view `data` (never consumed) — like `encoding.*`.
            ExprKind::Compress { data, level, .. } => {
                self.expr(data, moved, false, false);
                self.expr(level, moved, false, false);
            }
            ExprKind::Decompress { data, .. } => self.expr(data, moved, false, false),
            // `std.rand`: the `rng` receiver is Copy (advanced in place, never consumed) and `xs` is a
            // Copy slice view (borrowed), so nothing is moved — recurse non-consuming to catch a
            // use-after-move *inside* the operands.
            ExprKind::RandSeed => {}
            ExprKind::RandSeedWith { seed } => self.expr(seed, moved, false, false),
            ExprKind::RandNext { rng } => self.expr(rng, moved, false, false),
            ExprKind::RandRange { rng, lo, hi } => {
                self.expr(rng, moved, false, false);
                self.expr(lo, moved, false, false);
                self.expr(hi, moved, false, false);
            }
            ExprKind::RandShuffle { rng, xs, .. } => {
                self.expr(rng, moved, false, false);
                self.expr(xs, moved, false, false);
            }
            ExprKind::RandSample { rng, xs, k, .. } => {
                self.expr(rng, moved, false, false);
                self.expr(xs, moved, false, false);
                self.expr(k, moved, false, false);
            }
            // `std.cli`: every receiver (`cmd` / `parsed`) is **borrowed, never consumed** — `parse`
            // reads the flag table without moving the command (so `usage()` stays callable after),
            // and `get_*` reads the parsed map. The `str` name / argv / default args are borrowed too.
            // Recurse non-consuming to catch a use-after-move *inside* the operands.
            ExprKind::CliCommand { name } => self.expr(name, moved, false, false),
            ExprKind::CliFlag { cmd, name, default, .. } => {
                self.expr(cmd, moved, false, false);
                self.expr(name, moved, false, false);
                if let Some(d) = default {
                    self.expr(d, moved, false, false);
                }
            }
            ExprKind::CliParse { cmd, args } => {
                self.expr(cmd, moved, false, false);
                self.expr(args, moved, false, false);
            }
            ExprKind::CliGetBool { parsed, name } | ExprKind::CliGetI64 { parsed, name } | ExprKind::CliGetStr { parsed, name } => {
                self.expr(parsed, moved, false, false);
                self.expr(name, moved, false, false);
            }
            ExprKind::CliUsage { cmd } => self.expr(cmd, moved, false, false),
            // `std.http`: every receiver (`req` / `resp`) is **borrowed, never consumed** — `header`/
            // `body` mutate the request in place, `serialize`/`status`/`header`/`body`/`parse` read.
            // The `str`/byte args are borrowed too. Recurse non-consuming to catch a use-after-move
            // *inside* the operands.
            ExprKind::HttpRequest { method, url } => {
                self.expr(method, moved, false, false);
                self.expr(url, moved, false, false);
            }
            ExprKind::HttpHeader { req, name, value } => {
                self.expr(req, moved, false, false);
                self.expr(name, moved, false, false);
                self.expr(value, moved, false, false);
            }
            ExprKind::HttpBody { req, data } => {
                self.expr(req, moved, false, false);
                self.expr(data, moved, false, false);
            }
            ExprKind::HttpParse { data } => self.expr(data, moved, false, false),
            ExprKind::HttpRespStatus { resp } | ExprKind::HttpRespBody { resp } => self.expr(resp, moved, false, false),
            ExprKind::HttpRespHeader { resp, name } => {
                self.expr(resp, moved, false, false);
                self.expr(name, moved, false, false);
            }
            // `std.http` (Slice 2): the `client` receiver is **borrowed** (a client fires many
            // requests — `get`/`post`/`request` read it, never consume). `get`/`post`'s `url`/`body`
            // are borrowed views. `request`'s `req`, though, is a Move `http request` **consumed** by
            // the call (the runtime frees it) — so it moves (a use-after-move of `req` is caught here,
            // and the MIR nulls its slot so the exit `Drop` doesn't double-free).
            ExprKind::HttpClient => {}
            ExprKind::HttpClientGet { client, url } => {
                self.expr(client, moved, false, false);
                self.expr(url, moved, false, false);
            }
            ExprKind::HttpClientPost { client, url, body } => {
                self.expr(client, moved, false, false);
                self.expr(url, moved, false, false);
                self.expr(body, moved, false, false);
            }
            ExprKind::HttpClientRequest { client, req } => {
                self.expr(client, moved, false, false);
                self.expr(req, moved, true, true);
            }
            // `std.crypto` borrows both byte views (`constant_time_equal`) / the `out` buffer
            // (`random`, filled in place) — nothing is consumed. Recurse non-consuming to catch a
            // use-after-move *inside* the operands.
            ExprKind::CryptoCtEqual { a, b } => {
                self.expr(a, moved, false, false);
                self.expr(b, moved, false, false);
            }
            ExprKind::CryptoRandom { out } => self.expr(out, moved, false, false),
            // `crypto.sha256`/`sha512` borrow the byte view (never consume it). Recurse non-consuming
            // to catch a use-after-move *inside* the operand.
            ExprKind::CryptoHash { data, .. } => self.expr(data, moved, false, false),
            // `crypto.hmac_sha256`/`hkdf_sha256` borrow every operand (never consume). Recurse
            // non-consuming to catch a use-after-move inside them.
            ExprKind::CryptoHmac { key, data } => {
                self.expr(key, moved, false, false);
                self.expr(data, moved, false, false);
            }
            ExprKind::CryptoHkdf { salt, ikm, info, len } => {
                self.expr(salt, moved, false, false);
                self.expr(ikm, moved, false, false);
                self.expr(info, moved, false, false);
                self.expr(len, moved, false, false);
            }
            // AEAD seal/open borrow every operand (never consume). Recurse non-consuming to catch a
            // use-after-move inside them.
            ExprKind::CryptoAead { key, nonce, input, aad, .. } => {
                self.expr(key, moved, false, false);
                self.expr(nonce, moved, false, false);
                self.expr(input, moved, false, false);
                self.expr(aad, moved, false, false);
            }
            // `crypto.argon2id` borrows every operand (never consumes — `params` is Copy). Recurse
            // non-consuming to catch a use-after-move inside them.
            ExprKind::CryptoArgon2 { password, salt, params } => {
                self.expr(password, moved, false, false);
                self.expr(salt, moved, false, false);
                self.expr(params, moved, false, false);
            }
            // PR1 tuple elements are primitive (Copy) — a tuple literal moves nothing; tuple index
            // borrows. Recurse to catch moves in element subexpressions.
            ExprKind::Tuple { elems, .. } => {
                for el in elems {
                    self.expr(el, moved, true, true);
                }
            }
            ExprKind::MathOp { operands, .. } => {
                for o in operands {
                    self.expr(o, moved, false, false);
                }
            }
            // `t.N` of a bound tuple reads field `N` independently of the other fields: it is
            // invalid only if *that* field (or the whole tuple) was moved — NOT if some *other*
            // field was moved (that must not poison a Copy-field read). An owned field in a
            // consuming position (`a := t.0`) is moved out (marked per field); a Copy read, or a
            // borrowing read, moves nothing. A non-local receiver just recurses as a borrow.
            ExprKind::TupleIndex { recv, index } => {
                match &recv.kind {
                    ExprKind::Local(t) => {
                        if field_moved(moved, *t, *index) {
                            let name = &self.f.locals[*t as usize].name;
                            self.diags.error(format!("use of moved field '.{index}' of '{name}'"), e.span);
                        } else {
                            let owned = matches!(self.f.locals.get(*t as usize).map(|l| l.ty), Some(Ty::Tuple(tid))
                                if self.tuples.get(tid as usize).and_then(|td| td.elems.get(*index as usize)).is_some_and(|s| s.is_move()));
                            if owned && consuming {
                                moved.insert(MovedKey::Field(*t, *index));
                            }
                        }
                    }
                    _ => self.expr(recv, moved, false, false),
                }
            }
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::OptionNone => {}
        }
    }
}

struct Checker<'a, 't> {
    diags: &'a mut Diagnostics,
    sigs: &'a HashMap<String, FnSig>,
    struct_ids: &'a HashMap<String, u32>,
    enum_ids: &'a HashMap<String, u32>,
    /// The concrete enum table, grown with monomorph instances of generic sum types (mutable, like
    /// `structs`). `match` / variant construction read it.
    enums: &'t mut Vec<hir::EnumDef>,
    /// Generic sum-type templates (`Opt<T>`), monomorphized on demand by `resolve_type`.
    enum_templates: &'a HashMap<String, EnumTemplate>,
    /// Mangled monomorph name -> `enums` index — the shared monomorph dedup cache.
    enum_mono: &'t mut HashMap<String, u32>,
    /// The id of the builtin `Error` enum (so `Result<_, Error>` builtins build the right payload).
    error_enum_id: u32,
    /// The concrete struct table, grown with monomorph instances of generic structs during
    /// resolution (mutable, like the other interners). Field access reads it.
    structs: &'t mut Vec<StructDef>,
    /// Generic struct templates (`Pair<T>`), monomorphized on demand by `resolve_type`.
    struct_templates: &'a HashMap<String, StructTemplate>,
    /// Mangled monomorph name -> `structs` index — the shared monomorph dedup cache.
    struct_mono: &'t mut HashMap<String, u32>,
    /// The shared tuple-type interner (anonymous `(T, U, …)` types). A separate lifetime from
    /// `'a` so each per-function `Checker` can reborrow it mutably without conflicting with the
    /// long-lived shared `struct_ids` borrow.
    tuples: &'t mut Vec<hir::TupleDef>,
    /// The shared `Ty::Fn` interner (function-value types). Same lifetime as `tuples`.
    fn_types: &'t mut Vec<hir::FnTy>,
    // Integer/float inference variables. `*_vars[i]` is the binding for the *root* of var
    // `i`; `*_parent[i]` is its union-find parent (self when `i` is a root). Linking two
    // unconstrained vars (rather than dropping one) means a later constraint on either
    // reaches both — without it they would diverge and resolve to different concrete types.
    int_vars: Vec<Option<IntTy>>,
    int_parent: Vec<u32>,
    float_vars: Vec<Option<FloatTy>>,
    float_parent: Vec<u32>,
    /// All locals of the current function (slots), never shrinks.
    locals: Vec<Local>,
    /// Visibility stack: (name, id). Truncated on block exit.
    scope: Vec<(String, LocalId)>,
    /// Enclosing function's return type, so `return` checks against it.
    ret_hint: Ty,
    /// Nesting depth of `arena {}` blocks (0 = not in an arena).
    arena_depth: u32,
    /// Nesting depth of `unsafe {}` blocks (0 = in safe code). `raw.*` ops are valid only inside one.
    unsafe_depth: u32,
    /// Nesting depth of `task_group {}` blocks (0 = not in one). `spawn`/`wait` are valid only
    /// inside a `task_group` scope (slice ④).
    task_group_depth: u32,
    /// Per open `task_group` (innermost last): whether a `wait()` is guaranteed to have run at the
    /// current point (so `get()` is allowed). `spawn` clears it (a new task is pending), `wait`
    /// sets it, and `if`/`else` merge it by dominance (`then && else`). Slice ④c: the
    /// `get`-before-`wait` check.
    wait_state: Vec<bool>,
    /// Per open `task_group` (innermost last): whether any spawned task is fallible (its closure
    /// returns `Result`). When true, `wait()` yields `Result<(), Error>` (else `()`) and only a
    /// `wait()?` (not a bare `wait()`) makes `get()` safe. Slice ④c-2.
    task_group_fallible: Vec<bool>,
    /// For each slice local bound from an array/slice (`s: slice<T> := a`), the **root** buffer
    /// local it borrows. Used by the `out` no-alias check so `fill(a, s)` (where `s` views `a`)
    /// is caught even though `s` and `a` are different locals.
    slice_bases: std::collections::HashMap<LocalId, LocalId>,
    /// The enclosing function's name — used to generate unique names for lifted lambdas.
    cur_fn: String,
    /// The enclosing function's generic type-parameter names (`fn f<T, U>` → `["T", "U"]`); empty
    /// for a non-generic function. A `let`/lambda type annotation naming one resolves to `Ty::Param`.
    type_params: Vec<String>,
    /// The builtin bound of each type parameter (parallel to `type_params`); used to decide which
    /// operations are allowed on a `Ty::Param` value in a template body.
    param_bounds: Vec<Bound>,
    /// Concrete type arguments when checking a **monomorph** instance of a generic function
    /// (parallel to `type_params`); empty in normal / template mode. When set, every `Ty::Param(i)`
    /// produced by type resolution is immediately substituted to `mono_args[i]`, so the resulting
    /// HIR is fully concrete (no `Param` reaches MIR).
    mono_args: Vec<Ty>,
    /// Generic-call instantiations discovered while checking this function: `(generic_fn_name,
    /// concrete_type_args)`. Drained by `check_file` to drive monomorphization (the worklist).
    instantiations: Vec<(String, Vec<Ty>)>,
    /// Lambdas lifted to synthetic top-level functions while checking this function's body. Pass 2
    /// appends them to `program.fns` so later passes / codegen treat them like named functions.
    lifted: Vec<hir::Fn>,
    /// Set while checking a lambda body — lets a reference to an enclosing local become a capture
    /// (a synthetic value parameter of the lifted function, passed at the call site).
    capture: Option<CaptureScope>,
    /// The builtin modules `import`ed by this file (validated module paths, e.g. `core.json`).
    /// The prefix-accessed builtin namespaces (`json`/`fs`/`io`) must be imported before use —
    /// the "capability header" rule (`open-questions.md` module system).
    imports: &'a std::collections::HashSet<String>,
    /// The module this function belongs to (`main` for the entry). A bare call/function-value
    /// resolves in this module; a `mod.fn()` call resolves in `mod` (which must be imported here).
    cur_module: String,
    /// The program's module-resolution table (every module's functions + imports), used to map a
    /// bare or `mod.fn` reference to its mangled target and enforce `pub` visibility.
    mods: &'a ModuleTable,
    /// Every module's type names (bare → canonical + `pub`?), used to resolve a bare type in
    /// `cur_module` and a qualified `mod.Type` (with import + `pub` checks).
    type_table: &'a ModTypes,
    /// The user modules `cur_module` `import`s (distinct from `imports`, which is the builtin
    /// capability set). A qualified type / call into a user module must name one of these.
    user_imports: &'a std::collections::HashSet<String>,
    /// Every module's folded top-level constants. A bare name resolves in `cur_module`, a qualified
    /// `mod.NAME` in an imported `pub` constant; either is substituted as a literal.
    consts: &'a ConstTable,
}

/// Captured-variable bookkeeping while lifting a lambda. A reference in the body that misses the
/// lambda's own scope but resolves to an enclosing local is *captured*: a synthetic value parameter
/// is appended to the lifted function and the enclosing local is passed at the call site.
struct CaptureScope {
    /// The enclosing function's visible names → (enclosing LocalId, type), snapshot at lambda entry.
    enclosing: Vec<(String, LocalId, Ty)>,
    /// Captured enclosing locals, in capture order: (name, lifted-fn param LocalId, enclosing LocalId).
    captured: Vec<(String, LocalId, LocalId)>,
}

impl<'a, 't> Checker<'a, 't> {
    /// Build a fresh checker for one function body. `type_params` are the function's generic
    /// parameter names; `mono_args` are the concrete type arguments when checking a monomorph
    /// instance (empty in normal / template mode).
    #[allow(clippy::too_many_arguments)]
    fn new(
        diags: &'a mut Diagnostics,
        sigs: &'a HashMap<String, FnSig>,
        struct_ids: &'a HashMap<String, u32>,
        enum_ids: &'a HashMap<String, u32>,
        enums: &'t mut Vec<hir::EnumDef>,
        enum_templates: &'a HashMap<String, EnumTemplate>,
        enum_mono: &'t mut HashMap<String, u32>,
        error_enum_id: u32,
        structs: &'t mut Vec<StructDef>,
        struct_templates: &'a HashMap<String, StructTemplate>,
        struct_mono: &'t mut HashMap<String, u32>,
        tuples: &'t mut Vec<hir::TupleDef>,
        fn_types: &'t mut Vec<hir::FnTy>,
        type_params: Vec<String>,
        param_bounds: Vec<Bound>,
        mono_args: Vec<Ty>,
        imports: &'a std::collections::HashSet<String>,
        cur_module: String,
        mods: &'a ModuleTable,
        type_table: &'a ModTypes,
        user_imports: &'a std::collections::HashSet<String>,
        consts: &'a ConstTable,
    ) -> Self {
        Checker {
            diags,
            sigs,
            imports,
            cur_module,
            mods,
            type_table,
            user_imports,
            consts,
            struct_ids,
            enum_ids,
            enums,
            enum_templates,
            enum_mono,
            error_enum_id,
            structs,
            struct_templates,
            struct_mono,
            tuples,
            fn_types,
            int_vars: Vec::new(),
            int_parent: Vec::new(),
            float_vars: Vec::new(),
            float_parent: Vec::new(),
            locals: Vec::new(),
            scope: Vec::new(),
            ret_hint: Ty::Unit,
            arena_depth: 0,
            unsafe_depth: 0,
            task_group_depth: 0,
            wait_state: Vec::new(),
            task_group_fallible: Vec::new(),
            slice_bases: std::collections::HashMap::new(),
            cur_fn: String::new(),
            type_params,
            param_bounds,
            mono_args,
            instantiations: Vec::new(),
            lifted: Vec::new(),
            capture: None,
        }
    }

    fn fresh_int_var(&mut self) -> Ty {
        let id = self.int_vars.len() as u32;
        self.int_vars.push(None);
        self.int_parent.push(id);
        Ty::IntVar(id)
    }

    fn fresh_float_var(&mut self) -> Ty {
        let id = self.float_vars.len() as u32;
        self.float_vars.push(None);
        self.float_parent.push(id);
        Ty::FloatVar(id)
    }

    /// Union-find root of an int/float var (no path compression — callers only read).
    fn root_int(&self, mut v: u32) -> u32 {
        while self.int_parent[v as usize] != v {
            v = self.int_parent[v as usize];
        }
        v
    }
    fn root_float(&self, mut v: u32) -> u32 {
        while self.float_parent[v as usize] != v {
            v = self.float_parent[v as usize];
        }
        v
    }

    fn resolve(&self, ty: Ty) -> Ty {
        match ty {
            Ty::IntVar(v) => {
                let r = self.root_int(v);
                match self.int_vars[r as usize] {
                    Some(it) => Ty::Int(it),
                    None => Ty::IntVar(r),
                }
            }
            Ty::FloatVar(v) => {
                let r = self.root_float(v);
                match self.float_vars[r as usize] {
                    Some(ft) => Ty::Float(ft),
                    None => Ty::FloatVar(r),
                }
            }
            other => other,
        }
    }

    fn finalize(&self, ty: Ty) -> Ty {
        match self.resolve(ty) {
            Ty::IntVar(_) => Ty::Int(IntTy {
                bits: 64,
                signed: true,
            }),
            Ty::FloatVar(_) => Ty::Float(FloatTy { bits: 64 }),
            other => other,
        }
    }

    /// Unify two types, returning the resolved type. Pushes a diagnostic on mismatch.
    fn unify(&mut self, a: Ty, b: Ty, span: Span) -> Ty {
        let (a, b) = (self.resolve(a), self.resolve(b));
        match (a, b) {
            (Ty::Error, _) | (_, Ty::Error) => Ty::Error,
            (Ty::IntVar(v), Ty::Int(it)) | (Ty::Int(it), Ty::IntVar(v)) => {
                // `v` is a resolved root (see `resolve`); bind it.
                self.int_vars[v as usize] = Some(it);
                Ty::Int(it)
            }
            (Ty::IntVar(v1), Ty::IntVar(v2)) => {
                // Both unconstrained: link their roots so a later binding reaches both.
                if v1 != v2 {
                    self.int_parent[v2 as usize] = v1;
                }
                Ty::IntVar(v1)
            }
            (Ty::FloatVar(v), Ty::Float(ft)) | (Ty::Float(ft), Ty::FloatVar(v)) => {
                self.float_vars[v as usize] = Some(ft);
                Ty::Float(ft)
            }
            (Ty::FloatVar(v1), Ty::FloatVar(v2)) => {
                if v1 != v2 {
                    self.float_parent[v2 as usize] = v1;
                }
                Ty::FloatVar(v1)
            }
            _ if a == b => a,
            _ => {
                self.diags.error(
                    format!("type mismatch: {} vs {}", self.ty_display(a), self.ty_display(b)),
                    span,
                );
                Ty::Error
            }
        }
    }

    /// A user-facing type name that resolves struct/enum ids to their declared source names —
    /// unlike the free `ty_name`, which has no name tables and prints `struct#0` / `enum#0`. Used
    /// in type-mismatch diagnostics so a user sees `Error`, not `enum#0`. Recurses into composite
    /// payloads (a `Result<i32, Error>` shows `Error`, not `enum#0`).
    fn ty_display(&self, ty: Ty) -> String {
        match ty {
            Ty::Struct(id) => self.structs.get(id as usize).map(|s| s.name.clone()).unwrap_or_else(|| ty_name(ty)),
            Ty::Enum(id) => self.enums.get(id as usize).map(|e| e.name.clone()).unwrap_or_else(|| ty_name(ty)),
            Ty::Option(s) => format!("Option<{}>", self.scalar_display(s)),
            Ty::Result(o, e) => format!("Result<{}, {}>", self.scalar_display(o), self.scalar_display(e)),
            Ty::Box(s) => format!("box<{}>", self.scalar_display(s)),
            Ty::Task(s) => format!("Task<{}>", self.scalar_display(s)),
            Ty::Array(s, n) => format!("array<{}>[{n}]", self.scalar_display(s)),
            Ty::Slice(s) => format!("slice<{}>", self.scalar_display(s)),
            Ty::DynArray(s) => format!("array<{}>", self.scalar_display(s)),
            Ty::StructArray(id, n) => format!("array<{}>[{n}]", self.ty_display(Ty::Struct(id))),
            Ty::DynStructArray(id, _) => format!("array<{}>", self.ty_display(Ty::Struct(id))),
            Ty::Soa(id) => format!("soa<{}>", self.ty_display(Ty::Struct(id))),
            Ty::DictEncoded(id, _) => format!("dict_encoded<{}>", self.ty_display(Ty::Struct(id))),
            // No id (primitives), or no source name to resolve (tuple#, fn#) — the free form is fine.
            _ => ty_name(ty),
        }
    }

    /// [`ty_display`] for a scalar payload (an `Option`/`Result`/`box` element may itself be an enum).
    fn scalar_display(&self, s: Scalar) -> String {
        self.ty_display(scalar_to_ty(s))
    }

    /// Constrain `ty` to an expected type if one is given.
    fn constrain(&mut self, ty: Ty, expected: Option<Ty>, span: Span) {
        if let Some(exp) = expected {
            self.unify(ty, exp, span);
        }
    }

    // --- locals / scopes ---

    fn declare(&mut self, name: &str, ty: Ty, is_mut: bool) -> LocalId {
        let id = self.locals.len() as LocalId;
        self.locals.push(Local {
            id,
            name: name.to_string(),
            ty,
            is_mut,
            align: None,
            is_param: false,
        });
        self.scope.push((name.to_string(), id));
        id
    }

    fn lookup(&self, name: &str) -> Option<LocalId> {
        self.scope
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }

    /// The no-shadowing rule (`draft.md` §4 Variables; `open-questions.md` Settled 2026-07-09): a
    /// name binds **once** per scope chain. Emit an error at `span` if declaring `name` would shadow
    /// a binding that is already visible — a local/parameter below `floor` in the current scope, an
    /// enclosing (capturable) binding when inside a lambda body, or a top-level constant of the
    /// current module. `floor` is normally the full current scope length (so a same-scope re-`:=` or
    /// a duplicate parameter is caught); a `match` arm passes its pre-arm length so intra-pattern
    /// duplicates (`Rect(w, w)`) stay owned by the pattern's own duplicate-binding check. Disjoint
    /// sibling blocks reuse a name freely because each is truncated out of `scope` on exit, so a
    /// sibling's binding is never visible here. `_` (discard) never shadows.
    ///
    /// Every user-named binding site must call this before [`Checker::declare`]; synthetic names
    /// (the `_drop{i}` tuple-drop placeholders, error-recovery declares) skip it deliberately.
    fn check_shadow(&mut self, name: &str, span: Span, floor: usize) {
        if name == "_" {
            return;
        }
        let floor = floor.min(self.scope.len());
        let in_scope = self.scope[..floor].iter().any(|(n, _)| n == name);
        // Lambda captures are single-level today (`enclosing` is a snapshot of the immediate
        // enclosing scope only; a nested lambda does not chain into its grandparent's capture),
        // so checking just this one snapshot is exactly as deep as name visibility reaches. If
        // transitive/multi-level capture is ever implemented, this must walk the enclosing chain
        // transitively, or shadowing through a grandparent scope becomes silently accepted.
        let in_enclosing = || {
            self.capture
                .as_ref()
                .is_some_and(|c| c.enclosing.iter().any(|(n, _, _)| n == name))
        };
        let is_const = || {
            matches!(self.consts.resolve(&self.cur_module, name, &self.cur_module), Ok(Some(_)))
        };
        if in_scope || in_enclosing() || is_const() {
            self.diags.error(
                format!(
                    "`{name}` is already bound in this scope chain; a name binds once (no shadowing) — use `mut` for a value that changes, or a new name"
                ),
                span,
            );
        }
    }

    /// Resolve a name to a local, capturing an enclosing local if we are in a lambda body. A miss
    /// in the lambda's own scope that resolves to an enclosing local becomes a capture: a synthetic
    /// value parameter of the lifted function (reused on repeat references). The captured local's
    /// type is taken as-is here; `lift_lambda` rejects capturing a Move (owned) value afterward.
    fn lookup_or_capture(&mut self, name: &str) -> Option<LocalId> {
        if let Some(id) = self.lookup(name) {
            return Some(id);
        }
        let cap = self.capture.as_mut()?;
        if let Some(&(_, param_id, _)) = cap.captured.iter().find(|(n, _, _)| n == name) {
            return Some(param_id);
        }
        let (enc_id, ty) = cap.enclosing.iter().rev().find(|(n, _, _)| n == name).map(|(_, id, t)| (*id, *t))?;
        // A captured value becomes a synthetic parameter local (tracked in `captured`, *not* pushed
        // into the visible scope so a nested-block exit can't truncate it).
        let param_id = self.locals.len() as LocalId;
        self.locals.push(Local { id: param_id, name: name.to_string(), ty, is_mut: false, align: None, is_param: false });
        cap.captured.push((name.to_string(), param_id, enc_id));
        Some(param_id)
    }

    fn check_fn(&mut self, f: &ast::FnDecl) -> Fn {
        // Look the signature up by the function's **mangled** name (module-qualified outside the
        // entry module). `cur_fn` is the mangled name too, so lifted-lambda names stay unique across
        // modules (two modules may each have `fn run` with a lambda).
        let mangled = self.resolve_local_fn(&f.name.name).unwrap_or_else(|| f.name.name.clone());
        self.cur_fn = mangled.clone();
        let sig = &self.sigs[&mangled];
        let mut ret = sig.ret;
        let mut param_tys = sig.params.clone();
        // Monomorph mode: substitute the concrete type arguments into the (generic) signature so
        // the param locals and return type are concrete — no `Ty::Param` reaches the body.
        if !self.mono_args.is_empty() {
            ret = subst_param_ty(ret, &self.mono_args);
            for t in &mut param_tys {
                *t = subst_param_ty(*t, &self.mono_args);
            }
        }
        if f.name.name == "main" {
            let ret_span = f.ret.as_ref().map(|t| t.span()).unwrap_or(f.span);
            // A fallible `main` returns exactly `Result<(), Error>` (draft.md §17): the `Ok`
            // payload must be `()` and the error must be the builtin `Error`. The C-`main`
            // wrapper's exit-code lowering reads the `Err` payload as the builtin `Error` enum's
            // specific `{ i32 tag, i32 code }` shape (`Code(c)` → `clamp(c)`, a category →
            // `tag + 1`) and returns exit 0 on `Ok` — a non-unit `Ok` payload has no exit-code
            // meaning (it would be silently discarded) and a user-defined error enum has a
            // different layout with no defined mapping (it would miscompile). Reject both here with
            // a clear diagnostic; to return a value, use `-> i32`. (Relaxation of the `E`
            // restriction waits on the full `Error` design — see `open-questions.md` "Error type
            // design".) These checks apply to every `main` form (no-arg and argv alike).
            if let Ty::Result(ok, e) = ret {
                if ok != Scalar::Unit {
                    self.diags.error(
                        "main's Ok type must be `()` — a fallible main returns `Result<(), Error>`; use `-> i32` to return a value".to_string(),
                        ret_span,
                    );
                }
                if !matches!(e, Scalar::Enum(eid) if eid == self.error_enum_id) {
                    self.diags.error(
                        "main's error type must be the builtin `Error`; user-defined error types in main's return will be allowed once the full Error design lands".to_string(),
                        ret_span,
                    );
                }
            }
            // `main` takes no arguments, or exactly `args: array<str>` (argv, draft.md §19) with a
            // `Result<(), Error>` return — the argv form is the only one the C-`main` wrapper
            // marshals argv into, so it must return a `Result` (an `-> i32` argv `main` is a later
            // follow-up). The `Ok`/`E` shape is validated above, so this checks only the parameter
            // shape and that the return is *some* `Result` (one error per root cause).
            if !f.params.is_empty() {
                let params_ok = param_tys.as_slice() == [Ty::DynArray(Scalar::Str)];
                if !params_ok || !matches!(ret, Ty::Result(..)) {
                    self.diags.error(
                        "main takes no arguments, or exactly `args: array<str>` with a `Result<(), Error>` return".to_string(),
                        f.span,
                    );
                }
            }
        }
        self.ret_hint = ret;

        // "huge struct copy" lint (`draft.md` §16): a struct passed or returned **by value** is
        // copied in full at every call boundary; above `HUGE_STRUCT_BYTES` that is a data-oriented
        // smell — narrow the struct (split hot/cold fields, `draft.md` §9) or pass a `slice`/view.
        // A **warning** (a perf hint, not a hard error). Emitted only for a source signature
        // (`mono_args` empty) — a monomorph would duplicate it, and a generic template's params are
        // the opaque `Ty::Param` (never a `Ty::Struct`), so a generic instantiated *with* a huge
        // struct is not flagged here (the source signature does not name it).
        if self.mono_args.is_empty() {
            let mut visiting = Vec::new();
            // The struct name for the message — `.get()` (not direct indexing) so a stray id can
            // never panic; `sz > 0` already implies the struct exists (a missing one sizes to 0).
            let huge = |structs: &[StructDef], id: u32, visiting: &mut Vec<u32>| {
                let (sz, _) = struct_size_align(id, structs, visiting);
                (sz > HUGE_STRUCT_BYTES)
                    .then(|| structs.get(id as usize).map(|d| (sz, d.name.clone())))
                    .flatten()
            };
            for (p, ty) in f.params.iter().zip(&param_tys) {
                if let Ty::Struct(id) = *ty
                    && let Some((sz, name)) = huge(self.structs, id, &mut visiting)
                {
                    self.diags.push(align_diag::Diagnostic::warning(
                        format!("huge struct copy: `{name}` ({sz} bytes) is passed by value — every call copies it; narrow the struct (split hot/cold fields) or pass a `slice`/view"),
                        p.ty.span(),
                    ));
                }
            }
            if let (Ty::Struct(id), Some(rty)) = (ret, &f.ret)
                && let Some((sz, name)) = huge(self.structs, id, &mut visiting)
            {
                self.diags.push(align_diag::Diagnostic::warning(
                    format!("huge struct copy: returning `{name}` ({sz} bytes) by value copies it out; narrow the struct (split hot/cold fields) or return a handle"),
                    rty.span(),
                ));
            }
        }

        let mut params = Vec::new();
        for (p, ty) in f.params.iter().zip(param_tys) {
            // An `out` parameter is a writable output buffer — only a `slice<T>` (a borrow the
            // callee writes back through). Mark its local mutable so `dst[i] = v` is allowed.
            if p.is_out && !matches!(ty, Ty::Slice(_) | Ty::Error) {
                self.diags.error(
                    format!("an `out` parameter must be a slice (a writable output buffer), got {}", ty_name(ty)),
                    p.ty.span(),
                );
            }
            self.check_shadow(&p.name.name, p.name.span, self.scope.len());
            let id = self.declare(&p.name.name, ty, p.is_out);
            self.locals[id as usize].is_param = true;
            params.push(id);
        }

        let body = match &f.body {
            ast::FnBody::Block(b) => self.check_block(b, Some(ret)),
            ast::FnBody::Expr(e) => {
                let value = self.check_expr(e, Some(ret));
                Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(value)),
                }
            }
        };

        // Finalize all inferred types to concrete (or default i64).
        let mut body = body;
        self.finalize_block(&mut body);
        // The broad "unnecessary heap" lint: a whole-function scan for a box local that is only ever
        // read back with `.get()` and never escapes (the narrow inline `heap.new(x).get()` slice lives
        // in `finalize_expr`). A warning, not an error — it never blocks a build.
        UnnecessaryHeapScan::run(&body, self.diags);
        let mut locals = std::mem::take(&mut self.locals);
        for l in &mut locals {
            l.ty = self.finalize(l.ty);
        }

        Fn {
            name: f.name.name.clone(),
            params,
            ret: self.finalize(ret),
            locals,
            body,
            span: f.span,
            drop_locals: Vec::new(),
        }
    }

    /// Check a block. `expected` is the expected type of its trailing value (if any).
    fn check_block(&mut self, b: &ast::Block, expected: Option<Ty>) -> Block {
        let scope_mark = self.scope.len();
        let mut stmts = Vec::new();

        for s in &b.stmts {
            match s {
                ast::Stmt::Let { is_mut, name, ty, init, align } => {
                    let ann = ty.as_ref().map(|t| self.resolve_type(t));
                    // A struct literal is only legal here, as a `let` initializer.
                    let init = match &init.kind {
                        ast::ExprKind::StructLit { name: sname, fields } => {
                            self.check_struct_lit(sname, fields, init.span)
                        }
                        // A slice/str-annotated binding borrows its source (mirrors a call arg):
                        // `slice<T>` borrows an array, `str` borrows an owned `string`.
                        _ => match ann {
                            Some(Ty::Slice(ps)) => self.check_slice_init(init, ps),
                            Some(Ty::Str) => self.check_str_init(init),
                            _ => self.check_expr(init, ann),
                        },
                    };
                    let local_ty = ann.unwrap_or(init.ty);
                    self.check_shadow(&name.name, name.span, self.scope.len());
                    let local = self.declare(&name.name, local_ty, *is_mut);
                    // An `align(N) data := [...]` over-alignment prefix: restricted to a scalar
                    // fixed-array binding (the aligned-vector-load enabler). `N` is already a
                    // validated power of two (parser). A struct's over-alignment is declared on the
                    // type (`align(N) Name { … }`), not the binding, so reject it here.
                    if let Some(n) = *align {
                        let resolved = self.resolve(local_ty);
                        // The binding form is the aligned-vector-load enabler, so it applies to a
                        // fixed array of a **numeric** scalar (int/float) only — the sole element a
                        // `vecN<T>` load can target. `int` covers every byte-buffer / DMA case
                        // (`u8..u64`). A `str`/`bool`/`char`-element array (still a `Ty::Array`), a
                        // struct array (`Ty::StructArray` — a struct's over-alignment is declared on
                        // the type: `align(N) Name { … }`), or a scalar are all rejected.
                        if matches!(resolved, Ty::Array(s, _) if matches!(s, Scalar::Int(_) | Scalar::Float(_))) {
                            self.locals[local as usize].align = Some(n);
                        } else if resolved != Ty::Error {
                            self.diags.error(
                                format!(
                                    "`align(N)` on a binding applies to a fixed array of a numeric scalar (int/float), got {} (a struct's over-alignment is declared on the type: `align(N) Name {{ … }}`)",
                                    ty_name(resolved)
                                ),
                                name.span,
                            );
                        }
                    }
                    // Record slice provenance (`s: slice<T> := a` → `s` borrows `a`'s buffer) so
                    // the `out` no-alias check can see through slice variables.
                    if matches!(local_ty, Ty::Slice(_))
                        && let Some(root) = self.expr_root_local(&init) {
                            self.slice_bases.insert(local, root);
                        }
                    stmts.push(Stmt::Let { local, init });
                }
                ast::Stmt::LetTuple { names, init, span } => {
                    // `(a, b, ...) := expr` — the RHS must be a tuple; bind each name to its
                    // element type (`_` binds nothing). Element types are inferred from the tuple.
                    let init = self.check_expr(init, None);
                    if let Ty::Tuple(id) = self.resolve(init.ty) {
                        let elem_tys: Vec<Ty> =
                            self.tuples[id as usize].elems.iter().map(|s| scalar_to_ty(*s)).collect();
                        if elem_tys.len() != names.len() {
                            self.diags.error(
                                format!("this destructuring binds {} name(s) but the tuple has {} element(s)", names.len(), elem_tys.len()),
                                *span,
                            );
                        }
                        let mut locals = Vec::with_capacity(names.len());
                        for (i, n) in names.iter().enumerate() {
                            let ety = elem_tys.get(i).copied().unwrap_or(Ty::Error);
                            match n {
                                Some(name) => {
                                    self.check_shadow(&name.name, name.span, self.scope.len());
                                    locals.push(Some(self.declare(&name.name, ety, false)));
                                }
                                // An *ignored* (`_`) owned element must still be dropped, not leaked:
                                // bind it to a fresh hidden local so it joins the normal drop path
                                // (freed once at scope exit, or bulk-freed if arena-regioned). A
                                // Copy / `str` element needs no cleanup, so `_` binds nothing.
                                None if is_owned_droppable(ety, self.structs) => {
                                    locals.push(Some(self.declare(&format!("_drop{i}"), ety, false)));
                                }
                                None => locals.push(None),
                            }
                        }
                        stmts.push(Stmt::LetTuple { locals, tuple_id: id, init });
                    } else {
                        // Not a tuple: declare the names as `Ty::Error` (no cascade of "undefined
                        // name") and keep the RHS as a plain expression statement — never emit a
                        // `LetTuple` whose `TupleIndex` lowering would panic codegen.
                        if self.resolve(init.ty) != Ty::Error {
                            self.diags.error(
                                format!("destructuring needs a tuple value, got {}", ty_name(init.ty)),
                                *span,
                            );
                        }
                        for n in names.iter().flatten() {
                            self.declare(&n.name, Ty::Error, false);
                        }
                        stmts.push(Stmt::Expr(init));
                    }
                }
                ast::Stmt::Return(value) => {
                    // The enclosing function's return type is the expected one. We
                    // thread it via `expected` of the body block (M1: one level).
                    let v = value.as_ref().map(|e| self.check_expr(e, Some(self.ret_hint)));
                    stmts.push(Stmt::Return(v));
                }
                ast::Stmt::Expr(e) => {
                    let te = self.check_expr(e, None);
                    // Unhandled `Result` lint (`draft.md` §16): discarding a `Result` as a statement
                    // silently drops a possible error — against "errors are visible / handled". It is
                    // an error (not a warning): propagate with `?`, branch with `match`/`else`, or
                    // bind it (`r := f()`) to handle it explicitly.
                    if matches!(self.resolve(te.ty), Ty::Result(..)) {
                        self.diags.error(
                            "unhandled `Result`: propagate it with `?`, handle it with `match`, or bind it (`r := …`) — a discarded Result hides a possible error".to_string(),
                            te.span,
                        );
                    }
                    stmts.push(Stmt::Expr(te));
                }
                ast::Stmt::Assign { place, value } => match self.check_place(place) {
                    Place::Local { id, ty } => {
                        // A fixed stack array (`Array` / `StructArray`) can't be *wholly* reassigned:
                        // array values aren't materialized (only a `let` initializes one), and copying
                        // a Move-struct array would double-own its elements. Reject cleanly — else an
                        // array-literal RHS panics MIR lowering — and point at element assignment.
                        if matches!(ty, Ty::Array(..) | Ty::StructArray(..)) {
                            self.diags.error(
                                format!("whole-array reassignment of {} is not supported yet (array values aren't materialized); assign elements individually, `a[i] = …`", ty_name(ty)),
                                place.span,
                            );
                            let _ = self.check_expr(value, Some(ty)); // surface RHS errors; emit no store
                        } else {
                            // Mirror the `let` path: a slice/str place borrows its source.
                            let v = match ty {
                                Ty::Slice(ps) => self.check_slice_init(value, ps),
                                Ty::Str => self.check_str_init(value),
                                _ => self.check_expr(value, Some(ty)),
                            };
                            stmts.push(Stmt::Assign { local: id, value: v, drop_old: std::cell::Cell::new(false) });
                        }
                    }
                    Place::Field { root, path, ty } => {
                        let v = self.check_expr(value, Some(ty));
                        stmts.push(Stmt::AssignField { root, path, value: v });
                    }
                    Place::Index { base, index, elem } => {
                        let v = self.check_expr(value, Some(elem));
                        stmts.push(Stmt::AssignIndex { base, index, value: v });
                    }
                    Place::VecLane { local, lane, elem } => {
                        let v = self.check_expr(value, Some(elem));
                        stmts.push(Stmt::AssignVecLane { local, lane, value: v });
                    }
                    Place::ElemField { base, index, path, struct_id, soa, ty } => {
                        let v = self.check_expr(value, Some(ty));
                        stmts.push(Stmt::AssignElemField { base, index, path, struct_id, soa, value: v });
                    }
                    Place::Elem { base, index, struct_id, soa } => {
                        let v = self.check_expr(value, Some(Ty::Struct(struct_id)));
                        stmts.push(Stmt::AssignElem { base, index, struct_id, soa, value: v });
                    }
                    Place::Err => {
                        let v = self.check_expr(value, None);
                        stmts.push(Stmt::Expr(v));
                    }
                },
            }
        }

        let value = b
            .tail
            .as_ref()
            .map(|e| Box::new(self.check_expr(e, expected)));
        self.scope.truncate(scope_mark);
        Block { stmts, value }
    }

    /// The declared bound of type parameter `i` in the function being checked.
    fn param_bound(&self, i: u32) -> Bound {
        self.param_bounds.get(i as usize).copied().unwrap_or(Bound::Unconstrained)
    }

    /// "operation X on the generic type 'T' requires the `B` bound (`<T: B>`)".
    fn bound_needed_msg(&self, i: u32, what: &str, needed: Bound) -> String {
        let name = self.type_params.get(i as usize).map(|s| s.as_str()).unwrap_or("T");
        format!("{what} on the generic type '{name}' requires the `{}` bound (declare `<{name}: {}>`)", needed.name(), needed.name())
    }

    fn resolve_type(&mut self, t: &ast::Type) -> Ty {
        let mut cx = TyCx {
            cur_module: &self.cur_module,
            imports: self.user_imports,
            type_table: self.type_table,
            struct_ids: self.struct_ids,
            enum_ids: self.enum_ids,
            struct_templates: self.struct_templates,
            structs: self.structs,
            struct_mono: self.struct_mono,
            enum_templates: self.enum_templates,
            enums: self.enums,
            enum_mono: self.enum_mono,
            tuples: self.tuples,
            fn_types: self.fn_types,
        };
        let ty = resolve_type(t, &mut cx, &self.type_params, self.diags);
        // In monomorph mode a type-parameter annotation (`let x: T`) resolves to the concrete arg.
        if self.mono_args.is_empty() {
            ty
        } else {
            subst_param_ty(ty, &self.mono_args)
        }
    }

    /// Resolve an assignable place: a `mut` local, or `mut_local.field`.
    fn check_place(&mut self, place: &ast::Expr) -> Place {
        // `local[index] = v` — element store into a `mut` array local or `out` slice parameter.
        if let ast::ExprKind::Index { recv, index } = &place.kind {
            let Some((id, local_ty)) = self.place_local(recv) else {
                self.diags.error("invalid assignment target".to_string(), place.span);
                return Place::Err;
            };
            if !self.locals[id as usize].is_mut {
                let name = self.locals[id as usize].name.clone();
                self.diags.error(
                    format!("cannot assign to an element of immutable '{name}' (declare with `mut`, or use an `out` parameter)"),
                    place.span,
                );
            }
            // `v[lane] = x` — write one lane of a `mut` vector (a constant lane in `0..N`, M6).
            if let Ty::Vec(s, n) = local_ty {
                let lane = match &index.kind {
                    ast::ExprKind::Int(v) if *v >= 0 && (*v as u128) < n as u128 => *v as u32,
                    _ => {
                        self.diags.error(format!("a vector lane index must be a constant in 0..{n}"), index.span);
                        return Place::Err;
                    }
                };
                return Place::VecLane { local: id, lane, elem: scalar_to_ty(s) };
            }
            // `arr[i] = structval` / `s[i] = structval` — store a whole struct element (the write
            // counterpart of the `arr[i]` read / `s[i]` gather). First cut: plain-old-data structs
            // (flat primitive numeric/bool/char fields), so the value is Copy with no region; a str /
            // nested / owned field would need escape handling and is deferred.
            if let Ty::StructArray(sid, _) | Ty::Soa(sid) = local_ty {
                let soa = matches!(local_ty, Ty::Soa(_));
                let fields = &self.structs[sid as usize].fields;
                // `!is_empty()` guards the vacuous-true on a zero-field struct: it must not count as
                // POD here, since the soa lowering reads `fields.first()`. (Empty structs aren't
                // constructible today, so this is defensive — but keeps the predicate honest.)
                let pod = !fields.is_empty()
                    && fields.iter().all(|f| {
                        matches!(
                            ty_to_scalar(f.ty).and_then(scalar_to_prim),
                            Some(PrimScalar::Int(_) | PrimScalar::Float(_) | PrimScalar::Bool | PrimScalar::Char)
                        )
                    });
                // A view-Copy struct: every field is a Copy scalar — a primitive (numeric/bool/char)
                // or a `str` **view** (16-byte `{ptr,len}`, borrowed, owns nothing). The aggregate is
                // Copy, so the scatter (`StoreColumn` per field) needs no per-field drop; a stored
                // `str` view's escape is caught by the `AssignElem` region rule (the value must
                // outlive the soa). Restricted to a `soa`: the fixed `array<Struct>` str-element store
                // (a whole-aggregate `StoreIndex`, plus a str-field read back) stays deferred with the
                // rest of the AoS str element work. Owned columns (`string`/`array<T>`, which need a
                // per-column drop of the overwritten value + soa drop) are not scalars, so they fall
                // out of this set and stay rejected — the forward-safe default.
                // Lazy: only walked when `pod` is false (short-circuited in the gate below), so a
                // POD struct never pays the field scan twice.
                let str_view = || {
                    soa && !fields.is_empty()
                        && fields.iter().all(|f| {
                            matches!(
                                ty_to_scalar(f.ty).and_then(scalar_to_prim),
                                Some(
                                    PrimScalar::Int(_)
                                        | PrimScalar::Float(_)
                                        | PrimScalar::Bool
                                        | PrimScalar::Char
                                        | PrimScalar::Str
                                )
                            )
                        })
                };
                // Allowed element-store shapes: a POD or str-view struct into a `soa` (a Copy
                // aggregate scatter; a stored `str` view is escape-checked below); a POD struct into a
                // fixed `array<Struct>` (a Copy aggregate store); or — for a *fixed* `array<Struct>`
                // only — a **Move** struct (Slice 4b: the lowering drops the old element's owned
                // fields, then moves the new value in). Still deferred: a soa of *owned* columns
                // (per-column drop), and a str-view struct into a *fixed* array.
                let is_move = struct_is_move(sid, self.structs);
                if !(pod || str_view() || (!soa && is_move)) {
                    // In the error block: either `soa` is true (an owned-column soa — neither the POD
                    // nor the str field set matched), or `soa` is false with a non-POD, non-Move
                    // struct (a borrowed `str`/view field in a fixed array).
                    let why = if soa {
                        "a soa element store needs scalar or `str` columns for now (owned `string`/array columns are deferred)"
                    } else {
                        "the struct has borrowed `str`/view fields that need region handling — deferred (owned `string` fields are supported)"
                    };
                    self.diags.error(
                        format!("whole-element assignment of {} is not supported yet ({why})", ty_name(local_ty)),
                        place.span,
                    );
                    return Place::Err;
                }
                let i = self.check_expr(index, Some(Ty::Int(IntTy { bits: 64, signed: true })));
                if i.ty == Ty::Error {
                    return Place::Err;
                }
                if !i.ty.is_int_like() {
                    self.diags.error(format!("an array index must be an integer, got {}", ty_name(i.ty)), index.span);
                    return Place::Err;
                }
                return Place::Elem { base: id, index: i, struct_id: sid, soa };
            }
            let elem = match local_ty {
                Ty::Slice(s) | Ty::Array(s, _) | Ty::DynArray(s) => scalar_to_ty(s),
                Ty::Error => return Place::Err,
                other => {
                    self.diags.error(
                        format!("cannot index-assign into {} (only an array or slice)", ty_name(other)),
                        place.span,
                    );
                    return Place::Err;
                }
            };
            // First cut: element stores are primitive-scalar only (int/float/bool/char). A `str`
            // element store would need a region check (storing a borrowed view into the buffer);
            // struct / Move elements need whole-struct / ownership handling. Both deferred.
            if ty_to_scalar(elem).and_then(scalar_to_prim).is_none() {
                self.diags.error(
                    format!("element assignment of {} is not supported yet (primitive elements only for now)", ty_name(elem)),
                    place.span,
                );
                return Place::Err;
            }
            let i = self.check_expr(index, Some(Ty::Int(IntTy { bits: 64, signed: true })));
            if i.ty == Ty::Error {
                return Place::Err;
            }
            if !i.ty.is_int_like() {
                self.diags.error(format!("an array index must be an integer, got {}", ty_name(i.ty)), index.span);
                return Place::Err;
            }
            return Place::Index { base: id, index: i, elem };
        }
        // `local[index].f0.f1.… = v` — store the leaf field of a (possibly nested) struct-array /
        // soa element (the write counterpart of the `c[i].f0.f1.…` read). One leaf is written, no
        // whole-element copy. The receiver spine bottoms at an `Index` of a `mut` local
        // (`peel_index_field_chain`); a pure field path (`local.f0.f1`) returns `None` and falls
        // through to the ordinary field-path handling below.
        if let Some((arr, index, fields)) = peel_index_field_chain(place)
            && let Some((id, local_ty)) = self.place_local(arr)
        {
            // `soa` selects the lowering (`StoreColumn`); `is_dyn` marks an owned dynamic
            // `array<Struct>` view (`StoreElemFieldPtr`), a fixed slot array is neither.
            let kind = match local_ty {
                Ty::Soa(sid) => Some((sid, true, false)),
                Ty::StructArray(sid, _) => Some((sid, false, false)),
                Ty::DynStructArray(sid, _) => Some((sid, false, true)),
                _ => None,
            };
            if let Some((struct_id, soa, is_dyn)) = kind {
                if !self.locals[id as usize].is_mut {
                    let name = self.locals[id as usize].name.clone();
                    self.diags.error(
                        format!("cannot assign to a field of an element of immutable '{name}' (declare with `mut`)"),
                        place.span,
                    );
                }
                // Resolve the field path through the (possibly nested) element struct — each
                // non-final field must itself be a struct so the path can continue (`arr[i].a.x`),
                // mirroring the read side (`check_index_field`). The final field is the leaf written.
                let mut path = Vec::with_capacity(fields.len());
                let mut cur = Ty::Struct(struct_id);
                let mut leaf_ty = Ty::Error;
                for (k, f) in fields.iter().enumerate() {
                    let Some((idx, fty)) = self.field_of(cur, &f.name, f.span) else { return Place::Err };
                    path.push(idx);
                    if k + 1 == fields.len() {
                        leaf_ty = fty;
                    } else if let Ty::Struct(nid) = fty {
                        cur = Ty::Struct(nid);
                    } else {
                        self.diags.error(format!("field '{}' is {}, not a struct — cannot access '.{}' through it", f.name, ty_name(fty), fields[k + 1].name), f.span);
                        return Place::Err;
                    }
                }
                // An owned dynamic `array<Struct>` element-field write goes through the buffer
                // pointer (`StoreElemFieldPtr`), which has no per-element drop of the overwritten
                // field. Restrict its leaf to a primitive scalar (int/float/bool/char) — a `str`/
                // owned/nested-Move leaf write would leak the old value, so it is deferred (matching
                // the fixed-array whole-element restriction: owned columns need region/drop handling).
                let leaf_is_prim = matches!(
                    ty_to_scalar(leaf_ty).and_then(scalar_to_prim),
                    Some(PrimScalar::Int(_) | PrimScalar::Float(_) | PrimScalar::Bool | PrimScalar::Char)
                );
                if is_dyn && !leaf_is_prim {
                    self.diags.error(
                        format!("element-field assignment of `{}` into a dynamic array<{}> is not supported yet (primitive fields only for now; a str/owned field needs region/drop handling — deferred)", ty_name(leaf_ty), self.ty_display(Ty::Struct(struct_id))),
                        place.span,
                    );
                    return Place::Err;
                }
                let i = self.check_expr(index, Some(Ty::Int(IntTy { bits: 64, signed: true })));
                if i.ty == Ty::Error {
                    return Place::Err;
                }
                if !i.ty.is_int_like() {
                    self.diags.error(format!("an array index must be an integer, got {}", ty_name(i.ty)), index.span);
                    return Place::Err;
                }
                return Place::ElemField { base: id, index: i, path, struct_id, soa, ty: leaf_ty };
            }
        }
        // `local.f0.f1.… = v` — a (possibly nested) field path rooted at a mutable local.
        if let ast::ExprKind::FieldAccess { recv, field } = &place.kind {
            let Some((root, mut path, recv_ty)) = self.resolve_place(recv) else {
                self.diags.error("invalid assignment target", place.span);
                return Place::Err;
            };
            if !self.locals[root as usize].is_mut {
                let name = self.locals[root as usize].name.clone();
                self.diags.error(
                    format!("cannot assign to a field of immutable '{name}' (declare with `mut`)"),
                    place.span,
                );
            }
            return match self.field_of(recv_ty, &field.name, place.span) {
                Some((index, ty)) => {
                    path.push(index);
                    Place::Field { root, path, ty }
                }
                None => Place::Err,
            };
        }
        // `local = v`
        let Some((id, local_ty)) = self.place_local(place) else {
            self.diags.error("invalid assignment target", place.span);
            return Place::Err;
        };
        if !self.locals[id as usize].is_mut {
            let name = self.locals[id as usize].name.clone();
            self.diags
                .error(format!("cannot assign to immutable '{name}' (declare with `mut`)"), place.span);
        }
        Place::Local { id, ty: local_ty }
    }

    /// Resolve `(field_index, field_type)` for `ty.name`, reporting errors against `span`.
    fn field_of(&mut self, ty: Ty, name: &str, span: Span) -> Option<(u32, Ty)> {
        let Ty::Struct(id) = ty else {
            if ty != Ty::Error {
                self.diags
                    .error(format!("type {} has no fields", ty_name(ty)), span);
            }
            return None;
        };
        let def = &self.structs[id as usize];
        match def.field_index(name) {
            Some(idx) => Some((idx, def.fields[idx as usize].ty)),
            None => {
                self.diags
                    .error(format!("no field '{name}' on '{}'", def.name), span);
                None
            }
        }
    }

    /// Type-check an expression against its `expected` context type.
    ///
    /// This wrapper adds the **single reconciliation point** between a value's concrete type and the
    /// slot it flows into. Literal / path / constructor arms thread `expected` inward (so a bare `1`
    /// takes the context width); but value-producing arms — a call, a `box.get()`, an `as` cast —
    /// return a *fixed* type and ignore `expected`. Without a final `constrain`, a mismatch there is
    /// silently accepted: the binding site (`let`, assignment, struct field, `return`, call arg) takes
    /// the annotation type while codegen stores the value's real type — a miscompile (e.g. an `i64`
    /// box read into an `i32` slot). Reconciling here catches it as one clean type error, uniformly,
    /// for every context. `constrain` is a no-op when `expected` is `None`, when the arm already
    /// unified with the same `expected` (idempotent), or when either side is `Ty::Error`.
    ///
    /// Gated on "checking this subtree reported no error of its own": an arm that already reported a
    /// mismatch (a reduction terminal enforcing its own result type, or any erroring subexpression)
    /// must not be double-reported. This never lets a real mismatch reach codegen — any error halts
    /// compilation before lowering — so a skipped reconciliation only defers the diagnostic to the
    /// recompile after the pre-existing error is fixed. A *warning* (e.g. the unnecessary-heap lint)
    /// does not gate it: the error count, not the diagnostic count, is the checkpoint.
    fn check_expr(&mut self, e: &ast::Expr, expected: Option<Ty>) -> Expr {
        // No expected type → nothing to reconcile; `constrain(_, None)` is a no-op, so skip the
        // error-count checkpoints entirely (the common case for most sub-expressions).
        let Some(_) = expected else {
            return self.check_expr_inner(e, None);
        };
        let errors_before = self.diags.error_count();
        let result = self.check_expr_inner(e, expected);
        if self.diags.error_count() == errors_before {
            self.constrain(result.ty, expected, e.span);
        }
        result
    }

    fn check_expr_inner(&mut self, e: &ast::Expr, expected: Option<Ty>) -> Expr {
        match &e.kind {
            ast::ExprKind::Unit => {
                self.constrain(Ty::Unit, expected, e.span);
                Expr { kind: ExprKind::Unit, ty: Ty::Unit, span: e.span }
            }
            ast::ExprKind::Int(v) => {
                let ty = self.fresh_int_var();
                self.constrain(ty, expected, e.span);
                Expr { kind: ExprKind::Int(*v), ty, span: e.span }
            }
            ast::ExprKind::Float(v) => {
                let ty = self.fresh_float_var();
                self.constrain(ty, expected, e.span);
                Expr { kind: ExprKind::Float(*v), ty, span: e.span }
            }
            ast::ExprKind::Char(v) => {
                self.constrain(Ty::Char, expected, e.span);
                Expr { kind: ExprKind::Char(*v), ty: Ty::Char, span: e.span }
            }
            ast::ExprKind::Str(s) => {
                self.constrain(Ty::Str, expected, e.span);
                Expr { kind: ExprKind::Str(s.clone()), ty: Ty::Str, span: e.span }
            }
            ast::ExprKind::Bool(b) => {
                self.constrain(Ty::Bool, expected, e.span);
                Expr { kind: ExprKind::Bool(*b), ty: Ty::Bool, span: e.span }
            }
            ast::ExprKind::Path(p) => self.check_path(p, expected, e.span),
            ast::ExprKind::Unary { op, expr } => {
                let inner = self.check_expr(expr, expected);
                let ty = match op {
                    UnOp::Neg => {
                        if !inner.ty.is_numeric() && inner.ty != Ty::Error {
                            self.diags.error("unary '-' expects a number", e.span);
                        }
                        inner.ty
                    }
                    UnOp::Not => {
                        self.unify(inner.ty, Ty::Bool, e.span);
                        Ty::Bool
                    }
                    UnOp::BitNot => {
                        if !inner.ty.is_int_like() && inner.ty != Ty::Error {
                            self.diags.error("unary '~' expects an integer", e.span);
                        }
                        inner.ty
                    }
                };
                Expr { kind: ExprKind::Unary { op: *op, expr: Box::new(inner) }, ty, span: e.span }
            }
            ast::ExprKind::Cast { expr, ty } => self.check_cast(expr, ty, expected, e.span),
            ast::ExprKind::Binary { op, lhs, rhs } => self.check_binary(*op, lhs, rhs, expected, e.span),
            ast::ExprKind::Call { callee, args } => self.check_call(callee, args, expected, e.span),
            ast::ExprKind::FieldAccess { recv, field } => {
                self.check_field_access(recv, field, expected, e.span)
            }
            ast::ExprKind::ArrayLit(elems) => {
                let want = self.resolve(expected.unwrap_or(Ty::Error));
                let lit = match want {
                    // `[…]` under a `vecN<T>` annotation builds a SIMD vector, not an array.
                    Ty::Vec(s, n) => self.check_vec_lit(elems, s, n, e.span),
                    _ => self.check_array_lit(elems, None, e.span),
                };
                // A fixed array literal is a *stack* value; an owned `array<T>` (`DynArray`) is
                // heap-allocated. A bare literal cannot silently become one (that would hide the
                // allocation — "Nothing hidden", and codegen currently miscompiles it): reject it
                // in an owned-array context and point to `.to_array()` (the visible materialization).
                if matches!(lit.ty, Ty::Array(..) | Ty::StructArray(..))
                    && matches!(want, Ty::DynArray(_) | Ty::DynStructArray(..))
                {
                    self.diags.error(
                        "a fixed array literal is not an owned `array<T>` — materialize it with `.to_array()` (its heap allocation is explicit)".to_string(),
                        e.span,
                    );
                    return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span: e.span };
                }
                lit
            }
            ast::ExprKind::Index { recv, index } => self.check_index(recv, index, e.span),
            ast::ExprKind::SliceRange { recv, start, end } => self.check_slice_range(recv, start.as_deref(), end.as_deref(), e.span),
            ast::ExprKind::Template(parts) => self.check_template(parts, expected, e.span),
            ast::ExprKind::FieldShorthand(_) => {
                self.diags.error(
                    "`.field` is only valid as a pipeline stage argument (e.g. `where(.active)`)".to_string(),
                    e.span,
                );
                Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span: e.span }
            }
            // A lambda used as a value (`f := fn x: i32 { … }`) is a first-class function value
            // (`Ty::Fn`): lift it like a stage lambda, but its parameter types come from explicit
            // annotations (there is no use site to infer from). Slice ②a: non-capturing only.
            ast::ExprKind::Lambda { params, body } => self.check_lambda_value(params, body, expected, e.span),
            ast::ExprKind::ElseUnwrap { opt, fallback } => {
                self.check_else_unwrap(opt, fallback, expected, e.span)
            }
            ast::ExprKind::Try(inner) => self.check_try(inner, expected, e.span),
            ast::ExprKind::Arena(b) => {
                let diverges = ast_block_diverges(b);
                self.arena_depth += 1;
                let block = self.check_block(b, if diverges { None } else { expected });
                self.arena_depth -= 1;
                let ty = if diverges {
                    expected.unwrap_or(Ty::Unit)
                } else {
                    let t = block.value.as_ref().map(|v| v.ty).unwrap_or(Ty::Unit);
                    self.constrain(t, expected, e.span);
                    t
                };
                Expr { kind: ExprKind::Arena(block), ty, span: e.span }
            }
            ast::ExprKind::Unsafe(b) => {
                // A marker block — no region, no runtime effect. It only raises `unsafe_depth` so the
                // `raw.*` ops inside are permitted, and (via the effect scan) marks the fn impure. The
                // block value passes through exactly like a plain block / `arena {}`.
                let diverges = ast_block_diverges(b);
                self.unsafe_depth += 1;
                let block = self.check_block(b, if diverges { None } else { expected });
                self.unsafe_depth -= 1;
                let ty = if diverges {
                    expected.unwrap_or(Ty::Unit)
                } else {
                    let t = block.value.as_ref().map(|v| v.ty).unwrap_or(Ty::Unit);
                    self.constrain(t, expected, e.span);
                    t
                };
                Expr { kind: ExprKind::Unsafe(block), ty, span: e.span }
            }
            ast::ExprKind::TaskGroup(b) => {
                let diverges = ast_block_diverges(b);
                // A `task_group` opens a region (like `arena {}`): spawned task handles are boxes
                // in it, region-tied to the scope (so a `Task` can't escape).
                self.task_group_depth += 1;
                self.arena_depth += 1;
                self.wait_state.push(false);
                self.task_group_fallible.push(false);
                let block = self.check_block(b, if diverges { None } else { expected });
                self.task_group_fallible.pop();
                self.wait_state.pop();
                self.arena_depth -= 1;
                self.task_group_depth -= 1;
                let ty = if diverges {
                    expected.unwrap_or(Ty::Unit)
                } else {
                    let t = block.value.as_ref().map(|v| v.ty).unwrap_or(Ty::Unit);
                    self.constrain(t, expected, e.span);
                    t
                };
                Expr { kind: ExprKind::TaskGroup(block), ty, span: e.span }
            }
            ast::ExprKind::StructLit { name, fields } => {
                // A struct literal is a value expression (constructed, then passed/returned/
                // stored). The `let` path checks it directly to store fields in place.
                self.check_struct_lit(name, fields, e.span)
            }
            ast::ExprKind::Tuple(elems) => self.check_tuple(elems, expected, e.span),
            ast::ExprKind::TupleIndex { recv, index } => self.check_tuple_index(recv, *index, expected, e.span),
            ast::ExprKind::If { cond, then, els } => self.check_if(cond, then, els.as_deref(), expected, e.span),
            ast::ExprKind::Match { scrutinee, arms } => self.check_match(scrutinee, arms, expected, e.span),
            ast::ExprKind::Block(b) => {
                // A block that always returns never yields a value; let it take the
                // expected type so it fits any value position.
                if ast_block_diverges(b) {
                    let block = self.check_block(b, None);
                    let ty = expected.unwrap_or(Ty::Unit);
                    return Expr { kind: ExprKind::Block(block), ty, span: e.span };
                }
                let block = self.check_block(b, expected);
                let ty = block.value.as_ref().map(|v| v.ty).unwrap_or(Ty::Unit);
                Expr { kind: ExprKind::Block(block), ty, span: e.span }
            }
        }
    }

    /// `(e0, e1, ...)` — a tuple literal. Element types are taken from the expected tuple type
    /// when context fixes one (e.g. a multi-value `return`), else each element defaults like a
    /// bare `:=` binding (int → i64, float → f64). PR1 cut: elements are primitive scalars.
    fn check_tuple(&mut self, elems: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // If the context fixes a concrete tuple type, use its element types to drive checking.
        let expected_elems: Option<Vec<Ty>> = match expected.map(|t| self.resolve(t)) {
            Some(Ty::Tuple(id)) => {
                Some(self.tuples[id as usize].elems.iter().map(|s| scalar_to_ty(*s)).collect())
            }
            _ => None,
        };
        if let Some(exp) = &expected_elems
            && exp.len() != elems.len() {
                self.diags.error(
                    format!("expected a tuple of {} element(s), got {}", exp.len(), elems.len()),
                    span,
                );
                return err;
            }
        let mut checked = Vec::with_capacity(elems.len());
        let mut scalars = Vec::with_capacity(elems.len());
        let mut ok = true;
        for (i, el) in elems.iter().enumerate() {
            let exp_i = expected_elems.as_ref().map(|v| v[i]);
            let ce = self.check_expr(el, exp_i);
            // Commit the element to a concrete scalar: bind any inference var to the expected type
            // or its default, so the interned tuple type (and later uses of the element) agree.
            let concrete = self.finalize(ce.ty);
            self.constrain(ce.ty, Some(concrete), ce.span);
            match ty_to_scalar(self.resolve(ce.ty)) {
                Some(s @ (Scalar::Int(_) | Scalar::Float(_) | Scalar::Bool | Scalar::Char
                | Scalar::Str | Scalar::String | Scalar::DynArray(_) | Scalar::DynStructArray(_))) => scalars.push(s),
                _ => {
                    if ce.ty != Ty::Error {
                        self.diags.error(
                            format!("tuple elements must be a scalar, str, owned string, or owned array for now, got {}", ty_name(ce.ty)),
                            ce.span,
                        );
                    }
                    ok = false;
                }
            }
            checked.push(ce);
        }
        if !ok {
            return err;
        }
        let id = intern_tuple(self.tuples, scalars);
        let ty = Ty::Tuple(id);
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::Tuple { tuple_id: id, elems: checked }, ty, span }
    }

    /// `recv.N` — positional tuple access.
    fn check_tuple_index(&mut self, recv: &ast::Expr, index: u32, expected: Option<Ty>, span: Span) -> Expr {
        // On any error return a dummy `Ty::Error` expr (not a `TupleIndex` node): a `TupleIndex`
        // whose receiver is not a tuple would panic codegen's `into_struct_value()`.
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let r = self.check_expr(recv, None);
        match self.resolve(r.ty) {
            Ty::Tuple(id) => {
                let elems = &self.tuples[id as usize].elems;
                match elems.get(index as usize) {
                    // Reading an *owned* element by index moves it out of the tuple, leaving the
                    // other elements usable (MoveCheck tracks the per-field move; MIR nulls the
                    // moved field so the tuple's `Drop` frees null there). This needs a bound
                    // tuple (a `Local`) to name the field being moved; a non-local tuple temporary
                    // (`f().0`) would orphan its other owned elements, so destructure that instead.
                    Some(s) if s.is_move() => {
                        if !matches!(r.kind, ExprKind::Local(_)) {
                            self.diags.error(
                                format!("`.{index}` would move the owned element {} out of a temporary tuple — bind it to a variable, or destructure with `(a, b) := …`", scalar_name(*s)),
                                span,
                            );
                            return err;
                        }
                        let ty = scalar_to_ty(*s);
                        self.constrain(ty, expected, span);
                        Expr { kind: ExprKind::TupleIndex { recv: Box::new(r), index }, ty, span }
                    }
                    Some(s) => {
                        let ty = scalar_to_ty(*s);
                        self.constrain(ty, expected, span);
                        Expr { kind: ExprKind::TupleIndex { recv: Box::new(r), index }, ty, span }
                    }
                    None => {
                        self.diags.error(
                            format!("tuple index .{index} is out of range (tuple has {} element(s))", elems.len()),
                            span,
                        );
                        err
                    }
                }
            }
            Ty::Error => err,
            other => {
                self.diags.error(
                    format!("`.{index}` needs a tuple value, got {}", ty_name(other)),
                    span,
                );
                err
            }
        }
    }

    /// Substitute a folded constant `(ty, value)` as a literal HIR expression (constrained against
    /// the expected type — a constant has a fixed type, so a mismatch is a normal type error).
    fn const_literal(&mut self, ty: Ty, val: &ConstVal, expected: Option<Ty>, span: Span) -> Expr {
        self.constrain(ty, expected, span);
        let kind = match val {
            ConstVal::Int(v) => ExprKind::Int(*v),
            ConstVal::Float(v) => ExprKind::Float(*v),
            ConstVal::Bool(b) => ExprKind::Bool(*b),
            ConstVal::Char(c) => ExprKind::Char(*c),
            ConstVal::Str(s) => ExprKind::Str(s.clone()),
        };
        Expr { kind, ty, span }
    }

    fn check_path(&mut self, p: &ast::Path, expected: Option<Ty>, span: Span) -> Expr {
        let err = |s: Span| Expr { kind: ExprKind::Local(u32::MAX), ty: Ty::Error, span: s };
        // `None` builtin: its Option type comes from context.
        if single_name(p) == Some("None") {
            return match expected {
                Some(Ty::Option(s)) => Expr { kind: ExprKind::OptionNone, ty: Ty::Option(s), span },
                _ => {
                    self.diags
                        .error("cannot infer the Option type of `None` here (add an annotation)".to_string(), span);
                    Expr { kind: ExprKind::OptionNone, ty: Ty::Error, span }
                }
            };
        }
        let base = p.segments.first().map(|s| s.name.as_str()).unwrap_or("");
        let Some(id) = self.lookup_or_capture(base) else {
            // A top-level constant in this module — substitute its folded value as a literal.
            if let Ok(Some((ty, val))) = self.consts.resolve(&self.cur_module, base, &self.cur_module) {
                return self.const_literal(ty, &val, expected, span);
            }
            // A top-level function used as a value (`f := double`) is a first-class function
            // pointer (`Ty::Fn`). Slice ①: scalar params/ret, no `out` params. The name resolves
            // in the current module to its mangled codegen name (cross-module fn values are later).
            if let Some(mangled) = self.resolve_local_fn(base) {
                let sig = &self.sigs[&mangled];
                let params: Option<Vec<Scalar>> = sig.params.iter().map(|t| ty_to_scalar(*t)).collect();
                let ret = ty_to_scalar(sig.ret);
                match (params, ret) {
                    (Some(ps), Some(r)) if !sig.out.iter().any(|o| *o) => {
                        let fid = intern_fn_type(self.fn_types, ps, r);
                        let ty = Ty::Fn(fid);
                        self.constrain(ty, expected, span);
                        return Expr { kind: ExprKind::FnValue(mangled), ty, span };
                    }
                    _ => {
                        self.diags.error(
                            format!("'{base}' cannot be used as a function value yet (only scalar parameters/return, no `out`)"),
                            span,
                        );
                        return err(span);
                    }
                }
            }
            self.diags.error(format!("undefined name: '{base}'"), span);
            return err(span);
        };
        let local_ty = self.locals[id as usize].ty;
        // A struct is a value: it may be read whole (copied), passed, and returned.
        self.constrain(local_ty, expected, span);
        Expr { kind: ExprKind::Local(id), ty: local_ty, span }
    }

    /// `recv.field` (not a method call) — a struct field read. M4: the receiver must be
    /// a local (chained field access on a value comes later).
    fn check_field_access(&mut self, recv: &ast::Expr, field: &ast::Ident, expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Local(u32::MAX), ty: Ty::Error, span };
        // `io.stdin` / `io.stdout` / `io.stderr` — a std.io `reader` / `writer` VALUE (a constructor).
        // "One type, many constructors": the bare stream is unbuffered; `.buffered()` (intercepted in
        // `check_method_call`) is the buffered writer. A local/captured `io` shadows this.
        if let ast::ExprKind::Path(p) = &recv.kind
            && single_name(p) == Some("io")
            && matches!(field.name.as_str(), "stdin" | "stdout" | "stderr")
            && !self.name_in_scope("io") {
                self.require_import("std.io", &format!("io.{}", field.name), span);
                return match field.name.as_str() {
                    "stdin" => Expr { kind: ExprKind::ReaderStdin, ty: Ty::Reader, span },
                    "stdout" => Expr { kind: ExprKind::WriterStd { fd: 1, buffered: false }, ty: Ty::Writer, span },
                    _ => Expr { kind: ExprKind::WriterStd { fd: 2, buffered: false }, ty: Ty::Writer, span },
                };
            }
        // `Type.Variant` / `mod.Type.Variant` — a (tag-only) sum-type value, not field access on a
        // value. The receiver names a sum type (bare in this module, or a qualified import); a
        // payload variant is a *call* (handled in `check_call`). `check_variant_ctor` reports a
        // payload variant named without arguments here ("takes N argument(s)").
        match self.resolve_type_receiver(recv) {
            Ok(Some(canonical)) => {
                if let Some(tmpl) = self.enum_templates.get(&canonical).cloned() {
                    // A no-arg variant of a generic sum type (`Opt.None`) — the type parameters must
                    // come from the payload args (here none), so they are only inferable when the
                    // variant carries a payload. Routed through the same path for a clear error.
                    return self.check_generic_variant_ctor(&canonical, &tmpl, field, &[], expected, span);
                }
                if let Some(&enum_id) = self.enum_ids.get(&canonical) {
                    return self.check_variant_ctor(enum_id, field, &[], expected, span);
                }
            }
            Ok(None) => {}
            Err(()) => return err,
        }
        // `mod.NAME` / `a.b.NAME` — a qualified reference to an imported module's `pub` constant.
        // The receiver is a pure dotted module name; a local shadowing the leftmost segment makes
        // this ordinary value field access instead.
        // A local *or captured* variable shadowing the leftmost segment makes this value field
        // access, not a `mod.NAME` constant reference (mirrors `check_method_call`).
        let leftmost_is_local = leftmost_segment(recv).is_some_and(|leftmost| {
            self.lookup(leftmost).is_some()
                || self.capture.as_ref().is_some_and(|cap| {
                    cap.captured.iter().any(|(n, _, _)| n == leftmost)
                        || cap.enclosing.iter().any(|(n, _, _)| n == leftmost)
                })
        });
        if let Some(modpath) = flatten_module_path(recv).filter(|m| {
            !leftmost_is_local && m != &self.cur_module && self.user_imports.contains(m)
        }) {
            match self.consts.resolve(&modpath, &field.name, &self.cur_module) {
                Ok(Some((ty, val))) => return self.const_literal(ty, &val, expected, span),
                Ok(None) => {
                    self.diags.error(format!("module `{modpath}` has no constant `{}`", field.name), span);
                    return err;
                }
                Err(msg) => {
                    self.diags.error(msg, span);
                    return err;
                }
            }
        }
        // `arr[i].f0.f1…` — field access on a (possibly nested) field of a struct-array element. A
        // depth-1 `arr[i].field` is one bounds-checked element-field load; a nested path
        // (`arr[i].a.x`) loads the first sub-struct then projects. A whole-struct `arr[i]` value is
        // not materialized.
        if let Some((arr, index, mut names)) = peel_index_field_chain(recv) {
            names.push(field);
            return self.check_index_field(arr, index, &names, expected, span);
        }
        // Resolve the receiver to a struct **place** — a local, or a nested field path `l.a.b`.
        let (root, mut path, recv_ty) = match self.resolve_place(recv) {
            Some(t) => t,
            None => {
                self.diags
                    .error("field access is only supported on a local binding".to_string(), span);
                return err;
            }
        };
        // `soa_value.field` — project one column of a struct-of-arrays as a `slice<FieldTy>` (the
        // cache lever: a scan reads only the touched columns). Reuses the slice pipeline downstream.
        // (Only directly on a soa local; a column of a *nested* field is a later slice.)
        if let Ty::Soa(id) = recv_ty {
            if !path.is_empty() {
                self.diags.error("soa column projection of a nested field is not supported yet".to_string(), span);
                return err;
            }
            let def = &self.structs[id as usize];
            let Some(index) = def.field_index(&field.name) else {
                self.diags.error(format!("no field '{}' on soa<{}>", field.name, def.name), span);
                return err;
            };
            let field_ty = def.fields[index as usize].ty;
            let elem = ty_to_scalar(field_ty).expect("soa fields are primitive scalars");
            let ty = Ty::Slice(elem);
            self.constrain(ty, expected, span);
            return Expr { kind: ExprKind::SoaColumn { base: root, struct_id: id, field: index }, ty, span };
        }
        match self.field_of(recv_ty, &field.name, span) {
            Some((index, ty)) => {
                path.push(index);
                // An **owned** (Move) leaf field — a `string`, or a whole nested Move struct — may be
                // *borrowed* (`u.name.len()`, a `str` argument, `io.stdout.write(u.name)`): the
                // `string` → `str` coercion / `Len` wrap the access non-consuming, and the borrow is
                // `Frame`-regioned (tied to the struct, can't escape). *Moving* it out (`n := u.name`,
                // `f(u.name)` by value, `return u.name`) is a partial move — still deferred, rejected
                // by `MoveCheck` at the consuming site (it would need the field nulled so the
                // struct's `Drop` doesn't double-free it).
                self.constrain(ty, expected, span);
                Expr { kind: ExprKind::Field { root, path }, ty, span }
            }
            None => err,
        }
    }

    /// If `e` is a bare local name, return its id and type.
    /// Follow a slice local to the root buffer it borrows (an array, or a slice parameter — its
    /// own root). A non-slice / unborrowed local is its own root.
    fn root_local(&self, id: LocalId) -> LocalId {
        let mut cur = id;
        let mut guard = 0;
        while let Some(&base) = self.slice_bases.get(&cur) {
            if base == cur || guard > self.locals.len() {
                break;
            }
            cur = base;
            guard += 1;
        }
        cur
    }

    /// Whether a resolved root buffer local (`root_local`) is a *known, distinct* backing buffer —
    /// the soundness test for `map_into`'s alias gate. Known ⇔ the root is a slice/array parameter
    /// (its buffer is distinct from the other arguments by the caller's `out` no-alias contract) or
    /// a real array local (a genuine backing buffer). A slice-typed **body** local that resolved to
    /// itself was `let`-bound to a value of unknown origin (a fn-returned slice, a soa column, a
    /// struct-field slice) — its buffer could be anything, so it is *not* known and must not back a
    /// `noalias` claim. (A slice local borrowed from an array is provenance-tracked, so its root is
    /// the array local, a non-slice type, and is known.)
    ///
    /// **Why trusting a parameter is sound** (the `noalias` trust chain): a slice parameter's
    /// buffer is distinct from the function's other slice arguments *only because every caller is
    /// checked*. That holds because (1) the caller-side `out` no-alias check (in `check_named_call`)
    /// now **rejects** any call it cannot prove disjoint (an unresolvable or unknown-origin
    /// argument), not just same-named locals; (2) a function with an `out` parameter **cannot become
    /// a first-class `fn` value** (rejected — see `resolve_local_fn`), so there is no unchecked
    /// indirect call; and (3) `extern "C"` is **import-only** (bodyless declarations) — Align has no
    /// C-ABI *export* of a function body, so no external, unchecked caller exists. If a
    /// separate-compilation / C-export path is ever added, param-derived `map_into` `noalias`
    /// emission must be re-gated (its callers would no longer be provably checked).
    fn slice_root_is_known(&self, root: LocalId) -> bool {
        let l = &self.locals[root as usize];
        l.is_param || !matches!(self.resolve(l.ty), Ty::Slice(_))
    }

    /// The root buffer local an HIR expression borrows, if it resolves to one (a local or an
    /// array→slice borrow). Used to record slice provenance for the `out` no-alias check.
    fn expr_root_local(&self, e: &Expr) -> Option<LocalId> {
        match &e.kind {
            ExprKind::Local(id) => Some(self.root_local(*id)),
            ExprKind::ArrayToSlice(inner) => self.expr_root_local(inner),
            // A sub-slice `recv[a..b]` is a view into `recv`'s storage, so its root buffer is the
            // receiver's root (recursively — handles nested `xs[0..4][1..2]`). Without this, a
            // slice binding `s := xs[0..2]` records no provenance and the `out` no-alias check
            // cannot see that `s` and `xs` share a buffer.
            ExprKind::SliceRange { recv, .. } => self.expr_root_local(recv),
            _ => None,
        }
    }

    /// The root buffer local an **AST** call argument borrows, if it resolves to one. A bare name
    /// resolves through slice provenance to its root buffer (`root_local`); a sub-slice expression
    /// `recv[a..b]` (an *inline* argument, which has no binding to record provenance for) resolves
    /// to the receiver's root recursively. Used by the `out` no-alias check, which runs on the raw
    /// argument AST before it is checked.
    fn arg_root_local(&self, a: &ast::Expr) -> Option<LocalId> {
        match &a.kind {
            ast::ExprKind::Path(_) => self.place_local(a).map(|(id, _)| self.root_local(id)),
            ast::ExprKind::SliceRange { recv, .. } => self.arg_root_local(recv),
            _ => None,
        }
    }

    fn place_local(&self, e: &ast::Expr) -> Option<(LocalId, Ty)> {
        if let ast::ExprKind::Path(p) = &e.kind
            && let Some(name) = single_name(p)
                && let Some(id) = self.lookup(name) {
                    return Some((id, self.locals[id as usize].ty));
                }
        None
    }

    /// Resolve `e` to a struct **place** rooted at a local: `(root_local, field_path, place_ty)`.
    /// A bare local is `(id, [], ty)`; a nested `recv.field` appends the field's index. Returns
    /// `None` if the root isn't a local (caller reports the diagnostic) or an intermediate isn't a
    /// struct (a soa/array nested place is a later slice); a missing field is reported via `field_of`.
    fn resolve_place(&mut self, e: &ast::Expr) -> Option<(LocalId, Vec<u32>, Ty)> {
        match &e.kind {
            ast::ExprKind::Path(_) => {
                let (id, ty) = self.place_local(e)?;
                Some((id, Vec::new(), ty))
            }
            ast::ExprKind::FieldAccess { recv, field } => {
                let (root, mut path, recv_ty) = self.resolve_place(recv)?;
                if !matches!(recv_ty, Ty::Struct(_)) {
                    return None;
                }
                let (idx, fty) = self.field_of(recv_ty, &field.name, e.span)?;
                path.push(idx);
                Some((root, path, fty))
            }
            _ => None,
        }
    }

    /// `Name { field: value, ... }`. Reorders inits into declaration order and requires
    /// every field exactly once. Only reached from a `let` initializer (M1).
    /// Monomorphize a generic struct from the Checker (builds a `TyCx` over its interner fields).
    fn instantiate_struct(&mut self, name: &str, tmpl: &StructTemplate, args: &[Ty], span: Span) -> u32 {
        let mut cx = TyCx {
            cur_module: &self.cur_module,
            imports: self.user_imports,
            type_table: self.type_table,
            struct_ids: self.struct_ids,
            enum_ids: self.enum_ids,
            struct_templates: self.struct_templates,
            structs: self.structs,
            struct_mono: self.struct_mono,
            enum_templates: self.enum_templates,
            enums: self.enums,
            enum_mono: self.enum_mono,
            tuples: self.tuples,
            fn_types: self.fn_types,
        };
        instantiate_struct(name, tmpl, args, &mut cx, span, self.diags)
    }

    /// Resolve a type-name path to its canonical key (a bare name in `cur_module`, or a qualified
    /// `mod.Type` with import + `pub` checks). Emits a diagnostic on failure.
    fn canonical_type(&mut self, path: &ast::Path, span: Span) -> Option<String> {
        canonical_type_name(path, &self.cur_module, self.user_imports, self.type_table, true, span, self.diags)
    }

    /// Resolve a bare type name to its canonical key in the current module (or the builtin `Error`),
    /// without emitting an error — for speculative interpretations (e.g. `Name.Variant`) that fall
    /// through to other meanings when `Name` is not a type here.
    fn local_type(&self, bare: &str) -> Option<String> {
        if bare == "Error" {
            return Some("Error".to_string());
        }
        if bare == "argon2_params" {
            return Some("argon2_params".to_string());
        }
        self.type_table.get(&self.cur_module)?.get(bare).map(|e| e.canonical.clone())
    }

    /// Resolve the receiver of a `Type.Variant` access/constructor to the type's canonical name —
    /// a bare `Type` (current module) or a qualified `mod.Type` (an imported module's `pub` type).
    /// `Ok(None)` means the receiver is not a type reference (the caller falls through to other
    /// meanings); `Err(())` means it definitively names an imported type that is not `pub` (the
    /// error is already reported — the caller should stop, not cascade).
    fn resolve_type_receiver(&mut self, recv: &ast::Expr) -> Result<Option<String>, ()> {
        // A local/captured variable shadowing the leftmost segment makes this value access, not a
        // type reference — checked first, so it applies to a bare `Type` as well as `mod.Type`.
        let Some(leftmost) = leftmost_segment(recv) else { return Ok(None) };
        let shadowed = self.lookup(leftmost).is_some()
            || self.capture.as_ref().is_some_and(|cap| {
                cap.captured.iter().any(|(n, _, _)| n == leftmost)
                    || cap.enclosing.iter().any(|(n, _, _)| n == leftmost)
            });
        if shadowed {
            return Ok(None);
        }
        // Bare `Type` in the current module.
        if let ast::ExprKind::Path(p) = &recv.kind
            && let Some(name) = single_name(p) {
                return Ok(self.local_type(name));
            }
        // Qualified `mod.Type` — the receiver is itself a pure dotted name.
        let Some(flat) = flatten_module_path(recv) else { return Ok(None) };
        let Some((module, type_name)) = flat.rsplit_once('.') else { return Ok(None) };
        if module == self.cur_module || !self.user_imports.contains(module) {
            return Ok(None);
        }
        let Some(entry) = self.type_table.get(module).and_then(|m| m.get(type_name)) else {
            return Ok(None);
        };
        if !entry.is_pub {
            self.diags.error(
                format!("type `{type_name}` is private to module `{module}` (mark it `pub` to export it)"),
                recv.span,
            );
            return Err(());
        }
        Ok(Some(entry.canonical.clone()))
    }

    fn check_struct_lit(&mut self, name: &ast::Path, fields: &[ast::FieldInit], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Some(canonical) = self.canonical_type(name, name.span) else { return err };
        // A generic struct literal (`Pair { a: 1, b: 2 }`): infer the type arguments from the field
        // values, then monomorphize. (Type arguments are not written at the literal — same as calls.)
        if let Some(tmpl) = self.struct_templates.get(&canonical).cloned() {
            return self.check_generic_struct_lit(&canonical, &tmpl, fields, span);
        }
        let Some(&id) = self.struct_ids.get(&canonical) else {
            let bare = name.segments.last().map(|s| s.name.as_str()).unwrap_or("");
            self.diags.error(format!("'{bare}' is not a struct"), name.span);
            return err;
        };
        let layout: Vec<(String, Ty)> = self.structs[id as usize]
            .fields
            .iter()
            .map(|f| (f.name.clone(), f.ty))
            .collect();
        let sname = self.structs[id as usize].name.clone();

        let mut values: Vec<Option<Expr>> = (0..layout.len()).map(|_| None).collect();
        for fi in fields {
            match layout.iter().position(|(n, _)| *n == fi.name.name) {
                Some(idx) => {
                    if values[idx].is_some() {
                        self.diags
                            .error(format!("duplicate field '{}'", fi.name.name), fi.span);
                    }
                    values[idx] = Some(self.check_expr(&fi.value, Some(layout[idx].1)));
                }
                None => {
                    self.diags
                        .error(format!("no field '{}' on '{sname}'", fi.name.name), fi.span);
                    let _ = self.check_expr(&fi.value, None);
                }
            }
        }

        let mut out = Vec::with_capacity(layout.len());
        for (idx, v) in values.into_iter().enumerate() {
            match v {
                Some(e) => out.push(e),
                None => {
                    self.diags
                        .error(format!("missing field '{}' in '{sname}'", layout[idx].0), span);
                    out.push(Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span });
                }
            }
        }
        Expr { kind: ExprKind::StructLit { struct_id: id, fields: out }, ty: Ty::Struct(id), span }
    }

    /// A generic struct literal `Pair { a: 1, b: 2 }`: check each field value, infer the type
    /// parameters by matching the value's type against the template field's type, monomorphize the
    /// struct, and build the literal against the resulting concrete instance.
    fn check_generic_struct_lit(&mut self, name: &str, tmpl: &StructTemplate, fields: &[ast::FieldInit], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let mut subst: Vec<Option<Ty>> = vec![None; tmpl.type_params.len()];
        let mut values: Vec<Option<Expr>> = (0..tmpl.fields.len()).map(|_| None).collect();
        for fi in fields {
            match tmpl.fields.iter().position(|f| f.name == fi.name.name) {
                Some(idx) => {
                    if values[idx].is_some() {
                        self.diags.error(format!("duplicate field '{}'", fi.name.name), fi.span);
                    }
                    // A `Param` field applies no coercion (its type is being inferred); a concrete
                    // field checks against its declared type.
                    let declared = tmpl.fields[idx].ty;
                    let ce = if ty_mentions_param(declared) {
                        self.check_expr(&fi.value, None)
                    } else {
                        self.check_expr(&fi.value, Some(declared))
                    };
                    self.match_param(declared, ce.ty, &mut subst, fi.span, false);
                    values[idx] = Some(ce);
                }
                None => {
                    self.diags.error(format!("no field '{}' on '{name}'", fi.name.name), fi.span);
                    let _ = self.check_expr(&fi.value, None);
                }
            }
        }
        // Every type parameter must be inferable from the fields; finalize each (a field carrying a
        // `Param` resolves to a concrete struct field, so finalize eagerly — defaults a bare literal).
        let mut args = Vec::with_capacity(tmpl.type_params.len());
        for (i, s) in subst.iter().enumerate() {
            let concrete = s.map(|t| self.finalize(t)).unwrap_or(Ty::Error);
            if matches!(concrete, Ty::Param(_)) {
                // The field value's type is a (generic function's) type parameter — constructing a
                // generic struct from inside a generic function. Deferred (see `resolve_type`).
                self.diags.error(
                    format!("constructing a generic struct ('{name}') with a type parameter inside a generic function is not supported yet"),
                    span,
                );
                return err;
            }
            if matches!(concrete, Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Error) {
                self.diags.error(
                    format!("cannot infer type parameter '{}' of '{name}' from the fields", tmpl.type_params[i]),
                    span,
                );
                return err;
            }
            args.push(concrete);
        }
        let mut out = Vec::with_capacity(tmpl.fields.len());
        for (idx, v) in values.into_iter().enumerate() {
            match v {
                Some(e) => out.push(e),
                None => {
                    self.diags.error(format!("missing field '{}' in '{name}'", tmpl.fields[idx].name), span);
                    out.push(Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span });
                }
            }
        }
        let id = self.instantiate_struct(name, tmpl, &args, span);
        Expr { kind: ExprKind::StructLit { struct_id: id, fields: out }, ty: Ty::Struct(id), span }
    }

    /// A binary op with a **vector** operand: returns its `(element, width)` after broadcasting a
    /// scalar operand across the lanes (M6 — `a + 2.0`, `scores > 80`, and `2.0 + a`, `80 < scores`).
    /// The scalar's type is unified with the element type; a vector–vector op must match exactly.
    /// `None` when neither operand is a vector (the ordinary scalar path runs).
    fn vec_binop(&mut self, l: &Expr, r: &Expr, span: Span) -> Option<(Scalar, u32)> {
        match (self.resolve(l.ty), self.resolve(r.ty)) {
            (Ty::Vec(s, n), Ty::Vec(..)) => {
                self.unify(l.ty, r.ty, span); // vec OP vec — element + width match
                Some((s, n))
            }
            (Ty::Vec(s, n), _) => {
                self.unify(scalar_to_ty(s), r.ty, span); // vec OP scalar — broadcast the rhs
                Some((s, n))
            }
            (_, Ty::Vec(s, n)) => {
                self.unify(scalar_to_ty(s), l.ty, span); // scalar OP vec — broadcast the lhs
                Some((s, n))
            }
            _ => None,
        }
    }

    /// Check the right operand of a binary op, supporting a scalar–vector broadcast in either order.
    /// Normally the rhs is hinted with the lhs type (so a literal adopts it). But if the lhs is a
    /// scalar and the rhs turns out to be a vector (`2.0 + a`), that hint mis-constrains, so the
    /// speculative diagnostics are rolled back and the rhs is re-checked unhinted (the scalar then
    /// broadcasts against the vector in [`Self::vec_binop`]).
    fn check_binop_rhs(&mut self, lhs_ty: Ty, rhs: &ast::Expr) -> Expr {
        if matches!(self.resolve(lhs_ty), Ty::Vec(..)) {
            return self.check_expr(rhs, None); // vec lhs: rhs self-types
        }
        let mark = self.diags.len();
        let r = self.check_expr(rhs, Some(lhs_ty));
        if matches!(self.resolve(r.ty), Ty::Vec(..)) {
            // Sound to roll back here: the only side effect of the speculative check was the
            // diagnostic. The hint applied `unify(rhs = Vec, lhs_ty)`, and `unify` binds a variable
            // only to a concrete `Int`/`Float` (its `(IntVar, Int)` / `(FloatVar, Float)` arms) — never
            // to a `Vec` (that hits the mismatch arm), so `lhs_ty` is left exactly as it was.
            self.diags.truncate(mark);
            return self.check_expr(rhs, None);
        }
        r
    }

    fn check_binary(&mut self, op: BinOp, lhs: &ast::Expr, rhs: &ast::Expr, expected: Option<Ty>, span: Span) -> Expr {
        let ty;
        let (l, r);
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                l = self.check_expr(lhs, expected);
                r = self.check_binop_rhs(l.ty, rhs);
                if let Some((s, n)) = self.vec_binop(&l, &r, span) {
                    // Vectors support all elementwise arithmetic `+` `-` `*` `/` `%` (M6). Integer
                    // `/`/`%` carry the same lane-wise divisor guard as scalars (zero lane → abort,
                    // signed `INT_MIN/-1` lane wraps); float `%` is IEEE `frem`, lane-wise.
                    ty = Ty::Vec(s, n);
                } else {
                    let t = self.unify(l.ty, r.ty, span);
                    // `str + str` is concatenation; other ops on str are errors.
                    if let Ty::Param(i) = t {
                        // A generic value: arithmetic needs the `Num` bound.
                        if !self.param_bound(i).grants_arith() {
                            self.diags.error(self.bound_needed_msg(i, "arithmetic", Bound::Num), span);
                        }
                    } else if t == Ty::Str && op != BinOp::Add {
                        self.diags.error("str supports only `+` (concatenation)", span);
                    } else if t != Ty::Str && !t.is_numeric() && t != Ty::Error {
                        self.diags.error("arithmetic expects numbers (int or float)", span);
                    }
                    if t == Ty::Str && op == BinOp::Add {
                        self.guard_lambda_alloc_leak("string concatenation (`str + str`)", span);
                    }
                    ty = t;
                }
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                l = self.check_expr(lhs, None);
                r = self.check_binop_rhs(l.ty, rhs);
                // A `vecN<T>` comparison (`==`/`<`/…, incl. against a broadcast scalar in either
                // order like `scores > 80` / `80 < scores`) is elementwise and yields a `mask` (M6).
                if let Some((s, n)) = self.vec_binop(&l, &r, span) {
                    ty = Ty::Mask(s, n);
                } else {
                    let t = self.unify(l.ty, r.ty, span);
                    let is_eq = matches!(op, BinOp::Eq | BinOp::Ne);
                    if let Ty::Param(i) = t {
                        // A generic value: equality needs `Eq`, ordering needs `Ord`.
                        let (ok, needed) = if is_eq {
                            (self.param_bound(i).grants_eq(), Bound::Eq)
                        } else {
                            (self.param_bound(i).grants_ord(), Bound::Ord)
                        };
                        if !ok {
                            let what = if is_eq { "equality" } else { "ordering" };
                            self.diags.error(self.bound_needed_msg(i, what, needed), span);
                        }
                    } else if t == Ty::Str && !is_eq {
                        // `str` supports only equality (no ordering yet).
                        self.diags
                            .error("str supports only == and != (ordering is not available)".to_string(), span);
                    } else if t == Ty::String {
                        // Owned `string` comparison is not implemented yet (only the `str` view is
                        // comparable). Comparing it would otherwise fall through to codegen's integer
                        // path and ICE — reject it here with a clear "not yet" message (the same
                        // "deferred, not structural" treatment as `str` ordering above).
                        self.diags.error(
                            "owned `string` values are not directly comparable yet — take a `str` view of each (numbers, bool, char, and `str` are the comparable types)".to_string(),
                            span,
                        );
                    } else if t != Ty::Error {
                        // A concrete non-generic operand. Equality is defined for scalars + `str`
                        // only, ordering for numbers + `char` (+ `str` once available) — there is NO
                        // structural comparison (draft.md §5 "Equality and Ordering"). Reject every
                        // other type — struct / tuple / array / slice / sum / Option / Result / box /
                        // soa / vector-header / handle — here, so a non-comparable operand is a clean
                        // compile error instead of an `IntValue`-variant ICE in `align_codegen_llvm`.
                        // The allow-list is a *positive* set (the exact `Eq` / `Ord` bound predicate,
                        // one source of truth) — never "unknown types pass through" (the fail-open
                        // bug class this repo has a history of).
                        let ok = if is_eq { Bound::Eq.satisfied_by(t) } else { Bound::Ord.satisfied_by(t) };
                        if !ok {
                            let shown = self.ty_display(t);
                            let msg = if is_eq {
                                format!(
                                    "`==` and `!=` compare scalars and strings only (numbers, bool, char, str); {shown} has no equality — compare a struct's fields explicitly, `match` on a sum value, or compare arrays element-wise as a pipeline",
                                )
                            } else {
                                format!("`<`, `<=`, `>`, `>=` order numbers and char only; {shown} has no ordering")
                            };
                            self.diags.error(msg, span);
                        }
                    }
                    ty = Ty::Bool;
                }
            }
            BinOp::And | BinOp::Or => {
                l = self.check_expr(lhs, Some(Ty::Bool));
                r = self.check_expr(rhs, Some(Ty::Bool));
                ty = Ty::Bool;
            }
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                // Bitwise / shift: integer-only, no implicit coercion. The shift amount shares the
                // value's type (unified like arithmetic), so the result is that integer type.
                l = self.check_expr(lhs, expected);
                r = self.check_expr(rhs, Some(l.ty));
                let t = self.unify(l.ty, r.ty, span);
                if matches!(t, Ty::Vec(..)) {
                    // Vectors carry only elementwise `+` `-` `*` `/` `%` (and comparisons → `mask`), not
                    // the bitwise/shift family. Reject explicitly here — not by relying on `is_int_like()`
                    // happening to be false for `Ty::Vec` — so the domain restriction is intentional and
                    // can't silently regress into a codegen `unreachable!` (self-review Gate 3 / #235).
                    self.diags.error("vectors do not support bitwise or shift operators (only `+` `-` `*` `/` `%` and comparisons)".to_string(), span);
                } else if let Ty::Param(_) = t {
                    self.diags.error("bitwise and shift operators need a concrete integer (not a generic type parameter)".to_string(), span);
                } else if !t.is_int_like() && t != Ty::Error {
                    self.diags.error("bitwise and shift operators expect integers".to_string(), span);
                }
                ty = t;
            }
        }
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::Binary { op, lhs: Box::new(l), rhs: Box::new(r) }, ty, span }
    }

    /// `x.wrapping_add(y)` / `x.saturating_sub(y)` / `x.checked_mul(y)` etc. (`core.math`). The
    /// receiver and the single operand must be the same integer type. `wrapping_*` is the default
    /// (the language already wraps), so it lowers to a plain `Binary`; `saturating_*` clamps and
    /// yields the int type; `checked_*` yields `Option<T>` (`None` on overflow).
    fn check_int_arith_method(&mut self, recv: &ast::Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let (op, mode) = parse_int_arith(method).expect("dispatched only for an int-arith method");
        let r = self.check_expr(recv, None);
        if r.ty == Ty::Error {
            return err;
        }
        if !r.ty.is_int_like() {
            self.diags.error(
                format!("'{method}' is an integer overflow operation, but the receiver is {}", ty_name(r.ty)),
                span,
            );
            return err;
        }
        let [arg] = args else {
            self.diags.error(format!("'{method}' takes 1 argument (the other operand), got {}", args.len()), span);
            return err;
        };
        let a = self.check_expr(arg, Some(r.ty));
        if a.ty == Ty::Error {
            return err;
        }
        // Unify the operands (binds an unconstrained literal operand to the other's type) rather
        // than compare — `a.checked_add(5)` must accept the literal `5` as the same int.
        let t = self.unify(r.ty, a.ty, span);
        if t == Ty::Error {
            return err;
        }
        if !t.is_int_like() {
            self.diags.error(format!("'{method}' needs integer operands, got {}", ty_name(t)), span);
            return err;
        }
        let (lhs, rhs) = (Box::new(r), Box::new(a));
        match mode {
            // `wrapping_*` is the default wrapping arithmetic.
            None => Expr { kind: ExprKind::Binary { op, lhs, rhs }, ty: t, span },
            Some(m @ hir::ArithMode::Saturating) => Expr { kind: ExprKind::IntArith { op, mode: m, lhs, rhs }, ty: t, span },
            // `checked_*` yields `Option<T>`. The payload scalar must be concrete now (no inference
            // var inside a composite), so resolve `t` — an unconstrained literal pair defaults to i64.
            Some(m @ hir::ArithMode::Checked) => {
                let scalar = ty_to_scalar(self.finalize(t)).expect("an integer type is a scalar payload");
                Expr { kind: ExprKind::IntArith { op, mode: m, lhs, rhs }, ty: Ty::Option(scalar), span }
            }
        }
    }

    /// `x.abs()` / `a.min(b)` / `a.max(b)` (`core.math`). The receiver must be numeric; `min`/`max`
    /// take one operand of the same type. The result is that numeric type.
    fn check_scalar_math(&mut self, recv: &ast::Expr, fn_: hir::MathFn, args: &[ast::Expr], span: Span) -> Expr {
        use hir::MathFn::*;
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let name = match fn_ {
            Abs => "abs",
            Min => "min",
            Max => "max",
            Sqrt => "sqrt",
            Floor => "floor",
            Ceil => "ceil",
            Round => "round",
            Trunc => "trunc",
            Pow => "pow",
            // `fma` is a free builtin (`check_fma`), never a method — listed only for exhaustiveness.
            Fma => "fma",
        };
        // `(want_args, float_only)`: `abs`/`min`/`max` accept any numeric; the rest are float-only.
        // `min`/`max`/`pow` take one operand; the others take none.
        let (want_args, float_only) = match fn_ {
            Abs => (0, false),
            Min | Max => (1, false),
            Sqrt | Floor | Ceil | Round | Trunc => (0, true),
            Pow => (1, true),
            Fma => (2, true), // free builtin; never reached here
        };
        let r = self.check_expr(recv, None);
        if r.ty == Ty::Error {
            return err;
        }
        // Element-wise vector math (M6): every op below maps to **one lane-wise hardware
        // instruction**, so it vectorizes. A float vector takes the unary float ops + min/max; an
        // integer vector takes abs + min/max (the float-only ops don't apply). `pow` is excluded —
        // it lowers to a libcall, not a lane-wise instruction, so it stays scalar-only.
        let vec_ok = match r.ty {
            Ty::Vec(Scalar::Float(_), _) => matches!(fn_, Abs | Sqrt | Floor | Ceil | Round | Trunc | Min | Max),
            Ty::Vec(Scalar::Int(_), _) => matches!(fn_, Abs | Min | Max),
            _ => false,
        };
        let ok_ty = if matches!(r.ty, Ty::Vec(..)) {
            vec_ok
        } else if float_only {
            r.ty.is_float_like()
        } else {
            r.ty.is_numeric()
        };
        if !ok_ty {
            // A vector receiver that didn't qualify gets a vector-specific reason (it *is* a vector,
            // so "needs a float-vector" would be confusing): `pow` is a libcall (not lane-wise), or a
            // float-only op was applied to an integer vector.
            let msg = match r.ty {
                Ty::Vec(..) if matches!(fn_, Pow) => {
                    format!("'{name}' is not supported on a vector (it lowers to a libcall, not a lane-wise instruction), got {}", ty_name(r.ty))
                }
                // The reachable case is a float-only op (`sqrt`/`floor`/…) on an integer vector
                // (`abs`/`min`/`max` are accepted on int vectors); the fallback is defensive.
                Ty::Vec(..) if float_only => format!("'{name}' on a vector needs a float vector (it is float-only), got {}", ty_name(r.ty)),
                Ty::Vec(..) => format!("'{name}' on a vector needs a float or integer vector, got {}", ty_name(r.ty)),
                _ => {
                    let want = if float_only { "a float (or float-vector)" } else { "a numeric" };
                    format!("'{name}' needs {want} receiver, got {}", ty_name(r.ty))
                }
            };
            self.diags.error(msg, span);
            return err;
        }
        if args.len() != want_args {
            self.diags.error(format!("'{name}' takes {want_args} argument(s), got {}", args.len()), span);
            return err;
        }
        let mut operands = vec![r];
        if let [arg] = args {
            let recv_ty = operands[0].ty;
            let a = self.check_expr(arg, Some(recv_ty));
            if a.ty == Ty::Error {
                return err;
            }
            let t = self.unify(recv_ty, a.ty, span);
            if t == Ty::Error {
                return err;
            }
            operands.push(a);
        }
        let ty = operands[0].ty;
        Expr { kind: ExprKind::MathOp { fn_, operands }, ty, span }
    }

    /// `f := fn x: i32 { … }` — a lambda used as a value. Lifts the lambda (its parameter types
    /// from the explicit annotations, the return type from the body) to a synthetic top-level
    /// function and yields a `Ty::Fn` value. Slice ②a: non-capturing only; scalar signatures.
    fn check_lambda_value(&mut self, params: &[ast::LambdaParam], body: &ast::Block, expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // Parameter types come from the explicit annotations (no use site to infer from).
        let mut param_tys = Vec::with_capacity(params.len());
        for p in params {
            let Some(ann) = &p.ty else {
                self.diags.error(
                    format!("lambda parameter '{}' needs a type annotation to be used as a value (e.g. `fn {}: i32 {{ … }}`)", p.name.name, p.name.name),
                    p.name.span,
                );
                return err;
            };
            param_tys.push(self.resolve_type(ann));
        }
        // Lift with the annotated parameter types as the expected signature; the return type is
        // inferred from the body.
        let Some((name, ret, captures)) = self.lift_lambda(params, body, &param_tys, None, span) else {
            return err;
        };
        // A type error in the annotations or the body has already been reported — don't pile on a
        // confusing secondary "only scalar" message.
        if param_tys.iter().any(|t| self.finalize(*t) == Ty::Error) || self.finalize(ret) == Ty::Error {
            return err;
        }
        // Scalar signature only (slice ②a), matching named function values. The captures are
        // hidden from the closure's *type* — only the explicit parameters appear in `Ty::Fn`.
        let pscalars: Option<Vec<Scalar>> = param_tys.iter().map(|t| ty_to_scalar(self.finalize(*t))).collect();
        let rscalar = ty_to_scalar(self.finalize(ret));
        let (Some(ps), Some(r)) = (pscalars, rscalar) else {
            self.diags.error("a lambda value supports only scalar parameters and return type".to_string(), span);
            return err;
        };
        let fid = intern_fn_type(self.fn_types, ps, r);
        let ty = Ty::Fn(fid);
        self.constrain(ty, expected, span);
        // No captures → a plain function pointer (slice ②a). Captures → a closure carrying its
        // captured values in an environment (slice ②b-2); since a `Ty::Fn` value cannot leave its
        // frame yet (no fn-typed returns/fields/parameters), the environment is frame-local.
        if captures.is_empty() {
            Expr { kind: ExprKind::FnValue(name), ty, span }
        } else {
            Expr { kind: ExprKind::Closure { lifted: name, captures }, ty, span }
        }
    }

    /// `spawn(fn { … })` — defer a task in the enclosing `task_group`. The argument is a
    /// `fn() -> R` value (a no-parameter lambda, captures by value); the result is `Task<R>`.
    fn check_spawn(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if self.task_group_depth == 0 {
            self.diags.error("'spawn' is only valid inside a `task_group { … }` scope".to_string(), span);
            return err;
        }
        let [arg] = args else {
            self.diags.error(format!("'spawn' takes one argument (a `fn {{ … }}`), got {}", args.len()), span);
            return err;
        };
        // `spawn` takes a literal lambda (consumed here, never a free `Ty::Fn` value), so it is
        // lifted directly — which lets the task closure return `Result<R, Error>` (a fallible
        // task), unlike a `Ty::Fn` value whose return is scalar-only.
        let ast::ExprKind::Lambda { params, body } = &arg.kind else {
            self.diags.error("'spawn' takes a `fn { … }` literal".to_string(), arg.span);
            return err;
        };
        if !params.is_empty() {
            self.diags.error("a spawned task takes no parameters (`spawn(fn { … })`)".to_string(), arg.span);
            return err;
        }
        let Some((name, ret, captures)) = self.lift_lambda(params, body, &[], None, arg.span) else {
            return err;
        };
        // Classify the result: `Result<ok, Error>` → a fallible task (`wait()?` surfaces the `Err`),
        // `Task<ok>`; a primitive scalar → an infallible task, `Task<scalar>`. The result is stored
        // in a `box` in the region, so `ok` must be a box-able primitive (owned/view results are a
        // later slice).
        let is_prim = |s: Scalar| matches!(s, Scalar::Int(_) | Scalar::Float(_) | Scalar::Bool | Scalar::Char | Scalar::Unit);
        let (ok, fallible) = match self.finalize(ret) {
            Ty::Result(o, Scalar::Enum(eid)) if eid == self.error_enum_id && is_prim(o) => (o, true),
            // A type error in the lambda body was already reported — don't cascade.
            Ty::Error => return err,
            other => match ty_to_scalar(other) {
                Some(s) if is_prim(s) => (s, false),
                _ => {
                    self.diags.error(
                        format!("a spawned task must return a primitive scalar or `Result<scalar, Error>` for now, got {}", ty_name(other)),
                        arg.span,
                    );
                    return err;
                }
            },
        };
        // The closure value (the `{thunk, env}` machinery is reused as-is; the lifted function may
        // return `Result` — the thunk just forwards it). Its `Ty::Fn` tag uses the `ok` scalar as
        // the return (a repr-only tag — a closure value is a pointer pair regardless).
        let fid = intern_fn_type(self.fn_types, Vec::new(), ok);
        let cty = Ty::Fn(fid);
        let closure = if captures.is_empty() {
            Expr { kind: ExprKind::FnValue(name), ty: cty, span: arg.span }
        } else {
            Expr { kind: ExprKind::Closure { lifted: name, captures }, ty: cty, span: arg.span }
        };
        if fallible
            && let Some(f) = self.task_group_fallible.last_mut() {
                *f = true;
            }
        // A new task is now pending and unjoined, so a prior `wait()` no longer covers everything.
        if let Some(w) = self.wait_state.last_mut() {
            *w = false;
        }
        Expr { kind: ExprKind::Spawn { closure: Box::new(closure), fallible }, ty: Ty::Task(ok), span }
    }

    /// `wait()` — join all spawned tasks. ④a: a no-op marker (eager execution).
    fn check_wait(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        if self.task_group_depth == 0 {
            self.diags.error("'wait' is only valid inside a `task_group { … }` scope".to_string(), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        if !args.is_empty() {
            self.diags.error(format!("'wait' takes no arguments, got {}", args.len()), span);
        }
        let fallible = self.task_group_fallible.last().copied().unwrap_or(false);
        if fallible {
            // A fallible group's `wait()` yields `Result<(), Error>`. `get()` is made safe only by a
            // *successful* `wait()` — i.e. `wait()?` (the `Try` sets the wait-state); a bare `wait()`
            // whose `Err` is ignored leaves failed tasks' slots uninitialised, so it does NOT enable
            // `get()` here.
            Expr { kind: ExprKind::Wait, ty: Ty::Result(Scalar::Unit, Scalar::Enum(self.error_enum_id)), span }
        } else {
            // Infallible group: `wait()` joins and yields `()`; all results are now readable.
            if let Some(w) = self.wait_state.last_mut() {
                *w = true;
            }
            Expr { kind: ExprKind::Wait, ty: Ty::Unit, span }
        }
    }

    /// `f(args)` where `f` is a `Ty::Fn` local — an indirect call through a function value.
    fn check_call_fn_value(&mut self, lid: LocalId, fid: u32, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // Clone only the parameter scalars (needed across the `&mut self` `check_expr` calls);
        // the per-arg LLVM type is computed in the loop, with no intermediate `Vec<Ty>`.
        let params = self.fn_types[fid as usize].params.clone();
        let ret = scalar_to_ty(self.fn_types[fid as usize].ret);
        if args.len() != params.len() {
            self.diags.error(
                format!("this function value expects {} argument(s), got {}", params.len(), args.len()),
                span,
            );
            return err;
        }
        let mut checked = Vec::with_capacity(args.len());
        for (a, p) in args.iter().zip(&params) {
            let pt = scalar_to_ty(*p);
            let e = self.check_expr(a, Some(pt));
            if e.ty != Ty::Error && self.resolve(e.ty) != pt {
                self.diags.error(
                    format!("argument type mismatch: expected {}, got {}", self.ty_display(pt), self.ty_display(e.ty)),
                    e.span,
                );
            }
            checked.push(e);
        }
        let callee = Expr { kind: ExprKind::Local(lid), ty: Ty::Fn(fid), span };
        self.constrain(ret, expected, span);
        Expr { kind: ExprKind::CallFnValue { callee: Box::new(callee), args: checked }, ty: ret, span }
    }

    fn check_call(&mut self, callee: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        // `Type.Variant(args)` / `mod.Type.Variant(args)` — constructing a sum-type value with a
        // payload (the receiver names a sum type, bare in this module or a qualified import).
        if let ast::ExprKind::FieldAccess { recv, field } = &callee.kind {
            match self.resolve_type_receiver(recv) {
                Ok(Some(canonical)) => {
                    if let Some(&enum_id) = self.enum_ids.get(&canonical) {
                        return self.check_variant_ctor(enum_id, field, args, expected, span);
                    }
                    if let Some(tmpl) = self.enum_templates.get(&canonical).cloned() {
                        return self.check_generic_variant_ctor(&canonical, &tmpl, field, args, expected, span);
                    }
                }
                Ok(None) => {}
                Err(()) => return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span },
            }
        }
        // Method call `recv.method(...)`: a module builtin (`heap.new`) or a method on a
        // value (`box.get()`, `box.clone()`).
        if let ast::ExprKind::FieldAccess { recv, field } = &callee.kind {
            return self.check_method_call(recv, &field.name, args, expected, span);
        }
        let name = match &callee.kind {
            ast::ExprKind::Path(p) => single_name(p).map(|s| s.to_string()),
            _ => None,
        };
        let Some(name) = name else {
            self.diags.error("only direct function calls are supported", span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        };
        if name == "print" {
            return self.check_print(args, span);
        }
        if name == "hash64" || name == "hash128" {
            return self.check_hash(&name, args, span);
        }
        if name == "Some" {
            return self.check_some(args, expected, span);
        }
        if name == "Ok" || name == "Err" {
            return self.check_result_ctor(&name, args, expected, span);
        }
        if name == "error" {
            return self.check_error_ctor(args, span);
        }
        if name == "builder" {
            return self.check_builder_new(args, span);
        }
        if name == "buffer" {
            return self.check_buffer_new(args, span);
        }
        if name == "select" {
            return self.check_select(args, span);
        }
        if name == "dot" {
            return self.check_vec_dot(args, span);
        }
        if name == "fma" {
            return self.check_fma(args, span);
        }
        if name == "spawn" {
            return self.check_spawn(args, span);
        }
        if name == "wait" {
            return self.check_wait(args, span);
        }
        // An indirect call through a function-value local: `f(args)` where `f: Ty::Fn`.
        if let Some(lid) = self.lookup(&name)
            && let Ty::Fn(fid) = self.resolve(self.locals[lid as usize].ty) {
                return self.check_call_fn_value(lid, fid, args, expected, span);
            }
        // A user function: a bare call resolves in the caller's own module (`module$fn` mangled
        // name); cross-module calls are written `mod.fn(...)` (handled in `check_method_call`).
        let Some(name) = self.resolve_local_fn(&name) else {
            self.diags.error(format!("undefined function: '{name}'"), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        };
        self.check_named_call(name, args, expected, span)
    }

    /// The shared tail of a direct call once its target is a resolved (mangled) function name:
    /// signature lookup, generic-call dispatch, the `out` no-alias check, argument checking, and the
    /// `Call` node. Reused by a bare call (`check_call`) and a cross-module `mod.fn(...)` call.
    fn check_named_call(&mut self, name: String, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let Some(sig) = self.sigs.get(&name) else {
            self.diags.error(format!("undefined function: '{name}'"), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        };
        // A foreign (`extern "C"`) call can violate every safe-core invariant, so it is confined to
        // an `unsafe {}` block — exactly like a `raw.*` op (draft.md §15).
        if sig.is_extern && self.unsafe_depth == 0 {
            self.diags.error(
                format!("calling the extern function '{name}' requires an `unsafe {{ }}` block (foreign code is outside the safe core)"),
                span,
            );
        }
        let (param_tys, ret, out) = (sig.params.clone(), sig.ret, sig.out.clone());
        // A generic function: infer the concrete type arguments from the call, then take its own
        // dedicated path (the result type and `type_args` come from the substitution).
        if !sig.type_params.is_empty() {
            let type_params = sig.type_params.clone();
            return self.check_generic_call(&name, &type_params, &param_tys, ret, args, expected, span);
        }
        if args.len() != param_tys.len() {
            self.diags.error(
                format!("'{name}' expects {} argument(s), got {}", param_tys.len(), args.len()),
                span,
            );
        }
        // No-alias check: an `out` argument must be **provably disjoint** from every other argument
        // (the no-alias guarantee `out` lowers to — and the precondition a callee's `map_into`
        // scoped `noalias` metadata trusts). Disjointness is proven by root buffer: each root must
        // resolve to a **known backing buffer** (a slice/array parameter — distinct from the other
        // arguments by *this* caller's own `out` contract — or a real array local), and the roots
        // must differ. An argument whose buffer cannot be proven distinct — an **unresolvable** root
        // (a fn-call / `if` / block result) or a **slice of unknown origin** (bound to a fn-returned
        // slice, a soa column, a struct field, all `slice_root_is_known == false`) — is **rejected,
        // not silently skipped**: skipping it (the earlier behavior) let an aliasing pair through
        // (`scale(ident(ys[0..4]), ys[1..5])`) and turned the callee's `noalias` into a miscompile.
        // Sub-slices of the same array are rejected whether or not their ranges actually overlap
        // (range analysis is a separate follow-up). A fresh array-literal argument is stack storage
        // disjoint from any other buffer, so it is allowed; only slice-typed parameters can share
        // storage, so scalar arguments are never compared.
        for (i, is_out) in out.iter().enumerate() {
            if !is_out {
                continue;
            }
            let Some(arg_i) = args.get(i) else { continue };
            // The `out` argument's own root must be a known backing buffer, or nothing can be proven
            // disjoint from it (an unknown-origin `out` view could itself alias an input).
            let out_known = self.arg_root_local(arg_i).filter(|r| self.slice_root_is_known(*r));
            for (j, a) in args.iter().enumerate() {
                if j == i {
                    continue;
                }
                // Only a slice-typed parameter can share storage with the `out` buffer.
                if !matches!(param_tys.get(j).map(|t| self.resolve(*t)), Some(Ty::Slice(_))) {
                    continue;
                }
                // A fresh array literal is disjoint stack storage.
                if matches!(a.kind, ast::ExprKind::ArrayLit(_)) {
                    continue;
                }
                match (out_known, self.arg_root_local(a)) {
                    (Some(o), Some(p)) if o == p => {
                        let lname = self.locals[o as usize].name.clone();
                        self.diags.error(
                            format!("the `out` argument also aliases '{lname}', another argument to '{name}' — an `out` buffer must not alias the other arguments"),
                            arg_i.span,
                        );
                        break;
                    }
                    // Both roots are known, distinct backing buffers → provably disjoint.
                    (Some(_), Some(p)) if self.slice_root_is_known(p) => {}
                    _ => {
                        self.diags.error(
                            format!("cannot prove this argument is disjoint from the `out` buffer of '{name}' — pass a named slice/array (or a subslice of one) whose buffer is known, not a slice of unknown origin (a fn result, `if`/block value, soa column, or struct field)"),
                            a.span,
                        );
                        break;
                    }
                }
            }
        }
        let checked = args
            .iter()
            .enumerate()
            .map(|(i, a)| self.check_arg(a, param_tys.get(i).copied()))
            .collect();
        Expr { kind: ExprKind::Call { func: name, args: checked, type_args: Vec::new() }, ty: ret, span }
    }

    /// Check a call to a **generic** function `fn f<T, …>(…)`. Type arguments are inferred — never
    /// written at the call site (no turbofish; settled). Each declared parameter typed `Ty::Param(p)`
    /// binds `p` from the corresponding argument's type (all occurrences of `p` are unified
    /// together); a `Param` appearing only in the return type is taken from the expected type. Every
    /// parameter must be inferable. The skeleton restricts a type parameter to a **bare** position
    /// (a whole parameter / return), so `T` is passed/returned by value with no operations — the
    /// constraint model (`Num`/`Ord`/`Eq`) is a later slice. `type_args` is recorded for
    /// monomorphization, which generates the concrete instance and rewrites the call target.
    #[allow(clippy::too_many_arguments)]
    fn check_generic_call(&mut self, name: &str, type_params: &[String], param_tys: &[Ty], ret: Ty, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != param_tys.len() {
            self.diags.error(
                format!("'{name}' expects {} argument(s), got {}", param_tys.len(), args.len()),
                span,
            );
            return err;
        }
        // One inference slot per type parameter. Each argument's type is matched structurally
        // against its declared parameter type, binding any `Param` it carries (bare `T`, or nested
        // in `Option<T>` / `Result<T, E>` / `slice<T>` / `box<T>` / a fixed `array<T>`).
        let mut subst: Vec<Option<Ty>> = vec![None; type_params.len()];
        let mut checked = Vec::with_capacity(args.len());
        for (i, a) in args.iter().enumerate() {
            let declared = param_tys[i];
            // A position mentioning a type parameter applies no coercion (the type is unknown), so
            // check the argument unconstrained; a fully concrete parameter checks against it.
            let ce = if ty_mentions_param(declared) {
                self.check_expr(a, None)
            } else {
                self.check_arg(a, Some(declared))
            };
            self.match_param(declared, ce.ty, &mut subst, a.span, false);
            checked.push(ce);
        }
        // Seed the remaining parameters from the expected type (the binding annotation), structurally
        // — `o: Option<i32> := wrap(x)` gives `T = i32` from the return position. Bind-only: do not
        // unify concrete leaves against `expected` (that constraint comes later via `result_ty`).
        if let Some(exp) = expected {
            self.match_param(ret, exp, &mut subst, span, true);
        }
        // A parameter that appears *nested* (inside `Option<T>` / `Result<…>` / …) must resolve to a
        // concrete scalar now — a `Scalar` cannot hold an inference variable, so leaving it deferred
        // would leak a `Param` into the result type and downstream checking. Finalize those eagerly
        // (an unconstrained literal defaults). A purely-*bare* parameter stays deferred so a literal
        // can still infer its type from the call's context (the 4c-1 return-context behavior).
        let mut nested = vec![false; type_params.len()];
        for t in param_tys.iter().chain(std::iter::once(&ret)) {
            mark_nested_params(*t, &mut nested);
        }
        for (p, &is_nested) in nested.iter().enumerate() {
            if !is_nested {
                continue;
            }
            if let Some(s) = subst[p].as_mut() {
                *s = self.finalize(*s);
            }
        }
        // A type parameter mentioned by no argument and not supplied by the expected type is
        // immediately uninferable; the rest may still be inference variables (e.g. an integer
        // literal argument) that whole-function inference resolves later.
        let mut type_args = Vec::with_capacity(type_params.len());
        for (i, s) in subst.iter().enumerate() {
            match s {
                Some(t) => type_args.push(*t),
                None => {
                    self.diags.error(
                        format!("cannot infer type parameter '{}' of '{name}'; annotate the call's context", type_params[i]),
                        span,
                    );
                    return err;
                }
            }
        }
        let result_ty = subst_param_ty(ret, &type_args);
        self.constrain(result_ty, expected, span);
        // The type arguments are kept (still possibly inference variables) and finalized in
        // `finalize_expr`, which then records the instantiation and rewrites `func` to the
        // monomorph's mangled name — by then a literal argument's type has flowed from context.
        Expr { kind: ExprKind::Call { func: name.to_string(), args: checked, type_args }, ty: result_ty, span }
    }

    /// Match an `actual` type against a `declared` parameter/return type, binding each `Ty::Param`
    /// it carries into `subst` (unifying repeated occurrences). `bind_only` skips unifying concrete
    /// leaves (used when seeding from the expected type). Handles `Param` bare or nested one level
    /// in a scalar-payload composite (`Option`/`Result`/`slice`/`box`/`array`/`Task`).
    fn match_param(&mut self, declared: Ty, actual: Ty, subst: &mut [Option<Ty>], span: Span, bind_only: bool) {
        let a = self.resolve(actual);
        match (declared, a) {
            (Ty::Param(p), _) => self.bind_param(p, a, subst, span),
            (Ty::Option(ds), Ty::Option(asc)) => self.match_scalar_param(ds, asc, subst, span, bind_only),
            (Ty::Result(dok, derr), Ty::Result(aok, aerr)) => {
                self.match_scalar_param(dok, aok, subst, span, bind_only);
                self.match_scalar_param(derr, aerr, subst, span, bind_only);
            }
            (Ty::Slice(ds), Ty::Slice(asc))
            | (Ty::Box(ds), Ty::Box(asc))
            | (Ty::Array(ds, _), Ty::Array(asc, _))
            | (Ty::Task(ds), Ty::Task(asc)) => self.match_scalar_param(ds, asc, subst, span, bind_only),
            _ => {
                if !bind_only {
                    self.unify(a, declared, span);
                }
            }
        }
    }

    /// The scalar-level companion of [`match_param`]: bind a `Scalar::Param` from the actual scalar,
    /// or (when not seeding) unify a concrete declared scalar against the actual so a mismatch in a
    /// concrete nested position (`Result<T, i32>` vs `Result<_, bool>`) is still a type error.
    fn match_scalar_param(&mut self, declared: Scalar, actual: Scalar, subst: &mut [Option<Ty>], span: Span, bind_only: bool) {
        if let Scalar::Param(p) = declared {
            self.bind_param(p, scalar_to_ty(actual), subst, span);
        } else if !bind_only {
            self.unify(scalar_to_ty(declared), scalar_to_ty(actual), span);
        }
    }

    fn bind_param(&mut self, p: u32, actual: Ty, subst: &mut [Option<Ty>], span: Span) {
        let slot = &mut subst[p as usize];
        *slot = Some(match *slot {
            Some(prev) => self.unify(prev, actual, span),
            None => actual,
        });
    }

    /// Check a call argument against a parameter type, applying an array → slice borrow
    /// when the parameter is a `slice<T>` and the argument is a matching array.
    fn check_arg(&mut self, a: &ast::Expr, param: Option<Ty>) -> Expr {
        if let Some(Ty::Slice(ps)) = param {
            return self.check_slice_init(a, ps);
        }
        if let Some(Ty::Str) = param {
            return self.check_str_init(a);
        }
        self.check_expr(a, param)
    }

    /// Check an expression expected to be a `str`, applying the `string` → `str` borrow
    /// (`StrBorrow`) when the source is an owned `string` (MMv2 slice 7b/7e): zero-cost (same
    /// `{ptr,len}` layout), non-consuming (the `string` stays owned by its slot). Shared by call
    /// arguments, `str`-annotated `let` bindings, and `str`-place assignments. Pass `None` first so
    /// the source types as `string`, then wrap the borrow.
    fn check_str_init(&mut self, a: &ast::Expr) -> Expr {
        let e = self.check_expr(a, None);
        if e.ty == Ty::String {
            let span = e.span;
            return Expr { kind: ExprKind::StrBorrow(Box::new(e)), ty: Ty::Str, span };
        }
        if e.ty != Ty::Str {
            self.constrain(e.ty, Some(Ty::Str), e.span);
        }
        e
    }

    /// Check an expression expected to be a `slice<T>`, applying the array → slice borrow
    /// (`ArrayToSlice`) when the source is a matching array. Shared by call arguments and
    /// slice-annotated `let` bindings so both produce a real slice value (not a bare array).
    fn check_slice_init(&mut self, a: &ast::Expr, ps: Scalar) -> Expr {
        // An inline array literal takes the slice's element type.
        let e = match &a.kind {
            ast::ExprKind::ArrayLit(elems) => self.check_array_lit(elems, Some(scalar_to_ty(ps)), a.span),
            _ => self.check_expr(a, None),
        };
        if let Ty::Array(es, _) = e.ty
            && es == ps {
                // The borrow lowers via the same slot-materialization as a pipeline source,
                // so the same restriction applies: only a literal or a named local.
                if !matches!(e.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
                    self.diags.error(
                        "an array coerced to a slice must be an array literal or a variable (an arbitrary array expression is not supported yet)".to_string(),
                        e.span,
                    );
                    return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span: e.span };
                }
                let span = e.span;
                return Expr { kind: ExprKind::ArrayToSlice(Box::new(e)), ty: Ty::Slice(ps), span };
            }
        // Already a slice, or a mismatch: let unification report any error.
        if e.ty != Ty::Slice(ps) {
            self.constrain(e.ty, Some(Ty::Slice(ps)), e.span);
        }
        e
    }

    /// Builtin `print`. M1: exactly one integer argument; prints decimal + newline,
    /// returns `()`. `bool`/string and a no-newline form arrive with `std.io` (M5).
    fn check_print(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        if args.len() != 1 {
            self.diags
                .error(format!("'print' expects 1 argument, got {}", args.len()), span);
        }
        let checked = args
            .iter()
            .map(|a| {
                let e = self.check_expr(a, None);
                if !is_printable(e.ty) {
                    self.diags
                        .error("'print' expects an int, float, str, bool, or char".to_string(), e.span);
                }
                e
            })
            .collect();
        Expr {
            kind: ExprKind::Call { func: "print".to_string(), args: checked, type_args: Vec::new() },
            ty: Ty::Unit,
            span,
        }
    }

    /// `hash64(data)` / `hash128(data)` — the `core.hash` non-crypto hash over a byte view
    /// (`str` / owned `string` / `slice<u8>`). `hash64` yields `u64`; `hash128` yields `(u64, u64)`.
    /// An owned `string` is borrowed (not consumed), like `print` / `io.stdout.write`.
    fn check_hash(&mut self, name: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 1 {
            self.diags
                .error(format!("'{name}' expects 1 argument, got {}", args.len()), span);
            return err;
        }
        let mut arg = self.check_expr(&args[0], None);
        let u8s = Scalar::Int(IntTy { bits: 8, signed: false });
        let ok = match self.resolve(arg.ty) {
            Ty::Str => true,
            Ty::String => {
                let s = arg.span;
                arg = Expr { kind: ExprKind::StrBorrow(Box::new(arg)), ty: Ty::Str, span: s };
                true
            }
            Ty::Slice(el) => el == u8s,
            _ => false,
        };
        if !ok {
            if arg.ty != Ty::Error {
                self.diags
                    .error(format!("'{name}' expects a str, string, or slice<u8>, got {}", ty_name(arg.ty)), arg.span);
            }
            return err;
        }
        let u64s = Scalar::Int(IntTy { bits: 64, signed: false });
        let ty = if name == "hash128" {
            Ty::Tuple(intern_tuple(self.tuples, vec![u64s, u64s]))
        } else {
            Ty::Int(IntTy { bits: 64, signed: false })
        };
        Expr {
            kind: ExprKind::Call { func: name.to_string(), args: vec![arg], type_args: Vec::new() },
            ty,
            span,
        }
    }

    /// Builtin `Some(x)`. The payload resolves to a concrete scalar here (an
    /// unconstrained literal defaults), so the resulting `Option<T>` carries no
    /// inference variable.
    fn check_some(&mut self, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        if args.len() != 1 {
            self.diags
                .error(format!("'Some' takes 1 argument, got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let inner_expected = match expected {
            Some(Ty::Option(s)) => Some(scalar_to_ty(s)),
            _ => None,
        };
        let arg = self.check_expr(&args[0], inner_expected);
        let scalar = self.payload_scalar(arg.ty, args[0].span);
        let ty = Ty::Option(scalar);
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::OptionSome(Box::new(arg)), ty, span }
    }

    /// Resolve a type to a concrete payload [`Scalar`], defaulting inference vars and
    /// reporting non-scalar payloads (M2 restriction).
    fn payload_scalar(&mut self, ty: Ty, span: Span) -> Scalar {
        let f = self.finalize(ty);
        match ty_to_scalar(f) {
            Some(s) => s,
            None => {
                if f != Ty::Error {
                    self.diags
                        .error(format!("Option payload must be a scalar (composite payloads are not supported yet), got {}", ty_name(f)), span);
                }
                Scalar::Int(IntTy { bits: 64, signed: true })
            }
        }
    }

    /// Enforce the "capability header" rule: a prefix-accessed builtin namespace (`json`/`fs`/`io`)
    /// must be `import`ed before use (`open-questions.md` module system). `module` is the required
    /// module path, `used` the surface form for the diagnostic. Checked once per *source* function
    /// — skipped for monomorph instances (`mono_args` non-empty), which would re-report the same use.
    fn require_import(&mut self, module: &str, used: &str, span: Span) {
        if self.mono_args.is_empty() && !self.imports.contains(module) {
            self.diags.error(
                format!("`{used}` requires `import {module}` — the capability is not imported"),
                span,
            );
        }
    }

    /// Resolve a bare function name to its mangled codegen name within the *current* module
    /// (`module$fn`, or the plain name in the entry module). `None` if no such function — a bare
    /// name never reaches into another module (cross-module calls are written `mod.fn(...)`).
    fn resolve_local_fn(&self, name: &str) -> Option<String> {
        self.mods.get(&self.cur_module)?.fns.get(name).map(|(m, _)| m.clone())
    }

    /// Resolve a cross-module call `mod.fn` to its mangled target, checking that `mod` is imported
    /// here and that `fn` exists and is `pub`. `Ok(None)` means `mod` is not an imported user module
    /// (so the `mod.fn` shape is something else — a builtin namespace or a method); `Err` is a
    /// reported resolution error.
    fn resolve_qualified_fn(&mut self, module: &str, name: &str, span: Span) -> Result<Option<String>, ()> {
        let is_self = module == self.cur_module;
        if !is_self {
            let here = self.mods.get(&self.cur_module);
            if !here.is_some_and(|mi| mi.user_imports.contains(module)) {
                return Ok(None); // not a `mod.fn` user-module call — let the caller try other shapes
            }
        }
        match self.mods.get(module).and_then(|mi| mi.fns.get(name)) {
            Some((mangled, pub_exported)) => {
                if *pub_exported || is_self {
                    Ok(Some(mangled.clone()))
                } else {
                    self.diags.error(format!("'{name}' is private to module '{module}' (mark it `pub` to export it)"), span);
                    Err(())
                }
            }
            None => {
                self.diags.error(format!("module '{module}' has no function '{name}'"), span);
                Err(())
            }
        }
    }

    fn resolve_group_field(
        &mut self,
        sname: &str,
        fields: &[FieldDef],
        src_kind: &str,
        fld: &ast::Ident,
        role: &str,
        want: Ty,
    ) -> Option<u32> {
        let Some(idx) = fields.iter().position(|f| f.name == fld.name) else {
            self.diags.error(format!("no field '{}' on {}<{}>", fld.name, src_kind, sname), fld.span);
            return None;
        };
        if fields[idx].ty != want {
            self.diags.error(
                format!("`group_by` {role} '{}' must be {} (first cut), got {}", fld.name, ty_name(want), ty_name(fields[idx].ty)),
                fld.span,
            );
            return None;
        }
        Some(idx as u32)
    }

    /// Whether `name` is an in-scope value — a local/parameter, or a captured/enclosing binding of a
    /// closure. A builtin **module** dispatch (`heap.*`, `raw.*`, `json.*`, `fs.*`, `path.*`, `env.*`,
    /// `time.*`, `io.*`) must fire only when its module name is *not* shadowed by such a value:
    /// `path`/`env`/`time`/`fs` are ordinary parameter names, so `fn f(path: str) { path.base("x") }`
    /// must route to normal value-method resolution (→ "no method `base` on `str`"), never to the
    /// builtin (which would silently ignore the `path` receiver). Mirrors the `leftmost_is_local`
    /// guard used below for cross-module calls — one rule, applied at every builtin dispatch.
    fn name_in_scope(&self, name: &str) -> bool {
        self.lookup(name).is_some()
            || self.capture.as_ref().is_some_and(|c| {
                c.captured.iter().any(|(n, _, _)| n == name) || c.enclosing.iter().any(|(n, _, _)| n == name)
            })
    }

    /// A method call `recv.method(args)`: the `heap.new` builtin, or a method on a value
    /// (`box.get()`, `box.clone()`).
    fn check_method_call(&mut self, recv: &ast::Expr, method: &str, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // Builtin **module** dispatches (`heap.*`, `raw.*`, `json.*`, `fs.*`, `path.*`, `env.*`,
        // `time.*`, `io.*`). Each fires only when the module name is a bare path *and* is not
        // shadowed by an in-scope value (`name_in_scope`) — otherwise `fn f(path: str) { path.base(x) }`
        // would silently swallow the `path` receiver into the builtin. A shadowing binding falls
        // through to normal value-method resolution (and never triggers a spurious `require_import`).
        if let ast::ExprKind::Path(p) = &recv.kind
            && let Some(module) = single_name(p)
            && !self.name_in_scope(module)
        {
            // `heap.new(...)` — `heap` is a module name, not a value.
            if module == "heap" && method == "new" {
                return self.check_heap_new(args, expected, span);
            }
            // `raw.alloc(size)` / `raw.free(p)` / `raw.load(p, off)` / `raw.store(p, off, v)` /
            // `raw.offset(p, n)` — the unsafe raw-pointer ops (`raw` is a module name, not a value).
            // `unsafe {}`-only.
            if module == "raw" && matches!(method, "alloc" | "free" | "load" | "store" | "offset") {
                return self.check_raw_op(method, args, expected, span);
            }
            if module == "json" && method == "encode" {
                self.require_import("core.json", "json.encode", span);
                return self.check_json_encode(args, span);
            }
            if module == "json" && method == "decode" {
                self.require_import("core.json", "json.decode", span);
                return self.check_json_decode(args, expected, span);
            }
            if module == "fs" && method == "read_file" {
                self.require_import("std.fs", "fs.read_file", span);
                return self.check_fs_read_file(args, span);
            }
            // `fs.open(path)` -> Result<reader, Error>; `fs.create(path)` -> Result<writer, Error>.
            if module == "fs" && (method == "open" || method == "create") {
                self.require_import("std.fs", &format!("fs.{method}"), span);
                return self.check_fs_open_create(method == "create", args, span);
            }
            // `fs.write_file(path, data)` -> Result<(), Error> (data: str | bytes | builder).
            if module == "fs" && method == "write_file" {
                self.require_import("std.fs", "fs.write_file", span);
                return self.check_fs_write_file(args, span);
            }
            // `fs.exists(path)` -> bool; `fs.remove(path)` / `fs.read_dir(path)` / `fs.read_file_view(path)`
            // — the single-path std.fs ops.
            if module == "fs" && matches!(method, "exists" | "remove" | "read_dir" | "read_file_view") {
                self.require_import("std.fs", &format!("fs.{method}"), span);
                return self.check_fs_path_op(method, args, span);
            }
            // `std.net` — `dns.resolve(host)` -> Result<array<string>, Error> (owned IP strings).
            if module == "dns" && method == "resolve" {
                self.require_import("std.net", "dns.resolve", span);
                return self.check_dns_resolve(args, span);
            }
            // `std.net` — `tcp.connect(host, port)` -> Result<tcp_conn, Error> (a connected socket).
            if module == "tcp" && method == "connect" {
                self.require_import("std.net", "tcp.connect", span);
                return self.check_tcp_connect(args, span);
            }
            // `std.net` — `tcp.listen(host, port)` -> Result<tcp_listener, Error> (a listening socket).
            if module == "tcp" && method == "listen" {
                self.require_import("std.net", "tcp.listen", span);
                return self.check_tcp_listen(args, span);
            }
            // `std.net` — `udp.bind(host, port)` -> Result<udp_socket, Error> (a bound UDP socket).
            if module == "udp" && method == "bind" {
                self.require_import("std.net", "udp.bind", span);
                return self.check_udp_bind(args, span);
            }
            // `std.path` — `path.join`/`base`/`dir`/`ext`/`normalize` (pure lexical string ops).
            if module == "path" && matches!(method, "join" | "base" | "dir" | "ext" | "normalize") {
                self.require_import("std.path", &format!("path.{method}"), span);
                return self.check_path_op(method, args, span);
            }
            // `std.env` — `env.get(name)` -> Option<string>; `env.set(name, value)` -> Result<(), Error>.
            if module == "env" && matches!(method, "get" | "set") {
                self.require_import("std.env", &format!("env.{method}"), span);
                return self.check_env_op(method, args, span);
            }
            // `std.time` — `time.now()`/`time.instant()` -> i64 ns; `time.sleep(ns)`.
            if module == "time" && matches!(method, "now" | "instant" | "sleep") {
                self.require_import("std.time", &format!("time.{method}"), span);
                return self.check_time_op(method, args, span);
            }
            // `std.process` — `process.exit(code)` (cleanup-then-exit) / `process.abort()` (immediate
            // `_exit`, no cleanup). Both diverge; typed `()` (no `Never` type yet).
            if module == "process" && matches!(method, "exit" | "abort") {
                self.require_import("std.process", &format!("process.{method}"), span);
                return self.check_process_op(method, args, span);
            }
            // `std.process` — `process.spawn(cmd, args)` -> Result<child, Error> (fork+execvp).
            if module == "process" && method == "spawn" {
                self.require_import("std.process", "process.spawn", span);
                return self.check_process_spawn(args, span);
            }
            // `std.process` — `process.exec(cmd, args)` -> Result<(), Error> (execvp in-place; returns
            // only on failure — success replaces the image).
            if module == "process" && method == "exec" {
                self.require_import("std.process", "process.exec", span);
                return self.check_process_exec(args, span);
            }
            // `io.copy(r, w)` -> Result<i64, Error> (bytes transferred).
            if module == "io" && method == "copy" {
                self.require_import("std.io", "io.copy", span);
                return self.check_io_copy(args, span);
            }
            // `std.encoding` — Base64 (standard + URL-safe), hex, and UTF-8 validation. Pure byte
            // transforms: encode -> owned `string`, decode -> `Result<buffer, Error>`.
            if module == "encoding"
                && matches!(
                    method,
                    "base64_encode" | "base64_decode" | "base64url_encode" | "base64url_decode" | "hex_encode" | "hex_decode" | "utf8_valid"
                )
            {
                self.require_import("std.encoding", &format!("encoding.{method}"), span);
                return self.check_encoding_op(method, args, span);
            }
            // `std.compress` — gzip via libz (M11 Slice 1) / zstd via libzstd (Slice 2). Impure
            // byte→byte codecs (owned `buffer` output) wrapping the tuned C engines (draft §15
            // keystone strategy). The codec is the method prefix; the direction is the suffix.
            if module == "compress"
                && matches!(method, "gzip_compress" | "gzip_decompress" | "zstd_compress" | "zstd_decompress")
            {
                self.require_import("std.compress", &format!("compress.{method}"), span);
                return self.check_compress_op(method, args, span);
            }
            // `std.rand` — a Copy `rng`: `rand.seed()` (OS-seeded) / `rand.seed_with(s)`
            // (deterministic). The `r.next()`/`range`/`shuffle`/`sample` methods dispatch on the
            // receiver type below (value methods on an `rng`), not here.
            if module == "rand" && matches!(method, "seed" | "seed_with") {
                self.require_import("std.rand", &format!("rand.{method}"), span);
                return self.check_rand_seed(method, args, span);
            }
            // `std.cli` — `cli.command(name)` builds a Move `cli command`. The `flag_*`/`parse`/
            // `usage`/`get_*` methods dispatch on the receiver type below (methods on a bound handle).
            if module == "cli" && method == "command" {
                self.require_import("std.cli", "cli.command", span);
                return self.check_cli_command(args, span);
            }
            // `std.http` (Slice 1) — `http.request(method, url)` builds a Move `http request`; the
            // `header`/`body` methods dispatch on the receiver type below. `http.parse(bytes)` parses a
            // response buffer -> `Result<response, Error>` (the response's `status`/`header`/`body`
            // methods also dispatch below). All Pure (no sockets in this slice).
            if module == "http" && method == "request" {
                self.require_import("std.http", "http.request", span);
                return self.check_http_request(args, span);
            }
            if module == "http" && method == "parse" {
                self.require_import("std.http", "http.parse", span);
                return self.check_http_parse(args, span);
            }
            // `std.http` (Slice 2) — `http.client()` builds a Move `http client`; the `get`/`post`/
            // `request` methods dispatch on the receiver type below. Impure (network).
            if module == "http" && method == "client" {
                self.require_import("std.http", "http.client", span);
                return self.check_http_client(args, span);
            }
            // `std.crypto` — `constant_time_equal(a, b)` (the self-hosted branchless CT byte-compare,
            // Pure) / `random(out)` (fill a `buffer` from the OS CSPRNG, Impure) from Slice 1;
            // `sha256(data)` / `sha512(data)` (EVP digests via libcrypto, Impure) from Slice 2.
            // `hmac_sha256(key, data)` (owned `array<u8>` tag) / `hkdf_sha256(salt, ikm, info, len)`
            // (`Result<buffer, Error>`) are Slice 3, both Impure libcrypto calls. The four AEAD
            // surfaces `{aes_gcm,chacha20_poly1305}_{seal,open}(key, nonce, data, aad)` (Slice 4,
            // `Result<buffer, Error>`) are Impure libcrypto calls too. `argon2id(password, salt,
            // params)` (Slice 5, `Result<buffer, Error>`, `params` = the builtin `argon2_params`
            // struct) closes the module — an Impure `EVP_KDF("ARGON2ID")` call.
            if module == "crypto"
                && matches!(
                    method,
                    "constant_time_equal"
                        | "random"
                        | "sha256"
                        | "sha512"
                        | "hmac_sha256"
                        | "hkdf_sha256"
                        | "aes_gcm_seal"
                        | "aes_gcm_open"
                        | "chacha20_poly1305_seal"
                        | "chacha20_poly1305_open"
                        | "argon2id"
                )
            {
                self.require_import("std.crypto", &format!("crypto.{method}"), span);
                return self.check_crypto_op(method, args, span);
            }
        }
        // `io.stdout.buffered()` / `io.stderr.buffered()` — a buffered `writer` over a standard
        // stream. The receiver is the 2-segment `io.stdout` / `io.stderr`, so it parses as a
        // `FieldAccess` (`io` . `stdout`). `buffered` is intercepted here (before the receiver is
        // evaluated as an unbuffered writer value) so `io.stdout.buffered()` builds one buffered
        // writer, not an unbuffered one wrapped in a buffered one.
        if method == "buffered"
            && let ast::ExprKind::FieldAccess { recv: inner, field } = &recv.kind
                && let ast::ExprKind::Path(p) = &inner.kind
                && single_name(p) == Some("io")
                && !self.name_in_scope("io") {
                    let fd = match field.name.as_str() {
                        "stdout" => 1,
                        "stderr" => 2,
                        _ => 0,
                    };
                    if fd != 0 {
                        self.require_import("std.io", &format!("io.{}.buffered", field.name), span);
                        return self.check_io_buffered(&field.name, fd, args, span);
                    }
                }
        // `mod.fn(...)` / `a.b.fn(...)` — a cross-module call into an imported user module. The
        // receiver is a pure dotted name (`geom` or `util.math`); resolved + visibility-checked here.
        // `Ok(None)` (not an imported user module) falls through to the value-method dispatch below.
        // A local/captured variable shadows a module of the same name: if the leftmost segment names
        // an in-scope value, this is value-method dispatch (`box.get()`), not a cross-module call.
        let leftmost_is_local = leftmost_segment(recv).is_some_and(|leftmost| {
            self.lookup(leftmost).is_some()
                || self.capture.as_ref().is_some_and(|cap| {
                    cap.captured.iter().any(|(n, _, _)| n == leftmost)
                        || cap.enclosing.iter().any(|(n, _, _)| n == leftmost)
                })
        });
        if !leftmost_is_local
            && let Some(modpath) = flatten_module_path(recv) {
                match self.resolve_qualified_fn(&modpath, method, span) {
                    Ok(Some(mangled)) => return self.check_named_call(mangled, args, expected, span),
                    Ok(None) => {}
                    Err(()) => return err,
                }
            }
        // Explicit-overflow integer arithmetic (`core.math`): `x.{wrapping,saturating,checked}_{add,sub,mul}(y)`.
        if parse_int_arith(method).is_some() {
            return self.check_int_arith_method(recv, method, args, span);
        }
        // `s.dict_encode(.key)` — the A2 reuse transform: intern the str key column once into a
        // `dict_encoded<Struct>` value reused by later `group_by`s.
        if method == "dict_encode" {
            return self.check_dict_encode(recv, args, span);
        }
        // `group_by(.key)` is only meaningful immediately before an aggregate; on its own it is an
        // error (there is no first-class "groups" value).
        if method == "group_by" {
            self.diags.error(
                "`group_by(.key)` must be followed by an aggregate, e.g. `.sum(.value)`".to_string(),
                span,
            );
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        // `X.group_by(.key).<agg>(…)` — a grouped aggregate (`sum`/`min`/`max(.value)` or `count()`).
        // Any method whose receiver is a `group_by(.key)` routes here, so a normal `xs.sum()` (recv
        // is not a `group_by`) falls through to the pipeline terminals below.
        if let Some((src, key_field)) = as_group_by(recv) {
            return self.check_group_agg(src, key_field, method, args, span);
        }
        // `sum` / `reduce` are the terminals of a fused pipeline.
        if method == "sum" {
            // A `vecN<T>` receiver makes this the SIMD horizontal sum (the same surface as the array
            // reduction `arr.sum()`); otherwise the array path runs. (Mirrors `min`/`max`.)
            if args.is_empty()
                && let Some((rv, s)) = self.try_vec_recv(recv) {
                return Expr { kind: ExprKind::VecSum { vec: Box::new(rv) }, ty: scalar_to_ty(s), span };
            }
            return self.check_array_sum(recv, args, expected, span);
        }
        if method == "reduce" {
            return self.check_array_reduce(recv, args, expected, span);
        }
        if method == "scan" {
            return self.check_array_scan(recv, args, span);
        }
        if method == "dot" {
            return self.check_array_dot(recv, args, expected, span);
        }
        if method == "sum_where" {
            return self.check_vec_sum_where(recv, args, span);
        }
        if method == "load" {
            return self.check_vec_load(recv, args, expected, span);
        }
        if method == "store" {
            return self.check_vec_store(recv, args, span);
        }
        if method == "sort" {
            return self.check_array_sort(recv, args, span);
        }
        if method == "sort_by_key" {
            return self.check_array_sort_by_key(recv, args, span);
        }
        if method == "count" {
            return self.check_array_count(recv, args, span);
        }
        if method == "any" || method == "all" {
            return self.check_array_any_all(recv, args, method == "all", span);
        }
        // `arr.min()` / `arr.max()` (no args) is the array reduction; `a.min(b)` / `a.max(b)`
        // (one arg) is the pairwise scalar math op.
        if (method == "min" || method == "max") && args.is_empty() {
            // A `vecN<T>` receiver makes this the SIMD horizontal min/max reduction (the same surface
            // as the array reduction `arr.min()`); otherwise the array path runs.
            if let Some((rv, s)) = self.try_vec_recv(recv) {
                return Expr { kind: ExprKind::VecMinMax { vec: Box::new(rv), max: method == "max" }, ty: scalar_to_ty(s), span };
            }
            return self.check_array_min_max(recv, args, expected, method == "max", span);
        }
        if method == "abs" {
            return self.check_scalar_math(recv, hir::MathFn::Abs, args, span);
        }
        if method == "min" {
            return self.check_scalar_math(recv, hir::MathFn::Min, args, span);
        }
        if method == "max" {
            return self.check_scalar_math(recv, hir::MathFn::Max, args, span);
        }
        // Float-only math functions (`core.math`).
        let float_fn = match method {
            "sqrt" => Some(hir::MathFn::Sqrt),
            "floor" => Some(hir::MathFn::Floor),
            "ceil" => Some(hir::MathFn::Ceil),
            "round" => Some(hir::MathFn::Round),
            "trunc" => Some(hir::MathFn::Trunc),
            "pow" => Some(hir::MathFn::Pow),
            _ => None,
        };
        if let Some(f) = float_fn {
            return self.check_scalar_math(recv, f, args, span);
        }
        if method == "to_array" {
            return self.check_array_to_array(recv, args, span);
        }
        if method == "map_into" {
            return self.check_array_map_into(recv, args, span);
        }
        if method == "to_soa" {
            return self.check_array_to_soa(recv, args, span);
        }
        if method == "partition" {
            return self.check_array_partition(recv, args, span);
        }
        if method == "par_map" {
            return self.check_array_par_map(recv, args, span);
        }
        if method == "chunks" {
            return self.check_array_chunks(recv, args, span);
        }
        // Sink/source methods: a `builder`'s typed `write*` / `to_string`, a `writer`'s `write` /
        // `flush`, a `reader`'s `read`, a `buffer`'s `bytes`. `.write` is shared, so evaluate the
        // receiver once and dispatch on its type. (`write_int`/`to_string` are builder-only.)
        if matches!(method, "write" | "write_int" | "write_bool" | "write_char" | "write_float" | "to_string" | "flush" | "read" | "bytes") {
            let recv_expr = self.check_expr(recv, None);
            if recv_expr.ty == Ty::Writer {
                return self.check_writer_method(recv_expr, method, args, span);
            }
            if recv_expr.ty == Ty::Reader {
                return self.check_reader_method(recv_expr, method, args, span);
            }
            if recv_expr.ty == Ty::Buffer && method == "bytes" {
                return self.check_buffer_bytes(recv_expr, args, span);
            }
            if method != "read" && method != "bytes"
                && let Some(kind) = builder_write_kind(method) {
                return self.check_builder_write(recv_expr, args, kind, span);
            }
            if method == "to_string" {
                return self.check_builder_to_string(recv_expr, args, span);
            }
            // A sink/source method on a value that is none of the above.
            if recv_expr.ty != Ty::Error {
                self.diags
                    .error(format!("'.{method}()' is not a method on {}", ty_name(recv_expr.ty)), span);
            }
            return err;
        }
        // `std.net` stream borrows on a `tcp_conn`: `c.reader()` / `c.writer()` hand back an M9
        // reader/writer over the conn's socket fd (`owns_fd:false`), region-bound to `c`. Dispatched
        // on the receiver type so the names stay free on other values.
        if matches!(method, "reader" | "writer") {
            let recv_expr = self.check_expr(recv, None);
            if recv_expr.ty == Ty::TcpConn {
                return self.check_conn_stream(recv_expr, method, args, span);
            }
            if recv_expr.ty != Ty::Error {
                self.diags
                    .error(format!("'.{method}()' is not a method on {} (it is a `tcp_conn` method)", ty_name(recv_expr.ty)), span);
            }
            return err;
        }
        // `std.net` — `l.accept()` on a `tcp_listener` blocks for an inbound connection and returns a
        // new owned `tcp_conn` (`Result<tcp_conn, Error>`). Dispatched on the receiver type so the
        // name stays free on other values.
        if method == "accept" {
            let recv_expr = self.check_expr(recv, None);
            if recv_expr.ty == Ty::TcpListener {
                return self.check_listener_accept(recv_expr, args, span);
            }
            if recv_expr.ty != Ty::Error {
                self.diags
                    .error(format!("'.accept()' is not a method on {} (it is a `tcp_listener` method)", ty_name(recv_expr.ty)), span);
            }
            return err;
        }
        // `std.process` — `ch.wait()` on a `child` blocks in `waitpid` and returns the exit code
        // (`Result<i64, Error>`). Dispatched on the receiver type so the name stays free on other
        // values (in particular the bare `wait(handle)` task_group builtin is unaffected).
        if method == "wait" {
            let recv_expr = self.check_expr(recv, None);
            if recv_expr.ty == Ty::Child {
                return self.check_child_wait(recv_expr, args, span);
            }
            if recv_expr.ty != Ty::Error {
                self.diags
                    .error(format!("'.wait()' is not a method on {} (it is a `child` method)", ty_name(recv_expr.ty)), span);
            }
            return err;
        }
        // `std.process` — `ch.kill(sig)` on a `child` sends signal `sig` via libc `kill`
        // (`Result<(), Error>`). Dispatched on the receiver type so the name stays free on other values.
        if method == "kill" {
            let recv_expr = self.check_expr(recv, None);
            if recv_expr.ty == Ty::Child {
                return self.check_child_kill(recv_expr, args, span);
            }
            if recv_expr.ty != Ty::Error {
                self.diags
                    .error(format!("'.kill()' is not a method on {} (it is a `child` method)", ty_name(recv_expr.ty)), span);
            }
            return err;
        }
        // `std.net` — `u.send_to(data, host, port)` / `u.recv_from(buf)` on a `udp_socket`. Datagram
        // ops, each returning `Result<i64, Error>` (a byte count). Dispatched on the receiver type so
        // the names stay free on other values.
        if matches!(method, "send_to" | "recv_from") {
            let recv_expr = self.check_expr(recv, None);
            if recv_expr.ty == Ty::UdpSocket {
                return self.check_udp_socket_method(recv_expr, method, args, span);
            }
            if recv_expr.ty != Ty::Error {
                self.diags
                    .error(format!("'.{method}()' is not a method on {} (it is a `udp_socket` method)", ty_name(recv_expr.ty)), span);
            }
            return err;
        }
        // `std.rand` value methods on an `rng`: `r.next()` / `r.range(lo, hi)` / `r.shuffle(out xs)`
        // / `r.sample(xs, k)`. Each advances the receiver state in place, so the receiver must be a
        // **mut** local (checked in `check_rng_method`). Dispatched on the receiver type so a same-
        // named user method on another value still resolves normally.
        if matches!(method, "next" | "range" | "shuffle" | "sample") {
            let recv_expr = self.check_expr(recv, None);
            if recv_expr.ty == Ty::Rng {
                return self.check_rng_method(recv, recv_expr, method, args, span);
            }
            if recv_expr.ty != Ty::Error {
                self.diags
                    .error(format!("'.{method}()' is not a method on {} (it is an `rng` method)", ty_name(recv_expr.ty)), span);
            }
            return err;
        }
        // `.len()` of a `str`/`slice`/array/`buffer` — the element/byte count (an `i64`).
        if method == "len" {
            return self.check_len(recv, args, span);
        }
        // `map`/`where` are only valid as pipeline stages under a terminal reduction.
        if method == "map" || method == "where" {
            self.diags.error(
                format!("'.{method}()' must be part of a pipeline ending in a reduction like `.sum()`"),
                span,
            );
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        // For `box.get()` / `box.clone()` whose receiver is a fresh `heap.new(...)`, thread the
        // caller's expected type inward so the boxed literal's payload infers from context
        // (`v: i32 := heap.new(7).get()` → `box<i32>`) instead of defaulting the literal to i64 and
        // then failing the slot's width check. Scoped to a `heap.new` receiver: a box-typed *variable*
        // already has a fixed payload, so threading a box-expected there would double-report a genuine
        // mismatch that the `check_expr` reconciliation already catches once.
        let recv_expected = if is_heap_new_call(recv) {
            match method {
                // `.get()` yields the payload scalar; the receiver is a `box<that scalar>`.
                "get" => expected.and_then(|e| ty_to_scalar(self.resolve(e))).map(Ty::Box),
                // `.clone()` yields a `box<T>`; pass the expected box straight through.
                "clone" if matches!(expected, Some(Ty::Box(_))) => expected,
                _ => None,
            }
        } else {
            None
        };
        let recv_expr = self.check_expr(recv, recv_expected);
        let recv_ty = recv_expr.ty;
        match method {
            // `box<T>.get()` / `Task<R>.get()` — but NOT `http client.get(url)` (routed to the
            // http-client arm below; `check_box_get` otherwise swallows it with a box-only error).
            "get" if recv_ty != Ty::HttpClient => self.check_box_get(recv_expr, recv_ty, args, span),
            "clone" => self.check_box_clone(recv_expr, recv_ty, args, span),
            "contains" | "starts_with" | "ends_with" | "find" | "rfind" | "eq_ignore_ascii_case"
                if matches!(recv_ty, Ty::Str | Ty::String) =>
            {
                self.check_str_predicate(recv_expr, method, args, span)
            }
            "trim" | "trim_start" | "trim_end" if matches!(recv_ty, Ty::Str | Ty::String) => {
                self.check_str_trim(recv_expr, method, args, span)
            }
            "map_err" if matches!(self.resolve(recv_ty), Ty::Result(..)) => {
                self.check_map_err(recv_expr, args, expected, span)
            }
            // `std.cli` command methods on a `cli command`: `c.flag_bool/str/i64(...)` register a
            // flag, `c.parse(args)` yields `Result<parsed, Error>`, `c.usage()` renders help.
            // Type-guarded (like `trim`/`contains`/`map_err`): a same-named method on any other type
            // falls through to the `_` arm's normal "unknown method" resolution, never intercepted.
            "flag_bool" | "flag_str" | "flag_i64" | "parse" | "usage" if recv_ty == Ty::CliCommand => {
                self.check_cli_command_method(recv_expr, method, args, span)
            }
            // `std.cli` parsed getters on a `cli parsed`: `p.get_bool/i64/str(name)`. Total after a
            // successful parse; unregistered / wrong-kind aborts at runtime. Type-guarded, same as above.
            "get_bool" | "get_i64" | "get_str" if recv_ty == Ty::CliParsed => {
                self.check_cli_parsed_method(recv_expr, method, args, span)
            }
            // `std.http` request methods on an `http request`: `r.header(name, value)` /
            // `r.body(data)` mutate the builder in place. Type-guarded, same as the cli methods above.
            "header" | "body" if recv_ty == Ty::HttpRequest => {
                self.check_http_request_method(recv_expr, method, args, span)
            }
            // `std.http` response getters on an `http response`: `resp.status()` / `resp.header(name)`
            // (case-insensitive `Option<str>` view) / `resp.body()` (`slice<u8>` view). Type-guarded.
            "status" | "header" | "body" if recv_ty == Ty::HttpResponse => {
                self.check_http_response_method(recv_expr, method, args, span)
            }
            // `std.http` (Slice 2) client requests on an `http client`: `cl.get(url)` /
            // `cl.post(url, body)` / `cl.request(req)` each yield `Result<response, Error>`. Impure
            // (network). Type-guarded, same as the response getters above.
            "get" | "post" | "request" if recv_ty == Ty::HttpClient => {
                self.check_http_client_method(recv_expr, method, args, span)
            }
            _ => {
                if recv_ty != Ty::Error {
                    self.diags
                        .error(format!("unknown method '.{method}()' on {}", ty_name(recv_ty)), span);
                }
                err
            }
        }
    }

    /// `[e1, e2, ...]` — a fixed-length array literal. Elements share one scalar type
    /// (resolved here; an unconstrained literal defaults). Empty literals need a type
    /// annotation, which is not supported yet.
    /// `template "...{hole}..."` — each hole is a local of int or str type; the result
    /// is a `str`.
    fn check_template(&mut self, parts: &[ast::TemplatePart], expected: Option<Ty>, span: Span) -> Expr {
        let mut hparts = Vec::new();
        for p in parts {
            match p {
                ast::TemplatePart::Text(s) => hparts.push(TemplatePart::Text(s.clone())),
                ast::TemplatePart::Hole(expr) => {
                    let e = self.check_expr(expr, None);
                    if !is_printable(e.ty) {
                        self.diags.error(
                            format!("a template hole must be an int, float, str, bool, or char, got {}", ty_name(e.ty)),
                            e.span,
                        );
                    }
                    hparts.push(TemplatePart::Hole(e));
                }
            }
        }
        self.constrain(Ty::Str, expected, span);
        self.guard_lambda_alloc_leak("a `template` string", span);
        Expr { kind: ExprKind::Template(hparts), ty: Ty::Str, span }
    }

    /// `dot(a, b)` — the dot product of two `vecN<T>` (M6): the element scalar `sum(a[i] * b[i])`.
    /// Both operands must be the same vector type. (The free-function form per `draft.md` §9, the
    /// vector sibling of `select`; the array pipeline `xs.dot(ys)` is a separate method terminal.)
    fn check_vec_dot(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [a, b] = args else {
            self.diags.error(format!("'dot' takes 2 arguments (two vectors), got {}", args.len()), span);
            return err;
        };
        let ac = self.check_expr(a, None);
        let bc = self.check_expr(b, Some(ac.ty));
        if ac.ty == Ty::Error || bc.ty == Ty::Error {
            return err;
        }
        // Resolve before reporting so a diagnostic never prints an unresolved inference variable.
        let (resolved_a, resolved_b) = (self.resolve(ac.ty), self.resolve(bc.ty));
        let Ty::Vec(s, n) = resolved_a else {
            self.diags.error(format!("'dot' takes two vectors, got {}", ty_name(resolved_a)), a.span);
            return err;
        };
        if resolved_b != Ty::Vec(s, n) {
            self.diags.error(
                format!("'dot' operands must have the same vector type, got {} and {}", ty_name(resolved_a), ty_name(resolved_b)),
                b.span,
            );
            return err;
        }
        Expr { kind: ExprKind::VecDot { a: Box::new(ac), b: Box::new(bc) }, ty: scalar_to_ty(s), span }
    }

    /// For a no-arg reduction (`recv.sum()`/`.min()`/`.max()`) whose surface is shared with the array
    /// pipeline: if `recv` is a **vector value**, return its checked form + element scalar (→ the SIMD
    /// reduction); otherwise `None` (→ the array path). An array-pipeline-shaped receiver
    /// (`xs.map(f)`, a `.field` projection) is never type-checked here — a pipeline without a terminal
    /// is an error. Any other receiver is checked **speculatively**: if it is not a vector, the check's
    /// diagnostics are rolled back so the array path re-checks and reports the single, clean error.
    fn try_vec_recv(&mut self, recv: &ast::Expr) -> Option<(Expr, Scalar)> {
        if is_array_pipeline_recv(recv) {
            return None;
        }
        let mark = self.diags.len();
        let rv = self.check_expr(recv, None);
        if let Ty::Vec(s, _) = self.resolve(rv.ty) {
            return Some((rv, s));
        }
        self.diags.truncate(mark);
        None
    }

    /// `s.load(i)` — load `N` consecutive elements of a `slice<T>` starting at `i` into a `vecN<T>`
    /// (M6). `N`/`T` come from the target annotation (like a vector literal); `i` is a runtime `i64`,
    /// bounds-checked (`0 <= i && i + N <= len`). The source is a `slice<T>` (borrow a fixed array
    /// with `xs[..]`).
    fn check_vec_load(&mut self, recv: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Some(Ty::Vec(s, n)) = expected.map(|t| self.resolve(t)) else {
            self.diags.error("'load' needs a vector type annotation (e.g. `v: vec4<i32> := s.load(i)`)".to_string(), span);
            return err;
        };
        let [i] = args else {
            self.diags.error(format!("'load' takes 1 argument (the start index), got {}", args.len()), span);
            return err;
        };
        let src = self.check_expr(recv, None);
        if src.ty == Ty::Error {
            return err;
        }
        let Ty::Slice(es) = self.resolve(src.ty) else {
            self.diags.error(format!("'load' reads a slice<T>, got {} (borrow a fixed array with `xs[..]`)", ty_name(src.ty)), recv.span);
            return err;
        };
        if es != s {
            self.diags.error(
                format!("'load' element type {} does not match the target vector element {}", scalar_name(es), scalar_name(s)),
                span,
            );
            return err;
        }
        let idx = self.check_expr(i, Some(Ty::Int(IntTy { bits: 64, signed: true })));
        if !idx.ty.is_int_like() && idx.ty != Ty::Error {
            self.diags.error(format!("a load index must be an integer, got {}", ty_name(idx.ty)), i.span);
            return err;
        }
        Expr { kind: ExprKind::VecLoad { src: Box::new(src), index: Box::new(idx), elem: s, n }, ty: Ty::Vec(s, n), span }
    }

    /// `s.store(i, v)` — store the `N` lanes of `v: vecN<T>` into a writable `slice<T>` at `i..i+N`
    /// (M6), bounds-checked. The destination must be a `mut` / `out` slice (the same writability rule
    /// as `place[i] = v`). Yields `()`.
    fn check_vec_store(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [i, v] = args else {
            self.diags.error(format!("'store' takes 2 arguments (the start index and a vector), got {}", args.len()), span);
            return err;
        };
        // Type-check the receiver first, so an error *inside* it (an undefined name, a bad field)
        // surfaces cleanly rather than being masked by the writability/slice checks below.
        let dst = self.check_expr(recv, None);
        if dst.ty == Ty::Error {
            return err;
        }
        let Ty::Slice(es) = self.resolve(dst.ty) else {
            self.diags.error(format!("'store' writes a slice<T>, got {}", ty_name(dst.ty)), recv.span);
            return err;
        };
        // The destination must be a writable slice place (a `mut` local or an `out` slice parameter).
        let Some((id, _)) = self.place_local(recv) else {
            self.diags.error("'store' needs a writable slice (a `mut` local or `out` parameter)".to_string(), recv.span);
            return err;
        };
        if !self.locals[id as usize].is_mut {
            let name = self.locals[id as usize].name.clone();
            self.diags.error(format!("cannot store into immutable '{name}' (declare with `mut`, or use an `out` parameter)"), recv.span);
            return err;
        }
        let idx = self.check_expr(i, Some(Ty::Int(IntTy { bits: 64, signed: true })));
        if !idx.ty.is_int_like() && idx.ty != Ty::Error {
            self.diags.error(format!("a store index must be an integer, got {}", ty_name(idx.ty)), i.span);
            return err;
        }
        let value = self.check_expr(v, None);
        let (vs, n) = match self.resolve(value.ty) {
            Ty::Vec(vs, n) => (vs, n),
            Ty::Error => return err,
            other => {
                self.diags.error(format!("'store' takes a vector value, got {}", ty_name(other)), v.span);
                return err;
            }
        };
        if vs != es {
            self.diags.error(
                format!("'store' vector element {} does not match the slice element {}", scalar_name(vs), scalar_name(es)),
                span,
            );
            return err;
        }
        Expr { kind: ExprKind::VecStore { dst: Box::new(dst), index: Box::new(idx), value: Box::new(value), elem: es, n }, ty: Ty::Unit, span }
    }

    /// `vec.sum_where(mask)` — masked horizontal sum (M6): the sum of the lanes where the mask is
    /// set, as the element scalar. The mask's width must match the vector's.
    fn check_vec_sum_where(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [m] = args else {
            self.diags.error(format!("'sum_where' takes 1 argument (a mask), got {}", args.len()), span);
            return err;
        };
        let v = self.check_expr(recv, None);
        let mc = self.check_expr(m, None);
        if v.ty == Ty::Error || mc.ty == Ty::Error {
            return err;
        }
        let Ty::Vec(s, n) = self.resolve(v.ty) else {
            self.diags.error(format!("'sum_where' is a vector reduction, but the receiver is {}", ty_name(v.ty)), span);
            return err;
        };
        let mc_ty = self.resolve(mc.ty);
        match mc_ty {
            Ty::Mask(ms, mn) if ms == s && mn == n => {}
            Ty::Mask(..) => {
                self.diags.error(format!("'sum_where' mask {} does not match the vector {}", ty_name(mc_ty), ty_name(self.resolve(v.ty))), m.span);
                return err;
            }
            other => {
                self.diags.error(format!("'sum_where' needs a mask (a vector comparison result), got {}", ty_name(other)), m.span);
                return err;
            }
        }
        Expr { kind: ExprKind::VecSumWhere { vec: Box::new(v), mask: Box::new(mc) }, ty: scalar_to_ty(s), span }
    }

    /// `select(mask, a, b)` — lane-wise blend of two `vecN<T>` by a `mask` (M6 slice 2). The mask's
    /// width must match the vectors' width; the result is the vectors' type.
    /// `fma(a, b, c)` — fused multiply-add `a*b + c` with a single rounding. A free builtin (like
    /// `dot`/`select`). Float-only (scalar `f32`/`f64` or `vecN<f32>`/`vecN<f64>`); the three
    /// operands share the type. Lowers to one `llvm.fma` (a `vfmadd`/`fmla` instruction).
    fn check_fma(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [a, b, c] = args else {
            self.diags.error(format!("'fma' takes 3 arguments (the fused `a*b + c`), got {}", args.len()), span);
            return err;
        };
        let ac = self.check_expr(a, None);
        if ac.ty == Ty::Error {
            return err;
        }
        // Check b/c with `a`'s type as context, then unify all three to one type — so a float
        // literal in *any* position can constrain the others before the float-only check below
        // (checking `a` alone could prematurely reject an as-yet-unresolved var).
        let bc = self.check_expr(b, Some(ac.ty));
        let cc = self.check_expr(c, Some(ac.ty));
        if bc.ty == Ty::Error || cc.ty == Ty::Error {
            return err;
        }
        if self.unify(ac.ty, bc.ty, b.span) == Ty::Error || self.unify(ac.ty, cc.ty, c.span) == Ty::Error {
            return err;
        }
        // Float-only: a fused float multiply-add. A scalar float (incl. an undetermined float
        // literal) or a float vector.
        let ty = ac.ty;
        let rt = self.resolve(ty);
        if !(rt.is_float_like() || matches!(rt, Ty::Vec(Scalar::Float(_), _))) {
            self.diags.error(format!("'fma' needs a float or float-vector (it is a fused float multiply-add), got {}", ty_name(rt)), span);
            return err;
        }
        Expr { kind: ExprKind::MathOp { fn_: hir::MathFn::Fma, operands: vec![ac, bc, cc] }, ty, span }
    }

    fn check_select(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [m, a, b] = args else {
            self.diags.error(format!("'select' takes 3 arguments (a mask and two vectors), got {}", args.len()), span);
            return err;
        };
        let mc = self.check_expr(m, None);
        let ac = self.check_expr(a, None);
        let bc = self.check_expr(b, Some(ac.ty));
        if mc.ty == Ty::Error || ac.ty == Ty::Error || bc.ty == Ty::Error {
            return err;
        }
        let Ty::Vec(s, n) = self.resolve(ac.ty) else {
            self.diags.error(format!("'select' picks between two vectors, got {}", ty_name(ac.ty)), a.span);
            return err;
        };
        if self.resolve(bc.ty) != Ty::Vec(s, n) {
            self.diags.error(
                format!("'select' vectors must have the same type, got {} and {}", ty_name(ac.ty), ty_name(bc.ty)),
                b.span,
            );
            return err;
        }
        let mc_ty = self.resolve(mc.ty);
        match mc_ty {
            Ty::Mask(ms, mn) if ms == s && mn == n => {}
            Ty::Mask(..) => {
                self.diags.error(format!("'select' mask {} does not match the vectors {}", ty_name(mc_ty), ty_name(Ty::Vec(s, n))), m.span);
                return err;
            }
            other => {
                self.diags.error(
                    format!("'select' needs a mask (a vector comparison result) as its first argument, got {}", ty_name(other)),
                    m.span,
                );
                return err;
            }
        }
        Expr { kind: ExprKind::Select { mask: Box::new(mc), a: Box::new(ac), b: Box::new(bc) }, ty: Ty::Vec(s, n), span }
    }

    /// `[e0, …, e(N-1)]` under a `vecN<T>` annotation — a fixed-width SIMD vector literal. Exactly
    /// `n` elements, each of the element type `s` (M6 slice 1). The element is a numeric scalar.
    fn check_vec_lit(&mut self, elems: &[ast::Expr], s: Scalar, n: u32, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let elem_ty = scalar_to_ty(s);
        if elems.len() as u32 != n {
            self.diags.error(
                format!("a vec{n}<{}> literal needs exactly {n} elements, got {}", scalar_name(s), elems.len()),
                span,
            );
            return err;
        }
        let checked: Vec<Expr> = elems.iter().map(|e| self.check_expr(e, Some(elem_ty))).collect();
        if checked.iter().any(|e| e.ty == Ty::Error) {
            return err;
        }
        Expr { kind: ExprKind::VecLit { elems: checked, elem: s }, ty: Ty::Vec(s, n), span }
    }

    fn check_array_lit(&mut self, elems: &[ast::Expr], elem_expected: Option<Ty>, span: Span) -> Expr {
        if elems.is_empty() {
            self.diags
                .error("an empty array literal needs a type annotation (not supported yet)".to_string(), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let n = elems.len() as u32;
        // An array of struct literals → a struct array (AoS).
        if let ast::ExprKind::StructLit { .. } = &elems[0].kind {
            let mut checked = Vec::new();
            let mut sid = None;
            for e in elems {
                let ast::ExprKind::StructLit { name, fields } = &e.kind else {
                    self.diags.error("array elements must all be struct literals here".to_string(), e.span);
                    continue;
                };
                let lit = self.check_struct_lit(name, fields, e.span);
                if let Ty::Struct(id) = lit.ty {
                    match sid {
                        None => sid = Some(id),
                        Some(prev) if prev != id => {
                            self.diags.error("array elements must be the same struct type".to_string(), e.span);
                        }
                        _ => {}
                    }
                }
                checked.push(lit);
            }
            return match sid {
                // A fixed `[S{…}, …]` array of an `align(N)` struct is supported: its stack slot is
                // over-aligned (`type_align`) and the struct's LLVM size is padded up to `N`, so every
                // element's stride keeps the alignment. (A *dynamic* `array<align(N)Struct>` stays
                // rejected — its heap buffer over-alignment is a separate, still-deferred concern.)
                Some(id) => Expr {
                    kind: ExprKind::ArrayLit { elems: checked, elem: Ty::Struct(id) },
                    ty: Ty::StructArray(id, n),
                    span,
                },
                None => Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span },
            };
        }
        // Otherwise a scalar array.
        let first = self.check_expr(&elems[0], elem_expected);
        let elem_ty = first.ty;
        // A `reader`/`writer`/`buffer`/cli handle element is rejected at construction (like a struct
        // field / tuple element): the array read copies the handle by value, so collecting handles
        // would alias one fd/buffer across copies → double close/free (UB). Bind the handle to a local.
        if matches!(self.resolve(elem_ty), Ty::Reader | Ty::Writer | Ty::Buffer | Ty::CliCommand | Ty::CliParsed | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child | Ty::HttpRequest | Ty::HttpResponse | Ty::HttpClient) {
            self.diags.error(
                format!("`{}` cannot be an array element — an owned I/O handle/buffer is bound to one local, not collected (bind it to a local)", ty_name(elem_ty)),
                span,
            );
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let mut checked = vec![first];
        for e in &elems[1..] {
            checked.push(self.check_expr(e, Some(elem_ty)));
        }
        // Wasteful-default lint (`draft.md` §16): a large literal array whose element type is left
        // unconstrained falls to the `i64` / `f64` default, spending 8 bytes/element even when the
        // data would fit a narrower type. If nothing constrained the element type (`elem_expected`
        // is `None`) and it is still an inference variable (so `payload_scalar` below will default
        // it), warn once for the whole literal and point at a narrower annotation. A **warning**:
        // the default is correct, just potentially wasteful. (`elem_expected` present, or a concrete
        // element type from a real value like `[a, b]` where `a: i32`, stays silent.)
        if elem_expected.is_none() && n >= DEFAULT_ELEM_LITERAL_ARRAY_LEN {
            let (dflt, narrower) = match self.resolve(elem_ty) {
                Ty::IntVar(_) => (Some("i64"), "a narrower integer type (e.g. `i32`/`i16`/`i8`)"),
                Ty::FloatVar(_) => (Some("f64"), "`f32`"),
                _ => (None, ""),
            };
            if let Some(dflt) = dflt {
                self.diags.push(align_diag::Diagnostic::warning(
                    format!("this {n}-element literal array has an unconstrained element type, so it defaults to `{dflt}` (8 bytes/element); annotate {narrower} if the values fit"),
                    span,
                ));
            }
        }
        let scalar = self.payload_scalar(elem_ty, span);
        Expr { kind: ExprKind::ArrayLit { elems: checked, elem: scalar_to_ty(scalar) }, ty: Ty::Array(scalar, n), span }
    }

    /// Collect a pipeline `src.map(f).where(p)…` from the AST: the innermost receiver is
    /// the source array; `.map`/`.where` calls become ordered stages (source-first).
    /// Check a `map`/`where` stage function against the current element type, returning
    /// its return type. `is_pred` requires a `bool` result.
    fn check_stage_fn(&mut self, fname: &ast::Ident, elem: Ty, is_pred: bool) -> Ty {
        let Some(sig) = self.sigs.get(&fname.name) else {
            self.diags.error(format!("undefined function: '{}'", fname.name), fname.span);
            return Ty::Error;
        };
        let (params, ret) = (sig.params.clone(), sig.ret);
        if params.len() != 1 || params[0] != elem {
            self.diags.error(
                format!("'{}' must take one {} argument here", fname.name, ty_name(elem)),
                fname.span,
            );
        }
        if is_pred && ret != Ty::Bool {
            self.diags
                .error(format!("'where' predicate '{}' must return bool", fname.name), fname.span);
        }
        ret
    }

    /// Resolve a stage's function argument (named or lambda) over element type `elem`, returning
    /// the (possibly synthetic) function name to lower to and its return type. A lambda is lifted
    /// to a synthetic top-level function (`lift_lambda`); a named function is checked in place.
    fn resolve_stage_fn(&mut self, sf: &StageFn, elem: Ty, is_pred: bool) -> Option<(String, Ty, Vec<Expr>)> {
        match sf {
            StageFn::Named(fname) => Some((fname.name.clone(), self.check_stage_fn(fname, elem, is_pred), Vec::new())),
            StageFn::Lambda { params, body, span } => {
                let expected_ret = if is_pred { Some(Ty::Bool) } else { None };
                self.lift_lambda(params, body, &[elem], expected_ret, *span)
            }
        }
    }

    /// Lift an inline lambda to a synthetic top-level function and return its generated name + its
    /// return type. The lambda is checked as an isolated function body (its own locals/scope/
    /// inference), with parameter types taken from `expected_params` (so a `slice<i32>.map(fn x …)`
    /// gives `x: i32`); an inline-literal element type defaults like any unconstrained literal. The
    /// lifted function joins `program.fns` (Pass 2 collects `self.lifted`), so move/escape/purity
    /// analysis and codegen treat it exactly like a named function — including the `par_map` Pure
    /// requirement. The fused-loop lowering is then identical to a named stage function.
    ///
    /// Slice ① cut: **non-capturing** only — the lambda body sees its parameters and top-level
    /// functions, but not enclosing locals (capturing those is a follow-up; such a reference
    /// surfaces as an undefined-variable error here).
    /// Reject an allocating string op inside a lifted lambda that has no arena to hold the result.
    /// A pipeline lambda (`reduce`/`map`/`par_map`/…) is lifted to a synthetic top-level function;
    /// the enclosing `arena {}` is **not** threaded into it, so an allocation there lowers with no
    /// arena and is leaked to process-lifetime (`align_rt_builder_finish`). Per call, in a reduce
    /// loop, that OOMs (Gemini's M2 report, Gap A). A silent leak violates "nothing hidden", so this
    /// is a hard error pointing at the right tool. `capture.is_some()` ⇒ we are inside a lambda body;
    /// `arena_depth == 0` ⇒ the lambda opened no arena of its own to catch the allocation.
    fn guard_lambda_alloc_leak(&mut self, what: &str, span: Span) {
        if self.capture.is_some() && self.arena_depth == 0 {
            self.diags.error(
                format!(
                    "{what} inside a pipeline lambda leaks — the enclosing `arena {{}}` is not available in a \
                     lifted lambda. Accumulate with a `builder` instead, e.g. \
                     `reduce(builder(), fn b, x {{ b.write(x); b }})`, or open an `arena {{}}` inside the lambda."
                ),
                span,
            );
        }
    }

    fn lift_lambda(
        &mut self,
        params: &[ast::LambdaParam],
        body: &ast::Block,
        expected_params: &[Ty],
        expected_ret: Option<Ty>,
        span: Span,
    ) -> Option<(String, Ty, Vec<Expr>)> {
        if params.len() != expected_params.len() {
            self.diags.error(
                format!("this lambda must take {} parameter(s), but has {}", expected_params.len(), params.len()),
                span,
            );
            return None;
        }
        // Parameter types must be concrete at the lambda boundary (a function signature can't carry
        // another function's inference variable), so resolve the element type now.
        let param_tys: Vec<Ty> = expected_params.iter().map(|t| self.finalize(*t)).collect();

        // Snapshot the enclosing scope (with finalized types) so a body reference to an enclosing
        // local can be captured. Finalizing now (enclosing inference still live) keeps the capture
        // parameter's type consistent with the enclosing local once both default the same way.
        let enclosing: Vec<(String, LocalId, Ty)> = self
            .scope
            .iter()
            .map(|(n, id)| (n.clone(), *id, self.finalize(self.locals[*id as usize].ty)))
            .collect();

        // Swap in fresh per-function state; the lambda is a separate function body.
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_scope = std::mem::take(&mut self.scope);
        let saved_int_vars = std::mem::take(&mut self.int_vars);
        let saved_int_parent = std::mem::take(&mut self.int_parent);
        let saved_float_vars = std::mem::take(&mut self.float_vars);
        let saved_float_parent = std::mem::take(&mut self.float_parent);
        let saved_ret = self.ret_hint;
        let saved_arena = self.arena_depth;
        // A lambda body is a separate function: it is not lexically inside the enclosing
        // `task_group`, so reset the task-group / `wait`-state tracking (else a `wait()` inside the
        // lambda would set the enclosing group's flag at compile time and bypass the check).
        let saved_tg_depth = self.task_group_depth;
        let saved_wait_state = std::mem::take(&mut self.wait_state);
        let saved_tg_fallible = std::mem::take(&mut self.task_group_fallible);
        let saved_bases = std::mem::take(&mut self.slice_bases);
        let saved_capture = self.capture.take();
        self.ret_hint = expected_ret.unwrap_or(Ty::Unit);
        self.arena_depth = 0;
        self.task_group_depth = 0;
        self.capture = Some(CaptureScope { enclosing, captured: Vec::new() });

        let mut param_ids: Vec<LocalId> = params
            .iter()
            .zip(&param_tys)
            .map(|(p, ty)| {
                // A lambda parameter shadows if it collides with another parameter of this lambda or
                // with an enclosing (capturable) binding of the surrounding function.
                self.check_shadow(&p.name.name, p.name.span, self.scope.len());
                self.declare(&p.name.name, *ty, false)
            })
            .collect();
        let checked = self.check_block(body, expected_ret);
        let ret = match expected_ret {
            Some(t) => t,
            None => checked.value.as_ref().map(|v| v.ty).unwrap_or(Ty::Unit),
        };
        let mut body_fin = checked;
        self.finalize_block(&mut body_fin);
        // Run the broad unnecessary-heap scan on the lifted lambda body too (parity with the narrow
        // lint in `finalize_expr`); a box local here is function-local (Move values cannot be
        // captured), so the scan is self-contained and never double-reports the enclosing function.
        UnnecessaryHeapScan::run(&body_fin, self.diags);

        // Collect captures: each becomes a trailing parameter of the lifted function, and the
        // enclosing local is passed at the call site. Slice ③ supports copy-value captures only.
        let captured = self.capture.take().unwrap().captured;
        let mut locals = std::mem::take(&mut self.locals);
        for l in &mut locals {
            l.ty = self.finalize(l.ty);
        }
        let mut capture_ops = Vec::new();
        for (cname, pid, enc_id) in &captured {
            let ty = locals[*pid as usize].ty;
            if ty_capture_is_move(ty, self.structs, self.tuples) {
                self.diags.error(
                    format!("a lambda cannot capture the owned value '{cname}' yet (capture supports copy values like int/float/bool/char)"),
                    span,
                );
            }
            param_ids.push(*pid);
            capture_ops.push(Expr { kind: ExprKind::Local(*enc_id), ty, span });
        }

        let ret = self.finalize(ret);
        let name = format!("{}$lambda{}", self.cur_fn, self.lifted.len());
        self.lifted.push(hir::Fn {
            name: name.clone(),
            params: param_ids,
            ret,
            locals,
            body: body_fin,
            span,
            drop_locals: Vec::new(),
        });

        // Restore the enclosing function's state.
        self.locals = saved_locals;
        self.scope = saved_scope;
        self.int_vars = saved_int_vars;
        self.int_parent = saved_int_parent;
        self.float_vars = saved_float_vars;
        self.float_parent = saved_float_parent;
        self.ret_hint = saved_ret;
        self.arena_depth = saved_arena;
        self.task_group_depth = saved_tg_depth;
        self.wait_state = saved_wait_state;
        self.task_group_fallible = saved_tg_fallible;
        self.slice_bases = saved_bases;
        self.capture = saved_capture;
        // A lambda must not return a function value: the returned closure's environment is
        // frame-local to *this* lifted function and would dangle once it returns (the same rule as
        // a top-level fn — checked here too so a stage/value lambda can't slip a closure out).
        if matches!(ret, Ty::Fn(_)) {
            self.diags.error(
                "a lambda cannot return a function value (a closure's environment is frame-local)".to_string(),
                span,
            );
            return None;
        }
        Some((name, ret, capture_ops))
    }

    /// Resolve a reducer/terminal function argument — a named function or an inline lambda — given
    /// its expected parameter types and (optionally) return type. Returns the (possibly synthetic
    /// lifted) function name and its actual return type. A named function is validated against the
    /// expected signature; a lambda is lifted (`lift_lambda`). `label` names the operation for
    /// diagnostics. Used by `reduce`/`par_map`/`scan`/`partition`/`any`/`all` (the element/acc
    /// types are known after `check_pipeline`/the initial value).
    fn resolve_fn(&mut self, arg: &ast::Expr, expected_params: &[Ty], expected_ret: Option<Ty>, label: &str, span: Span) -> Option<(String, Ty, Vec<Expr>)> {
        if let ast::ExprKind::Lambda { params, body } = &arg.kind {
            return self.lift_lambda(params, body, expected_params, expected_ret, arg.span);
        }
        let Some(fname) = self.pipeline_fn_name(arg) else {
            self.diags.error(format!("'{label}' needs a function (named or `fn … {{ … }}`)"), span);
            return None;
        };
        let Some(sig) = self.sigs.get(&fname.name) else {
            self.diags.error(format!("undefined function: '{}'", fname.name), fname.span);
            return None;
        };
        let (params, ret) = (sig.params.clone(), sig.ret);
        // Resolve the expected types first: an unresolved inference variable (e.g. an inline
        // literal's element type) must not false-positive against the concrete signature.
        let expected_resolved: Vec<Ty> = expected_params.iter().map(|&t| self.resolve(t)).collect();
        if params.as_slice() != expected_resolved.as_slice() || expected_ret.is_some_and(|er| self.resolve(er) != ret) {
            let want_ret = self.resolve(expected_ret.unwrap_or(ret));
            self.diags.error(
                format!(
                    "'{}' must have type ({}) -> {} here",
                    fname.name,
                    expected_resolved.iter().map(|t| ty_name(*t)).collect::<Vec<_>>().join(", "),
                    ty_name(want_ret),
                ),
                fname.span,
            );
            return None;
        }
        Some((fname.name, ret, Vec::new()))
    }

    /// The `idx`-th parameter type of a *named* function argument, to seed an inline-literal source's
    /// element type. A lambda has no signature to peek (its parameters are inferred), so it yields
    /// `None` (the literal then defaults like any unconstrained value).
    fn named_param_hint(&self, arg: &ast::Expr, idx: usize) -> Option<Ty> {
        if matches!(arg.kind, ast::ExprKind::Lambda { .. }) {
            return None;
        }
        self.pipeline_fn_name(arg).and_then(|f| self.sigs.get(&f.name).cloned()).and_then(|s| s.params.get(idx).copied())
    }

    /// The signature of a *named* function argument (`None` for a lambda or an unresolved name) —
    /// used by `reduce`/`scan` to take the accumulator/element types from a named fold's signature.
    fn named_sig(&self, arg: &ast::Expr) -> Option<FnSig> {
        if matches!(arg.kind, ast::ExprKind::Lambda { .. }) {
            return None;
        }
        self.pipeline_fn_name(arg).and_then(|f| self.sigs.get(&f.name).cloned())
    }

    fn collect_pipeline<'e>(&mut self, e: &'e ast::Expr) -> (&'e ast::Expr, Vec<RawStage>) {
        match &e.kind {
            // `.map(f)` / `.where(p)`
            ast::ExprKind::Call { callee, args } => {
                if let ast::ExprKind::FieldAccess { recv, field } = &callee.kind {
                    let is_map = field.name == "map";
                    let is_where = field.name == "where";
                    if is_map || is_where {
                        let arg = if args.len() == 1 { Some(&args[0]) } else { None };
                        let (src, mut stages) = self.collect_pipeline(recv);
                        // `where(.field)` — a field predicate.
                        if is_where
                            && let Some(ast::Expr { kind: ast::ExprKind::FieldShorthand(f), .. }) = arg {
                                stages.push(RawStage::WhereField(f.clone()));
                                return (src, stages);
                            }
                        // An inline lambda (`map(fn x { … })`) or a named function.
                        let stage_fn = match arg {
                            Some(a) => match &a.kind {
                                ast::ExprKind::Lambda { params, body } => Some(StageFn::Lambda {
                                    params: params.clone(),
                                    body: body.clone(),
                                    span: a.span,
                                }),
                                _ => self.pipeline_fn_name(a).map(StageFn::Named),
                            },
                            None => None,
                        };
                        match stage_fn {
                            Some(f) if is_map => stages.push(RawStage::Map(f)),
                            Some(f) => stages.push(RawStage::Where(f)),
                            None => self.diags.error(
                                format!("'.{}()' needs a function (named or `fn … {{ … }}`) or `.field`", field.name),
                                e.span,
                            ),
                        }
                        return (src, stages);
                    }
                }
                (e, Vec::new())
            }
            // `.field` projection on an array.
            ast::ExprKind::FieldAccess { recv, field } => {
                let (src, mut stages) = self.collect_pipeline(recv);
                stages.push(RawStage::Project(field.clone()));
                (src, stages)
            }
            _ => (e, Vec::new()),
        }
    }

    fn pipeline_fn_name(&self, a: &ast::Expr) -> Option<ast::Ident> {
        if let ast::ExprKind::Path(p) = &a.kind
            && p.segments.len() == 1 {
                return Some(p.segments[0].clone());
            }
        None
    }

    /// `src.map(f).where(p).field….sum()` — a fused reduction. Threads the element type
    /// through each stage (a struct array is projected to a scalar) and folds the final
    /// numeric element type with `+`.
    /// Collect and type-check a pipeline `src.map(f).where(p).field…`, returning the
    /// checked source, its stages, and the final element type. `elem_expected_no_stages`
    /// is the element type to push into an inline literal when there are no stages.
    fn check_pipeline(&mut self, recv: &ast::Expr, elem_expected_no_stages: Option<Ty>, span: Span) -> Option<(Expr, Vec<Stage>, Ty)> {
        let (source_ast, raw_stages) = self.collect_pipeline(recv);
        // The expected element type for an inline scalar literal source: the first Map
        // stage's parameter, or (with no stages) the caller-provided hint.
        let elem_expected = match raw_stages.first() {
            // A named first `map` fixes the element type from its parameter; a lambda's parameter
            // type is inferred (the literal defaults), so there is no hint to pull.
            Some(RawStage::Map(StageFn::Named(fname))) => self.sigs.get(&fname.name).and_then(|s| s.params.first().copied()),
            None => elem_expected_no_stages,
            _ => None,
        };
        let source = match &source_ast.kind {
            ast::ExprKind::ArrayLit(elems) => self.check_array_lit(elems, elem_expected, span),
            _ => self.check_expr(source_ast, None),
        };
        let mut elem = match source.ty {
            Ty::Array(s, _) | Ty::Slice(s) | Ty::DynArray(s) => scalar_to_ty(s),
            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => Ty::Struct(id),
            // A `soa<Struct>` source is a struct source whose field access is column-addressed
            // (the `Layout::Soa` seam in MIR `lower_field_access`). It flows through the same
            // `where(.field)` / `.field` / reduce pipeline as AoS, but a scan touches only the
            // columns it reads — so `ps.where(.active).pay.sum()` streams just `active` and `pay`.
            Ty::Soa(id) => Ty::Struct(id),
            // An `array<slice<T>>` (a `chunks` result): each element is a `slice<T>` chunk —
            // the input to `chunks(n).par_map(f)`'s `f: (slice<T>) -> R`.
            Ty::DynSliceArray(p) => Ty::Slice(prim_to_scalar(p)),
            Ty::Error => return None,
            other => {
                self.diags
                    .error(format!("a pipeline source must be an array, got {}", ty_name(other)), span);
                return None;
            }
        };
        // MIR materializes a stack-array source only when it is an array literal or a named
        // local (slot-addressable); an arbitrary array-valued expression (e.g. an `if` or
        // block) would otherwise crash lowering. A `{ptr,len}` view (`slice`/owned array) is
        // fine as a value, but a dynamic `array<Struct>` must be a variable: its field
        // projection indexes through the buffer pointer (`IndexFieldPtr`), and binding it first
        // keeps the owned buffer alive across the loop. Reject other array shapes cleanly here.
        let needs_var = matches!(source.ty, Ty::Array(..) | Ty::StructArray(..) | Ty::DynStructArray(..) | Ty::Soa(_));
        if needs_var && !matches!(source.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
            self.diags.error(
                "a pipeline over an array must start from an array literal or a variable (an arbitrary array expression is not supported yet)".to_string(),
                span,
            );
            return None;
        }

        // Field projection / field-predicate stages index the source by element, which needs a
        // slot-backed stack array / struct array (`IndexField`) or a dynamic `array<Struct>`
        // view addressed through its buffer pointer (`IndexFieldPtr`, slice 8d-2). A scalar
        // `{ptr,len}` view (`slice` / owned scalar `array`) has no per-element struct to project.
        let slot_backed = matches!(source.ty, Ty::Array(..) | Ty::StructArray(..) | Ty::DynStructArray(..) | Ty::Soa(_));
        let mut stages = Vec::new();
        // `.field` / `where(.field)` read the *source* element by index; after a `map` the logical
        // element is a computed value no longer in the source buffer, so those stages are only
        // valid before any `map`.
        let mut mapped = false;
        for raw in raw_stages {
            match raw {
                RawStage::Project(field) => {
                    if !slot_backed {
                        self.diags.error(
                            format!("'.{}' field projection needs an array source, not a slice/array view", field.name),
                            field.span,
                        );
                        return None;
                    }
                    if !matches!(elem, Ty::Struct(_)) {
                        self.diags.error(
                            format!("'.{}' projection needs a struct element, got {}", field.name, ty_name(elem)),
                            field.span,
                        );
                        return None;
                    }
                    // A struct element after a `map` is the map's (struct) result, not a source
                    // element — projection reads the source, so reject it (checked after the
                    // struct-type check so a non-struct gets the more fundamental diagnostic).
                    if mapped {
                        self.diags.error(
                            format!("'.{}' field projection after 'map' is not supported (map produces a computed value, not a source element)", field.name),
                            field.span,
                        );
                        return None;
                    }
                    match self.field_of(elem, &field.name, field.span) {
                        Some((index, ty)) => {
                            stages.push(Stage { kind: StageKind::Project { field: index }, out_ty: ty });
                            elem = ty;
                        }
                        None => return None,
                    }
                }
                RawStage::Map(sf) => {
                    // `map(f)` accepts a scalar element or a whole struct element: a struct array
                    // stays index-addressed until used, and a struct-consuming `map` loads the
                    // element by index in MIR (`lower_struct_elem`). The function (named or lambda)
                    // takes the current element type and returns the new one.
                    if matches!(source.ty, Ty::Soa(_)) && matches!(elem, Ty::Struct(_)) {
                        self.diags.error("a whole-struct `map`/`where` over `soa<T>` is not supported (it would gather every column); project a field first (`s.field.…`) or filter a field (`where(.field)`)".to_string(), span);
                        return None;
                    }
                    let (func, ret, captures) = self.resolve_stage_fn(&sf, elem, false)?;
                    stages.push(Stage { kind: StageKind::Map { func, captures }, out_ty: ret });
                    elem = ret;
                    mapped = true;
                }
                RawStage::Where(sf) => {
                    // `where(f)` accepts a scalar element or a whole struct element (a multi-field
                    // predicate). A struct-consuming predicate loads the element by value in MIR
                    // (the same `lower_struct_elem` as `map`); `where` filters, so the element is
                    // unchanged (no `mapped`, and a later `.field` / `where(.field)` still reads the
                    // source).
                    if matches!(source.ty, Ty::Soa(_)) && matches!(elem, Ty::Struct(_)) {
                        self.diags.error("a whole-struct `map`/`where` over `soa<T>` is not supported (it would gather every column); filter a field with `where(.field)`".to_string(), span);
                        return None;
                    }
                    let (func, _, captures) = self.resolve_stage_fn(&sf, elem, true)?;
                    stages.push(Stage { kind: StageKind::Where { func, captures }, out_ty: elem });
                }
                RawStage::WhereField(field) => {
                    if !slot_backed {
                        self.diags.error(
                            format!("'where(.{})' needs an array source, not a slice/array view", field.name),
                            field.span,
                        );
                        return None;
                    }
                    if !matches!(elem, Ty::Struct(_)) {
                        self.diags.error(
                            format!("'where(.{})' needs a struct element, got {}", field.name, ty_name(elem)),
                            field.span,
                        );
                        return None;
                    }
                    // Same as projection: a struct element after a `map` is the map result, not a
                    // source element (struct-type check first so a non-struct reports that first).
                    if mapped {
                        self.diags.error(
                            format!("'where(.{})' after 'map' is not supported (map produces a computed value, not a source element)", field.name),
                            field.span,
                        );
                        return None;
                    }
                    match self.field_of(elem, &field.name, field.span) {
                        Some((index, fty)) => {
                            if fty != Ty::Bool {
                                self.diags.error(
                                    format!("'where(.{})' field must be bool, got {}", field.name, ty_name(fty)),
                                    field.span,
                                );
                            }
                            stages.push(Stage { kind: StageKind::WhereField { field: index }, out_ty: elem });
                        }
                        None => return None,
                    }
                }
            }
        }
        Some((source, stages, elem))
    }

    /// `src.…​.sum()` — fold the (numeric) post-stage elements with `+`.
    fn check_array_sum(&mut self, recv: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'sum' takes no arguments".to_string(), span);
        }
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Some((source, stages, elem)) = self.check_pipeline(recv, expected, span) else {
            return err;
        };
        if !elem.is_numeric() {
            self.diags
                .error(format!("'sum' needs a numeric element type, got {}", ty_name(elem)), span);
            return err;
        }
        self.constrain(elem, expected, span);
        Expr { kind: ExprKind::ArraySum { source: Box::new(source), stages }, ty: elem, span }
    }

    /// `s.group_by(.key).<agg>(…)` — column-oriented grouped aggregate over a `soa<Struct>` local:
    /// `sum`/`min`/`max(.value)` or `count()`. Yields `(array<i64>, array<i64>)` (distinct keys,
    /// per-key aggregate). First slice: the source must be a `soa<Struct>` local and the `key` (and
    /// `value`, for sum/min/max) fields must be `i64`. Idiomatic Rust reaches for a generic
    /// `HashMap<K, Acc>`; Align reads the columns sequentially into a primitive-key open-addressing
    /// aggregate. `method`/`args` are the aggregate call (`recv` was `X.group_by(.key)`).
    fn check_group_agg(&mut self, source: &ast::Expr, key_field: &ast::Ident, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // `.agg(sum(.a), max(.b), count(), …)` — the fused multi-aggregate terminal — is its own path.
        if method == "agg" {
            return self.check_group_agg_multi(source, key_field, args, span);
        }
        // Resolve the aggregate op + its (optional) value field from the method + args.
        let (op, value_field): (hir::GroupOp, Option<&ast::Ident>) = match method {
            "count" => {
                if !args.is_empty() {
                    self.diags.error("`group_by(.key).count()` takes no arguments".to_string(), span);
                    return err;
                }
                (hir::GroupOp::Count, None)
            }
            "sum" | "min" | "max" => {
                let [ast::Expr { kind: ast::ExprKind::FieldShorthand(v), .. }] = args else {
                    self.diags.error(format!("`group_by(.key).{method}(.value)` needs a `.field` value argument"), span);
                    return err;
                };
                let op = match method {
                    "sum" => hir::GroupOp::Sum,
                    "min" => hir::GroupOp::Min,
                    _ => hir::GroupOp::Max,
                };
                (op, Some(v))
            }
            _ => {
                self.diags.error(
                    format!("`group_by(.key)` supports `.sum/.min/.max(.value)` or `.count()`, not `.{method}()`"),
                    span,
                );
                return err;
            }
        };
        // The source is a struct-collection local: a `soa<Struct>` (contiguous columns, grouped on an
        // i64 OR a `str` key column — decided below by the key field's type), an AoS `array<Struct>`
        // (a `str` key, dictionary-encoded inline), or a precomputed `dict_encode`d value.
        let base = match self.place_local(source) {
            Some((id, _)) => id,
            None => {
                self.diags.error("`group_by` source must be a `soa<Struct>` or `array<Struct>` local".to_string(), span);
                return err;
            }
        };
        // The source local is a `soa<Struct>` (i64 key), an AoS `array<Struct>` (str key, encoded
        // inline), or a precomputed `dict_encoded<Struct>` (str key, reuse the encoded ids). For the
        // encoded source, `enc_key` is the field the dictionary was built on — the group key must
        // match it (grouping by a different field has no precomputed ids).
        let (id, source, enc_key) = match self.locals[base as usize].ty {
            // A soa keys on an i64 column (SoaI64) or a `str` column (SoaStr) — decided by the key
            // field's declared type. A str key column is interned like the AoS str key, but read from
            // its own contiguous column (the two-column runtime path).
            Ty::Soa(id) => {
                let is_str_key = self.structs[id as usize]
                    .fields
                    .iter()
                    .find(|f| f.name == key_field.name)
                    .is_some_and(|f| f.ty == Ty::Str);
                let src = if is_str_key { hir::GroupSource::SoaStr } else { hir::GroupSource::SoaI64 };
                (id, src, None)
            }
            Ty::DynStructArray(id, Layout::Aos) => (id, hir::GroupSource::AosStr, None),
            Ty::DictEncoded(id, kf) => (id, hir::GroupSource::Encoded, Some(kf)),
            Ty::Error => return err,
            other => {
                self.diags.error(
                    format!("`group_by` needs a `soa<Struct>`, `array<Struct>`, or `dict_encode`d source, got {}", ty_name(other)),
                    span,
                );
                return err;
            }
        };
        let key_str = matches!(source, hir::GroupSource::AosStr | hir::GroupSource::Encoded | hir::GroupSource::SoaStr);
        // Clone the field table so the field resolution below can also touch `self.diags`.
        let sname = self.structs[id as usize].name.clone();
        let fields = self.structs[id as usize].fields.clone();
        let i64t = Ty::Int(IntTy { bits: 64, signed: true });
        // Diagnostic label for the physical source (a soa str-key is still a `soa`, not an `array`).
        let src_kind = match source {
            hir::GroupSource::SoaI64 | hir::GroupSource::SoaStr => "soa",
            hir::GroupSource::AosStr | hir::GroupSource::Encoded => "array",
        };
        if key_str {
            // str key (dictionary-encoded, inline or precomputed) + i64 value, `sum`/`min`/`max`/
            // `count`. `count` reads no value column; the others require an i64 value field.
            let Some(ki) = self.resolve_group_field(&sname, &fields, src_kind, key_field, "key", Ty::Str) else { return err };
            // For an already-encoded source, the group key must be the field the dictionary was built
            // on — otherwise the precomputed ids don't correspond to this key.
            if let Some(kf) = enc_key
                && ki != kf {
                    self.diags.error(
                        format!("`group_by` on a `dict_encode`d value must use the encoded key '{}', not '{}'", fields[kf as usize].name, key_field.name),
                        key_field.span,
                    );
                    return err;
                }
            let vi = match value_field {
                Some(v) => match self.resolve_group_field(&sname, &fields, src_kind, v, "value", i64t) {
                    Some(idx) => Some(idx),
                    None => return err,
                },
                None => None, // `count` — no value field
            };
            // Result: `(array<str>, array<i64>)` — distinct keys (views borrowing `base`), per-key aggregate.
            let karr = ty_to_scalar(Ty::DynArray(Scalar::Str)).expect("array<str> is a payload scalar");
            let varr = ty_to_scalar(Ty::DynArray(ty_to_scalar(i64t).unwrap())).expect("array<i64> is a payload scalar");
            let tuple_id = intern_tuple(self.tuples, vec![karr, varr]);
            return Expr {
                kind: ExprKind::ArrayGroupAgg { base, struct_id: id, key_field: ki, value_field: vi, op, source },
                ty: Ty::Tuple(tuple_id),
                span,
            };
        }

        // i64 key + i64 value (soa source).
        let Some(ki) = self.resolve_group_field(&sname, &fields, src_kind, key_field, "key", i64t) else { return err };
        let vi = match value_field {
            Some(v) => match self.resolve_group_field(&sname, &fields, src_kind, v, "value", i64t) {
                Some(idx) => Some(idx),
                None => return err,
            },
            None => None,
        };
        // Result: a tuple of two owned arrays `(array<i64>, array<i64>)` (keys, per-key aggregate).
        let arr = ty_to_scalar(Ty::DynArray(ty_to_scalar(i64t).unwrap())).expect("array<i64> is a payload scalar");
        let tuple_id = intern_tuple(self.tuples, vec![arr, arr]);
        Expr {
            kind: ExprKind::ArrayGroupAgg { base, struct_id: id, key_field: ki, value_field: vi, op, source },
            ty: Ty::Tuple(tuple_id),
            span,
        }
    }

    /// `s.group_by(.key).agg(sum(.a), max(.b), count(), …)` — the fused multi-aggregate terminal. Each
    /// argument is one aggregate, written as a call `sum(.field)` / `min(.field)` / `max(.field)` /
    /// `count()` (parsed as an ordinary call whose `.field` argument is a `FieldShorthand`; this method
    /// interprets it). Computes all K aggregates in one pass over the key column, yielding a tuple
    /// `(array<key>, array<i64>, …)` — distinct keys + one column per aggregate. First cut: the AoS
    /// `str` key (the idiomatic-fast-Rust `HashMap<&str,[i64;K]>` shape).
    fn check_group_agg_multi(&mut self, source: &ast::Expr, key_field: &ast::Ident, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.is_empty() {
            self.diags.error("`group_by(.key).agg(...)` needs at least one aggregate, e.g. `.agg(sum(.a), max(.b))`".to_string(), span);
            return err;
        }
        // Interpret each argument as an aggregate spec: a call `sum(.f)`/`min(.f)`/`max(.f)`/`count()`.
        // Returns `(op, value-field ident)` per aggregate; the field is resolved against the struct below.
        let mut specs: Vec<(hir::GroupOp, Option<&ast::Ident>)> = Vec::with_capacity(args.len());
        for a in args {
            let ast::ExprKind::Call { callee, args: cargs } = &a.kind else {
                self.diags.error("each `agg(...)` argument must be an aggregate call, e.g. `sum(.value)` or `count()`".to_string(), a.span);
                return err;
            };
            let ast::ExprKind::Path(p) = &callee.kind else {
                self.diags.error("an `agg(...)` aggregate must be `sum`/`min`/`max`/`count`".to_string(), a.span);
                return err;
            };
            let [name] = p.segments.as_slice() else {
                self.diags.error("an `agg(...)` aggregate must be `sum`/`min`/`max`/`count`".to_string(), a.span);
                return err;
            };
            let spec = match name.name.as_str() {
                "count" => {
                    if !cargs.is_empty() {
                        self.diags.error("`count()` in `agg(...)` takes no arguments".to_string(), a.span);
                        return err;
                    }
                    (hir::GroupOp::Count, None)
                }
                m @ ("sum" | "min" | "max") => {
                    let [ast::Expr { kind: ast::ExprKind::FieldShorthand(v), .. }] = cargs.as_slice() else {
                        self.diags.error(format!("`{m}(.value)` in `agg(...)` needs a `.field` value argument"), a.span);
                        return err;
                    };
                    let op = match m {
                        "sum" => hir::GroupOp::Sum,
                        "min" => hir::GroupOp::Min,
                        _ => hir::GroupOp::Max,
                    };
                    (op, Some(v))
                }
                other => {
                    self.diags.error(format!("`agg(...)` supports `sum`/`min`/`max(.value)` or `count()`, not `{other}(...)`"), a.span);
                    return err;
                }
            };
            specs.push(spec);
        }

        let base = match self.place_local(source) {
            Some((id, _)) => id,
            None => {
                self.diags.error("`group_by` source must be a `soa<Struct>` or `array<Struct>` local".to_string(), span);
                return err;
            }
        };
        // First cut: the AoS `str` key (the one-pass `HashMap<&str,[i64;K]>` shape that matches fast
        // Rust). i64-key soa / precomputed dict_encoded multi-aggregate are deferred follow-ups.
        let id = match self.locals[base as usize].ty {
            Ty::DynStructArray(id, Layout::Aos) => id,
            Ty::Error => return err,
            other => {
                self.diags.error(
                    format!("fused `group_by(.key).agg(...)` first cut needs an AoS `array<Struct>` with a `str` key, got {}", ty_name(other)),
                    span,
                );
                return err;
            }
        };
        let sname = self.structs[id as usize].name.clone();
        let fields = self.structs[id as usize].fields.clone();
        let i64t = Ty::Int(IntTy { bits: 64, signed: true });
        let Some(ki) = self.resolve_group_field(&sname, &fields, "array", key_field, "key", Ty::Str) else { return err };
        let mut aggs: Vec<hir::GroupAgg1> = Vec::with_capacity(specs.len());
        for (op, vf) in specs {
            let value_field = match vf {
                Some(v) => match self.resolve_group_field(&sname, &fields, "array", v, "value", i64t) {
                    Some(idx) => Some(idx),
                    None => return err,
                },
                None => None, // count — no value field
            };
            aggs.push(hir::GroupAgg1 { op, value_field });
        }
        // Result tuple: `(array<str>, array<i64> × K)` — distinct keys + one column per aggregate.
        let karr = ty_to_scalar(Ty::DynArray(Scalar::Str)).expect("array<str> is a payload scalar");
        let varr = ty_to_scalar(Ty::DynArray(ty_to_scalar(i64t).unwrap())).expect("array<i64> is a payload scalar");
        let mut elems = vec![karr];
        elems.extend(std::iter::repeat_n(varr, aggs.len()));
        let tuple_id = intern_tuple(self.tuples, elems);
        Expr {
            kind: ExprKind::ArrayGroupAggMulti { base, struct_id: id, key_field: ki, aggs, source: hir::GroupSource::AosStr },
            ty: Ty::Tuple(tuple_id),
            span,
        }
    }

    /// `s.dict_encode(.key)` — intern the AoS `array<Struct>` local `s`'s `str` `key` column to a
    /// dense-id column + dictionary, yielding a `dict_encoded<Struct>` value (the A2 reuse rail). The
    /// source must be an AoS `array<Struct>` local and `.key` a `str` field; the encoded value borrows
    /// `s` (region-tied) and is consumed by a later `e.group_by(.key).<agg>(.value)`.
    fn check_dict_encode(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [ast::Expr { kind: ast::ExprKind::FieldShorthand(key), .. }] = args else {
            self.diags.error("`dict_encode(.key)` needs a single `.field` key argument".to_string(), span);
            return err;
        };
        let base = match self.place_local(recv) {
            Some((id, _)) => id,
            None => {
                self.diags.error("`dict_encode` source must be an `array<Struct>` local".to_string(), span);
                return err;
            }
        };
        let id = match self.locals[base as usize].ty {
            Ty::DynStructArray(id, Layout::Aos) => id,
            Ty::Error => return err,
            other => {
                self.diags.error(format!("`dict_encode` needs an `array<Struct>` source, got {}", ty_name(other)), span);
                return err;
            }
        };
        let sname = self.structs[id as usize].name.clone();
        let fields = &self.structs[id as usize].fields;
        let Some(ki) = fields.iter().position(|f| f.name == key.name) else {
            self.diags.error(format!("no field '{}' on array<{sname}>", key.name), key.span);
            return err;
        };
        if fields[ki].ty != Ty::Str {
            self.diags.error(
                format!("`dict_encode` key '{}' must be str (first cut), got {}", key.name, ty_name(fields[ki].ty)),
                key.span,
            );
            return err;
        }
        Expr {
            kind: ExprKind::ArrayDictEncode { base, struct_id: id, key_field: ki as u32 },
            ty: Ty::DictEncoded(id, ki as u32),
            span,
        }
    }

    /// `source.….min()` / `.max()` — the smallest / largest surviving (numeric scalar)
    /// element, as the element type. Like `sum`, it takes no arguments and an empty pipeline
    /// yields the fold identity (the type's extreme value).
    fn check_array_min_max(&mut self, recv: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, is_max: bool, span: Span) -> Expr {
        let name = if is_max { "max" } else { "min" };
        if !args.is_empty() {
            self.diags.error(format!("'{name}' takes no arguments"), span);
        }
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Some((source, stages, elem)) = self.check_pipeline(recv, expected, span) else {
            return err;
        };
        if !elem.is_numeric() {
            self.diags
                .error(format!("'{name}' needs a numeric element type, got {}", ty_name(elem)), span);
            return err;
        }
        self.constrain(elem, expected, span);
        Expr { kind: ExprKind::ArrayMinMax { source: Box::new(source), stages, is_max }, ty: elem, span }
    }

    /// `source.….count()` — the count of elements surviving the stages, as an `i64`. The
    /// element type is unconstrained (a struct element needs no projection), unlike `sum`.
    fn check_array_count(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'count' takes no arguments".to_string(), span);
        }
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Some((source, stages, _elem)) = self.check_pipeline(recv, None, span) else {
            return err;
        };
        Expr {
            kind: ExprKind::ArrayCount { source: Box::new(source), stages },
            ty: Ty::Int(IntTy { bits: 64, signed: true }),
            span,
        }
    }

    /// `src.….to_array()` — materialize the surviving (scalar) elements into an *owned*
    /// `array<T>`. MMv2 slice 3: the result is arena-bump-allocated (bulk-freed), so it is
    /// only allowed inside an `arena {}`; free-standing (heap + drop) arrives in slice 4.
    fn check_array_to_array(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'to_array' takes no arguments".to_string(), span);
        }
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // Inside an arena → bump-allocated (bulk-freed). Outside → free-standing heap with a
        // per-binding drop (MMv2 slice 4). Both are fine now.
        let Some((source, stages, elem)) = self.check_pipeline(recv, None, span) else {
            return err;
        };
        let Some(scalar) = ty_to_scalar(elem) else {
            self.diags.error(
                format!("'to_array' needs a scalar element, got {} (project a field first)", ty_name(elem)),
                span,
            );
            return err;
        };
        if matches!(elem, Ty::Struct(_)) {
            self.diags.error("'to_array' over struct elements is not supported yet (project a field first)".to_string(), span);
            return err;
        }
        Expr {
            kind: ExprKind::ArrayToArray { source: Box::new(source), stages, elem },
            ty: Ty::DynArray(scalar),
            span,
        }
    }

    /// `src.….map_into(dst)` — a materializing terminal that writes each post-stage element into a
    /// caller-provided writable slice `dst` (`out`/`mut`), instead of allocating a fresh buffer (the
    /// `to_array` sibling that reuses caller storage — draft.md §7's `out` parameter as a terminal).
    ///
    /// v1 cut (the ideal length-preserving form): only length-preserving stages (`map` / `.field`);
    /// `where` is rejected. That keeps the loop a clean `dst[i] = f(src[i])` — the exact vectorizable
    /// shape the scoped-`noalias` metadata targets — and avoids a second return convention (a
    /// filtering write returning a survivor count is a different operation, deferred). The runtime
    /// requires `dst.len() == src.len()` (abort otherwise). Primitive scalar elements only. Yields `()`.
    ///
    /// Alias soundness (the precondition for the `noalias` codegen): `dst` must not share a backing
    /// buffer with the pipeline source. `dst` is always a resolvable named place (a writable slice),
    /// so its root is compared to the source root — same root ⇒ rejected. A source whose provenance
    /// cannot be resolved (a slice returned from a function, a soa column, a struct-field slice) is
    /// rejected too, because it could alias `dst` and we are about to claim they are disjoint. A fixed
    /// stack array literal source has no name root but is provably disjoint from any caller slice, so
    /// it is allowed.
    fn check_array_map_into(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [dst_arg] = args else {
            self.diags.error(
                format!("'map_into' takes 1 argument (a writable `out`/`mut` slice), got {}", args.len()),
                span,
            );
            return err;
        };
        // The destination must be a writable slice place: a `mut` local slice or an `out` parameter.
        let dst = self.check_expr(dst_arg, None);
        if dst.ty == Ty::Error {
            return err;
        }
        let Ty::Slice(dst_es) = self.resolve(dst.ty) else {
            self.diags.error(format!("'map_into' writes into a slice<T>, got {}", ty_name(dst.ty)), dst_arg.span);
            return err;
        };
        let Some((dst_id, _)) = self.place_local(dst_arg) else {
            self.diags.error(
                "'map_into' needs a writable slice place (a `mut` local or an `out` parameter)".to_string(),
                dst_arg.span,
            );
            return err;
        };
        if !self.locals[dst_id as usize].is_mut {
            let name = self.locals[dst_id as usize].name.clone();
            self.diags.error(
                format!("cannot write into immutable '{name}' (declare with `mut`, or use an `out` parameter)"),
                dst_arg.span,
            );
            return err;
        }
        // Type-check the pipeline; a stageless inline literal source infers its element from `dst`.
        let Some((source, stages, elem)) = self.check_pipeline(recv, Some(scalar_to_ty(dst_es)), span) else {
            return err;
        };
        // v1: length-preserving stages only — a filtering `where` (a dynamic survivor count) is deferred.
        if stages.iter().any(|s| matches!(s.kind, StageKind::Where { .. } | StageKind::WhereField { .. })) {
            self.diags.error(
                "'map_into' v1 supports only length-preserving stages (`map` / `.field`); a filtering `where` before `map_into` (which would write a variable prefix and return a count) is deferred".to_string(),
                span,
            );
            return err;
        }
        let Some(scalar) = ty_to_scalar(elem) else {
            self.diags.error(
                format!("'map_into' needs a scalar element, got {} (project a field first)", ty_name(elem)),
                span,
            );
            return err;
        };
        if matches!(elem, Ty::Struct(_)) {
            self.diags.error("'map_into' over struct elements is not supported yet (project a field first)".to_string(), span);
            return err;
        }
        if scalar != dst_es {
            self.diags.error(
                format!("'map_into' element {} does not match the destination slice element {}", scalar_name(scalar), scalar_name(dst_es)),
                span,
            );
            return err;
        }
        // Alias soundness: `dst` must not alias the pipeline source (the precondition for the
        // scoped-`noalias` metadata this lowers to). `dst_root` is the destination's root buffer.
        // Both roots must resolve to a *known* backing buffer — a slice/array parameter (distinct by
        // the caller's `out` no-alias contract) or a real array local. A slice `let`-bound to a value
        // of unknown origin (a fn-returned slice, a soa column, a struct-field slice) has itself as
        // its own root and would falsely read as "distinct", so it is rejected: it could alias the
        // other buffer, and we are about to claim they are disjoint.
        let dst_root = self.root_local(dst_id);
        if !self.slice_root_is_known(dst_root) {
            self.diags.error(
                "'map_into' destination is a view of unknown origin (a fn-returned slice, a soa column, a struct field); its buffer cannot be proven distinct from the source — use a named array/slice or an `out` parameter (deferred)".to_string(),
                dst_arg.span,
            );
            return err;
        }
        match self.expr_root_local(&source) {
            Some(sr) if sr == dst_root => {
                let name = self.locals[dst_root as usize].name.clone();
                self.diags.error(
                    format!("'map_into' destination aliases the pipeline source '{name}' — the output slice must be a distinct buffer"),
                    dst_arg.span,
                );
                return err;
            }
            Some(sr) if !self.slice_root_is_known(sr) => {
                // The source root is a slice local of unknown origin (bound to a fn return / soa
                // column / struct field) — it could alias `dst`, so it cannot back a `noalias` claim.
                self.diags.error(
                    "'map_into' source is a view of unknown origin (a fn-returned slice, a soa column, a struct field); its buffer cannot be proven distinct from the `out` buffer, so it is rejected (deferred)".to_string(),
                    span,
                );
                return err;
            }
            // Two distinct *known* backing buffers → provably disjoint. Two slice parameters are
            // guaranteed distinct by the caller's `out` no-alias check; a param vs. an array local,
            // or two distinct array locals, never share storage.
            Some(_) => {}
            None => {
                // No name root. Sound only if the source is provably fresh/stack storage (a fixed
                // array literal) — never a view of unknown origin that could alias `dst`.
                if !matches!(source.ty, Ty::Array(..) | Ty::StructArray(..)) {
                    self.diags.error(
                        "'map_into' needs a source whose buffer is known — a named array/slice (or a sub-slice of one) or an array literal. A slice of unknown origin (returned from a function, a soa column, a struct field) could alias the `out` buffer, so it is rejected (deferred)".to_string(),
                        span,
                    );
                    return err;
                }
            }
        }
        Expr {
            kind: ExprKind::ArrayMapInto { source: Box::new(source), stages, dst: Box::new(dst), elem },
            ty: Ty::Unit,
            span,
        }
    }

    /// `arr.to_soa()` — transpose an AoS `array<Struct>` into a column-major `soa<Struct>`. The
    /// construction primitive that makes `soa<T>` usable in pure Align (it was parameter-only). The
    /// buffer is arena-bump-allocated, so this requires an `arena {}` and the view is region-tied to
    /// it (escape is checked like any arena value). First cut: a pure transpose — no `where`/`map`
    /// stages and the struct's fields must all be primitive scalars (the `soa<T>` field rule).
    fn check_array_to_soa(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if !args.is_empty() {
            self.diags.error("'to_soa' takes no arguments".to_string(), span);
        }
        // A pipeline with no stages: `elem` is the source's whole element type. `to_soa` keeps the
        // whole struct (it transposes every field), so stages (`where`/`map`/`.field`) are rejected.
        let Some((source, stages, elem)) = self.check_pipeline(recv, None, span) else {
            return err;
        };
        if !stages.is_empty() {
            self.diags.error(
                "'to_soa' is a transpose of the whole struct array — pipeline stages (where/map/.field) before it are not supported yet".to_string(),
                span,
            );
            return err;
        }
        let Ty::Struct(id) = elem else {
            self.diags.error(
                format!("'to_soa' needs an array of structs, got an array of {}", ty_name(elem)),
                span,
            );
            return err;
        };
        // The struct must satisfy the `soa<T>` field rule (primitive scalars and/or `str`) — the same
        // check `resolve_type` makes for the `soa<Struct>` type. A `str` column transposes the source
        // elements' `str` views into a view column (16-byte `{ptr,len}`), so a str-bearing soa is
        // region-tied to the source array (see the `ArrayToSoa` arm of `region_of`).
        let fields = &self.structs[id as usize].fields;
        if fields.is_empty() || !fields.iter().all(|f| matches!(f.ty, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Str)) {
            self.diags.error(
                format!("'to_soa' needs a non-empty struct of primitive-scalar or `str` fields (no nested/owned fields), '{}' has other fields", self.structs[id as usize].name),
                span,
            );
            return err;
        }
        // The buffer is arena-bump-allocated (a borrowed Copy view, no owned-soa type / per-value
        // drop yet), so it requires an enclosing `arena {}`.
        if self.arena_depth == 0 {
            self.diags.error(
                "'to_soa' allocates its column buffer in an arena — call it inside an `arena {}`".to_string(),
                span,
            );
            return err;
        }
        Expr {
            kind: ExprKind::ArrayToSoa { source: Box::new(source), struct_id: id },
            ty: Ty::Soa(id),
            span,
        }
    }

    /// `source.….partition(p)` — split the surviving (scalar) elements into two owned arrays:
    /// those satisfying the predicate `p`, then the rest. Yields a tuple `(array<T>, array<T>)`,
    /// filled by one fused loop. The element must be a primitive scalar (the `array<T>` payload).
    fn check_array_partition(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [fn_arg] = args else {
            self.diags.error(
                format!("'partition' takes 1 argument (a predicate function), got {}", args.len()),
                span,
            );
            return err;
        };
        let elem_hint = self.named_param_hint(fn_arg, 0);
        let Some((source, stages, elem)) = self.check_pipeline(recv, elem_hint, span) else {
            return err;
        };
        if matches!(elem, Ty::Struct(_)) {
            self.diags.error(
                "'partition' over struct elements is not supported yet (project a field first)".to_string(),
                span,
            );
            return err;
        }
        // The predicate has type `(elem) -> bool` (named or lambda).
        let Some((func, _, captures)) = self.resolve_fn(fn_arg, &[elem], Some(Ty::Bool), "partition", span) else {
            return err;
        };
        // The element must materialize into `array<T>`, i.e. be a primitive scalar.
        let prim_ok = ty_to_scalar(elem).and_then(scalar_to_prim).is_some();
        if !prim_ok {
            self.diags.error(
                format!("'partition' element must be a primitive scalar (int/float/bool/char), got {}", ty_name(elem)),
                span,
            );
            return err;
        }
        // Result: a tuple of two owned arrays `(array<T>, array<T>)`.
        let arr = ty_to_scalar(Ty::DynArray(ty_to_scalar(elem).unwrap())).expect("array<prim> is a payload scalar");
        let tuple_id = intern_tuple(self.tuples, vec![arr, arr]);
        Expr {
            kind: ExprKind::ArrayPartition { source: Box::new(source), stages, func, captures, elem },
            ty: Ty::Tuple(tuple_id),
            span,
        }
    }

    /// `arr.chunks(n)` — split an array/slice of a primitive scalar into length-`n` sub-slices
    /// (the last may be shorter), yielding an owned `array<slice<T>>` whose elements borrow `arr`.
    /// The result is region-tied to `arr` (the chunk slices view its storage).
    fn check_array_chunks(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [n_arg] = args else {
            self.diags.error(format!("'chunks' takes 1 argument (the chunk size), got {}", args.len()), span);
            return err;
        };
        let n = self.check_expr(n_arg, Some(Ty::Int(IntTy { bits: 64, signed: true })));
        if n.ty == Ty::Error {
            return err;
        }
        if !n.ty.is_int_like() {
            self.diags.error(format!("'chunks' size must be an integer, got {}", ty_name(n.ty)), n_arg.span);
            return err;
        }
        let src = self.check_expr(recv, None);
        let elem_scalar = match src.ty {
            Ty::Array(s, _) | Ty::Slice(s) | Ty::DynArray(s) => s,
            Ty::Error => return err,
            other => {
                self.diags.error(
                    format!("'chunks' needs an array or slice, got {}", ty_name(other)),
                    span,
                );
                return err;
            }
        };
        let Some(prim) = scalar_to_prim(elem_scalar) else {
            self.diags.error(
                format!("'chunks' element must be a primitive scalar (int/float/bool/char), got {}", ty_name(scalar_to_ty(elem_scalar))),
                span,
            );
            return err;
        };
        // A fixed stack array source must be a literal or a named local (slot-addressable, like a
        // pipeline source) so MIR can take its buffer address; a `{ptr,len}` view is fine as a value.
        if matches!(src.ty, Ty::Array(..)) && !matches!(src.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
            self.diags.error(
                "'chunks' over a stack array must start from an array literal or a variable".to_string(),
                span,
            );
            return err;
        }
        Expr {
            kind: ExprKind::ArrayChunks { source: Box::new(src), n: Box::new(n), elem: scalar_to_ty(prim_to_scalar(prim)) },
            ty: Ty::DynSliceArray(prim),
            span,
        }
    }

    /// `source.….par_map(f)` — apply the Pure function `f` to each surviving element and
    /// materialize the results into an owned `array<R>`. `f` must be Pure (checked later, over the
    /// whole call graph) and return a primitive scalar. The first cut runs sequentially.
    fn check_array_par_map(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [fn_arg] = args else {
            self.diags.error(
                format!("'par_map' takes 1 argument (a function), got {}", args.len()),
                span,
            );
            return err;
        };
        let elem_hint = self.named_param_hint(fn_arg, 0);
        let Some((source, stages, elem)) = self.check_pipeline(recv, elem_hint, span) else {
            return err;
        };
        // `f: (elem) -> R` (named or lambda); `R` is inferred.
        let Some((func, r, captures)) = self.resolve_fn(fn_arg, &[elem], None, "par_map", span) else {
            return err;
        };
        if r == Ty::Error {
            return err;
        }
        // The result must materialize into `array<R>`, i.e. be a primitive scalar.
        let Some(scalar) = ty_to_scalar(r).filter(|s| scalar_to_prim(*s).is_some()) else {
            self.diags.error(
                format!("'par_map' result must be a primitive scalar (int/float/bool/char), got {}", ty_name(r)),
                span,
            );
            return err;
        };
        Expr {
            kind: ExprKind::ArrayParMap { source: Box::new(source), stages, func, captures, elem: r },
            ty: Ty::DynArray(scalar),
            span,
        }
    }

    /// `src.….any(p)` / `.all(p)` — whether predicate `p: E -> bool` holds for any / all
    /// surviving elements. The element must be a scalar (project a struct field first), so
    /// the fused loop has a concrete value to test. Always returns `bool`.
    fn check_array_any_all(&mut self, recv: &ast::Expr, args: &[ast::Expr], all: bool, span: Span) -> Expr {
        let name = if all { "all" } else { "any" };
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [fn_arg] = args else {
            self.diags
                .error(format!("'{name}' takes 1 argument (a predicate function), got {}", args.len()), span);
            return err;
        };
        // The predicate's parameter type guides an inline source's element type (named only).
        let elem_hint = self.named_param_hint(fn_arg, 0);
        let Some((source, stages, elem)) = self.check_pipeline(recv, elem_hint, span) else {
            return err;
        };
        if ty_to_scalar(elem).is_none() {
            self.diags.error(
                format!("'{name}' needs a scalar element, got {} (project a field first)", ty_name(elem)),
                span,
            );
            return err;
        }
        // Predicate must be `(elem) -> bool` (named or lambda).
        let Some((func, _, captures)) = self.resolve_fn(fn_arg, &[elem], Some(Ty::Bool), name, span) else {
            return err;
        };
        Expr {
            kind: ExprKind::ArrayAnyAll { source: Box::new(source), stages, func, captures, all },
            ty: Ty::Bool,
            span,
        }
    }

    /// `src.…​.reduce(init, f)` — fold the post-stage elements with `f: (A, E) -> A`,
    /// starting from `init: A`.
    fn check_array_reduce(&mut self, recv: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [init_arg, fn_arg] = args else {
            self.diags.error(format!("'reduce' takes 2 arguments (an initial value and a function), got {}", args.len()), span);
            return err;
        };
        // The accumulator type + element hint: a named fold fixes both from its signature; a
        // lambda infers the accumulator from the initial value (and the element from the source).
        let named_sig = self.named_sig(fn_arg);
        let (acc_ty, elem_hint, init) = match &named_sig {
            Some(sig) => (sig.ret, sig.params.get(1).copied(), self.check_expr(init_arg, Some(sig.ret))),
            None => {
                let init = self.check_expr(init_arg, expected);
                (self.finalize(init.ty), None, init)
            }
        };
        let Some((source, stages, elem)) = self.check_pipeline(recv, elem_hint, span) else {
            return err;
        };
        // A failed initial value leaves `acc_ty == Ty::Error`; bail before resolving the function
        // so it doesn't cascade into the lambda body / signature check (matching `scan`).
        if acc_ty == Ty::Error {
            return err;
        }
        // `f: (acc, elem) -> acc` (named or lambda).
        let Some((func, _, captures)) = self.resolve_fn(fn_arg, &[acc_ty, elem], Some(acc_ty), "reduce", span) else {
            return err;
        };
        self.constrain(acc_ty, expected, span);
        Expr {
            kind: ExprKind::ArrayReduce { source: Box::new(source), stages, func, captures, init: Box::new(init) },
            ty: acc_ty,
            span,
        }
    }

    /// `source.….scan(init, f)` — a materializing prefix fold: emit the running accumulator
    /// after each surviving element, yielding an owned `array<A>`. `f: (A, E) -> A`, `init: A`.
    fn check_array_scan(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [init_arg, fn_arg] = args else {
            self.diags.error(format!("'scan' takes 2 arguments (an initial value and a function), got {}", args.len()), span);
            return err;
        };
        // Accumulator type + element hint: a named fold fixes both from its signature; a lambda
        // infers the accumulator from the initial value (and the element from the source).
        let named_sig = self.named_sig(fn_arg);
        let (acc_ty, elem_hint, init) = match &named_sig {
            Some(sig) => (sig.ret, sig.params.get(1).copied(), self.check_expr(init_arg, Some(sig.ret))),
            None => {
                let init = self.check_expr(init_arg, None);
                (self.finalize(init.ty), None, init)
            }
        };
        let Some((source, stages, elem)) = self.check_pipeline(recv, elem_hint, span) else {
            return err;
        };
        // A failed initial value leaves `acc_ty == Ty::Error`; bail before the scalar check so it
        // doesn't cascade into a confusing "accumulator must be a scalar" diagnostic (matching reduce).
        if acc_ty == Ty::Error {
            return err;
        }
        // A struct element must be projected to a scalar first (the fused loop has no scalar
        // value loaded for a struct array, like `map`/`to_array`).
        if matches!(elem, Ty::Struct(_)) {
            self.diags.error(
                "'scan' over struct elements is not supported yet (project a field first)".to_string(),
                span,
            );
            return err;
        }
        // The accumulator (output element) must be a *primitive* scalar to materialize into
        // `array<A>`. `ty_to_scalar` accepts `Ty::Struct` (a valid Option/Result payload), but
        // the buffer/PtrStore path has no struct-element support, so reject structs explicitly.
        if matches!(acc_ty, Ty::Struct(_)) {
            self.diags.error(
                "'scan' accumulator must be a primitive scalar (struct accumulators are not supported yet)".to_string(),
                span,
            );
            return err;
        }
        let Some(scalar) = ty_to_scalar(acc_ty) else {
            self.diags.error(
                format!("'scan' accumulator must be a scalar to materialize, got {}", ty_name(acc_ty)),
                span,
            );
            return err;
        };
        // `f: (acc, elem) -> acc` (named or lambda).
        let Some((func, _, captures)) = self.resolve_fn(fn_arg, &[acc_ty, elem], Some(acc_ty), "scan", span) else {
            return err;
        };
        Expr {
            kind: ExprKind::ArrayScan { source: Box::new(source), stages, func, captures, init: Box::new(init), elem: acc_ty },
            ty: Ty::DynArray(scalar),
            span,
        }
    }

    /// `source.….sort()` — materialize the surviving elements into an owned `array<T>` and sort
    /// them ascending. First cut: numeric scalar elements only (an ordering exists), no
    /// comparator argument (a `sort(cmp)` overload is a follow-up).
    fn check_array_sort(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'sort' takes no arguments yet (a comparator overload is a follow-up)".to_string(), span);
        }
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Some((source, stages, elem)) = self.check_pipeline(recv, None, span) else {
            return err;
        };
        if matches!(elem, Ty::Struct(_)) {
            self.diags.error("'sort' over struct elements is not supported yet (project a field first)".to_string(), span);
            return err;
        }
        if !elem.is_numeric() {
            self.diags.error(format!("'sort' needs a numeric element type, got {}", ty_name(elem)), span);
            return err;
        }
        let Some(scalar) = ty_to_scalar(elem) else {
            self.diags.error(format!("'sort' needs a scalar element, got {}", ty_name(elem)), span);
            return err;
        };
        Expr {
            kind: ExprKind::ArraySort { source: Box::new(source), stages, elem },
            ty: Ty::DynArray(scalar),
            span,
        }
    }

    /// `source.….sort_by_key(f)` — materialize the surviving (primitive scalar) elements and sort
    /// them ascending by `f(element)`. Unlike `sort`, the element need not be numeric (it is ordered
    /// by the key); the key `f` must return an orderable scalar (int/float/char). `f` may be a named
    /// function or a lambda (which may capture).
    fn check_array_sort_by_key(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [fn_arg] = args else {
            self.diags.error(format!("'sort_by_key' takes 1 argument (a key function), got {}", args.len()), span);
            return err;
        };
        let elem_hint = self.named_param_hint(fn_arg, 0);
        let Some((source, stages, elem)) = self.check_pipeline(recv, elem_hint, span) else {
            return err;
        };
        if matches!(elem, Ty::Struct(_)) {
            self.diags.error("'sort_by_key' over struct elements is not supported yet (project a field first)".to_string(), span);
            return err;
        }
        // The element must materialize into `array<T>`, i.e. be a primitive scalar.
        let Some(scalar) = ty_to_scalar(elem).filter(|s| scalar_to_prim(*s).is_some()) else {
            self.diags.error(
                format!("'sort_by_key' element must be a primitive scalar (int/float/bool/char), got {}", ty_name(elem)),
                span,
            );
            return err;
        };
        // The key function `f: (elem) -> K`; `K` must be an orderable scalar.
        let Some((key_func, key_ty, captures)) = self.resolve_fn(fn_arg, &[elem], None, "sort_by_key", span) else {
            return err;
        };
        if key_ty == Ty::Error {
            return err;
        }
        if !(key_ty.is_numeric() || key_ty == Ty::Char) {
            self.diags.error(
                format!("'sort_by_key' key must be an orderable scalar (int/float/char), got {}", ty_name(key_ty)),
                span,
            );
            return err;
        }
        Expr {
            kind: ExprKind::ArraySortBy { source: Box::new(source), stages, key_func, captures, key_ty, elem },
            ty: Ty::DynArray(scalar),
            span,
        }
    }

    /// `a.dot(b)` — the inner product `Σ a[i]*b[i]`. First cut: both operands must be
    /// fixed-length arrays of the same numeric scalar element and the same statically known
    /// length (the SIMD/vector case; `slice`/`array<T>` dot with runtime lengths is a follow-up).
    fn check_array_dot(&mut self, recv: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [b_arg] = args else {
            self.diags.error(format!("'dot' takes 1 argument (another array), got {}", args.len()), span);
            return err;
        };
        // The receiver must be a bare fixed array — no pipeline stages on the left yet.
        let Some((a_src, stages, elem)) = self.check_pipeline(recv, expected, span) else {
            return err;
        };
        if !stages.is_empty() {
            self.diags.error("'dot' does not support map/where stages yet".to_string(), span);
            return err;
        }
        let na = match a_src.ty {
            Ty::Array(_, n) => n,
            Ty::Error => return err,
            other => {
                self.diags.error(
                    format!("'dot' needs a fixed-length array on the left, got {} (slice/array<T> dot is not supported yet)", ty_name(other)),
                    span,
                );
                return err;
            }
        };
        if !elem.is_numeric() {
            self.diags.error(format!("'dot' needs a numeric element type, got {}", ty_name(elem)), span);
            return err;
        }
        // No type hint for `b`: passing `a`'s full array type would make a length mismatch
        // produce a duplicate "array[m] vs array[n]" error on top of the clearer one below.
        // The element-type and length checks here cover correctness.
        let b = self.check_expr(b_arg, None);
        // MIR materializes both operands via `array_source_slot`, which only handles a literal
        // or a local (the M4 restriction). Reject an arbitrary array expression (an `if`, a
        // call, a block, …) here so it cannot reach lowering and panic — mirrors `check_pipeline`'s
        // restriction on the left operand.
        if !matches!(b.ty, Ty::Error) && !matches!(b.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
            self.diags.error(
                "the right operand of 'dot' must be an array literal or a variable (an arbitrary array expression is not supported yet)".to_string(),
                b.span,
            );
            return err;
        }
        let (nb, b_elem) = match b.ty {
            Ty::Array(s, n) => (n, scalar_to_ty(s)),
            Ty::Error => return err,
            other => {
                self.diags.error(
                    format!("'dot' needs a fixed-length array on the right, got {}", ty_name(other)),
                    b.span,
                );
                return err;
            }
        };
        if b_elem != elem {
            self.diags.error(
                format!("'dot' operands must have the same element type, got {} and {}", ty_name(elem), ty_name(b_elem)),
                b.span,
            );
            return err;
        }
        if na != nb {
            self.diags.error(
                format!("'dot' operands must have the same length, got {na} and {nb}"),
                b.span,
            );
            return err;
        }
        self.constrain(elem, expected, span);
        Expr { kind: ExprKind::ArrayDot { a: Box::new(a_src), b: Box::new(b), elem }, ty: elem, span }
    }

    /// `r.map_err(f)` — convert a `Result<T, E>`'s error with `f: fn(E) -> E'`, yielding
    /// `Result<T, E'>` (`Ok` passes through). The explicit, visible way to change a result's error
    /// type — Align has no implicit `?` conversion (that would be a hidden coercion).
    fn check_map_err(&mut self, recv: Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Ty::Result(ok, e) = self.resolve(recv.ty) else {
            return err; // guarded by the caller
        };
        if args.len() != 1 {
            self.diags.error(format!("'map_err' takes 1 argument, got {}", args.len()), span);
            return err;
        }
        let f = self.check_expr(&args[0], None);
        let Ty::Fn(fid) = self.resolve(f.ty) else {
            if f.ty != Ty::Error {
                self.diags.error(format!("'map_err' expects a function `fn(E) -> E'`, got {}", ty_name(f.ty)), args[0].span);
            }
            return err;
        };
        let (params, e2) = {
            let ft = &self.fn_types[fid as usize];
            (ft.params.clone(), ft.ret)
        };
        if params.as_slice() != [e] {
            self.diags.error(
                format!("'map_err' function must take the error type {} (got {})", scalar_name(e), ty_name(f.ty)),
                args[0].span,
            );
            return err;
        }
        let ty = Ty::Result(ok, e2);
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::ResultMapErr { result: Box::new(recv), f: Box::new(f) }, ty, span }
    }

    /// `b.clone()` — deep-copy a `box<T>`. Allocates a fresh box, so it needs an arena.
    fn check_box_clone(&mut self, recv: Expr, recv_ty: Ty, args: &[ast::Expr], span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'clone' takes no arguments".to_string(), span);
        }
        match recv_ty {
            Ty::Box(s) => {
                if self.arena_depth == 0 {
                    self.diags
                        .error("clone allocates; it must be used inside an `arena {}` block".to_string(), span);
                }
                Expr { kind: ExprKind::BoxClone(Box::new(recv)), ty: Ty::Box(s), span }
            }
            // `str.clone()` deep-copies into a free-standing heap-owned `string` (MMv2 slice 7).
            // Unlike `box.clone`, it needs no arena: the result owns its buffer and is `Drop`-freed,
            // so it can outlive any region — this is how a zero-copy view escapes. (Arena-bump
            // cloning, the in-arena optimization, is a later sub-slice.)
            Ty::Str | Ty::String => Expr { kind: ExprKind::StrClone(Box::new(recv)), ty: Ty::String, span },
            Ty::Error => Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span },
            other => {
                self.diags
                    .error(format!("'.clone()' is available on box<T>, str, and string, got {}", ty_name(other)), span);
                Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span }
            }
        }
    }

    /// `s.contains(n)` / `s.starts_with(p)` / `s.ends_with(s)` / `s.find(n)` — byte-oriented `str`
    /// scans (`core.string`, draft.md §18). The receiver (`recv`, already a `str`/`string`) and the
    /// single argument are both treated as `str` views: an owned `string` is auto-borrowed
    /// (`StrBorrow`), so neither operand is moved — the scan only reads bytes. The predicates yield
    /// `bool`; `find` yields `Option<i64>` (the first byte index, `None` if absent).
    fn check_str_predicate(&mut self, recv: Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // Borrow an owned `string` receiver as a `str` view; a `str` receiver passes through.
        let haystack = if recv.ty == Ty::String {
            let rspan = recv.span;
            Expr { kind: ExprKind::StrBorrow(Box::new(recv)), ty: Ty::Str, span: rspan }
        } else {
            recv
        };
        if args.len() != 1 {
            self.diags
                .error(format!("'.{method}()' takes exactly one str argument"), span);
            return err;
        }
        // The needle: a `str`, or an owned `string` auto-borrowed; anything else is constrained to str.
        let needle = self.check_str_init(&args[0]);
        if haystack.ty == Ty::Error || needle.ty == Ty::Error {
            return err;
        }
        let opt_i64 = Ty::Option(Scalar::Int(IntTy { bits: 64, signed: true }));
        let (kind, ty) = match method {
            "contains" => (hir::StrPredKind::Contains, Ty::Bool),
            "starts_with" => (hir::StrPredKind::StartsWith, Ty::Bool),
            "ends_with" => (hir::StrPredKind::EndsWith, Ty::Bool),
            "eq_ignore_ascii_case" => (hir::StrPredKind::EqIgnoreCase, Ty::Bool),
            "find" => (hir::StrPredKind::Find, opt_i64),
            "rfind" => (hir::StrPredKind::Rfind, opt_i64),
            _ => unreachable!("check_str_predicate called with non-scan method"),
        };
        Expr {
            kind: ExprKind::StrPredicate { kind, haystack: Box::new(haystack), needle: Box::new(needle) },
            ty,
            span,
        }
    }

    /// `s.trim()` / `s.trim_start()` / `s.trim_end()` — strip ASCII whitespace, yielding a borrowed
    /// sub-`str` of the receiver (`core.string`, draft.md §12). An owned `string` receiver is
    /// auto-borrowed (`StrBorrow`); the result views the same bytes, so it inherits the receiver's
    /// region (see `region_of`) and cannot escape it.
    fn check_str_trim(&mut self, recv: Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if !args.is_empty() {
            self.diags.error(format!("'.{method}()' takes no arguments"), span);
            return err;
        }
        // Borrow an owned `string` receiver as a `str` view; a `str` receiver passes through.
        let recv = if recv.ty == Ty::String {
            let rspan = recv.span;
            Expr { kind: ExprKind::StrBorrow(Box::new(recv)), ty: Ty::Str, span: rspan }
        } else {
            recv
        };
        if recv.ty == Ty::Error {
            return err;
        }
        let kind = match method {
            "trim" => hir::StrTrimKind::Both,
            "trim_start" => hir::StrTrimKind::Start,
            "trim_end" => hir::StrTrimKind::End,
            _ => unreachable!("check_str_trim called with non-trim method"),
        };
        Expr { kind: ExprKind::StrTrim { kind, recv: Box::new(recv) }, ty: Ty::Str, span }
    }

    /// `builder()` — open an append-oriented string builder (MMv2 slice 7c, draft.md §12).
    fn check_builder_new(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        // `builder()` (default capacity) or `builder(n)` (pre-size the backing buffer to `n` bytes
        // so appends don't reallocate as it grows). `n` is an `i64`.
        let capacity = match args {
            [] => None,
            [cap] => {
                let c = self.check_expr(cap, Some(Ty::Int(IntTy { bits: 64, signed: true })));
                if c.ty.is_int_like() {
                    Some(Box::new(c))
                } else {
                    if c.ty != Ty::Error {
                        self.diags.error(format!("'builder' capacity must be an integer, got {}", ty_name(c.ty)), cap.span);
                    }
                    // Drop the ill-typed capacity so MIR/codegen never see a non-i64 operand.
                    None
                }
            }
            _ => {
                self.diags.error(format!("'builder' takes an optional capacity (0 or 1 argument), got {}", args.len()), span);
                None
            }
        };
        Expr { kind: ExprKind::BuilderNew { capacity }, ty: Ty::Builder, span }
    }

    /// `buffer(cap)` — open an owned growable byte buffer with read window `cap` bytes (the sink a
    /// `reader.read` fills). `cap` is a required `i64` (a 0-window buffer reads nothing).
    fn check_buffer_new(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [cap] = args else {
            self.diags.error(format!("'buffer' takes a capacity (1 argument, the read window in bytes), got {}", args.len()), span);
            return err;
        };
        let c = self.check_expr(cap, Some(Ty::Int(IntTy { bits: 64, signed: true })));
        if c.ty == Ty::Error {
            return err;
        }
        if !c.ty.is_int_like() {
            self.diags.error(format!("'buffer' capacity must be an integer, got {}", ty_name(c.ty)), cap.span);
            return err;
        }
        Expr { kind: ExprKind::BufferNew { capacity: Box::new(c) }, ty: Ty::Buffer, span }
    }

    /// `b.write(s)` / `b.write_int(n)` / `b.write_bool(v)` / `b.write_char(c)` /
    /// `b.write_float(x)` — append to a builder (MMv2 slice 7c/7d). The builder is borrowed
    /// (mutated through its handle, not consumed). Each writer takes the matching scalar; `write`
    /// takes a `str` (a `string` borrows as one — zero-cost, non-consuming, reuses the slice-7b
    /// borrow, so `b.write(owned_string)` keeps it usable). `write_int` widens to `i64` at codegen,
    /// like `print`; `write_float` accepts `f32`/`f64` (codegen picks the runtime fn by width).
    fn check_builder_write(&mut self, recv_expr: Expr, args: &[ast::Expr], kind: BuilderWriteKind, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let mname = builder_write_method_name(kind);
        if recv_expr.ty != Ty::Builder {
            if recv_expr.ty != Ty::Error {
                self.diags
                    .error(format!("'.{mname}()' is a builder method, got {}", ty_name(recv_expr.ty)), span);
            }
            return err;
        }
        if args.len() != 1 {
            self.diags
                .error(format!("'.{mname}()' takes 1 argument, got {}", args.len()), span);
            return err;
        }
        let mut arg = self.check_expr(&args[0], None);
        if arg.ty == Ty::Error {
            return err;
        }
        // `write` accepts a `str`; a `string` borrows as one (zero-cost, non-consuming — reuses
        // the slice-7b borrow), so `b.write(owned_string)` keeps `owned_string` usable.
        if kind == BuilderWriteKind::Str && arg.ty == Ty::String {
            let s = arg.span;
            arg = Expr { kind: ExprKind::StrBorrow(Box::new(arg)), ty: Ty::Str, span: s };
        }
        let (ok, want) = match kind {
            BuilderWriteKind::Str => (arg.ty == Ty::Str, "a str"),
            BuilderWriteKind::Int => (matches!(arg.ty, Ty::Int(_) | Ty::IntVar(_)), "an integer"),
            BuilderWriteKind::Bool => (arg.ty == Ty::Bool, "a bool"),
            BuilderWriteKind::Char => (arg.ty == Ty::Char, "a char"),
            BuilderWriteKind::Float => (matches!(arg.ty, Ty::Float(_) | Ty::FloatVar(_)), "a float"),
        };
        if !ok {
            self.diags
                .error(format!("'.{mname}()' expects {want}, got {}", ty_name(arg.ty)), arg.span);
            return err;
        }
        Expr {
            kind: ExprKind::BuilderWrite { builder: Box::new(recv_expr), arg: Box::new(arg), kind },
            ty: Ty::Unit,
            span,
        }
    }

    /// `b.to_string()` — finish a builder into an **owned** `string`, consuming (moving) it.
    fn check_builder_to_string(&mut self, recv_expr: Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if recv_expr.ty != Ty::Builder {
            if recv_expr.ty != Ty::Error {
                self.diags
                    .error(format!("'.to_string()' is a builder method, got {}", ty_name(recv_expr.ty)), span);
            }
            return err;
        }
        if !args.is_empty() {
            self.diags
                .error(format!("'.to_string()' takes no arguments, got {}", args.len()), span);
        }
        Expr { kind: ExprKind::BuilderToString(Box::new(recv_expr)), ty: Ty::String, span }
    }

    /// `heap.new(x)` — allocate `box<T>` in the enclosing arena. M3 requires an arena.
    fn check_heap_new(&mut self, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        if self.arena_depth == 0 {
            self.diags
                .error("heap.new must be used inside an `arena {}` block".to_string(), span);
        }
        if args.len() != 1 {
            self.diags
                .error(format!("'heap.new' takes 1 argument, got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let inner_expected = match expected {
            Some(Ty::Box(s)) => Some(scalar_to_ty(s)),
            _ => None,
        };
        let arg = self.check_expr(&args[0], inner_expected);
        // A box payload must be a true (owned) *primitive* scalar. Resolve the payload scalar
        // first, then reject the non-primitive ones at the scalar level so every shape is caught
        // consistently — including an *un-annotated* `heap.new(move_value)` (the `box<…>`
        // annotation path is guarded in `resolve_type`, but inference here must reject the same
        // set or codegen's `scalar_bytes` hits `unreachable!`): a Move scalar (`string`/`array`),
        // a `Struct` (codegen can't size a struct box), or a `str` view (not boxable).
        let scalar = self.payload_scalar(arg.ty, args[0].span);
        let reject = match scalar {
            _ if scalar.is_move() => Some(format!("an owned `{}` cannot be boxed", scalar_name(scalar))),
            Scalar::Struct(_) => Some("struct boxes are not supported".to_string()),
            Scalar::Enum(_) => Some("sum-type boxes are not supported".to_string()),
            Scalar::Str => Some("a `str` view is not boxable".to_string()),
            _ => None,
        };
        if let Some(why) = reject {
            self.diags
                .error(format!("a box payload must be a primitive scalar ({why})"), args[0].span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        Expr { kind: ExprKind::HeapNew(Box::new(arg)), ty: Ty::Box(scalar), span }
    }

    /// Whether a value of this type may be stored to / loaded from `raw` memory: a primitive scalar
    /// (int/float/bool/char) or a `layout(C)` struct (which alone promises a stable flat byte
    /// layout). A non-`layout(C)` struct has a compiler-private layout, so it is not raw-storable.
    fn is_raw_storable(&self, ty: Ty) -> bool {
        is_raw_scalar(ty)
            || matches!(ty, Ty::Struct(id) if self.structs.get(id as usize).is_some_and(|s| s.c_repr))
    }

    /// `raw.alloc(size)` / `raw.free(p)` — the unsafe raw-pointer ops (draft.md §6.5). Valid only
    /// inside an `unsafe {}` block. `alloc` takes an integer byte size and yields a `raw` pointer;
    /// `free` takes a `raw` pointer and yields unit. The memory is manually managed (no auto-drop),
    /// which is exactly why these are confined to `unsafe`.
    fn check_raw_op(&mut self, method: &str, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if self.unsafe_depth == 0 {
            self.diags.error(
                format!("'raw.{method}' is an unsafe operation — use it inside an `unsafe {{}}` block"),
                span,
            );
        }
        let nargs = match method {
            "alloc" | "free" => 1,
            "load" | "offset" => 2,
            "store" => 3,
            _ => unreachable!("check_raw_op is only dispatched for alloc/free/load/store/offset"),
        };
        if args.len() != nargs {
            self.diags.error(format!("'raw.{method}' takes {nargs} argument(s), got {}", args.len()), span);
            return err;
        }
        let i64t = Ty::Int(IntTy { bits: 64, signed: true });
        // Helper: check a `raw`-pointer argument.
        match method {
            "alloc" => {
                // `size` is a byte count — an integer (an unconstrained literal defaults to i64, and
                // codegen widens any narrower width to the i64 runtime signature).
                let size = self.check_expr(&args[0], Some(i64t));
                if !matches!(size.ty, Ty::Int(_) | Ty::IntVar(_) | Ty::Error) {
                    self.diags.error(format!("'raw.alloc' size must be an integer, got {}", ty_name(size.ty)), args[0].span);
                    return err;
                }
                Expr { kind: ExprKind::RawAlloc(Box::new(size)), ty: Ty::Raw, span }
            }
            "free" => {
                let p = self.check_expr(&args[0], Some(Ty::Raw));
                if !matches!(p.ty, Ty::Raw | Ty::Error) {
                    self.diags.error(format!("'raw.free' takes a `raw` pointer, got {}", ty_name(p.ty)), args[0].span);
                    return err;
                }
                Expr { kind: ExprKind::RawFree(Box::new(p)), ty: Ty::Unit, span }
            }
            // `raw.load(p, offset)` reads a value at byte `offset` from `p`. The value type is inferred
            // from the expected type (no turbofish — like `json.decode`); it must be a primitive
            // scalar (int/float/bool/char), the only thing raw memory holds soundly in this first cut.
            "load" => {
                let p = self.check_expr(&args[0], Some(Ty::Raw));
                let off = self.check_expr(&args[1], Some(i64t));
                self.check_raw_ptr_offset(&p, &off, "load", args[0].span, args[1].span);
                let Some(ty) = expected.filter(|t| self.is_raw_storable(*t)) else {
                    self.diags.error(
                        "'raw.load' needs a primitive-scalar or `layout(C)` struct result type — annotate it, e.g. `x: i64 := raw.load(p, 0)`".to_string(),
                        span,
                    );
                    return err;
                };
                let scalar = ty_to_scalar(ty).expect("a raw-storable type has a Scalar");
                Expr { kind: ExprKind::RawLoad { ptr: Box::new(p), offset: Box::new(off), scalar }, ty, span }
            }
            // `raw.store(p, offset, v)` writes `v` at byte `offset`. The stored type is `v`'s type; it
            // must be a primitive scalar.
            "store" => {
                let p = self.check_expr(&args[0], Some(Ty::Raw));
                let off = self.check_expr(&args[1], Some(i64t));
                self.check_raw_ptr_offset(&p, &off, "store", args[0].span, args[1].span);
                let v = self.check_expr(&args[2], None);
                // Accept a still-unresolved int/float literal (it defaults to i64/f64, which codegen
                // reads after finalization) or a `layout(C)` struct; reject everything else (a
                // non-`layout(C)` struct, str, slice, …).
                let store_ok = self.is_raw_storable(v.ty) || matches!(v.ty, Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Error);
                if !store_ok {
                    self.diags.error(
                        format!("'raw.store' stores a primitive scalar or a `layout(C)` struct, got {}", ty_name(v.ty)),
                        args[2].span,
                    );
                    return err;
                }
                Expr { kind: ExprKind::RawStore { ptr: Box::new(p), offset: Box::new(off), value: Box::new(v) }, ty: Ty::Unit, span }
            }
            // `raw.offset(p, n)` advances a `raw` pointer by `n` bytes, yielding a new `raw` pointer
            // (unsafe pointer arithmetic — for stepping through a buffer or passing an interior
            // pointer). Distinct from `load`/`store`'s inline offset: this returns the pointer itself.
            "offset" => {
                let p = self.check_expr(&args[0], Some(Ty::Raw));
                let n = self.check_expr(&args[1], Some(i64t));
                self.check_raw_ptr_offset(&p, &n, "offset", args[0].span, args[1].span);
                Expr { kind: ExprKind::RawOffset { ptr: Box::new(p), offset: Box::new(n) }, ty: Ty::Raw, span }
            }
            _ => unreachable!("check_raw_op is only dispatched for alloc/free/load/store/offset"),
        }
    }

    /// Type-check a `raw` pointer + i64 offset pair for `raw.load`/`raw.store`, emitting a diagnostic
    /// on a mismatch (but not bailing — the caller still produces a typed node).
    fn check_raw_ptr_offset(&mut self, p: &Expr, off: &Expr, method: &str, pspan: Span, ospan: Span) {
        if !matches!(p.ty, Ty::Raw | Ty::Error) {
            self.diags.error(format!("'raw.{method}' takes a `raw` pointer, got {}", ty_name(p.ty)), pspan);
        }
        if !matches!(off.ty, Ty::Int(_) | Ty::IntVar(_) | Ty::Error) {
            self.diags.error(format!("'raw.{method}' offset must be an integer, got {}", ty_name(off.ty)), ospan);
        }
    }

    /// `json.encode(s)` — encode a flat struct into a JSON object `str`. Desugars to the
    /// string-builder `template` machinery: static JSON syntax interleaved with per-field
    /// value holes (`str` fields are emitted as JSON-escaped string literals). M5: fields
    /// must be int/float/bool/str; nested structs/arrays/options are not supported yet. The
    /// result is arena-backed when inside an `arena {}` (else leaked), like any built string.
    fn check_json_encode(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 1 {
            self.diags
                .error(format!("'json.encode' expects 1 argument, got {}", args.len()), span);
            return err;
        }
        let Some((base, ty)) = self.place_local(&args[0]) else {
            self.diags
                .error("'json.encode' expects a struct or struct-array value (a local binding)".to_string(), args[0].span);
            return err;
        };
        let mut parts = vec![];
        let mut ok = true;
        match ty {
            // A single struct → a JSON object.
            Ty::Struct(sid) => {
                self.json_object_parts(base, sid, None, &mut parts, args[0].span, &mut ok);
            }
            // A fixed struct-array → a JSON array of objects (unrolled; length is static).
            Ty::StructArray(sid, n) => {
                parts.push(TemplatePart::Text("[".to_string()));
                for i in 0..n {
                    if i > 0 {
                        parts.push(TemplatePart::Text(",".to_string()));
                    }
                    self.json_object_parts(base, sid, Some(i), &mut parts, args[0].span, &mut ok);
                }
                parts.push(TemplatePart::Text("]".to_string()));
            }
            _ => {
                self.diags
                    .error(format!("'json.encode' expects a struct or struct-array, got {}", ty_name(ty)), args[0].span);
                return err;
            }
        }
        // An unsupported field left a `"name":` with no value part: return the error
        // sentinel rather than a malformed template (matches the other checks' convention).
        if !ok {
            return err;
        }
        // `json.encode` desugars to a `Template` `str` — the same arena-allocating path, so it leaks
        // the same way inside a lifted lambda with no arena.
        self.guard_lambda_alloc_leak("json.encode", span);
        Expr { kind: ExprKind::Template(parts), ty: Ty::Str, span }
    }

    /// `json.decode(input)` — parse a `str` into a struct at runtime, yielding
    /// `Result<Struct, Error>`. The target struct `T` is taken from the expected type
    /// (a `Result<T, _>`, e.g. from `let u: T := json.decode(d)?` — the type flows back
    /// through `?`). There is deliberately no `<T>` call syntax: Align has no
    /// expression-position type-argument form (no turbofish — `open-questions.md` Settled,
    /// `impl/02-frontend.md` §8); the annotate-the-binding error below is the fallback when
    /// context gives no type. M5 cut: a flat struct of `i64`/`i32`/`bool`/`str` fields
    /// (float/nested later).
    fn check_json_decode(&mut self, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 1 {
            self.diags
                .error(format!("'json.decode' expects 1 argument, got {}", args.len()), span);
            return err;
        }
        // The decode target is the Ok type of the expected `Result<T, _>`.
        let sid = match expected.map(|e| self.resolve(e)) {
            Some(Ty::Result(Scalar::Struct(id), _)) => id,
            // `array<T>` target (MMv2 slice 8c): parse a JSON array of scalars into an *owned*
            // `array<T>` (elements copied → `Static`/returnable, not region-tied to the input).
            Some(Ty::Result(Scalar::DynArray(prim), _)) => {
                let elem = scalar_to_ty(prim_to_scalar(prim));
                // The element must be runtime-parseable. A `str` element would be a zero-copy
                // view region-tied to the input (deferred — needs the array to carry that region).
                if !matches!(elem, Ty::Int(_) | Ty::Float(_) | Ty::Bool) {
                    self.diags.error(
                        format!("'json.decode' into array<{}> is not supported yet (int/float/bool elements only)", ty_name(elem)),
                        span,
                    );
                    return err;
                }
                // `check_str_init` accepts a `str` or auto-borrows an owned `string` (the result
                // is copied, so the input's region does not constrain it), and reports a mismatch.
                let input = self.check_str_init(&args[0]);
                return Expr {
                    kind: ExprKind::JsonDecodeArray { elem, input: Box::new(input) },
                    ty: Ty::Result(Scalar::DynArray(prim), Scalar::Enum(self.error_enum_id)),
                    span,
                };
            }
            // `array<Struct>` target (MMv2 slice 8d, the draft.md §19 headline): parse a JSON
            // array of objects into an owned, dynamic AoS. Each element decodes like the single
            // struct path; `str` fields are zero-copy views into the input, so the whole array is
            // region-tied to that input (see `region_of`) and cannot escape it.
            Some(Ty::Result(Scalar::DynStructArray(id), _)) => {
                if !self.decode_struct_fields_ok(id, span) {
                    return err;
                }
                // The input region bounds the result (its `str` fields borrow the input), so use
                // `check_str_init` — a borrowed owned `string`'s region then bounds the array.
                let input = self.check_str_init(&args[0]);
                return Expr {
                    kind: ExprKind::JsonDecodeStructArray { struct_id: id, input: Box::new(input) },
                    ty: Ty::Result(Scalar::DynStructArray(id), Scalar::Enum(self.error_enum_id)),
                    span,
                };
            }
            // `soa<Struct>` target (the cache-optimal decode, #228): parse the JSON array of objects
            // **directly** into a column-major `soa<Struct>` — a structural count pass discovers N,
            // then values are written straight into their columns (no AoS intermediate, no
            // transpose). The column buffer is arena-allocated, so this needs an `arena {}`. Fields
            // are primitive scalars or `str`: a `str` column holds zero-copy views into the JSON
            // input, so a str-bearing soa is region-tied to BOTH the arena and the input (see
            // `region_of` / `struct_has_str`); a primitive-only soa is self-contained (arena-only).
            Some(Ty::Result(Scalar::Soa(id), _)) => {
                let fields = &self.structs[id as usize].fields;
                if fields.is_empty() || !fields.iter().all(|f| matches!(f.ty, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Str)) {
                    self.diags.error(
                        format!("'json.decode' into soa<{}> needs a non-empty struct of primitive-scalar or `str` fields (no nested/owned columns)", self.structs[id as usize].name),
                        span,
                    );
                    return err;
                }
                if self.arena_depth == 0 {
                    self.diags.error(
                        "'json.decode' into a soa allocates its column buffer in an arena — decode inside an `arena {}`".to_string(),
                        span,
                    );
                    return err;
                }
                let input = self.check_str_init(&args[0]);
                return Expr {
                    kind: ExprKind::JsonDecodeSoa { struct_id: id, input: Box::new(input) },
                    ty: Ty::Result(Scalar::Soa(id), Scalar::Enum(self.error_enum_id)),
                    span,
                };
            }
            _ => {
                self.diags.error(
                    "cannot infer the decode target type; annotate the binding, e.g. `u: T := json.decode(d)?`".to_string(),
                    span,
                );
                return err;
            }
        };
        if !self.decode_struct_fields_ok(sid, span) {
            return err;
        }
        // The decoded struct's `str` fields are zero-copy views into the input, so the input's
        // region constrains the result (see `region_of`). `check_str_init` accepts a `str` or
        // auto-borrows an owned `string` (whose region then bounds the decoded value).
        let input = self.check_str_init(&args[0]);
        Expr {
            kind: ExprKind::JsonDecode { struct_id: sid, input: Box::new(input) },
            ty: Ty::Result(Scalar::Struct(sid), Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// Validate that struct `sid`'s fields are all `json.decode`-able (int / float / bool, or a
    /// `str` zero-copy view into the input). Reports the first offending field and returns false.
    /// Shared by the single-struct and `array<Struct>` decode paths (MMv2 slice 8d).
    fn decode_struct_fields_ok(&mut self, sid: u32, span: Span) -> bool {
        let fields = self.structs[sid as usize].fields.clone();
        for f in &fields {
            if !matches!(f.ty, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Str) {
                self.diags.error(
                    format!("'json.decode' field '{}' has type {} (only int/float/bool/str decode for now)", f.name, ty_name(f.ty)),
                    span,
                );
                return false;
            }
        }
        true
    }

    /// Emit the `{"field":value,...}` template parts for one struct value: either the struct
    /// local `base` itself (`elem` = None) or element `elem` of the struct-array local `base`.
    /// Sets `*ok = false` (and reports) on a field type `json.encode` can't render yet.
    fn json_object_parts(
        &mut self,
        base: LocalId,
        sid: u32,
        elem: Option<u32>,
        parts: &mut Vec<TemplatePart>,
        span: Span,
        ok: &mut bool,
    ) {
        // `self.structs` is a `&'a [StructDef]`, so this borrow is tied to `'a`, not `self`
        // — `self.diags` stays mutably borrowable in the loop (no clone needed).
        let fields = &self.structs[sid as usize].fields;
        parts.push(TemplatePart::Text("{".to_string()));
        for (i, f) in fields.iter().enumerate() {
            let sep = if i == 0 { "" } else { "," };
            parts.push(TemplatePart::Text(format!("{sep}\"{}\":", f.name)));
            let kind = match elem {
                None => ExprKind::Field { root: base, path: vec![i as u32] },
                Some(e) => ExprKind::IndexField { base, index: e, field: i as u32 },
            };
            let field_expr = Expr { kind, ty: f.ty, span };
            match f.ty {
                Ty::Str => parts.push(TemplatePart::JsonStr(field_expr)),
                t if t.is_numeric() || t == Ty::Bool => parts.push(TemplatePart::Hole(field_expr)),
                _ => {
                    self.diags.error(
                        format!(
                            "'json.encode' field '{}' has unsupported type {} (int/float/bool/str only for now)",
                            f.name,
                            ty_name(f.ty)
                        ),
                        span,
                    );
                    *ok = false;
                }
            }
        }
        parts.push(TemplatePart::Text("}".to_string()));
    }

    /// `.len()` — the element count of a `str`, `slice<T>`, or fixed array, as an `i64`.
    fn check_len(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        if !args.is_empty() {
            self.diags.error(format!("'.len()' takes no arguments, got {}", args.len()), span);
        }
        let r = self.check_expr(recv, None);
        match r.ty {
            // `str`/`slice`/`soa` carry a runtime length in their `{ ptr, len }` view (a `soa`'s
            // length is its row count).
            Ty::Str | Ty::String | Ty::Slice(_) | Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::DynSliceArray(_) | Ty::Soa(_) => Expr { kind: ExprKind::Len(Box::new(r)), ty: i64_ty, span },
            // A `buffer`'s length is its current byte count (the last read's size). Same v1
            // bound-receiver restriction as `.bytes()` (uniform across buffer methods, until Move
            // temporaries drop): reject `buffer(n).len()` on an unbound temporary.
            Ty::Buffer => {
                if !matches!(r.kind, ExprKind::Local(_)) {
                    self.diags.error(
                        "bind the buffer to a local first, then call the method (`b := buffer(n)` then `b.len()`) — a temporary buffer handle is not dropped yet".to_string(),
                        span,
                    );
                    return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
                }
                Expr { kind: ExprKind::BufferLen { buffer: Box::new(r) }, ty: i64_ty, span }
            }
            // A fixed array's length is known at compile time.
            Ty::Array(_, n) | Ty::StructArray(_, n) => Expr { kind: ExprKind::Int(n as i128), ty: i64_ty, span },
            Ty::Error => Expr { kind: ExprKind::Int(0), ty: Ty::Error, span },
            other => {
                self.diags
                    .error(format!("'.len()' is not defined on {}", ty_name(other)), span);
                Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span }
            }
        }
    }

    /// `recv[index]` — element access. M5/MMv2 cut: a scalar `array`/`slice`/owned `array<T>`
    /// (the element is a scalar, copied out); the bounds check + abort is emitted in MIR. Indexing
    /// a struct array (whole-element load) and `str` byte indexing are deferred.
    fn check_index(&mut self, recv: &ast::Expr, index: &ast::Expr, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let r = self.check_expr(recv, None);
        // `v[i]` on a vector reads lane `i`. The lane must be a **constant** literal in `0..N` (a SIMD
        // lane index is a fixed position; a dynamic lane would risk an out-of-range poison value). It
        // lowers to `extractelement`, reusing this `Index` node (MIR branches on the receiver type).
        if let Ty::Vec(s, n) = r.ty {
            let lane = match &index.kind {
                ast::ExprKind::Int(v) if *v >= 0 && (*v as u128) < n as u128 => *v,
                _ => {
                    self.diags.error(format!("a vector lane index must be a constant in 0..{n}"), index.span);
                    return err;
                }
            };
            let idx = Expr { kind: ExprKind::Int(lane), ty: Ty::Int(IntTy { bits: 64, signed: true }), span: index.span };
            return Expr { kind: ExprKind::Index { recv: Box::new(r), index: Box::new(idx) }, ty: scalar_to_ty(s), span };
        }
        // The index is an `i64` (matching `.len()` and loop counters). A non-integer index must
        // bail with `Ty::Error` — returning a typed `Index` node with a bad index would feed a
        // non-int operand into the MIR bounds-check `icmp` and panic codegen.
        let i = self.check_expr(index, Some(Ty::Int(IntTy { bits: 64, signed: true })));
        if i.ty == Ty::Error {
            return err;
        }
        if !i.ty.is_int_like() {
            self.diags.error(format!("an array index must be an integer, got {}", ty_name(i.ty)), index.span);
            return err;
        }
        let elem = match r.ty {
            Ty::Array(s, _) | Ty::Slice(s) | Ty::DynArray(s) => scalar_to_ty(s),
            // Indexing an `array<slice<T>>` (a `chunks` result) yields one chunk `slice<T>`.
            Ty::DynSliceArray(p) => Ty::Slice(prim_to_scalar(p)),
            // Indexing a struct array yields the whole struct by value (a copy). A plain-data struct
            // is Copy (primitive / `str` fields), so the copy transfers no ownership; if it holds
            // `str` views, the value is region-tied to the array (handled by `region_of`, which
            // inherits the receiver's region for an `Index`). A Move struct is rejected just below.
            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => Ty::Struct(id),
            // `s[i]` on a `soa<Struct>` gathers a whole struct value from the columns at index `i`
            // (M6). A primitive-only struct is copied field-by-field into a free Copy value
            // (`Static`); a struct with a `str` column gathers `str` views borrowing the soa's
            // buffer/input, so `region_of` ties the gathered value to the soa's region.
            Ty::Soa(id) => Ty::Struct(id),
            Ty::Error => return err,
            other => {
                self.diags.error(format!("cannot index {} (only array / slice / owned array)", ty_name(other)), span);
                return err;
            }
        };
        // A Move-only element (e.g. `array<string>`, `array<array<T>>`) cannot be indexed yet:
        // the load copies the element's `{ptr,len}` without transferring ownership, so the array
        // and the copy would both free the same buffer (double-free). Such element reads need a
        // borrow / move-out design (a later slice) — reject cleanly until then.
        if matches!(elem, Ty::Box(_) | Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::String | Ty::Builder | Ty::Reader | Ty::Writer | Ty::Buffer | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child)
            || payload_is_move(elem)
            || matches!(elem, Ty::Struct(id) if struct_is_move(id, self.structs))
        {
            self.diags.error(
                format!("indexing an array of the Move type {} is not supported yet (it would copy the element without transferring ownership)", ty_name(elem)),
                span,
            );
            return err;
        }
        // A slot-backed fixed array must be a literal or a variable (same restriction as a
        // pipeline source — MIR addresses it through a slot). A `{ptr,len}` view is fine as a value.
        if matches!(r.ty, Ty::Array(..) | Ty::StructArray(..)) && !matches!(r.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
            self.diags.error(
                "indexing a fixed array requires an array literal or a variable (an arbitrary array expression is not supported yet)".to_string(),
                span,
            );
            return err;
        }
        Expr { kind: ExprKind::Index { recv: Box::new(r), index: Box::new(i) }, ty: elem, span }
    }

    /// `recv[start..end]` — a half-open range slice of a `str` / `array<T>` / `slice<T>`. Yields a
    /// borrowed view (a sub-`str`, or a `slice<T>`) into the receiver's storage, region-inherited
    /// from `recv` (see `region_of`) so it cannot outlive it. `start`/`end` (each an `i64`) default
    /// to `0` / the receiver's length; bounds (`0 <= start <= end <= len`) are checked at runtime.
    fn check_slice_range(&mut self, recv: &ast::Expr, start: Option<&ast::Expr>, end: Option<&ast::Expr>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let r = self.check_expr(recv, None);
        if r.ty == Ty::Error {
            return err;
        }
        // The result view type: a `str` slices to a `str`; an array / slice of `T` slices to a
        // `slice<T>`. Owned (`string`/`array<T>`) receivers auto-borrow to their view first.
        let (recv_expr, result_ty) = match r.ty {
            Ty::Str => (r, Ty::Str),
            Ty::String => {
                let rspan = r.span;
                (Expr { kind: ExprKind::StrBorrow(Box::new(r)), ty: Ty::Str, span: rspan }, Ty::Str)
            }
            Ty::Slice(s) | Ty::Array(s, _) | Ty::DynArray(s) => {
                // A Move element would let the sub-slice alias an owned buffer the source still
                // frees — same double-free reasoning as `check_index`. Slices are read-only views,
                // so a `slice<scalar>` is fine; reject Move-element collections until a borrow design.
                let elem = scalar_to_ty(s);
                // `Ty::TcpConn` / `Ty::TcpListener` / `Ty::UdpSocket` are defensive parity with
                // `check_index`'s guard above: a `slice<tcp_conn>` / `array<udp_socket>` (etc.) is
                // unconstructible today (each handle is rejected as an array element at construction),
                // so this arm can't currently be reached — kept in sync so a future array-of-handle
                // path can't slip past this guard silently.
                if matches!(elem, Ty::Box(_) | Ty::DynArray(_) | Ty::String | Ty::Builder | Ty::Reader | Ty::Writer | Ty::Buffer | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child) || payload_is_move(elem) {
                    self.diags.error(
                        format!("slicing a collection of the Move type {} is not supported yet", ty_name(elem)),
                        span,
                    );
                    return err;
                }
                (r, Ty::Slice(s))
            }
            other => {
                self.diags.error(
                    format!("cannot slice {} with `[a..b]` (only str / array / slice)", ty_name(other)),
                    span,
                );
                return err;
            }
        };
        // A slot-backed fixed array must be a literal or a variable (same restriction as indexing /
        // pipeline sources — MIR addresses it through its slot to take a base pointer).
        if matches!(recv_expr.ty, Ty::Array(..)) && !matches!(recv_expr.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
            self.diags.error(
                "slicing a fixed array requires an array literal or a variable (an arbitrary array expression is not supported yet)".to_string(),
                span,
            );
            return err;
        }
        // Both bounds are `i64` (like `.len()` and element indices). An omitted bound is filled in
        // at lowering (0 / len), so only present bounds are type-checked here.
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        let check_bound = |this: &mut Self, b: Option<&ast::Expr>| -> Option<Option<Box<Expr>>> {
            match b {
                None => Some(None),
                Some(e) => {
                    let be = this.check_expr(e, Some(i64_ty));
                    if be.ty == Ty::Error {
                        return None;
                    }
                    if !be.ty.is_int_like() {
                        this.diags.error(format!("a slice bound must be an integer, got {}", ty_name(be.ty)), e.span);
                        return None;
                    }
                    Some(Some(Box::new(be)))
                }
            }
        };
        let Some(start_h) = check_bound(self, start) else { return err };
        let Some(end_h) = check_bound(self, end) else { return err };
        Expr {
            kind: ExprKind::SliceRange { recv: Box::new(recv_expr), start: start_h, end: end_h },
            ty: result_ty,
            span,
        }
    }

    /// `fs.read_file(path)` — read the whole file at `path` (a `str`) into a freshly heap-allocated
    /// owned `string`, yielding `Result<string, Error>`. The returned `string` owns its buffer
    /// (freed by the binding's `Drop`); an I/O error is `Err`. The first `std.fs` surface (the
    /// `std.io`/zero-copy work is later) — a builtin, dispatched like `json.decode`.
    fn check_fs_read_file(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        if args.len() != 1 {
            self.diags
                .error(format!("'fs.read_file' expects 1 argument (the path), got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        // The path is a `str` (or an owned `string`, auto-borrowed).
        let path = self.check_str_init(&args[0]);
        Expr {
            kind: ExprKind::FsReadFile { path: Box::new(path) },
            ty: Ty::Result(Scalar::String, Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `fs.open(path)` -> `Result<reader, Error>` / `fs.create(path)` -> `Result<writer, Error>`.
    /// Open (`create` = create/truncate) `path` (a `str`, owned `string` auto-borrowed); the handle
    /// owns its fd (closed on `Drop`). A builtin, dispatched like `fs.read_file`.
    fn check_fs_open_create(&mut self, create: bool, args: &[ast::Expr], span: Span) -> Expr {
        let name = if create { "fs.create" } else { "fs.open" };
        if args.len() != 1 {
            self.diags
                .error(format!("'{name}' expects 1 argument (the path), got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let path = self.check_str_init(&args[0]);
        let (kind, ok) = if create {
            (ExprKind::WriterCreate { path: Box::new(path) }, Scalar::Writer)
        } else {
            (ExprKind::ReaderOpen { path: Box::new(path) }, Scalar::Reader)
        };
        Expr {
            kind,
            ty: Ty::Result(ok, Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `fs.write_file(path, data)` -> `Result<(), Error>`. `data` is a `str` / owned `string`
    /// (auto-borrowed) / `bytes` (`slice<u8>`) / a `builder` — the same three accepted forms as
    /// `writer.write`. A builtin, dispatched like `fs.read_file`.
    fn check_fs_write_file(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 2 {
            self.diags
                .error(format!("'fs.write_file' expects 2 arguments (the path and the data), got {}", args.len()), span);
            return err;
        }
        let path = self.check_str_init(&args[0]);
        let mut data = self.check_expr(&args[1], None);
        if data.ty == Ty::Error {
            return err;
        }
        let result_ty = Ty::Result(Scalar::Unit, Scalar::Enum(self.error_enum_id));
        // A `builder`'s bytes are written directly (no `to_string()` materialization), borrowing it.
        if data.ty == Ty::Builder {
            return Expr {
                kind: ExprKind::FsWriteFile { path: Box::new(path), data: Box::new(data), builder: true },
                ty: result_ty,
                span,
            };
        }
        // An owned `string` borrows to a `str` (zero-cost, non-consuming); a `str` / `bytes`
        // (`slice<u8>`) is written as-is; anything else is a type error (mirrors `writer.write`).
        if data.ty == Ty::String {
            let s = data.span;
            data = Expr { kind: ExprKind::StrBorrow(Box::new(data)), ty: Ty::Str, span: s };
        }
        if data.ty != Ty::Str && data.ty != Ty::Slice(Scalar::Int(IntTy { bits: 8, signed: false })) {
            self.diags
                .error(format!("'fs.write_file' expects a str, bytes (slice<u8>), or builder, got {}", ty_name(data.ty)), args[1].span);
            return err;
        }
        Expr {
            kind: ExprKind::FsWriteFile { path: Box::new(path), data: Box::new(data), builder: false },
            ty: result_ty,
            span,
        }
    }

    /// The single-path `std.fs` ops: `fs.exists(path)` -> `bool` (errors fold to `false`);
    /// `fs.remove(path)` -> `Result<(), Error>`; `fs.read_dir(path)` -> `Result<array<string>, Error>`
    /// (owned strings); `fs.read_file_view(path)` -> `Result<str, Error>` (an mmap view — **requires an
    /// enclosing `arena {}`**, like `heap.new`, its region bound to that arena). Builtins.
    fn check_fs_path_op(&mut self, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        // `read_file_view`'s returned `str` views an mmap that is `munmap`ped at arena end, so it must
        // live inside an arena — checked here, exactly like `heap.new` (same diagnostic shape).
        if method == "read_file_view" && self.arena_depth == 0 {
            self.diags
                .error("fs.read_file_view must be used inside an `arena {}` block (the view is unmapped at arena end)".to_string(), span);
        }
        if args.len() != 1 {
            self.diags
                .error(format!("'fs.{method}' expects 1 argument (the path), got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let path = Box::new(self.check_str_init(&args[0]));
        let err_enum = Scalar::Enum(self.error_enum_id);
        match method {
            "exists" => Expr { kind: ExprKind::FsExists { path }, ty: Ty::Bool, span },
            "remove" => Expr {
                kind: ExprKind::FsRemove { path },
                ty: Ty::Result(Scalar::Unit, err_enum),
                span,
            },
            "read_dir" => Expr {
                kind: ExprKind::FsReadDir { path },
                ty: Ty::Result(Scalar::DynArray(PrimScalar::String), err_enum),
                span,
            },
            "read_file_view" => Expr {
                kind: ExprKind::FsReadFileView { path },
                ty: Ty::Result(Scalar::Str, err_enum),
                span,
            },
            _ => unreachable!("check_fs_path_op is only dispatched for exists/remove/read_dir/read_file_view"),
        }
    }

    /// `dns.resolve(host)` (`std.net`) -> `Result<array<string>, Error>` — resolve `host` to its IP
    /// strings (owned; a **deep**-`Drop` `array<string>`, exactly like `fs.read_dir`). `host` is a
    /// borrowed `str` (never consumed). Impure (a name-resolution syscall). Builtin.
    fn check_dns_resolve(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        if args.len() != 1 {
            self.diags
                .error(format!("'dns.resolve' expects 1 argument (the host), got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let host = Box::new(self.check_str_init(&args[0]));
        let err_enum = Scalar::Enum(self.error_enum_id);
        Expr {
            kind: ExprKind::DnsResolve { host },
            ty: Ty::Result(Scalar::DynArray(PrimScalar::String), err_enum),
            span,
        }
    }

    /// `tcp.connect(host, port)` (`std.net`) -> `Result<tcp_conn, Error>` — resolve `host` and open a
    /// TCP connection to `port`. `host` is a borrowed `str` (never consumed); `port` is an `i64`
    /// (validated to `1..=65535` at runtime → `Error.Invalid` on a bad port, never an abort). The Ok
    /// payload is an owned `tcp_conn` Move handle (`Drop` closes its fd). Impure. Builtin.
    fn check_tcp_connect(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 2 {
            self.diags
                .error(format!("'tcp.connect' expects 2 arguments (the host and port), got {}", args.len()), span);
            return err;
        }
        let host = Box::new(self.check_str_init(&args[0]));
        let port = self.check_expr(&args[1], Some(Ty::Int(IntTy { bits: 64, signed: true })));
        if host.ty == Ty::Error || port.ty == Ty::Error {
            return err;
        }
        // Defensive backstop only: `check_expr` already reconciled the port against the i64
        // hint (a non-i64 integer is a hard "type mismatch" error there), so a non-i64 type
        // cannot actually reach codegen through this path.
        if !port.ty.is_int_like() {
            self.diags
                .error(format!("'tcp.connect' port must be an integer, got {}", ty_name(port.ty)), args[1].span);
            return err;
        }
        Expr {
            kind: ExprKind::TcpConnect { host, port: Box::new(port) },
            ty: Ty::Result(Scalar::TcpConn, Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `tcp.listen(host, port)` (`std.net`) -> `Result<tcp_listener, Error>` — bind a listening TCP
    /// socket to `port` (`SO_REUSEADDR` before `bind`, then `listen`). `host` is a borrowed `str`
    /// (never consumed); an empty host binds the wildcard address (`AI_PASSIVE`). `port` is an `i64`
    /// validated to `1..=65535` at runtime → `Error.Invalid` on a bad port, never an abort. Port `0`
    /// (kernel-assigned) is rejected in v1 — there is no way to read the bound port back yet. The Ok
    /// payload is an owned `tcp_listener` Move handle (`Drop` closes its fd). Impure. Builtin.
    fn check_tcp_listen(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 2 {
            self.diags
                .error(format!("'tcp.listen' expects 2 arguments (the host and port), got {}", args.len()), span);
            return err;
        }
        let host = Box::new(self.check_str_init(&args[0]));
        let port = self.check_expr(&args[1], Some(Ty::Int(IntTy { bits: 64, signed: true })));
        if host.ty == Ty::Error || port.ty == Ty::Error {
            return err;
        }
        // Defensive backstop only: `check_expr` already reconciled the port against the i64 hint (a
        // non-i64 integer is a hard "type mismatch" error there), so a non-i64 type cannot actually
        // reach codegen through this path.
        if !port.ty.is_int_like() {
            self.diags
                .error(format!("'tcp.listen' port must be an integer, got {}", ty_name(port.ty)), args[1].span);
            return err;
        }
        Expr {
            kind: ExprKind::TcpListen { host, port: Box::new(port) },
            ty: Ty::Result(Scalar::TcpListener, Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `udp.bind(host, port)` (`std.net`) -> `Result<udp_socket, Error>` — open a `SOCK_DGRAM` (UDP)
    /// socket bound to `port`. `host` is a borrowed `str` (never consumed); an empty host binds the
    /// wildcard address (`AI_PASSIVE`). `port` is an `i64` validated to `1..=65535` at runtime →
    /// `Error.Invalid` on a bad port, never an abort. Port `0` (kernel-assigned) is rejected in v1 —
    /// there is no way to read the bound port back yet (the `tcp.listen` deferral). The Ok payload is
    /// an owned `udp_socket` Move handle (`Drop` closes its fd). Impure. Builtin.
    fn check_udp_bind(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 2 {
            self.diags
                .error(format!("'udp.bind' expects 2 arguments (the host and port), got {}", args.len()), span);
            return err;
        }
        let host = Box::new(self.check_str_init(&args[0]));
        let port = self.check_expr(&args[1], Some(Ty::Int(IntTy { bits: 64, signed: true })));
        if host.ty == Ty::Error || port.ty == Ty::Error {
            return err;
        }
        // Defensive backstop only: `check_expr` already reconciled the port against the i64 hint (a
        // non-i64 integer is a hard "type mismatch" error there), so a non-i64 type cannot actually
        // reach codegen through this path.
        if !port.ty.is_int_like() {
            self.diags
                .error(format!("'udp.bind' port must be an integer, got {}", ty_name(port.ty)), args[1].span);
            return err;
        }
        Expr {
            kind: ExprKind::UdpBind { host, port: Box::new(port) },
            ty: Ty::Result(Scalar::UdpSocket, Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `u.send_to(data, host, port)` / `u.recv_from(buf)` on a `udp_socket` ([`Ty::UdpSocket`]), the
    /// receiver already evaluated. Both are datagram ops returning `Result<i64, Error>` (a byte
    /// count). The receiver must be a **bound local** (the `check_conn_stream` / `accept` v1 rule — an
    /// unbound owned-socket temporary is not `Drop`ped yet, so its fd would leak).
    ///
    /// `send_to` resolves `host`/`port` per call (`SOCK_DGRAM`) and sends the byte view `data`
    /// (`str` / owned `string` auto-borrowed / `slice<u8>` — `encoding.base64_encode`'s accepted
    /// forms) as one datagram; the count is the bytes sent. `recv_from` blocks for one datagram and
    /// fills `buf` (a `mut buffer`) up to its capacity (overwriting its length, like `reader.read`);
    /// the count is the bytes received. A datagram larger than the buffer is truncated.
    ///
    /// v1 shape note: `recv_from` returns the received **count** only. `net.md` sketches a
    /// `datagram {n, peer}` return, but the peer address is an owned `string` and a `Result` `Ok`
    /// payload must be a single [`Scalar`] (there is no `Scalar::Tuple`, and synthesizing a builtin
    /// Move struct-with-owned-field aggregate would be a magic special-case) — so the ideal `{n, peer}`
    /// shape is **deferred** until first-class builtin-struct returns land, and v1 mirrors
    /// `reader.read`'s clean `Result<i64, Error>` (fill-buffer-return-count) exactly.
    fn check_udp_socket_method(&mut self, recv_expr: Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // v1 bound-receiver gate (mirrors `check_conn_stream` / `check_listener_accept`): the socket
        // must be a bound local — an unbound owned-socket temporary (`udp.bind(...)?.recv_from(b)`) is
        // not `Drop`ped yet, so its fd would leak. Bind it first. Lifted when Move temporaries drop.
        if !matches!(recv_expr.kind, ExprKind::Local(_)) {
            if recv_expr.ty != Ty::Error {
                self.diags.error(
                    "bind the socket to a local first, then call the method (`u := udp.bind(...)?` then `u.send_to(...)`) — a temporary owned socket handle is not dropped yet, so its fd would leak".to_string(),
                    span,
                );
            }
            return err;
        }
        let i64_result = Ty::Result(Scalar::Int(IntTy { bits: 64, signed: true }), Scalar::Enum(self.error_enum_id));
        if method == "recv_from" {
            if args.len() != 1 {
                self.diags.error(format!("'.recv_from()' takes 1 argument (a mut buffer), got {}", args.len()), span);
                return err;
            }
            let buffer = self.check_expr(&args[0], Some(Ty::Buffer));
            if buffer.ty == Ty::Error {
                return err;
            }
            if buffer.ty != Ty::Buffer {
                self.diags.error(format!("'.recv_from()' fills a buffer, got {}", ty_name(buffer.ty)), args[0].span);
                return err;
            }
            return Expr {
                kind: ExprKind::UdpRecvFrom { sock: Box::new(recv_expr), buffer: Box::new(buffer) },
                ty: i64_result,
                span,
            };
        }
        // `send_to(data, host, port)`.
        if args.len() != 3 {
            self.diags.error(format!("'.send_to()' takes 3 arguments (data, host, port), got {}", args.len()), span);
            return err;
        }
        // `data` is a byte view: `str` / owned `string` (auto-borrowed) / `slice<u8>` — the
        // `encoding.base64_encode` accepted forms.
        let mut data = self.check_expr(&args[0], None);
        let u8s = Scalar::Int(IntTy { bits: 8, signed: false });
        let resolved = self.resolve(data.ty);
        let data_ok = match resolved {
            Ty::Str => true,
            Ty::String => {
                let s = data.span;
                data = Expr { kind: ExprKind::StrBorrow(Box::new(data)), ty: Ty::Str, span: s };
                true
            }
            Ty::Slice(el) => el == u8s,
            _ => false,
        };
        if !data_ok {
            if resolved != Ty::Error {
                self.diags
                    .error(format!("'.send_to()' expects data as a str, string, or bytes (slice<u8>), got {}", ty_name(resolved)), args[0].span);
            }
            return err;
        }
        let host = self.check_str_init(&args[1]);
        let port = self.check_expr(&args[2], Some(Ty::Int(IntTy { bits: 64, signed: true })));
        if host.ty == Ty::Error || port.ty == Ty::Error {
            return err;
        }
        if !port.ty.is_int_like() {
            self.diags
                .error(format!("'.send_to()' port must be an integer, got {}", ty_name(port.ty)), args[2].span);
            return err;
        }
        Expr {
            kind: ExprKind::UdpSendTo { sock: Box::new(recv_expr), data: Box::new(data), host: Box::new(host), port: Box::new(port) },
            ty: i64_result,
            span,
        }
    }

    /// `std.path` — `path.join(a, b)` -> owned `string`; `path.base`/`dir`/`ext(p)` -> a zero-copy
    /// `str` **view** of `p` (region inherited from `p`, see `region_of`); `path.normalize(p)` ->
    /// owned `string`. All pure lexical POSIX string ops (no filesystem access). Builtins.
    fn check_path_op(&mut self, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if method == "join" {
            if args.len() != 2 {
                self.diags
                    .error(format!("'path.join' expects 2 arguments (two path fragments), got {}", args.len()), span);
                return err;
            }
            let a = self.check_str_init(&args[0]);
            let b = self.check_str_init(&args[1]);
            return Expr { kind: ExprKind::PathJoin { a: Box::new(a), b: Box::new(b) }, ty: Ty::String, span };
        }
        if args.len() != 1 {
            self.diags
                .error(format!("'path.{method}' expects 1 argument (the path), got {}", args.len()), span);
            return err;
        }
        let path = Box::new(self.check_str_init(&args[0]));
        match method {
            "base" => Expr { kind: ExprKind::PathComponent { kind: hir::PathComponentKind::Base, path }, ty: Ty::Str, span },
            "dir" => Expr { kind: ExprKind::PathComponent { kind: hir::PathComponentKind::Dir, path }, ty: Ty::Str, span },
            "ext" => Expr { kind: ExprKind::PathComponent { kind: hir::PathComponentKind::Ext, path }, ty: Ty::Str, span },
            "normalize" => Expr { kind: ExprKind::PathNormalize { path }, ty: Ty::String, span },
            _ => unreachable!("check_path_op is only dispatched for join/base/dir/ext/normalize"),
        }
    }

    /// `std.env` — `env.get(name)` -> `Option<string>` (owned; the environment is volatile, so the
    /// value is copied out, never a view); `env.set(name, value)` -> `Result<(), Error>`. Builtins.
    fn check_env_op(&mut self, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if method == "get" {
            if args.len() != 1 {
                self.diags
                    .error(format!("'env.get' expects 1 argument (the variable name), got {}", args.len()), span);
                return err;
            }
            let name = self.check_str_init(&args[0]);
            return Expr { kind: ExprKind::EnvGet { name: Box::new(name) }, ty: Ty::Option(Scalar::String), span };
        }
        // `env.set(name, value)`.
        if args.len() != 2 {
            self.diags
                .error(format!("'env.set' expects 2 arguments (the name and value), got {}", args.len()), span);
            return err;
        }
        let name = self.check_str_init(&args[0]);
        let value = self.check_str_init(&args[1]);
        Expr {
            kind: ExprKind::EnvSet { name: Box::new(name), value: Box::new(value) },
            ty: Ty::Result(Scalar::Unit, Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `std.time` — `time.now()` (wall clock) / `time.instant()` (monotonic) -> `i64` nanoseconds;
    /// `time.sleep(ns)` -> `()` (a negative `ns` is a no-op). One `i64`-nanosecond timeline, no
    /// `Duration` type ("one way"). Builtins.
    fn check_time_op(&mut self, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        if method == "sleep" {
            if args.len() != 1 {
                self.diags
                    .error(format!("'time.sleep' expects 1 argument (nanoseconds, i64), got {}", args.len()), span);
                return err;
            }
            let ns = self.check_expr(&args[0], None);
            if ns.ty == Ty::Error {
                return err;
            }
            // `sleep` takes an `i64` nanosecond count (draft §18.2), and the lowered
            // `align_rt_time_sleep` has an `i64` parameter. Require *exactly* `i64` — binding a
            // bare int literal's inference var to it — rather than accept any int-like width: a
            // narrower `i32`/`u8` operand would build a `TimeSleep` node whose value doesn't match
            // the runtime signature (only the compile-aborting mismatch would otherwise catch it).
            match self.resolve(ns.ty) {
                Ty::Int(IntTy { bits: 64, signed: true }) => {}
                Ty::IntVar(_) => self.constrain(ns.ty, Some(i64_ty), args[0].span),
                other => {
                    self.diags.error(
                        format!("'time.sleep' expects a nanosecond count (i64), got {}", ty_name(other)),
                        args[0].span,
                    );
                    return err;
                }
            }
            return Expr { kind: ExprKind::TimeSleep { ns: Box::new(ns) }, ty: Ty::Unit, span };
        }
        // `time.now()` / `time.instant()` — no arguments.
        if !args.is_empty() {
            self.diags
                .error(format!("'time.{method}' takes no arguments, got {}", args.len()), span);
            return err;
        }
        let kind = if method == "now" { ExprKind::TimeNow } else { ExprKind::TimeInstant };
        Expr { kind, ty: i64_ty, span }
    }

    /// `std.process` — `process.exit(code)` / `process.abort()` (draft §18.2). Both terminate the
    /// process and never return:
    /// - `exit(code)` runs the current function's pending cleanup (Drops / arena ends / buffered
    ///   flushes — the same emission a `return` uses) THEN calls libc `exit(code)`. The settled
    ///   cleanup-then-exit semantics (Nothing-hidden: no silently lost buffered output).
    /// - `abort()` is the named escape hatch: immediate `_exit`, NO cleanup.
    ///
    /// There is no `Never` type yet, so both are typed `()` (v1): they lower to a diverging runtime
    /// call, but the type system does not model the divergence — so `process.exit` cannot be the tail
    /// value of a non-unit-returning function (use it as a statement). Recorded in
    /// `docs/impl/std-design/process.md`.
    fn check_process_op(&mut self, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if method == "abort" {
            if !args.is_empty() {
                self.diags
                    .error(format!("'process.abort' takes no arguments, got {}", args.len()), span);
                return err;
            }
            return Expr { kind: ExprKind::ProcessAbort, ty: Ty::Unit, span };
        }
        // `process.exit(code)` — one `i64` exit code (the OS truncates it to the low 8 bits).
        if args.len() != 1 {
            self.diags
                .error(format!("'process.exit' expects 1 argument (exit code, i64), got {}", args.len()), span);
            return err;
        }
        let code = self.check_expr(&args[0], None);
        if code.ty == Ty::Error {
            return err;
        }
        // Require *exactly* `i64` (binding a bare int literal's inference var to it), mirroring
        // `time.sleep`: a narrower operand would build a `ProcessExit` node whose value doesn't match
        // the runtime `align_rt_process_exit(i64)` signature.
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        match self.resolve(code.ty) {
            Ty::Int(IntTy { bits: 64, signed: true }) => {}
            Ty::IntVar(_) => self.constrain(code.ty, Some(i64_ty), args[0].span),
            other => {
                self.diags.error(
                    format!("'process.exit' expects an exit code (i64), got {}", ty_name(other)),
                    args[0].span,
                );
                return err;
            }
        }
        Expr { kind: ExprKind::ProcessExit { code: Box::new(code) }, ty: Ty::Unit, span }
    }

    /// `process.spawn(cmd, args)` (`std.process`) -> `Result<child, Error>` — `fork` + `execvp` a
    /// child process. `cmd` is a borrowed `str` (owned `string` auto-borrowed) — the **lookup path**
    /// passed to `execvp` (resolved via `PATH` when it has no `/`). `args` is a borrowed str-view
    /// collection (`array<str>` — e.g. `main(args)` — or a `slice<str>` of it) that becomes the
    /// child's **full** `argv`, **including `argv[0]`** (P5: the caller supplies the program name, not
    /// the runtime; `cmd` and `args[0]` are independent). Both are borrowed (never consumed). The Ok
    /// payload is an owned `child` Move handle (`Drop` reaps it via a blocking `waitpid`). A `fork`
    /// failure is `Err(errno)`; an `execvp` failure cannot be reported synchronously — the forked child
    /// `_exit(127)`s (the shell convention), so an exec-not-found surfaces later as `wait() == 127`
    /// (`docs/impl/std-design/process.md` P5). Impure. Builtin.
    fn check_process_spawn(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 2 {
            self.diags
                .error(format!("'process.spawn' expects 2 arguments (the command path and the argv `array<str>`), got {}", args.len()), span);
            return err;
        }
        let cmd = Box::new(self.check_str_init(&args[0]));
        let Some(argv) = self.check_argv("process.spawn", &args[1]) else {
            return err;
        };
        if cmd.ty == Ty::Error {
            return err;
        }
        Expr {
            kind: ExprKind::ProcessSpawn { cmd, args: Box::new(argv) },
            ty: Ty::Result(Scalar::Child, Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// Check + coerce a `process.spawn` / `process.exec` argv operand into a borrowed str-view
    /// `{ptr,len}` (`array<str>` / `slice<str>`), the child/new image's **full** argv (incl. argv[0],
    /// P5). Shared by both call sites so the accepted forms + array→slice borrow stay identical. All
    /// forms lower to a `{ptr,len}` of `str` views the runtime marshals into C strings; an interior NUL /
    /// empty argv is rejected at runtime (`Error.Invalid`). Accepts:
    ///   * `array<str>` (`DynArray(Str)`, e.g. `main(args)`) and `slice<str>` (a sub-range of one, so a
    ///     caller can pass part of its own argv) — already `{ptr,len}` values;
    ///   * a fixed-size `array<str, N>` literal/local (the natural `["/bin/echo", "hi"]` call) — borrowed
    ///     to a `slice<str>` via the same `ArrayToSlice` coercion used for any slice-typed argument
    ///     (`check_slice_init`), which materializes the data-ptr + constant len. No new mechanism, no
    ///     argv copy: the borrow is a `MakeSlice(slot, N)`.
    ///
    /// Returns `None` (after emitting a diagnostic) on a type/shape error.
    fn check_argv(&mut self, callee: &str, arg: &ast::Expr) -> Option<Expr> {
        let argv = self.check_expr(arg, None);
        if argv.ty == Ty::Error {
            return None;
        }
        match self.resolve(argv.ty) {
            Ty::DynArray(Scalar::Str) | Ty::Slice(Scalar::Str) => Some(argv),
            Ty::Array(Scalar::Str, _) => {
                // Same restriction as every array→slice borrow: the source must be an array literal
                // or a named local (an arbitrary array expression has no materializable slot yet).
                if !matches!(argv.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
                    self.diags.error(
                        "an array coerced to a slice must be an array literal or a variable (an arbitrary array expression is not supported yet)".to_string(),
                        arg.span,
                    );
                    return None;
                }
                let span = argv.span;
                Some(Expr { kind: ExprKind::ArrayToSlice(Box::new(argv)), ty: Ty::Slice(Scalar::Str), span })
            }
            _ => {
                self.diags.error(
                    format!("'{callee}' takes the argv as an `array<str>` / `slice<str>` (the full argv incl. argv[0]), got {}", ty_name(argv.ty)),
                    arg.span,
                );
                None
            }
        }
    }

    /// `process.exec(cmd, args)` (`std.process`) -> `Result<(), Error>` — `execvp(cmd, argv)` **in the
    /// current process**. On success it replaces the image and never returns; the `Result` is only ever
    /// observed as `Err` (a mapped `execvp` errno). `cmd` is the borrowed `str` lookup path; `args` is
    /// the borrowed full argv (incl. argv[0], P5 — same convention as `spawn`). **No cleanup runs on the
    /// success path** — `execvp` discards the address space, so pending `Drop`s / arena ends / buffered
    /// writers are lost (flush first if needed); it is abort-class in cleanup terms. Impure. Builtin.
    fn check_process_exec(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 2 {
            self.diags
                .error(format!("'process.exec' expects 2 arguments (the command path and the argv `array<str>`), got {}", args.len()), span);
            return err;
        }
        let cmd = Box::new(self.check_str_init(&args[0]));
        let Some(argv) = self.check_argv("process.exec", &args[1]) else {
            return err;
        };
        if cmd.ty == Ty::Error {
            return err;
        }
        Expr {
            kind: ExprKind::ProcessExec { cmd, args: Box::new(argv) },
            ty: Ty::Result(Scalar::Unit, Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `ch.wait()` on a `child` ([`Ty::Child`]), the receiver already evaluated. Blocks in `waitpid`
    /// for the child to exit and returns its exit code as `Result<i64, Error>`: a normal exit yields
    /// `WEXITSTATUS` (`0..=255`), a signal-killed child yields `128 + signal` (the shell convention).
    /// `wait` **borrows** the child (never consumed — mirrors `l.accept()`), but flips its reaped state
    /// through the borrow so the later `Drop` is a no-op; a second `wait()` on an already-reaped child
    /// is a clean `Err` (the reaped flag makes double-wait detectable without an `ECHILD` race). No
    /// arguments.
    fn check_child_wait(&mut self, recv_expr: Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // v1 bound-receiver gate (mirrors `check_listener_accept`): the child must be a bound local —
        // an unbound owned-child temporary (`process.spawn(...)?.wait()`) is not `Drop`ped yet, so its
        // pid would never be reaped (a zombie). Bind it first. Lifted when Move temporaries drop.
        if !matches!(recv_expr.kind, ExprKind::Local(_)) {
            if recv_expr.ty != Ty::Error {
                self.diags.error(
                    "bind the child to a local first, then wait (`ch := process.spawn(...)?` then `ch.wait()`) — a temporary owned child handle is not dropped yet, so its pid would never be reaped".to_string(),
                    span,
                );
            }
            return err;
        }
        if !args.is_empty() {
            self.diags
                .error(format!("'.wait()' takes no arguments, got {}", args.len()), span);
            return err;
        }
        Expr {
            kind: ExprKind::ChildWait { child: Box::new(recv_expr) },
            ty: Ty::Result(Scalar::Int(IntTy { bits: 64, signed: true }), Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `ch.kill(sig)` on a `child` ([`Ty::Child`]), the receiver already evaluated. Sends signal `sig`
    /// (an `i64`) to the child via libc `kill`, returning `Result<(), Error>`. Like `wait`, `kill`
    /// **borrows** the child (never consumed) and is gated to a bound local (an unbound owned-child
    /// temporary is not `Drop`ped yet, so its pid would never be reaped). `sig` is required to be exactly
    /// `i64` (like `process.exit`'s code), so the operand matches the runtime `align_rt_child_kill(ch,
    /// sig: i64)` signature. `sig == 0` is the standard liveness probe; a negative / out-of-range `sig`
    /// (and killing an already-reaped child, guarded through the borrow) surfaces as a clean `Err` from
    /// the runtime rather than a stray signal.
    fn check_child_kill(&mut self, recv_expr: Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // v1 bound-receiver gate (mirrors `check_child_wait`): kill borrows the child, so the receiver
        // must be a bound local — an unbound owned-child temporary would never be reaped (a zombie).
        if !matches!(recv_expr.kind, ExprKind::Local(_)) {
            if recv_expr.ty != Ty::Error {
                self.diags.error(
                    "bind the child to a local first, then kill (`ch := process.spawn(...)?` then `ch.kill(sig)`) — a temporary owned child handle is not dropped yet, so its pid would never be reaped".to_string(),
                    span,
                );
            }
            return err;
        }
        if args.len() != 1 {
            self.diags
                .error(format!("'.kill()' expects 1 argument (the signal number, i64), got {}", args.len()), span);
            return err;
        }
        let sig = self.check_expr(&args[0], None);
        if sig.ty == Ty::Error {
            return err;
        }
        // Require *exactly* `i64` (binding a bare int literal's inference var), mirroring `process.exit`
        // / `time.sleep`: a narrower operand would mismatch the runtime `align_rt_child_kill(ch, i64)`.
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        match self.resolve(sig.ty) {
            Ty::Int(IntTy { bits: 64, signed: true }) => {}
            Ty::IntVar(_) => self.constrain(sig.ty, Some(i64_ty), args[0].span),
            other => {
                self.diags.error(
                    format!("'.kill()' expects a signal number (i64), got {}", ty_name(other)),
                    args[0].span,
                );
                return err;
            }
        }
        Expr {
            kind: ExprKind::ChildKill { child: Box::new(recv_expr), sig: Box::new(sig) },
            ty: Ty::Result(Scalar::Unit, Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `std.encoding` — Base64 (standard + URL-safe), hex, and UTF-8 validation. Pure byte
    /// transforms, no state / Move types of their own:
    /// - `base64_encode`/`base64url_encode`/`hex_encode(data)` take a byte view (`str` / owned
    ///   `string` (auto-borrowed) / `slice<u8>`, exactly `hash64`'s accepted forms) and yield an
    ///   owned `string`.
    /// - `base64_decode`/`base64url_decode`/`hex_decode(s)` take a `str` (owned `string`
    ///   auto-borrowed) and yield `Result<buffer, Error>` (invalid input -> `Error.Invalid`).
    /// - `utf8_valid(b)` takes `bytes` (`slice<u8>`) and yields `bool`.
    ///
    /// Builtins, dispatched like the other `std` namespaces.
    fn check_encoding_op(&mut self, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 1 {
            self.diags
                .error(format!("'encoding.{method}' expects 1 argument, got {}", args.len()), span);
            return err;
        }
        let kind = match method {
            "base64_encode" | "base64_decode" => hir::EncodingKind::Base64,
            "base64url_encode" | "base64url_decode" => hir::EncodingKind::Base64Url,
            _ => hir::EncodingKind::Hex, // hex_encode / hex_decode / utf8_valid (unused for utf8_valid)
        };
        // `utf8_valid(b)` — a byte-only check (`slice<u8>`); trivially true for a `str`, so it takes
        // raw `bytes` (`draft.md` §18.2: "check before turning bytes into str").
        if method == "utf8_valid" {
            let arg = self.check_expr(&args[0], None);
            if arg.ty == Ty::Error {
                return err;
            }
            let u8s = Scalar::Int(IntTy { bits: 8, signed: false });
            let resolved = self.resolve(arg.ty);
            if resolved != Ty::Slice(u8s) {
                self.diags
                    .error(format!("'encoding.utf8_valid' expects bytes (slice<u8>), got {}", ty_name(resolved)), args[0].span);
                return err;
            }
            return Expr { kind: ExprKind::Utf8Valid { data: Box::new(arg) }, ty: Ty::Bool, span };
        }
        // A decode consumes a `str`; the result is `Result<buffer, Error>`.
        if method.ends_with("_decode") {
            let input = self.check_str_init(&args[0]);
            let result_ty = Ty::Result(Scalar::Buffer, Scalar::Enum(self.error_enum_id));
            return Expr { kind: ExprKind::EncodingDecode { kind, input: Box::new(input) }, ty: result_ty, span };
        }
        // An encode takes a byte view (str / string / slice<u8>) and returns an owned `string`.
        let mut data = self.check_expr(&args[0], None);
        let u8s = Scalar::Int(IntTy { bits: 8, signed: false });
        let resolved = self.resolve(data.ty);
        let ok = match resolved {
            Ty::Str => true,
            Ty::String => {
                let s = data.span;
                data = Expr { kind: ExprKind::StrBorrow(Box::new(data)), ty: Ty::Str, span: s };
                true
            }
            Ty::Slice(el) => el == u8s,
            _ => false,
        };
        if !ok {
            if resolved != Ty::Error {
                self.diags
                    .error(format!("'encoding.{method}' expects a str, string, or bytes (slice<u8>), got {}", ty_name(resolved)), args[0].span);
            }
            return err;
        }
        Expr { kind: ExprKind::EncodingEncode { kind, data: Box::new(data) }, ty: Ty::String, span }
    }

    /// Check a `bytes` argument that accepts any byte-view form — `str` / owned `string`
    /// (auto-borrowed to a `str`) / `slice<u8>` — exactly `encoding.base64_encode`'s accepted forms.
    /// Returns the (possibly `StrBorrow`-wrapped) expr, or `None` after erroring (`what` names the op
    /// in the diagnostic). Shared by the `std.compress` codecs.
    fn check_byte_view(&mut self, a: &ast::Expr, what: &str) -> Option<Expr> {
        let mut data = self.check_expr(a, None);
        let u8s = Scalar::Int(IntTy { bits: 8, signed: false });
        let resolved = self.resolve(data.ty);
        let ok = match resolved {
            Ty::Str => true,
            Ty::String => {
                let s = data.span;
                data = Expr { kind: ExprKind::StrBorrow(Box::new(data)), ty: Ty::Str, span: s };
                true
            }
            Ty::Slice(el) => el == u8s,
            _ => false,
        };
        if !ok {
            if resolved != Ty::Error {
                self.diags.error(
                    format!("'{what}' expects a str, string, or bytes (slice<u8>), got {}", ty_name(resolved)),
                    a.span,
                );
            }
            return None;
        }
        Some(data)
    }

    /// `std.compress` — gzip via libz / zstd via libzstd (M11). The keystone-library strategy
    /// (draft §15): own the memory (Align allocates the owned `buffer` output), borrow the engine
    /// (zlib's DEFLATE / zstd). The codec is the method prefix (`gzip_` → [`hir::CompressKind::Gzip`],
    /// `zstd_` → [`hir::CompressKind::Zstd`]); the direction is the suffix (`_compress` / `_decompress`).
    /// Both codecs are byte→byte and yield `Result<buffer, Error>`:
    /// - `*_compress(data, level)` — compress the byte view `data` (`str` / owned `string`
    ///   auto-borrowed / `slice<u8>`) at `level` (an `i64`; the runtime aborts on an out-of-range
    ///   level — `0..=9` for gzip, `0..=22` for zstd — a programmer error like `rand.range`'s `lo >= hi`).
    /// - `*_decompress(data)` — inflate a byte view; corrupt/truncated input or a decompress-bomb
    ///   over the runtime output cap → `Error.Invalid`.
    ///
    /// Builtins, dispatched like the other `std` namespaces.
    fn check_compress_op(&mut self, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // Codec = method prefix. The dispatcher only routes the four `{gzip,zstd}_{compress,decompress}`
        // names here, so a `zstd_` prefix is the only non-gzip case.
        let kind = if method.starts_with("zstd_") { hir::CompressKind::Zstd } else { hir::CompressKind::Gzip };
        let result_ty = Ty::Result(Scalar::Buffer, Scalar::Enum(self.error_enum_id));
        let what = format!("compress.{method}");
        if method.ends_with("_decompress") {
            if args.len() != 1 {
                self.diags
                    .error(format!("'{what}' expects 1 argument (data), got {}", args.len()), span);
                return err;
            }
            let Some(data) = self.check_byte_view(&args[0], &what) else { return err };
            return Expr { kind: ExprKind::Decompress { kind, data: Box::new(data) }, ty: result_ty, span };
        }
        // `*_compress(data, level)`.
        if args.len() != 2 {
            self.diags
                .error(format!("'{what}' expects 2 arguments (data, level), got {}", args.len()), span);
            return err;
        }
        let Some(data) = self.check_byte_view(&args[0], &what) else { return err };
        // `level` must be exactly `i64` (the runtime ABI); the range is a runtime concern (abort).
        let level = self.check_expr(&args[1], None);
        if level.ty == Ty::Error {
            return err;
        }
        if !self.require_i64_arg(level.ty, args[1].span, &format!("'{what}' level")) {
            return err;
        }
        Expr { kind: ExprKind::Compress { kind, data: Box::new(data), level: Box::new(level) }, ty: result_ty, span }
    }

    /// `std.crypto` — the self-hosted `constant_time_equal` and the OS-CSPRNG `random` (Slice 1),
    /// plus the `sha256`/`sha512` EVP digests (Slice 2, delegated to [`Self::check_crypto_hash`]).
    /// Builtins, dispatched like the other `std` namespaces.
    ///
    /// - `constant_time_equal(a: bytes, b: bytes) -> bool` — a constant-time byte-equality test. Both
    ///   operands are byte views (`str` / owned `string` auto-borrowed / `slice<u8>`), same as the
    ///   `std.compress` codecs. **Pure** (a branchless self-hosted computation), so it may run inside
    ///   a `par_map` closure. The input length is **public** (crypto.md P1); the CT guarantee is over
    ///   equal-length content (see [`hir::ExprKind::CryptoCtEqual`] / the runtime).
    /// - `random(out: mut buffer)` — fill the whole `buffer` `out` with OS CSPRNG bytes. `out` is a
    ///   `buffer` value, borrowed and filled in place through its handle (not consumed, like
    ///   `reader.read`'s buffer). **Impure** (reads OS entropy); yields [`Ty::Unit`].
    fn check_crypto_op(&mut self, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // `sha256`/`sha512` (Slice 2) — the EVP digests; delegate to the shared hash builder.
        if matches!(method, "sha256" | "sha512") {
            return self.check_crypto_hash(method, args, span);
        }
        // `hmac_sha256`/`hkdf_sha256` (Slice 3) — delegate to their builders.
        if method == "hmac_sha256" {
            return self.check_crypto_hmac(args, span);
        }
        if method == "hkdf_sha256" {
            return self.check_crypto_hkdf(args, span);
        }
        // `{aes_gcm,chacha20_poly1305}_{seal,open}` (Slice 4) — the AEAD builder.
        if matches!(method, "aes_gcm_seal" | "aes_gcm_open" | "chacha20_poly1305_seal" | "chacha20_poly1305_open") {
            return self.check_crypto_aead(method, args, span);
        }
        // `argon2id(password, salt, params)` (Slice 5) — the Argon2id builder.
        if method == "argon2id" {
            return self.check_crypto_argon2(args, span);
        }
        if method == "constant_time_equal" {
            if args.len() != 2 {
                self.diags
                    .error(format!("'crypto.constant_time_equal' expects 2 arguments (a, b), got {}", args.len()), span);
                return err;
            }
            let Some(a) = self.check_byte_view(&args[0], "crypto.constant_time_equal") else { return err };
            let Some(b) = self.check_byte_view(&args[1], "crypto.constant_time_equal") else { return err };
            return Expr {
                kind: ExprKind::CryptoCtEqual { a: Box::new(a), b: Box::new(b) },
                ty: Ty::Bool,
                span,
            };
        }
        // `random(out)` — fill a `buffer` (mirrors `reader.read`'s mut-buffer argument: any expr of
        // type `buffer`, filled in place, not consumed).
        if args.len() != 1 {
            self.diags
                .error(format!("'crypto.random' expects 1 argument (a mut buffer), got {}", args.len()), span);
            return err;
        }
        let out = self.check_expr(&args[0], Some(Ty::Buffer));
        if out.ty == Ty::Error {
            return err;
        }
        if out.ty != Ty::Buffer {
            self.diags
                .error(format!("'crypto.random' fills a buffer, got {}", ty_name(out.ty)), args[0].span);
            return err;
        }
        Expr { kind: ExprKind::CryptoRandom { out: Box::new(out) }, ty: Ty::Unit, span }
    }

    /// `std.crypto` (M11 Slice 2) — `sha256(data)` / `sha512(data)`, the cryptographic digests via
    /// OpenSSL libcrypto's EVP one-shot. Both take one byte view (`str` / owned `string` auto-borrowed
    /// / `slice<u8>`, the shared [`Self::check_byte_view`], same as `std.compress`) and yield a fresh
    /// **owned** `array<u8>` of the algorithm's fixed length (SHA-256 → 32, SHA-512 → 64), the
    /// `rand.sample` return machinery. **Impure** (a C-engine call). The fixed length is a property of
    /// the algorithm — carried as a dynamic [`Ty::DynArray`] of `u8` (the runtime-return ABI hands
    /// back a `{ptr,len}` heap array; the runtime re-checks the length matches).
    fn check_crypto_hash(&mut self, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let algo = if method == "sha256" { hir::HashAlgo::Sha256 } else { hir::HashAlgo::Sha512 };
        if args.len() != 1 {
            self.diags
                .error(format!("'crypto.{method}' expects 1 argument (the data), got {}", args.len()), span);
            return err;
        }
        let Some(data) = self.check_byte_view(&args[0], &format!("crypto.{method}")) else { return err };
        // An owned `array<u8>` of unsigned 8-bit elements — the SHA-256/512 digest bytes.
        let u8s = Scalar::Int(IntTy { bits: 8, signed: false });
        Expr { kind: ExprKind::CryptoHash { algo, data: Box::new(data) }, ty: Ty::DynArray(u8s), span }
    }

    /// `std.crypto` (M11 Slice 3) — `hmac_sha256(key, data)`, the 32-byte HMAC-SHA-256 tag via
    /// OpenSSL libcrypto's `EVP_Q_mac`. Both arguments are byte views (`str` / owned `string`
    /// auto-borrowed / `slice<u8>`, the shared [`Self::check_byte_view`]); yields a fresh **owned**
    /// `array<u8>` of length 32 (the `crypto.sha256` return machinery — a `{ptr,len}` heap array,
    /// carried as [`Ty::DynArray`] of `u8`). **Impure** (a C-engine call). Empty key/data are valid.
    fn check_crypto_hmac(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 2 {
            self.diags
                .error(format!("'crypto.hmac_sha256' expects 2 arguments (key, data), got {}", args.len()), span);
            return err;
        }
        let Some(key) = self.check_byte_view(&args[0], "crypto.hmac_sha256") else { return err };
        let Some(data) = self.check_byte_view(&args[1], "crypto.hmac_sha256") else { return err };
        let u8s = Scalar::Int(IntTy { bits: 8, signed: false });
        Expr { kind: ExprKind::CryptoHmac { key: Box::new(key), data: Box::new(data) }, ty: Ty::DynArray(u8s), span }
    }

    /// `std.crypto` (M11 Slice 3) — `hkdf_sha256(salt, ikm, info, len)`, HKDF-SHA-256 key derivation
    /// via OpenSSL libcrypto's `EVP_KDF`. The three byte views (`salt` / `ikm` / `info`, the shared
    /// [`Self::check_byte_view`]) plus a `len` `i64` yield a `Result<buffer, Error>` (the
    /// `std.compress` status→owned-`buffer` machinery). A non-positive / over-limit `len` →
    /// `Error.Invalid` at runtime (a public value); `salt` and `info` may be empty. **Impure**.
    fn check_crypto_hkdf(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 4 {
            self.diags
                .error(format!("'crypto.hkdf_sha256' expects 4 arguments (salt, ikm, info, len), got {}", args.len()), span);
            return err;
        }
        let Some(salt) = self.check_byte_view(&args[0], "crypto.hkdf_sha256") else { return err };
        let Some(ikm) = self.check_byte_view(&args[1], "crypto.hkdf_sha256") else { return err };
        let Some(info) = self.check_byte_view(&args[2], "crypto.hkdf_sha256") else { return err };
        // `len` must be exactly `i64` (the runtime ABI); the range is a runtime concern (`Error.Invalid`).
        let len = self.check_expr(&args[3], None);
        if len.ty == Ty::Error {
            return err;
        }
        if !self.require_i64_arg(len.ty, args[3].span, "'crypto.hkdf_sha256' len") {
            return err;
        }
        let result_ty = Ty::Result(Scalar::Buffer, Scalar::Enum(self.error_enum_id));
        Expr {
            kind: ExprKind::CryptoHkdf {
                salt: Box::new(salt),
                ikm: Box::new(ikm),
                info: Box::new(info),
                len: Box::new(len),
            },
            ty: result_ty,
            span,
        }
    }

    /// `std.crypto` (M11 Slice 4) — the four AEAD surfaces
    /// `{aes_gcm,chacha20_poly1305}_{seal,open}(key, nonce, data, aad)`, authenticated
    /// encryption/decryption via OpenSSL libcrypto's `EVP_CIPHER`. All four arguments are byte views
    /// (the shared [`Self::check_byte_view`]); both directions yield a `Result<buffer, Error>` (the
    /// `std.compress` status→owned-`buffer` machinery). One [`ExprKind::CryptoAead`] node serves all
    /// four — the method name selects the [`hir::AeadCipher`] and [`hir::AeadDir`]. Key/nonce length
    /// (32/12) and the combined-format checks are **runtime** public-value validations (`Error.Invalid`
    /// before any engine call); `data` (plaintext/ciphertext) and `aad` may be empty. **Impure**.
    fn check_crypto_aead(&mut self, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // The method name is one of the four routed here (see the dispatcher / `check_crypto_op`).
        let (cipher, dir) = match method {
            "aes_gcm_seal" => (hir::AeadCipher::Aes256Gcm, hir::AeadDir::Seal),
            "aes_gcm_open" => (hir::AeadCipher::Aes256Gcm, hir::AeadDir::Open),
            "chacha20_poly1305_seal" => (hir::AeadCipher::ChaCha20Poly1305, hir::AeadDir::Seal),
            "chacha20_poly1305_open" => (hir::AeadCipher::ChaCha20Poly1305, hir::AeadDir::Open),
            _ => unreachable!("crypto AEAD dispatch gated by the method-name matches! above"),
        };
        // Seal takes the plaintext; open takes the ciphertext — name the third argument accordingly.
        let data_name = if matches!(dir, hir::AeadDir::Seal) { "plaintext" } else { "ciphertext" };
        if args.len() != 4 {
            self.diags.error(
                format!("'crypto.{method}' expects 4 arguments (key, nonce, {data_name}, aad), got {}", args.len()),
                span,
            );
            return err;
        }
        let Some(key) = self.check_byte_view(&args[0], &format!("crypto.{method}")) else { return err };
        let Some(nonce) = self.check_byte_view(&args[1], &format!("crypto.{method}")) else { return err };
        let Some(input) = self.check_byte_view(&args[2], &format!("crypto.{method}")) else { return err };
        let Some(aad) = self.check_byte_view(&args[3], &format!("crypto.{method}")) else { return err };
        let result_ty = Ty::Result(Scalar::Buffer, Scalar::Enum(self.error_enum_id));
        Expr {
            kind: ExprKind::CryptoAead {
                cipher,
                dir,
                key: Box::new(key),
                nonce: Box::new(nonce),
                input: Box::new(input),
                aad: Box::new(aad),
            },
            ty: result_ty,
            span,
        }
    }

    /// `std.crypto` (M11 Slice 5) — `argon2id(password, salt, params)`, Argon2id password hashing /
    /// KDF via OpenSSL libcrypto's `EVP_KDF_fetch("ARGON2ID")`. `password` / `salt` are byte views
    /// (the shared [`Self::check_byte_view`]; empty `password` is valid, `salt` must be >= 8 bytes —
    /// the engine's RFC-Argon2 minimum, mapped to `Error.Invalid`). `params` is the builtin **Copy**
    /// struct `argon2_params { m_cost, t_cost, parallelism, len }` (all `i64`) — any expression of
    /// that type (a literal or a variable), typically the struct literal `argon2_params{m_cost: …,
    /// t_cost: …, parallelism: …, len: …}`. Yields a `Result<buffer, Error>` (the `crypto.hkdf_sha256`
    /// status→owned-`buffer` machinery). Public param bounds are validated at runtime →
    /// `Error.Invalid`. **Impure**.
    fn check_crypto_argon2(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 3 {
            self.diags.error(
                format!("'crypto.argon2id' expects 3 arguments (password, salt, params), got {}", args.len()),
                span,
            );
            return err;
        }
        let Some(password) = self.check_byte_view(&args[0], "crypto.argon2id") else { return err };
        let Some(salt) = self.check_byte_view(&args[1], "crypto.argon2id") else { return err };
        // The third argument must be the builtin `argon2_params` struct (always registered in sema
        // setup — a miss is an internal invariant break, not a user error).
        let Some(&pid) = self.struct_ids.get("argon2_params") else {
            self.diags.error("internal: builtin 'argon2_params' struct is not registered".to_string(), span);
            return err;
        };
        let params = self.check_expr(&args[2], Some(Ty::Struct(pid)));
        // Resolve once, up front — then both the type gate and the diagnostic read the resolved type
        // (never an unbound `?N` tyvar). `Ty::Error` short-circuits (the operand already erred).
        let pty = self.resolve(params.ty);
        if pty == Ty::Error {
            return err;
        }
        if pty != Ty::Struct(pid) {
            self.diags.error(
                format!("'crypto.argon2id' expects an 'argon2_params' struct as the third argument, got {}", ty_name(pty)),
                args[2].span,
            );
            return err;
        }
        let result_ty = Ty::Result(Scalar::Buffer, Scalar::Enum(self.error_enum_id));
        Expr {
            kind: ExprKind::CryptoArgon2 {
                password: Box::new(password),
                salt: Box::new(salt),
                params: Box::new(params),
            },
            ty: result_ty,
            span,
        }
    }

    /// Require `ty` to be **exactly** `i64` (the `align_rt_rng_*` runtime ABI), binding a bare-int-
    /// literal inference var to it — not merely int-like. A narrower `i32`/`u8` operand would build a
    /// node whose value width doesn't match the runtime signature (the `time.sleep` #343 discipline;
    /// Align has no implicit int coercion). Returns `false` (after erroring) on a non-`i64` width.
    fn require_i64_arg(&mut self, ty: Ty, span: Span, what: &str) -> bool {
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        match self.resolve(ty) {
            Ty::Int(IntTy { bits: 64, signed: true }) => true,
            Ty::IntVar(_) => {
                self.constrain(ty, Some(i64_ty), span);
                true
            }
            Ty::Error => false,
            other => {
                self.diags.error(format!("{what} must be i64, got {}", ty_name(other)), span);
                false
            }
        }
    }

    /// `rand.seed()` / `rand.seed_with(s)` — build a Copy `rng` value ([`Ty::Rng`]). `seed()` reads
    /// the OS CSPRNG (no argument); `seed_with(s)` takes an `i64` seed (deterministic). Both are
    /// module functions (dispatched like `encoding.*`), not methods.
    fn check_rand_seed(&mut self, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if method == "seed" {
            if !args.is_empty() {
                self.diags.error(format!("'rand.seed' takes no arguments, got {}", args.len()), span);
                return err;
            }
            return Expr { kind: ExprKind::RandSeed, ty: Ty::Rng, span };
        }
        // `seed_with(s)` — one `i64` seed.
        if args.len() != 1 {
            self.diags.error(format!("'rand.seed_with' expects 1 argument (an i64 seed), got {}", args.len()), span);
            return err;
        }
        // No hint — `require_i64_arg` is the sole width check (binds a bare literal's var to i64),
        // so a non-i64 operand reports exactly once (not also a hint-unification mismatch).
        let seed = self.check_expr(&args[0], None);
        if seed.ty == Ty::Error {
            return err;
        }
        if !self.require_i64_arg(seed.ty, args[0].span, "'rand.seed_with' seed") {
            return err;
        }
        Expr { kind: ExprKind::RandSeedWith { seed: Box::new(seed) }, ty: Ty::Rng, span }
    }

    /// `r.next()` / `r.range(lo, hi)` / `r.shuffle(out xs)` / `r.sample(xs, k)` on an `rng`
    /// ([`Ty::Rng`]). Each advances the receiver state in place, so the receiver must be a **mut**
    /// local. `recv_expr` is the already-checked receiver (a [`ExprKind::Local`]); `recv` is its AST
    /// (for the mut/place check).
    fn check_rng_method(&mut self, recv: &ast::Expr, recv_expr: Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        // The receiver must be a bound **mut** local — the method mutates the rng state in place, so
        // an immutable binding (or a non-place receiver like `arr[0].next()`) is a compile error.
        let Some((rid, _)) = self.place_local(recv) else {
            self.diags.error(
                format!("'.{method}()' needs a `mut` rng local (bind it first: `mut r := rand.seed()`, then `r.{method}(...)`) — it advances the generator state in place"),
                recv.span,
            );
            return err;
        };
        if !self.locals[rid as usize].is_mut {
            let name = self.locals[rid as usize].name.clone();
            self.diags.error(
                format!("cannot advance immutable rng '{name}' (declare with `mut`) — '.{method}()' mutates the generator state"),
                recv.span,
            );
            return err;
        }
        match method {
            "next" => {
                if !args.is_empty() {
                    self.diags.error(format!("'.next()' takes no arguments, got {}", args.len()), span);
                    return err;
                }
                Expr { kind: ExprKind::RandNext { rng: Box::new(recv_expr) }, ty: i64_ty, span }
            }
            "range" => {
                let [lo_arg, hi_arg] = args else {
                    self.diags.error(format!("'.range()' takes 2 arguments (lo, hi), got {}", args.len()), span);
                    return err;
                };
                // No hint — `require_i64_arg` is the sole width check (single diagnostic on a bad width).
                let lo = self.check_expr(lo_arg, None);
                let hi = self.check_expr(hi_arg, None);
                if lo.ty == Ty::Error || hi.ty == Ty::Error {
                    return err;
                }
                // Both bounds must be exactly i64 (the runtime `align_rt_rng_range` ABI); check both
                // before bailing so a mistake in either is reported.
                let lo_ok = self.require_i64_arg(lo.ty, lo_arg.span, "'.range()' bound");
                let hi_ok = self.require_i64_arg(hi.ty, hi_arg.span, "'.range()' bound");
                if !lo_ok || !hi_ok {
                    return err;
                }
                Expr { kind: ExprKind::RandRange { rng: Box::new(recv_expr), lo: Box::new(lo), hi: Box::new(hi) }, ty: i64_ty, span }
            }
            "shuffle" => {
                let [xs_arg] = args else {
                    self.diags.error(format!("'.shuffle()' takes 1 argument (an `out` slice), got {}", args.len()), span);
                    return err;
                };
                // `out xs: slice<T>` — a writable slice place (a `mut` local slice), like `map_into`'s
                // destination. Fisher-Yates rearranges the elements in place through this slice.
                let xs = self.check_expr(xs_arg, None);
                if xs.ty == Ty::Error {
                    return err;
                }
                let Ty::Slice(es) = self.resolve(xs.ty) else {
                    self.diags.error(format!("'.shuffle()' rearranges a slice<T> in place, got {}", ty_name(xs.ty)), xs_arg.span);
                    return err;
                };
                if scalar_to_prim(es).is_none() {
                    self.diags.error(
                        format!("'.shuffle()' element must be a primitive scalar (int/float/bool/char), got {}", scalar_name(es)),
                        xs_arg.span,
                    );
                    return err;
                }
                let Some((xid, _)) = self.place_local(xs_arg) else {
                    self.diags.error(
                        "'.shuffle()' needs a writable slice place (a `mut` local, or an `out` parameter)".to_string(),
                        xs_arg.span,
                    );
                    return err;
                };
                if !self.locals[xid as usize].is_mut {
                    let name = self.locals[xid as usize].name.clone();
                    self.diags.error(
                        format!("cannot shuffle immutable '{name}' (declare with `mut`, or use an `out` parameter)"),
                        xs_arg.span,
                    );
                    return err;
                }
                Expr { kind: ExprKind::RandShuffle { rng: Box::new(recv_expr), xs: Box::new(xs), elem: scalar_to_ty(es) }, ty: Ty::Unit, span }
            }
            "sample" => {
                let [xs_arg, k_arg] = args else {
                    self.diags.error(format!("'.sample()' takes 2 arguments (a slice and a count), got {}", args.len()), span);
                    return err;
                };
                let xs = self.check_expr(xs_arg, None);
                if xs.ty == Ty::Error {
                    return err;
                }
                let Ty::Slice(es) = self.resolve(xs.ty) else {
                    self.diags.error(format!("'.sample()' draws from a slice<T>, got {}", ty_name(xs.ty)), xs_arg.span);
                    return err;
                };
                if scalar_to_prim(es).is_none() {
                    self.diags.error(
                        format!("'.sample()' element must be a primitive scalar (int/float/bool/char), got {}", scalar_name(es)),
                        xs_arg.span,
                    );
                    return err;
                }
                let k = self.check_expr(k_arg, None); // sole width check below (single diagnostic).
                if k.ty == Ty::Error {
                    return err;
                }
                if !self.require_i64_arg(k.ty, k_arg.span, "'.sample()' count") {
                    return err;
                }
                // The result is a fresh owned `array<T>` (the drawn elements copied out — it borrows
                // nothing from `xs`, so no region tie).
                Expr { kind: ExprKind::RandSample { rng: Box::new(recv_expr), xs: Box::new(xs), k: Box::new(k), elem: scalar_to_ty(es) }, ty: Ty::DynArray(es), span }
            }
            _ => {
                self.diags.error(format!("'.{method}()' is not a method on an rng (try next / range / shuffle / sample)"), span);
                err
            }
        }
    }

    /// `cli.command(name)` — build a Move `cli command` builder ([`Ty::CliCommand`]) named `name`
    /// (a `str`). A module function (dispatched like `encoding.*`), not a method.
    fn check_cli_command(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 1 {
            self.diags
                .error(format!("'cli.command' expects 1 argument (the command name), got {}", args.len()), span);
            return err;
        }
        let name = self.check_str_init(&args[0]);
        if name.ty == Ty::Error {
            return err;
        }
        Expr { kind: ExprKind::CliCommand { name: Box::new(name) }, ty: Ty::CliCommand, span }
    }

    /// `c.flag_bool(name)` / `c.flag_str(name, default)` / `c.flag_i64(name, default)` / `c.parse(args)`
    /// / `c.usage()` on a `cli command` ([`Ty::CliCommand`]), the receiver already evaluated. The
    /// receiver must be a **bound local** — an owned Move handle temporary is not dropped yet (v1
    /// restriction, the `check_reader_method`/`check_writer_method` precedent), so a chained
    /// `cli.command("x").flag_bool("v")` is rejected until Move-temporary drops land. `flag_*` do
    /// **not** require `mut`: they mutate the flag table in place through the handle pointer, exactly
    /// like a `buffer`/`writer` method.
    fn check_cli_command_method(&mut self, recv_expr: Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // v1 bound-receiver gate: the receiver must be a bound `cli command` local (there is no
        // borrowed/exempt form, unlike `io.stdin`/`io.stdout`). Bind it first, then call the method.
        if !matches!(recv_expr.kind, ExprKind::Local(_)) {
            if recv_expr.ty != Ty::Error {
                self.diags.error(
                    "bind the cli command to a local first, then call the method (`c := cli.command(\"...\")` then `c.flag_bool(...)`) — a temporary owned command handle is not dropped yet".to_string(),
                    span,
                );
            }
            return err;
        }
        match method {
            "flag_bool" | "flag_str" | "flag_i64" => {
                let kind = match method {
                    "flag_bool" => hir::CliFlagKind::Bool,
                    "flag_str" => hir::CliFlagKind::Str,
                    _ => hir::CliFlagKind::I64,
                };
                // `flag_bool(name)` takes one arg; `flag_str`/`flag_i64` take `(name, default)`.
                let want = if matches!(kind, hir::CliFlagKind::Bool) { 1 } else { 2 };
                if args.len() != want {
                    self.diags.error(
                        format!("'.{method}()' takes {want} argument{}, got {}", if want == 1 { "" } else { "s" }, args.len()),
                        span,
                    );
                    return err;
                }
                let name = self.check_str_init(&args[0]);
                if name.ty == Ty::Error {
                    return err;
                }
                let default = match kind {
                    hir::CliFlagKind::Bool => None,
                    hir::CliFlagKind::Str => {
                        let d = self.check_str_init(&args[1]);
                        if d.ty == Ty::Error {
                            return err;
                        }
                        Some(Box::new(d))
                    }
                    hir::CliFlagKind::I64 => {
                        // The default must be exactly `i64` (the `align_rt_cli_flag_i64` ABI; the
                        // `time.sleep` #343 discipline — `require_i64_arg` binds a bare literal's var).
                        let d = self.check_expr(&args[1], None);
                        if d.ty == Ty::Error {
                            return err;
                        }
                        if !self.require_i64_arg(d.ty, args[1].span, "'.flag_i64()' default") {
                            return err;
                        }
                        Some(Box::new(d))
                    }
                };
                Expr { kind: ExprKind::CliFlag { cmd: Box::new(recv_expr), kind, name: Box::new(name), default }, ty: Ty::Unit, span }
            }
            "parse" => {
                if args.len() != 1 {
                    self.diags.error(format!("'.parse()' takes 1 argument (the argv `array<str>`), got {}", args.len()), span);
                    return err;
                }
                let argv = self.check_expr(&args[0], None);
                if argv.ty == Ty::Error {
                    return err;
                }
                // The argument is `main(args)`'s `array<str>` (`DynArray(Str)`) — the one argv source.
                if self.resolve(argv.ty) != Ty::DynArray(Scalar::Str) {
                    self.diags.error(
                        format!("'.parse()' takes the `array<str>` from `main(args)`, got {}", ty_name(argv.ty)),
                        args[0].span,
                    );
                    return err;
                }
                Expr {
                    kind: ExprKind::CliParse { cmd: Box::new(recv_expr), args: Box::new(argv) },
                    ty: Ty::Result(Scalar::CliParsed, Scalar::Enum(self.error_enum_id)),
                    span,
                }
            }
            "usage" => {
                if !args.is_empty() {
                    self.diags.error(format!("'.usage()' takes no arguments, got {}", args.len()), span);
                    return err;
                }
                Expr { kind: ExprKind::CliUsage { cmd: Box::new(recv_expr) }, ty: Ty::String, span }
            }
            _ => {
                self.diags.error(format!("'.{method}()' is not a method on a cli command (try flag_bool / flag_str / flag_i64 / parse / usage)"), span);
                err
            }
        }
    }

    /// `p.get_bool(name)` / `p.get_i64(name)` / `p.get_str(name)` on a `cli parsed`
    /// ([`Ty::CliParsed`]), the receiver already evaluated. Total after a successful parse — an
    /// unregistered name / wrong kind aborts at runtime (no `Result`, no silent default). The
    /// receiver must be a bound local (the v1 gate); `get_str` returns a `str` **view** into `parsed`
    /// (region-bound to it — the `region_of` arm rejects an escape).
    fn check_cli_parsed_method(&mut self, recv_expr: Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if !matches!(recv_expr.kind, ExprKind::Local(_)) {
            if recv_expr.ty != Ty::Error {
                self.diags.error(
                    "bind the parsed result to a local first, then read a flag (`p := c.parse(args)?` then `p.get_bool(...)`) — a temporary owned parsed handle is not dropped yet".to_string(),
                    span,
                );
            }
            return err;
        }
        if args.len() != 1 {
            self.diags.error(format!("'.{method}()' takes 1 argument (the flag name), got {}", args.len()), span);
            return err;
        }
        let name = self.check_str_init(&args[0]);
        if name.ty == Ty::Error {
            return err;
        }
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        match method {
            "get_bool" => Expr { kind: ExprKind::CliGetBool { parsed: Box::new(recv_expr), name: Box::new(name) }, ty: Ty::Bool, span },
            "get_i64" => Expr { kind: ExprKind::CliGetI64 { parsed: Box::new(recv_expr), name: Box::new(name) }, ty: i64_ty, span },
            "get_str" => Expr { kind: ExprKind::CliGetStr { parsed: Box::new(recv_expr), name: Box::new(name) }, ty: Ty::Str, span },
            _ => {
                self.diags.error(format!("'.{method}()' is not a method on a cli parsed (try get_bool / get_i64 / get_str)"), span);
                err
            }
        }
    }

    /// Type-check a `bytes` argument (`arg_ix` of `args`): accept a `str`, an owned `string`
    /// (auto-borrowed to `str`), or a `bytes` `slice<u8>`; anything else is an error. `what` names the
    /// call for the diagnostic. Returns the coerced `Expr` (`Ty::Error` on failure).
    fn check_bytes_init(&mut self, arg: &ast::Expr, what: &str) -> Expr {
        let mut e = self.check_expr(arg, None);
        if e.ty == Ty::Error {
            return e;
        }
        if e.ty == Ty::String {
            let s = e.span;
            e = Expr { kind: ExprKind::StrBorrow(Box::new(e)), ty: Ty::Str, span: s };
        }
        if e.ty != Ty::Str && e.ty != Ty::Slice(Scalar::Int(IntTy { bits: 8, signed: false })) {
            self.diags
                .error(format!("{what} expects bytes (a str, string, or slice<u8>), got {}", ty_name(e.ty)), arg.span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span: arg.span };
        }
        e
    }

    /// `http.request(method, url)` — build a Move `http request` builder ([`Ty::HttpRequest`]). Both
    /// `method` and `url` are `str`. Total (the URL is validated later, at serialize) — a module
    /// function, dispatched like `cli.command`.
    fn check_http_request(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 2 {
            self.diags
                .error(format!("'http.request' expects 2 arguments (method, url), got {}", args.len()), span);
            return err;
        }
        let method = self.check_str_init(&args[0]);
        let url = self.check_str_init(&args[1]);
        if method.ty == Ty::Error || url.ty == Ty::Error {
            return err;
        }
        Expr { kind: ExprKind::HttpRequest { method: Box::new(method), url: Box::new(url) }, ty: Ty::HttpRequest, span }
    }

    /// `r.header(name, value)` / `r.body(data)` on an `http request` ([`Ty::HttpRequest`]), the
    /// receiver already evaluated. The receiver must be a **bound local** (the v1 Move-temporary gate,
    /// `check_cli_command_method` precedent); both methods mutate the builder in place (no `mut`
    /// needed) and yield `()`. A CR/LF/NUL in a header name/value aborts at runtime (P6).
    fn check_http_request_method(&mut self, recv_expr: Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if !matches!(recv_expr.kind, ExprKind::Local(_)) {
            if recv_expr.ty != Ty::Error {
                self.diags.error(
                    "bind the http request to a local first, then call the method (`r := http.request(\"GET\", url)` then `r.header(...)`) — a temporary owned request handle is not dropped yet".to_string(),
                    span,
                );
            }
            return err;
        }
        match method {
            "header" => {
                if args.len() != 2 {
                    self.diags.error(format!("'.header()' takes 2 arguments (name, value), got {}", args.len()), span);
                    return err;
                }
                let name = self.check_str_init(&args[0]);
                let value = self.check_str_init(&args[1]);
                if name.ty == Ty::Error || value.ty == Ty::Error {
                    return err;
                }
                Expr { kind: ExprKind::HttpHeader { req: Box::new(recv_expr), name: Box::new(name), value: Box::new(value) }, ty: Ty::Unit, span }
            }
            "body" => {
                if args.len() != 1 {
                    self.diags.error(format!("'.body()' takes 1 argument (the body bytes), got {}", args.len()), span);
                    return err;
                }
                let data = self.check_bytes_init(&args[0], "'.body()'");
                if data.ty == Ty::Error {
                    return err;
                }
                Expr { kind: ExprKind::HttpBody { req: Box::new(recv_expr), data: Box::new(data) }, ty: Ty::Unit, span }
            }
            _ => {
                self.diags.error(format!("'.{method}()' is not a method on an http request (try header / body)"), span);
                err
            }
        }
    }

    /// `http.parse(data)` — parse an HTTP/1.1 response buffer (a `bytes` view) into a Move
    /// `http response` ([`Ty::HttpResponse`]), yielding `Result<response, Error>`. A module function.
    fn check_http_parse(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 1 {
            self.diags
                .error(format!("'http.parse' expects 1 argument (the response bytes), got {}", args.len()), span);
            return err;
        }
        let data = self.check_bytes_init(&args[0], "'http.parse'");
        if data.ty == Ty::Error {
            return err;
        }
        Expr {
            kind: ExprKind::HttpParse { data: Box::new(data) },
            ty: Ty::Result(Scalar::HttpResponse, Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `resp.status()` / `resp.header(name)` / `resp.body()` on an `http response`
    /// ([`Ty::HttpResponse`]), the receiver already evaluated. The receiver must be a bound local (the
    /// v1 gate). `status` yields `i64`; `header` yields `Option<str>` (case-insensitive) whose `str` is
    /// a **view** into `resp` (region-bound — the `region_of` arm rejects an escape); `body` yields a
    /// `slice<u8>` **view** into `resp` (local-backed — likewise not returnable).
    fn check_http_response_method(&mut self, recv_expr: Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if !matches!(recv_expr.kind, ExprKind::Local(_)) {
            if recv_expr.ty != Ty::Error {
                self.diags.error(
                    "bind the http response to a local first, then read it (`resp := http.parse(bytes)?` then `resp.status()`) — a temporary owned response handle is not dropped yet".to_string(),
                    span,
                );
            }
            return err;
        }
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        match method {
            "status" => {
                if !args.is_empty() {
                    self.diags.error(format!("'.status()' takes no arguments, got {}", args.len()), span);
                    return err;
                }
                Expr { kind: ExprKind::HttpRespStatus { resp: Box::new(recv_expr) }, ty: i64_ty, span }
            }
            "header" => {
                if args.len() != 1 {
                    self.diags.error(format!("'.header()' takes 1 argument (the header name), got {}", args.len()), span);
                    return err;
                }
                let name = self.check_str_init(&args[0]);
                if name.ty == Ty::Error {
                    return err;
                }
                Expr {
                    kind: ExprKind::HttpRespHeader { resp: Box::new(recv_expr), name: Box::new(name) },
                    ty: Ty::Option(Scalar::Str),
                    span,
                }
            }
            "body" => {
                if !args.is_empty() {
                    self.diags.error(format!("'.body()' takes no arguments, got {}", args.len()), span);
                    return err;
                }
                Expr {
                    kind: ExprKind::HttpRespBody { resp: Box::new(recv_expr) },
                    ty: Ty::Slice(Scalar::Int(IntTy { bits: 8, signed: false })),
                    span,
                }
            }
            _ => {
                self.diags.error(format!("'.{method}()' is not a method on an http response (try status / header / body)"), span);
                err
            }
        }
    }

    /// `http.client()` — build a Move `http client` ([`Ty::HttpClient`]). No arguments. A module
    /// function, dispatched like `cli.command` / `http.request`. Impure requests come from its methods.
    fn check_http_client(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if !args.is_empty() {
            self.diags
                .error(format!("'http.client' takes no arguments, got {}", args.len()), span);
            return err;
        }
        Expr { kind: ExprKind::HttpClient, ty: Ty::HttpClient, span }
    }

    /// `cl.get(url)` / `cl.post(url, body)` / `cl.request(req)` on an `http client`
    /// ([`Ty::HttpClient`]), the receiver already evaluated. The receiver must be a **bound local**
    /// (the v1 Move-temporary gate, the reader/writer/cli precedent); `cl` is borrowed (a client fires
    /// many requests). Each yields `Result<response, Error>`. `get`/`post` take a `str` url (and `post`
    /// a `bytes` body); `request` takes an `http request` (`Ty::HttpRequest`) that is **consumed**
    /// (moved into the call — the runtime frees it). All Impure (network).
    fn check_http_client_method(&mut self, recv_expr: Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if !matches!(recv_expr.kind, ExprKind::Local(_)) {
            if recv_expr.ty != Ty::Error {
                self.diags.error(
                    "bind the http client to a local first, then call the method (`cl := http.client()` then `cl.get(url)`) — a temporary owned client handle is not dropped yet".to_string(),
                    span,
                );
            }
            return err;
        }
        let result_ty = Ty::Result(Scalar::HttpResponse, Scalar::Enum(self.error_enum_id));
        match method {
            "get" => {
                if args.len() != 1 {
                    self.diags.error(format!("'.get()' takes 1 argument (the url), got {}", args.len()), span);
                    return err;
                }
                let url = self.check_str_init(&args[0]);
                if url.ty == Ty::Error {
                    return err;
                }
                Expr { kind: ExprKind::HttpClientGet { client: Box::new(recv_expr), url: Box::new(url) }, ty: result_ty, span }
            }
            "post" => {
                if args.len() != 2 {
                    self.diags.error(format!("'.post()' takes 2 arguments (url, body), got {}", args.len()), span);
                    return err;
                }
                let url = self.check_str_init(&args[0]);
                let body = self.check_bytes_init(&args[1], "'.post()'");
                if url.ty == Ty::Error || body.ty == Ty::Error {
                    return err;
                }
                Expr {
                    kind: ExprKind::HttpClientPost { client: Box::new(recv_expr), url: Box::new(url), body: Box::new(body) },
                    ty: result_ty,
                    span,
                }
            }
            "request" => {
                if args.len() != 1 {
                    self.diags.error(format!("'.request()' takes 1 argument (an http request), got {}", args.len()), span);
                    return err;
                }
                let req = self.check_expr(&args[0], None);
                if req.ty == Ty::Error {
                    return err;
                }
                if self.resolve(req.ty) != Ty::HttpRequest {
                    self.diags.error(
                        format!("'.request()' expects an http request (from `http.request(...)`), got {}", ty_name(req.ty)),
                        args[0].span,
                    );
                    return err;
                }
                Expr { kind: ExprKind::HttpClientRequest { client: Box::new(recv_expr), req: Box::new(req) }, ty: result_ty, span }
            }
            _ => {
                self.diags.error(format!("'.{method}()' is not a method on an http client (try get / post / request)"), span);
                err
            }
        }
    }

    /// `io.stdout.buffered()` (fd 1) / `io.stderr.buffered()` (fd 2) — a buffered `writer` over the
    /// given standard stream. `sink` names it for the diagnostic; `fd` is the lowered target.
    fn check_io_buffered(&mut self, sink: &str, fd: i32, args: &[ast::Expr], span: Span) -> Expr {
        if !args.is_empty() {
            self.diags
                .error(format!("'io.{sink}.buffered' takes no arguments, got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        Expr { kind: ExprKind::WriterStd { fd, buffered: true }, ty: Ty::Writer, span }
    }

    /// `w.write(x)` / `w.flush()` on a `writer` ([`Ty::Writer`]), the receiver already evaluated.
    /// `write` appends a `str` / owned `string` (auto-borrowed) / `bytes` (`slice<u8>`) / a
    /// `builder`'s bytes; `flush` drains to the OS. Both yield `Result<(), Error>` and borrow the
    /// writer (never consumed — `Drop`-flushed/closed at scope exit).
    fn check_writer_method(&mut self, recv_expr: Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let result_ty = Ty::Result(Scalar::Unit, Scalar::Enum(self.error_enum_id));
        // v1 restriction (until Move *temporaries* get a `Drop`): the receiver of a writer method
        // must be a bound local — never an unbound owned-handle temporary. `fs.create(p)?.write(d)?`
        // would leave the temp writer un-`Drop`ped, so its buffered bytes are never flushed and its
        // fd never closed — silent data loss. Only an **unbuffered** borrowed std stream
        // (`io.stdout`/`io.stderr`) is exempt: it owns no fd and holds no buffer, so an un-`Drop`ped
        // temporary loses nothing. A **buffered** std writer (`io.stdout.buffered()`) accumulates
        // bytes that only reach the OS on `flush`/`Drop`, so it must be bound like any owned handle —
        // else its tail chunk (< the buffer size) is silently dropped. Lifted when dropping Move
        // temporaries lands (`draft.md` §18.2).
        if !matches!(recv_expr.kind, ExprKind::Local(_) | ExprKind::WriterStd { buffered: false, .. }) {
            if recv_expr.ty != Ty::Error {
                self.diags.error(
                    "bind the writer to a local first, then call the method (`w := <expr>` then `w.write(...)`) — a temporary owned/buffered writer handle is not dropped/flushed yet, so its output would be lost".to_string(),
                    span,
                );
            }
            return err;
        }
        match method {
            "flush" => {
                if !args.is_empty() {
                    self.diags
                        .error(format!("'.flush()' takes no arguments, got {}", args.len()), span);
                    return err;
                }
                Expr { kind: ExprKind::WriterFlush { writer: Box::new(recv_expr) }, ty: result_ty, span }
            }
            "write" => {
                if args.len() != 1 {
                    self.diags
                        .error(format!("'.write()' takes 1 argument, got {}", args.len()), span);
                    return err;
                }
                let mut arg = self.check_expr(&args[0], None);
                if arg.ty == Ty::Error {
                    return err;
                }
                // A `builder`'s bytes are written directly (no `to_string()` materialization),
                // borrowing it (not consumed).
                if arg.ty == Ty::Builder {
                    return Expr {
                        kind: ExprKind::WriterWrite { writer: Box::new(recv_expr), arg: Box::new(arg), builder: true },
                        ty: result_ty,
                        span,
                    };
                }
                // A `string` borrows as a `str` (zero-cost, non-consuming); `bytes` (a `slice<u8>`)
                // is written as-is; a `str` is written as-is; anything else is a type error.
                if arg.ty == Ty::String {
                    let s = arg.span;
                    arg = Expr { kind: ExprKind::StrBorrow(Box::new(arg)), ty: Ty::Str, span: s };
                }
                if arg.ty != Ty::Str && arg.ty != Ty::Slice(Scalar::Int(IntTy { bits: 8, signed: false })) {
                    self.diags
                        .error(format!("'.write()' expects a str, bytes (slice<u8>), or builder, got {}", ty_name(arg.ty)), arg.span);
                    return err;
                }
                Expr {
                    kind: ExprKind::WriterWrite { writer: Box::new(recv_expr), arg: Box::new(arg), builder: false },
                    ty: result_ty,
                    span,
                }
            }
            _ => {
                self.diags
                    .error(format!("'.{method}()' is not a method on a writer (try write / flush)"), span);
                err
            }
        }
    }

    /// `c.reader()` / `c.writer()` on a `tcp_conn` ([`Ty::TcpConn`]), the receiver already evaluated.
    /// Borrow an M9 `reader` / (unbuffered) `writer` over the conn's socket fd (`owns_fd:false` — only
    /// `c`'s `Drop` closes it), region-bound to `c` (see `region_of`). No arguments.
    fn check_conn_stream(&mut self, recv_expr: Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // v1 bound-receiver gate (mirrors `check_reader_method` / `check_writer_method`): the conn
        // must be a bound local — an unbound owned-conn temporary (`tcp.connect(...)?.reader()`) is
        // not `Drop`ped yet, so its fd would leak and the borrowed stream would outlive it. Bind the
        // conn first. Lifted when dropping Move temporaries lands (net.md P6).
        if !matches!(recv_expr.kind, ExprKind::Local(_)) {
            if recv_expr.ty != Ty::Error {
                self.diags.error(
                    "bind the connection to a local first, then borrow a stream (`c := tcp.connect(...)?` then `c.reader()`) — a temporary owned connection handle is not dropped yet, so its fd would leak".to_string(),
                    span,
                );
            }
            return err;
        }
        if !args.is_empty() {
            self.diags
                .error(format!("'.{method}()' takes no arguments, got {}", args.len()), span);
            return err;
        }
        if method == "reader" {
            Expr { kind: ExprKind::ConnReader { conn: Box::new(recv_expr) }, ty: Ty::Reader, span }
        } else {
            Expr { kind: ExprKind::ConnWriter { conn: Box::new(recv_expr) }, ty: Ty::Writer, span }
        }
    }

    /// `l.accept()` on a `tcp_listener` ([`Ty::TcpListener`]), the receiver already evaluated. Blocks
    /// for an inbound connection and returns a new **owned** `tcp_conn` (`Result<tcp_conn, Error>`) —
    /// the accepted conn is freshly owned (never a borrow of the listener), so its result is not
    /// region-bound to `l` (unlike `c.reader()`/`c.writer()`). No arguments.
    fn check_listener_accept(&mut self, recv_expr: Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // v1 bound-receiver gate (mirrors `check_conn_stream`): the listener must be a bound local —
        // an unbound owned-listener temporary (`tcp.listen(...)?.accept()`) is not `Drop`ped yet, so
        // its fd would leak. Bind the listener first. Lifted when dropping Move temporaries lands
        // (net.md P6).
        if !matches!(recv_expr.kind, ExprKind::Local(_)) {
            if recv_expr.ty != Ty::Error {
                self.diags.error(
                    "bind the listener to a local first, then accept (`l := tcp.listen(...)?` then `l.accept()`) — a temporary owned listener handle is not dropped yet, so its fd would leak".to_string(),
                    span,
                );
            }
            return err;
        }
        if !args.is_empty() {
            self.diags
                .error(format!("'.accept()' takes no arguments, got {}", args.len()), span);
            return err;
        }
        Expr {
            kind: ExprKind::TcpAccept { listener: Box::new(recv_expr) },
            ty: Ty::Result(Scalar::TcpConn, Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `r.read(b: mut buffer)` on a `reader` ([`Ty::Reader`]), the receiver already evaluated. Fills
    /// `b` up to its capacity (overwriting its length), yielding `Result<i64, Error>` (bytes read;
    /// `0` = EOF). Borrows both reader and buffer (neither consumed).
    fn check_reader_method(&mut self, recv_expr: Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // v1 restriction (mirrors `check_writer_method`): the receiver must be a bound local — an
        // unbound owned-handle temporary (`fs.open(p)?.read(buf)?`) would leak its fd (no `Drop`).
        // `io.stdin` (borrowed, no owned fd) is exempt. Lifted when Move temporaries drop.
        if !matches!(recv_expr.kind, ExprKind::Local(_) | ExprKind::ReaderStdin) {
            if recv_expr.ty != Ty::Error {
                self.diags.error(
                    "bind the reader to a local first, then call the method (`r := <expr>` then `r.read(...)`) — a temporary owned reader handle is not dropped yet, so its fd would leak".to_string(),
                    span,
                );
            }
            return err;
        }
        if method != "read" {
            self.diags.error(format!("'.{method}()' is not a method on a reader (try read)"), span);
            return err;
        }
        if args.len() != 1 {
            self.diags.error(format!("'.read()' takes 1 argument (a mut buffer), got {}", args.len()), span);
            return err;
        }
        let buffer = self.check_expr(&args[0], Some(Ty::Buffer));
        if buffer.ty == Ty::Error {
            return err;
        }
        if buffer.ty != Ty::Buffer {
            self.diags.error(format!("'.read()' fills a buffer, got {}", ty_name(buffer.ty)), args[0].span);
            return err;
        }
        Expr {
            kind: ExprKind::ReaderRead { reader: Box::new(recv_expr), buffer: Box::new(buffer) },
            ty: Ty::Result(Scalar::Int(IntTy { bits: 64, signed: true }), Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `io.copy(r: reader, w: writer)` — stream all of `r` into `w` through a fixed-size buffer,
    /// yielding `Result<i64, Error>` (bytes transferred; memory is O(buffer), never O(file size)).
    /// **Non-consuming**: both handles are borrowed (fd ownership does not move — like `print`'s
    /// argument), so `r`/`w` stay usable after the call. A builtin, dispatched like `fs.read_file`.
    fn check_io_copy(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 2 {
            self.diags
                .error(format!("'io.copy' expects 2 arguments (a reader and a writer), got {}", args.len()), span);
            return err;
        }
        let reader = self.check_expr(&args[0], Some(Ty::Reader));
        let writer = self.check_expr(&args[1], Some(Ty::Writer));
        if reader.ty == Ty::Error || writer.ty == Ty::Error {
            return err;
        }
        if reader.ty != Ty::Reader {
            self.diags
                .error(format!("'io.copy' reads from a reader, got {}", ty_name(reader.ty)), args[0].span);
            return err;
        }
        if writer.ty != Ty::Writer {
            self.diags
                .error(format!("'io.copy' writes to a writer, got {}", ty_name(writer.ty)), args[1].span);
            return err;
        }
        // v1 restriction (mirrors `check_reader_method` / `check_writer_method`): each owned handle
        // must be a bound local — an unbound temporary (`io.copy(fs.open(p)?, w)`) would leak its fd
        // (its `Drop` never runs). Only the **unbuffered** borrowed std streams (`io.stdin` /
        // `io.stdout` / `io.stderr`) are exempt (no fd, no buffer). A **buffered** std writer
        // (`io.stdout.buffered()`) holds bytes that only reach the OS on `flush`/`Drop`, so an
        // un-`Drop`ped temporary would silently lose `io.copy`'s tail chunk — it must be bound.
        // Lifted when Move temporaries get a `Drop`.
        if !matches!(reader.kind, ExprKind::Local(_) | ExprKind::ReaderStdin) {
            self.diags.error(
                "bind the reader to a local first, then pass it (`r := <expr>` then `io.copy(r, w)`) — a temporary owned reader handle is not dropped yet, so its fd would leak".to_string(),
                args[0].span,
            );
            return err;
        }
        if !matches!(writer.kind, ExprKind::Local(_) | ExprKind::WriterStd { buffered: false, .. }) {
            self.diags.error(
                "bind the writer to a local first, then pass it (`w := <expr>` then `io.copy(r, w)`) — a temporary owned/buffered writer handle is not dropped/flushed yet, so its buffered output would be lost".to_string(),
                args[1].span,
            );
            return err;
        }
        Expr {
            kind: ExprKind::IoCopy { reader: Box::new(reader), writer: Box::new(writer) },
            ty: Ty::Result(Scalar::Int(IntTy { bits: 64, signed: true }), Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `b.bytes()` on a `buffer` ([`Ty::Buffer`]), the receiver already evaluated. A `slice<u8>`
    /// view of the buffer's current contents, borrowing it (region-tracked: must not outlive `b`).
    fn check_buffer_bytes(&mut self, recv_expr: Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // v1 restriction (mirrors reader/writer): the receiver must be a bound local. On an unbound
        // `buffer` temporary (`buffer(4).bytes()`), `.bytes()` returns a `slice<u8>` viewing the
        // temp's storage — leaked-but-valid today, but a dangling slice (UAF) the moment Move
        // temporaries get a `Drop`. Bind the buffer first. Lifted with Move-temporary drop.
        if !matches!(recv_expr.kind, ExprKind::Local(_)) {
            if recv_expr.ty != Ty::Error {
                self.diags.error(
                    "bind the buffer to a local first, then call the method (`b := buffer(n)` then `b.bytes()`) — a temporary buffer handle is not dropped yet, and `.bytes()` returns a slice into it".to_string(),
                    span,
                );
            }
            return err;
        }
        if !args.is_empty() {
            self.diags.error(format!("'.bytes()' takes no arguments, got {}", args.len()), span);
            return err;
        }
        Expr {
            kind: ExprKind::BufferBytes { buffer: Box::new(recv_expr) },
            ty: Ty::Slice(Scalar::Int(IntTy { bits: 8, signed: false })),
            span,
        }
    }

    /// `arr[index].field` — field access on a struct-array element (MMv2 slice 8f). Fused into one
    /// bounds-checked element-field load; only the field (a scalar or a `str` view) is read. The
    /// result inherits the array's region (a `str` field views the array's input), so it cannot
    /// escape that input.
    fn check_index_field(&mut self, arr: &ast::Expr, index: &ast::Expr, fields: &[&ast::Ident], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Local(u32::MAX), ty: Ty::Error, span };
        let r = self.check_expr(arr, None);
        let i = self.check_expr(index, Some(Ty::Int(IntTy { bits: 64, signed: true })));
        if i.ty == Ty::Error {
            return err;
        }
        if !i.ty.is_int_like() {
            self.diags.error(format!("an array index must be an integer, got {}", ty_name(i.ty)), index.span);
            return err;
        }
        let struct_id = match r.ty {
            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
            // `s[i].field` on a soa reads one column's element directly (lowered via the shared
            // `lower_field_access` seam → `IndexColumn`) — cheaper than gathering the whole struct.
            // soa fields are scalar, so the path is always length 1 (a nested `.field.sub` fails in
            // `field_of` below, since the field isn't a struct).
            Ty::Soa(id) => id,
            Ty::Error => return err,
            other => {
                self.diags.error(format!("'arr[i].{}' needs a struct array or soa, got {}", fields[0].name, ty_name(other)), span);
                return err;
            }
        };
        // A fixed `array<Struct>` slot must be a literal or a variable (same restriction as a
        // pipeline source — MIR addresses it through a slot). A `{ptr,len}` view is fine as a value.
        if matches!(r.ty, Ty::StructArray(..)) && !matches!(r.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
            self.diags.error(
                "indexing a fixed array requires an array literal or a variable (an arbitrary array expression is not supported yet)".to_string(),
                span,
            );
            return err;
        }
        // Resolve the field path through the (possibly nested) element struct: each non-final field
        // must itself be a struct so the path can continue (`arr[i].a.x`). The final field is the
        // leaf whose value is read.
        let mut path = Vec::with_capacity(fields.len());
        let mut cur = Ty::Struct(struct_id);
        let mut leaf_ty = Ty::Error;
        for (k, f) in fields.iter().enumerate() {
            let Some((idx, fty)) = self.field_of(cur, &f.name, f.span) else { return err };
            path.push(idx);
            if k + 1 == fields.len() {
                leaf_ty = fty;
            } else if let Ty::Struct(nid) = fty {
                cur = Ty::Struct(nid);
            } else {
                self.diags.error(format!("field '{}' is {}, not a struct — cannot access '.{}' through it", f.name, ty_name(fty), fields[k + 1].name), f.span);
                return err;
            }
        }
        // An owned `string` leaf field is read as a **borrowed `str` view** into the element's
        // buffer — zero-copy, region-tied to the array (it must not outlive it), never an ownership
        // transfer (a runtime index can't track which element gave up its buffer). All read ops work
        // (`us[i].name.len()`, `.clone()` for an owned copy, comparison, `str`-arg); `String` and
        // `str` share the `{ptr,len}` layout, so the lowering is unchanged — only the type. (Slice 4b.)
        if leaf_ty == Ty::String {
            leaf_ty = Ty::Str;
        }
        // Any other Move leaf (a `box`/owned-collection/builder field) can't occur — struct fields are
        // scalar / `str` / `string` / plain-struct only — but guard defensively against a copy-without-
        // ownership-transfer double-free if that ever changes.
        if matches!(leaf_ty, Ty::Box(_) | Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::Builder | Ty::Reader | Ty::Writer | Ty::Buffer | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child) || payload_is_move(leaf_ty) {
            self.diags.error(
                format!("reading a Move-type field {} out of an array element is not supported yet", ty_name(leaf_ty)),
                span,
            );
            return err;
        }
        self.constrain(leaf_ty, expected, span);
        Expr {
            kind: ExprKind::ElemField { recv: Box::new(r), index: Box::new(i), path, struct_id },
            ty: leaf_ty,
            span,
        }
    }

    /// `b.get()` — copy the value out of a `box<T>`.
    fn check_box_get(&mut self, recv: Expr, recv_ty: Ty, args: &[ast::Expr], span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'get' takes no arguments".to_string(), span);
        }
        match recv_ty {
            Ty::Box(s) => Expr { kind: ExprKind::BoxGet(Box::new(recv)), ty: scalar_to_ty(s), span },
            // `task.get()` — read a spawned task's result (`task_group`, slice ④). The result is
            // only computed after `wait()` joins, so `get()` before `wait()` reads an uncomputed
            // slot — rejected (the result is guaranteed ready only if a `wait()` dominates here).
            Ty::Task(s) => {
                if !self.wait_state.last().copied().unwrap_or(false) {
                    let msg = if self.task_group_fallible.last().copied().unwrap_or(false) {
                        // A fallible group: a bare `wait()` ignores the error; only `wait()?` makes
                        // the results safe to read.
                        "cannot call '.get()' before a successful 'wait()?' — this task_group is fallible, so use 'wait()?' to join (its error propagates) before reading results"
                    } else {
                        "cannot call '.get()' before 'wait()' — a task's result is ready only after the group is joined"
                    };
                    self.diags.error(msg.to_string(), span);
                }
                Expr { kind: ExprKind::TaskGet(Box::new(recv)), ty: scalar_to_ty(s), span }
            }
            Ty::Error => Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span },
            other => {
                self.diags
                    .error(format!("'.get()' is only available on box<T> or Task<R>, got {}", ty_name(other)), span);
                Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span }
            }
        }
    }

    /// Builtin `error(code)` — sugar for `Error.Code(code)` (the generic error category).
    fn check_error_ctor(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let ty = Ty::Enum(self.error_enum_id);
        if args.len() != 1 {
            self.diags.error(format!("'error' takes 1 argument, got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let arg = self.check_expr(&args[0], Some(Ty::Int(IntTy { bits: 32, signed: true })));
        Expr {
            kind: ExprKind::EnumValue { enum_id: self.error_enum_id, variant: ERROR_VARIANT_CODE, payload: vec![arg] },
            ty,
            span,
        }
    }

    /// Builtins `Ok(x)` / `Err(e)`. Both payload types come from the expected
    /// `Result<T, E>` (so both arms are typed even though only one is supplied).
    fn check_result_ctor(&mut self, name: &str, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        if args.len() != 1 {
            self.diags
                .error(format!("'{name}' takes 1 argument, got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let (ok_exp, err_exp) = match expected {
            Some(Ty::Result(o, e)) => (Some(scalar_to_ty(o)), Some(scalar_to_ty(e))),
            _ => (None, None),
        };
        let is_ok = name == "Ok";
        let arg = self.check_expr(&args[0], if is_ok { ok_exp } else { err_exp });
        let arg_scalar = self.payload_scalar(arg.ty, args[0].span);

        // The other arm's scalar must be known from context; otherwise we cannot form
        // a complete Result type (M2 limitation).
        let other = if is_ok { err_exp } else { ok_exp };
        let other_scalar = match other.and_then(ty_to_scalar) {
            Some(s) => s,
            None => {
                self.diags.error(
                    format!("cannot infer the full Result type of `{name}` here (annotate the return type)"),
                    span,
                );
                return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
            }
        };
        let (ty, kind) = if is_ok {
            (Ty::Result(arg_scalar, other_scalar), ExprKind::ResultOk(Box::new(arg)))
        } else {
            (Ty::Result(other_scalar, arg_scalar), ExprKind::ResultErr(Box::new(arg)))
        };
        self.constrain(ty, expected, span);
        Expr { kind, ty, span }
    }

    /// `expr as T` — the language's only conversion (no implicit coercion). The source and
    /// target must each be a primitive numeric type (`i8..u64`, `f32`/`f64`) or `char`; `char`
    /// pairs only with integers (a code point is a `u32`), never with a float. The source/target
    /// must be concrete — casting a generic type parameter is unsupported.
    fn check_cast(&mut self, expr: &ast::Expr, ty: &ast::Type, expected: Option<Ty>, span: Span) -> Expr {
        let target = self.resolve_type(ty);
        // A cast re-types its operand, so the operand is checked with no expected type — a literal
        // keeps its own default width (`1 as i32` casts a default-`i64` literal to `i32`).
        let inner = self.check_expr(expr, None);
        let src = self.resolve(inner.ty);
        let ok_target = matches!(target, Ty::Int(_) | Ty::Float(_) | Ty::Char);
        if !ok_target && target != Ty::Error {
            self.diags.error(
                format!("cannot cast to `{}`: `as` converts only between numeric types and `char`", ty_name(target)),
                span,
            );
        }
        let ok_src = src.is_numeric() || src == Ty::Char;
        if !ok_src && src != Ty::Error {
            self.diags.error(
                format!("cannot cast from `{}`: `as` converts only between numeric types and `char`", ty_name(src)),
                span,
            );
        }
        // `char` is a code point — it converts to/from integers, never directly to/from a float.
        if ok_target && ok_src {
            let char_float = (src == Ty::Char && matches!(target, Ty::Float(_)))
                || (src.is_float_like() && target == Ty::Char);
            if char_float {
                self.diags.error(
                    "cannot cast between `char` and a float; convert through an integer".to_string(),
                    span,
                );
            }
        }
        // The lossy-conversion lint is emitted later, in `finalize_expr` — only there are both the
        // operand and target widths concrete (an unconstrained-default or forward-inferred source is
        // still an inference variable here). Classifying now would miss those and could misreport a
        // width later unified to something else (Gate 5: resolve fully before classifying).
        self.constrain(target, expected, span);
        Expr { kind: ExprKind::Cast(Box::new(inner)), ty: target, span }
    }

    /// `expr?` — propagate. The operand must be `Result<T, E>` and the enclosing
    /// function must return `Result<_, E>` (same `E`). Yields `T`.
    fn check_try(&mut self, inner: &ast::Expr, expected: Option<Ty>, span: Span) -> Expr {
        // Thread the expected unwrapped (Ok) type inward as a `Result<expected, ret_err>`, so
        // a context type can drive inference inside the `?` operand (e.g. `json.decode`'s T
        // from `let u: User := json.decode(d)?`). The err type comes from the function's
        // return Result, matching the `?` propagation rule below.
        let inner_expected = match (expected, self.resolve(self.ret_hint)) {
            (Some(ok), Ty::Result(_, err)) => ty_to_scalar(ok).map(|o| Ty::Result(o, err)),
            _ => None,
        };
        let v = self.check_expr(inner, inner_expected);
        // `wait()?` on a fallible task_group: control only continues past the `?` if no task failed
        // (the `Err` was propagated), so every task succeeded → `get()` is now safe (slice ④c-2).
        // Recognised when `?` is applied directly to `wait()` (`wait()?`, also `w := wait()?`);
        // binding the raw `Result` first and unwrapping the local later (`w := wait(); w?`) is a
        // sound over-restriction — `get()` would still be rejected. (Indirect unwrap is a later,
        // local-tracking refinement.)
        if matches!(v.kind, ExprKind::Wait)
            && let Some(w) = self.wait_state.last_mut() {
                *w = true;
            }
        let (ok, err) = match self.resolve(v.ty) {
            Ty::Result(o, e) => (o, e),
            Ty::Error => return Expr { kind: ExprKind::Try(Box::new(v)), ty: Ty::Error, span },
            other => {
                self.diags
                    .error(format!("`?` expects a Result, got {}", ty_name(other)), span);
                return Expr { kind: ExprKind::Try(Box::new(v)), ty: Ty::Error, span };
            }
        };
        match self.resolve(self.ret_hint) {
            Ty::Result(_, ret_err) if ret_err == err => {}
            Ty::Result(_, ret_err) => self.diags.error(
                format!(
                    "`?` error type {} does not match the function's error type {}",
                    scalar_name(err),
                    scalar_name(ret_err)
                ),
                span,
            ),
            _ => self.diags.error(
                "`?` can only be used in a function that returns a Result".to_string(),
                span,
            ),
        }
        Expr { kind: ExprKind::Try(Box::new(v)), ty: scalar_to_ty(ok), span }
    }

    /// `opt else fallback`. The fallback either yields the payload type or diverges via
    /// `return` (only the braced `else { … }` form is supported in M2).
    fn check_else_unwrap(&mut self, opt: &ast::Expr, fallback: &ast::Expr, expected: Option<Ty>, span: Span) -> Expr {
        let o = self.check_expr(opt, None);
        // The fallback runs only on `None`, so its `wait()`/`spawn()` must not leak into the
        // post-unwrap `wait`-state (slice ④c) — snapshot here and restore after the fallback.
        let w_snapshot = self.wait_state.last().copied();
        let payload = match self.resolve(o.ty) {
            Ty::Option(s) => scalar_to_ty(s),
            Ty::Error => Ty::Error,
            other => {
                self.diags
                    .error(format!("`else` unwrap expects an Option, got {}", ty_name(other)), span);
                Ty::Error
            }
        };
        // A diverging `{ … return … }` block has no value; don't constrain it to payload.
        let fb = if block_diverges(fallback) {
            self.check_expr(fallback, None)
        } else {
            self.check_expr(fallback, Some(payload))
        };
        // Dominance merge: the `Some` path skips the fallback (state `w`), the `None` path runs it
        // (current state). After the unwrap, a `wait()` is guaranteed only if both held — `w &&
        // current` — so a conditional `spawn` in the fallback correctly clears the flag.
        if let (Some(w), Some(top)) = (w_snapshot, self.wait_state.last_mut()) {
            *top = w && *top;
        }
        self.constrain(payload, expected, span);
        Expr { kind: ExprKind::ElseUnwrap { opt: Box::new(o), fallback: Box::new(fb) }, ty: payload, span }
    }

    /// `Type.Variant(args)` — construct a sum-type value with a payload. Checks the argument count
    /// and each argument against the variant's payload scalar.
    fn check_variant_ctor(&mut self, enum_id: u32, field: &ast::Ident, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Some(idx) = self.enums[enum_id as usize].variants.iter().position(|v| v.name == field.name) else {
            self.diags.error(format!("'{}' is not a variant of '{}'", field.name, self.enums[enum_id as usize].name), span);
            return err;
        };
        let payload = self.enums[enum_id as usize].variants[idx].payload.clone();
        if args.len() != payload.len() {
            self.diags.error(
                format!("variant '{}' takes {} argument(s), got {}", field.name, payload.len(), args.len()),
                span,
            );
            return err;
        }
        let checked: Vec<Expr> = args
            .iter()
            .zip(&payload)
            .map(|(a, &s)| {
                let pt = scalar_to_ty(s);
                let e = self.check_expr(a, Some(pt));
                if e.ty != Ty::Error && self.resolve(e.ty) != pt {
                    self.diags.error(format!("payload type mismatch: expected {}, got {}", self.ty_display(pt), self.ty_display(e.ty)), e.span);
                }
                e
            })
            .collect();
        let ty = Ty::Enum(enum_id);
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::EnumValue { enum_id, variant: idx as u32, payload: checked }, ty, span }
    }

    /// Monomorphize a generic sum type from the Checker (builds a `TyCx` over its interner fields).
    fn instantiate_enum(&mut self, name: &str, tmpl: &EnumTemplate, args: &[Ty], span: Span) -> u32 {
        let mut cx = TyCx {
            cur_module: &self.cur_module,
            imports: self.user_imports,
            type_table: self.type_table,
            struct_ids: self.struct_ids,
            enum_ids: self.enum_ids,
            struct_templates: self.struct_templates,
            structs: self.structs,
            struct_mono: self.struct_mono,
            enum_templates: self.enum_templates,
            enums: self.enums,
            enum_mono: self.enum_mono,
            tuples: self.tuples,
            fn_types: self.fn_types,
        };
        instantiate_enum(name, tmpl, args, &mut cx, span, self.diags)
    }

    /// Construct a generic sum-type value (`Opt.Some(42)`): infer the type arguments from the
    /// variant's payload args (no turbofish), monomorphize, and build the value. A no-payload
    /// variant (`Opt.None`) has nothing to infer from, so its type arguments are uninferable here.
    fn check_generic_variant_ctor(&mut self, name: &str, tmpl: &EnumTemplate, field: &ast::Ident, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Some(vidx) = tmpl.variants.iter().position(|v| v.name == field.name) else {
            self.diags.error(format!("'{}' is not a variant of '{name}'", field.name), span);
            return err;
        };
        let payload = tmpl.variants[vidx].payload.clone();
        if args.len() != payload.len() {
            self.diags.error(
                format!("variant '{}' takes {} argument(s), got {}", field.name, payload.len(), args.len()),
                span,
            );
            return err;
        }
        let mut subst: Vec<Option<Ty>> = vec![None; tmpl.type_params.len()];
        let mut checked = Vec::with_capacity(args.len());
        for (a, &ps) in args.iter().zip(&payload) {
            // A `Param` payload applies no coercion (its type is inferred); a concrete one checks
            // against its declared scalar.
            let ce = if matches!(ps, Scalar::Param(_)) {
                self.check_expr(a, None)
            } else {
                self.check_expr(a, Some(scalar_to_ty(ps)))
            };
            if let Scalar::Param(p) = ps {
                self.bind_param(p, ce.ty, &mut subst, a.span);
            }
            checked.push(ce);
        }
        // Each type parameter must be inferable from the payload args (a payload carrying a `Param`
        // resolves to a concrete scalar — finalize eagerly).
        let mut targs = Vec::with_capacity(tmpl.type_params.len());
        for (i, s) in subst.iter().enumerate() {
            let concrete = s.map(|t| self.finalize(t)).unwrap_or(Ty::Error);
            if matches!(concrete, Ty::Param(_) | Ty::IntVar(_) | Ty::FloatVar(_) | Ty::Error) {
                self.diags.error(
                    format!("cannot infer type parameter '{}' of '{name}' from the variant's arguments (annotate the binding)", tmpl.type_params[i]),
                    span,
                );
                return err;
            }
            targs.push(concrete);
        }
        let enum_id = self.instantiate_enum(name, tmpl, &targs, span);
        let ty = Ty::Enum(enum_id);
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::EnumValue { enum_id, variant: vidx as u32, payload: checked }, ty, span }
    }

    /// The variants of a matchable type: a user sum type, or the builtin `Option`/`Result`
    /// (so `match` works on them too). Each variant is `(name, positional payload scalars)`, in
    /// the order the lowering expects (Option: 0 = Some, 1 = None; Result: 0 = Ok, 1 = Err).
    fn match_variants(&self, ty: Ty) -> Option<(String, VariantList)> {
        match ty {
            Ty::Enum(id) => {
                let e = &self.enums[id as usize];
                Some((e.name.clone(), e.variants.iter().map(|v| (v.name.clone(), v.payload.clone())).collect()))
            }
            Ty::Option(s) => Some(("Option".into(), vec![("Some".into(), vec![s]), ("None".into(), Vec::new())])),
            Ty::Result(o, e) => Some(("Result".into(), vec![("Ok".into(), vec![o]), ("Err".into(), vec![e])])),
            _ => None,
        }
    }

    /// `match scrutinee { Variant => body, _ => body }` — exhaustive match over a sum type (a user
    /// `enum`, or builtin `Option`/`Result`). Each arm's body unifies to the match's type; every
    /// variant must be covered, or a `_` wildcard.
    fn check_match(&mut self, scrutinee: &ast::Expr, arms: &[ast::MatchArm], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let s = self.check_expr(scrutinee, None);
        if s.ty == Ty::Error {
            return err;
        }
        let Some((type_name, variants)) = self.match_variants(self.resolve(s.ty)) else {
            self.diags.error(format!("`match` expects a sum type, got {}", ty_name(s.ty)), scrutinee.span);
            return err;
        };
        let mut covered = vec![false; variants.len()];
        let mut has_wildcard = false;
        let mut checked: Vec<hir::MatchArm> = Vec::with_capacity(arms.len());
        // The match's value type: unify all arm bodies (drives inference from `expected`).
        let mut result_ty: Option<Ty> = expected;
        for arm in arms {
            // Payload bindings are scoped to this arm only — snapshot the scope and restore after.
            let scope_mark = self.scope.len();
            // Mark a variant covered, diagnosing a duplicate; returns its tag index, or None if the
            // name is not a variant of the scrutinee type (a hard error).
            let mut cover = |me: &mut Self, name: &ast::Ident| -> Option<u32> {
                match variants.iter().position(|(vn, _)| vn == &name.name) {
                    Some(idx) => {
                        if covered[idx] {
                            me.diags.error(format!("duplicate arm for variant '{}'", name.name), name.span);
                        }
                        covered[idx] = true;
                        Some(idx as u32)
                    }
                    None => {
                        me.diags.error(format!("'{}' is not a variant of '{}'", name.name, type_name), name.span);
                        None
                    }
                }
            };
            let (variant_tags, bindings) = match &arm.pattern {
                ast::MatchPattern::Wildcard(_) => {
                    if has_wildcard {
                        self.diags.error("duplicate `_` arm".to_string(), arm.span);
                    }
                    has_wildcard = true;
                    (Vec::new(), Vec::new())
                }
                ast::MatchPattern::Or { variants: names, .. } => {
                    // Bare variant names, no bindings. A payload variant may appear (its payload is
                    // not bound). Each must be a real, not-yet-covered variant.
                    let tags = names
                        .iter()
                        .filter_map(|n| cover(self, n))
                        .collect::<Vec<_>>();
                    if tags.len() != names.len() {
                        return err;
                    }
                    (tags, Vec::new())
                }
                ast::MatchPattern::Variant { name, bindings } => {
                    let Some(idx) = cover(self, name) else { return err };
                    let payload = &variants[idx as usize].1;
                    if bindings.len() != payload.len() {
                        self.diags.error(
                            format!("variant '{}' binds {} value(s), got {}", name.name, payload.len(), bindings.len()),
                            arm.span,
                        );
                    }
                    // Declare each binding (typed by the matching payload scalar) so the arm
                    // body resolves even when the count is wrong. Binding names must be
                    // distinct — `Rect(w, w)` would otherwise silently shadow.
                    let mut seen_bindings = std::collections::HashSet::new();
                    let locals = bindings
                        .iter()
                        .enumerate()
                        .map(|(i, b)| {
                            if !seen_bindings.insert(&b.name) {
                                self.diags.error(format!("duplicate binding '{}' in pattern", b.name), b.span);
                            } else {
                                // A pattern binding that shadows an outer binding/parameter is an
                                // error; the pre-arm `scope_mark` floor excludes this arm's own
                                // bindings (intra-pattern dupes are the branch above).
                                self.check_shadow(&b.name, b.span, scope_mark);
                            }
                            let ty = payload.get(i).map(|&s| scalar_to_ty(s)).unwrap_or(Ty::Error);
                            self.declare(&b.name, ty, false)
                        })
                        .collect();
                    (vec![idx], locals)
                }
            };
            // Each arm body is checked against the running result type, so the constraint (and any
            // mismatch error) comes from `check_expr`; the first non-error arm fixes the type.
            let body = self.check_expr(&arm.body, result_ty);
            if result_ty.is_none() && body.ty != Ty::Error {
                result_ty = Some(body.ty);
            }
            self.scope.truncate(scope_mark);
            checked.push(hir::MatchArm { variants: variant_tags, bindings, body });
        }
        // Exhaustiveness: every variant covered, or a `_` wildcard present.
        if !has_wildcard {
            let missing: Vec<&str> = variants
                .iter()
                .enumerate()
                .filter(|(i, _)| !covered[*i])
                .map(|(_, v)| v.0.as_str())
                .collect();
            if !missing.is_empty() {
                self.diags
                    .error(format!("non-exhaustive `match` on '{type_name}': missing {}", missing.join(", ")), span);
            }
        }
        let ty = result_ty.unwrap_or(Ty::Unit);
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::Match { scrutinee: Box::new(s), arms: checked }, ty, span }
    }

    fn check_if(&mut self, cond: &ast::Expr, then: &ast::Block, els: Option<&ast::Expr>, expected: Option<Ty>, span: Span) -> Expr {
        let c = self.check_expr(cond, Some(Ty::Bool));
        // `task_group` `wait`-state (slice ④c): each branch starts from the pre-`if` state; after
        // the `if`, a `wait()` is guaranteed only if it ran on *every* path — `then && else` (and
        // an absent `else` is a path that did not wait). Soundly tracks `get`-before-`wait`.
        let in_tg = !self.wait_state.is_empty();
        let w_before = self.wait_state.last().copied().unwrap_or(false);
        let then_b = self.check_block(then, expected);
        let w_then = self.wait_state.last().copied().unwrap_or(false);
        if in_tg {
            *self.wait_state.last_mut().unwrap() = w_before;
        }
        let els_b = match els {
            Some(ast::Expr { kind: ast::ExprKind::Block(b), .. }) => self.check_block(b, expected),
            Some(e) => {
                // `else if` chain: check as an expression and wrap as a block value.
                let v = self.check_expr(e, expected);
                Block { stmts: Vec::new(), value: Some(Box::new(v)) }
            }
            None => Block { stmts: Vec::new(), value: None },
        };
        if in_tg {
            let w_els = if els.is_some() { self.wait_state.last().copied().unwrap_or(false) } else { w_before };
            *self.wait_state.last_mut().unwrap() = w_then && w_els;
        }

        // If both branches produce a value, the if has that (unified) type; else Unit.
        let ty = match (&then_b.value, &els_b.value) {
            (Some(t), Some(e)) => self.unify(t.ty, e.ty, span),
            _ => Ty::Unit,
        };
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::If { cond: Box::new(c), then: then_b, els: els_b }, ty, span }
    }

    // --- finalize ---

    /// A compile-time integer literal whose value provably does not fit its resolved type is a hard
    /// error, not a silent two's-complement wrap: when both the value and the target type are known
    /// at compile time, wrapping it is hidden data corruption, at odds with "nothing hidden" (and it
    /// matches how `as`'s zero-UB design and negative-into-unsigned are already handled). This checks
    /// only *value* literals; runtime arithmetic overflow still wraps (unchanged), and `match`
    /// integer-pattern literals — when they exist — keep the defined-wrap rule (`draft.md` §5,
    /// "Integer Literals"). `v` is the literal's *effective* value (already negated for `-lit`).
    fn check_int_lit_range(&mut self, v: i128, ty: Ty, span: Span) {
        let Ty::Int(it) = ty else { return };
        let (min, max) = int_range(it);
        if v < min || v > max {
            let name = it.name();
            self.diags.error(
                format!("integer literal `{v}` is out of range for type `{name}` (valid range {min}..={max}): a provably-out-of-range literal would silently wrap. Write a value in range, or convert the bit pattern explicitly with `as {name}`."),
                span,
            );
        }
    }

    fn finalize_block(&mut self, b: &mut Block) {
        for s in &mut b.stmts {
            match s {
                Stmt::Let { init, .. } => self.finalize_expr(init),
                Stmt::Assign { value, .. } => self.finalize_expr(value),
                Stmt::AssignField { value, .. } => self.finalize_expr(value),
                Stmt::AssignIndex { index, value, .. }
                | Stmt::AssignElemField { index, value, .. }
                | Stmt::AssignElem { index, value, .. } => {
                    self.finalize_expr(index);
                    self.finalize_expr(value);
                }
                Stmt::AssignVecLane { value, .. } => self.finalize_expr(value),
                Stmt::Return(Some(e)) | Stmt::Expr(e) => self.finalize_expr(e),
                Stmt::Return(None) => {}
                Stmt::LetTuple { init, .. } => self.finalize_expr(init),
            }
        }
        if let Some(v) = &mut b.value {
            self.finalize_expr(v);
        }
    }

    fn finalize_expr(&mut self, e: &mut Expr) {
        let cur_ty = self.finalize(e.ty);
        e.ty = cur_ty;
        let span = e.span;
        // A nested-`Param` generic-call result (`Option<T>` → `Option<i32>`) is resolved here, after
        // the type arguments are finalized; set from the Call arm and applied after the match.
        let mut recomputed: Option<Ty> = None;
        match &mut e.kind {
            ExprKind::Unary { op, expr } => {
                // A negation applied to an integer *literal* — possibly through a *chain* of `-`
                // (`--128`, `-(-128)`; parentheses create no node) — has a compile-time-known
                // effective value: the literal with its sign flipped once per `-`, i.e. negated iff
                // the chain length is odd. Range-check *that* effective value, once, at the outermost
                // `-`, and do NOT recurse into the chain — otherwise only the innermost `-lit` is
                // checked (accepting `-128: i8` while the true value of `--128` is `+128`, out of
                // range) and a duplicate diagnostic is possible. `draft.md` §5.
                let neg_chain = matches!(op, UnOp::Neg)
                    .then(|| peel_neg_literal(expr))
                    .flatten();
                match neg_chain {
                    Some((count, v)) => {
                        // Finalize the `ty` of every node in the chain (the leaf literal + each `-`);
                        // `Neg` preserves the operand type, so they all share `cur_ty`, but each
                        // `Expr.ty` must be concrete (no inference variable) before MIR.
                        let mut node: &mut Expr = expr;
                        loop {
                            node.ty = self.finalize(node.ty);
                            match &mut node.kind {
                                ExprKind::Unary { op: UnOp::Neg, expr } => node = expr,
                                _ => break,
                            }
                        }
                        // Unary negation is a *signed* operation. Applying it to an unsigned type —
                        // by context (`x: u32 := -5`, `g(-5)` into a `u32` param), for any chain
                        // length — would silently wrap and lose the sign; reject it once (and skip
                        // the range check: `-` is illegal here regardless of the effective value).
                        // An explicit `(-5) as u32` is unaffected: the inner `-5` is signed and the
                        // cast does the intended conversion.
                        if matches!(cur_ty, Ty::Int(IntTy { signed: false, .. })) {
                            self.diags.error(
                                format!("cannot apply unary `-` to the unsigned type `{}`: a negative value cannot have an unsigned type (it would silently wrap). Use a signed type, or convert explicitly with `as {}`.", ty_name(cur_ty), ty_name(cur_ty)),
                                span,
                            );
                        } else {
                            let effective = if count % 2 == 1 { v.checked_neg().unwrap_or(v) } else { v };
                            self.check_int_lit_range(effective, cur_ty, span);
                        }
                    }
                    None => {
                        // The operand is not a literal (a variable, a call, an arithmetic expr, …):
                        // finalize it normally, then apply the same unsigned-`-` rejection to this node.
                        self.finalize_expr(expr);
                        if *op == UnOp::Neg && matches!(cur_ty, Ty::Int(IntTy { signed: false, .. })) {
                            self.diags.error(
                                format!("cannot apply unary `-` to the unsigned type `{}`: a negative value cannot have an unsigned type (it would silently wrap). Use a signed type, or convert explicitly with `as {}`.", ty_name(cur_ty), ty_name(cur_ty)),
                                span,
                            );
                        }
                    }
                }
            }
            ExprKind::Cast(expr) => {
                self.finalize_expr(expr);
                // Lossy-conversion lint (`draft.md` §16): now that inference is complete, both the
                // operand type (`expr.ty`, finalized above) and the target (`cur_ty`) are concrete —
                // classify the conversion *here*, not in `check_cast`, so a value whose width is only
                // fixed later (an unconstrained default like `x := 100000; x as i8`, or a
                // forward-inferred local) is seen at its final type (Gate 5: resolve fully first). A
                // narrowing / precision-losing / saturating `as` is defined behavior, so this is a
                // **warning**. Skipped for the `char`↔float pair (already a hard error in
                // `check_cast` — no cascade) and for a compile-time numeric-literal operand (an
                // explicit constant annotation, `1 as i8`).
                let (src, tgt) = (expr.ty, cur_ty);
                let numeric_or_char = |t: Ty| matches!(t, Ty::Int(_) | Ty::Float(_) | Ty::Char);
                let char_float = (src == Ty::Char && matches!(tgt, Ty::Float(_)))
                    || (matches!(src, Ty::Float(_)) && tgt == Ty::Char);
                if numeric_or_char(src)
                    && numeric_or_char(tgt)
                    && !char_float
                    && !is_numeric_literal(expr)
                    && let Some(reason) = cast_loss(src, tgt)
                {
                    self.diags.push(align_diag::Diagnostic::warning(
                        format!("lossy conversion: `{} as {}` {reason} — this is defined behavior, not an error", ty_name(src), ty_name(tgt)),
                        span,
                    ));
                }
            }
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::IntArith { lhs, rhs, .. } => {
                self.finalize_expr(lhs);
                self.finalize_expr(rhs);
            }
            ExprKind::Call { func, args, type_args } => {
                for a in args {
                    self.finalize_expr(a);
                }
                // A generic call: finalize its inferred type arguments (now that whole-function
                // inference is complete), record the instantiation for monomorphization, and
                // rewrite the target to the monomorph's mangled name.
                if !type_args.is_empty() {
                    for t in type_args.iter_mut() {
                        *t = self.finalize(*t);
                    }
                    // A `Param` type argument means this call sits inside a generic *template* and
                    // is parameterized by the enclosing `T`; it is instantiated (and recorded) when
                    // the template itself is monomorphized — skip it here (the template HIR is
                    // discarded). An `IntVar`/`FloatVar` never survives `finalize` (it defaults), and
                    // a truly uninferable parameter already errored in `check_generic_call`.
                    let abstract_call = type_args.iter().any(|t| matches!(t, Ty::Param(_)));
                    if !abstract_call && !type_args.contains(&Ty::Error) {
                        // Resolve a nested-`Param` result type (`Option<T>` → `Option<i32>`).
                        recomputed = Some(subst_param_ty(cur_ty, type_args));
                        // Each concrete type argument must satisfy its parameter's bound.
                        if let Some(bounds) = self.sigs.get(func).map(|s| s.bounds.clone()) {
                            for (i, (arg, bound)) in type_args.iter().zip(&bounds).enumerate() {
                                if !bound.satisfied_by(*arg) {
                                    self.diags.error(
                                        format!("type argument {} = `{}` does not satisfy the `{}` bound of '{func}'", i + 1, self.ty_display(*arg), bound.name()),
                                        span,
                                    );
                                }
                            }
                        }
                        self.instantiations.push((func.clone(), type_args.clone()));
                        *func = mangle_mono(func, type_args);
                    }
                }
            }
            ExprKind::FnValue(_) => {}
            ExprKind::Closure { captures, .. } => {
                for c in captures {
                    self.finalize_expr(c);
                }
            }
            ExprKind::CallFnValue { callee, args } => {
                self.finalize_expr(callee);
                for a in args {
                    self.finalize_expr(a);
                }
            }
            ExprKind::If { cond, then, els } => {
                self.finalize_expr(cond);
                self.finalize_block(then);
                self.finalize_block(els);
            }
            ExprKind::StructLit { fields, .. } => {
                for f in fields {
                    self.finalize_expr(f);
                }
            }
            ExprKind::Block(b) | ExprKind::Arena(b) | ExprKind::TaskGroup(b) | ExprKind::Unsafe(b) => self.finalize_block(b),
            ExprKind::RawAlloc(e) | ExprKind::RawFree(e) => self.finalize_expr(e),
            ExprKind::RawLoad { ptr, offset, .. } | ExprKind::RawOffset { ptr, offset } => {
                self.finalize_expr(ptr);
                self.finalize_expr(offset);
            }
            ExprKind::RawStore { ptr, offset, value } => {
                self.finalize_expr(ptr);
                self.finalize_expr(offset);
                self.finalize_expr(value);
            }
            ExprKind::Spawn { closure, .. } => self.finalize_expr(closure),
            ExprKind::EnumValue { payload, .. } => {
                for p in payload {
                    self.finalize_expr(p);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.finalize_expr(scrutinee);
                for a in arms {
                    self.finalize_expr(&mut a.body);
                }
            }
            ExprKind::ResultMapErr { result, f } => {
                self.finalize_expr(result);
                self.finalize_expr(f);
            }
            ExprKind::TaskGet(inner) => self.finalize_expr(inner),
            ExprKind::Wait => {}
            ExprKind::BoxGet(inner) => {
                self.finalize_expr(inner);
                // Unnecessary-heap lint (`draft.md` §16, `open-questions.md` M8 lint candidates):
                // `heap.new(x).get()` bump-allocates a box in the arena only to immediately read the
                // scalar straight back out — the allocation serves no purpose (a `box<T>` payload is a
                // scalar in M3, so `.get()` is a plain copy-out). A **warning**: the code is correct,
                // just wasteful. Detected purely locally — a `.get()` whose receiver is the allocating
                // `heap.new` *itself* — so it reuses no escape-analysis state and cannot false-positive.
                // The broader "box bound to a local, only ever `.get()`-ed, never escaping" case is
                // handled by the whole-function `UnnecessaryHeapScan` (run from `check_fn`); the two are
                // disjoint — this arm's receiver is a `HeapNew`, the scan's is a `Local`.
                if matches!(inner.kind, ExprKind::HeapNew(_)) {
                    self.diags.push(align_diag::Diagnostic::warning(
                        "unnecessary heap allocation: `heap.new(...).get()` boxes a value only to read it straight back — use the value directly (a stack value suffices)".to_string(),
                        span,
                    ));
                }
            }
            ExprKind::OptionSome(inner) | ExprKind::ResultOk(inner) | ExprKind::ResultErr(inner)
            | ExprKind::Try(inner) | ExprKind::HeapNew(inner)
            | ExprKind::BoxClone(inner) | ExprKind::StrClone(inner) | ExprKind::StrBorrow(inner) | ExprKind::BuilderToString(inner) | ExprKind::ArraySum { source: inner, .. } | ExprKind::ArrayCount { source: inner, .. } | ExprKind::ArrayAnyAll { source: inner, .. } | ExprKind::ArrayMinMax { source: inner, .. } | ExprKind::ArrayToArray { source: inner, .. } | ExprKind::ArrayToSoa { source: inner, .. } | ExprKind::ArrayPartition { source: inner, .. } | ExprKind::ArrayParMap { source: inner, .. } | ExprKind::ArraySort { source: inner, .. } | ExprKind::ArraySortBy { source: inner, .. } | ExprKind::ArrayToSlice(inner)
            | ExprKind::Len(inner) => {
                self.finalize_expr(inner)
            }
            ExprKind::Index { recv, index } | ExprKind::ElemField { recv, index, .. } => {
                self.finalize_expr(recv);
                self.finalize_expr(index);
            }
            ExprKind::SliceRange { recv, start, end } => {
                self.finalize_expr(recv);
                if let Some(s) = start { self.finalize_expr(s); }
                if let Some(e) = end { self.finalize_expr(e); }
            }
            ExprKind::BuilderWrite { builder, arg, .. } => {
                self.finalize_expr(builder);
                self.finalize_expr(arg);
            }
            ExprKind::ArrayReduce { source, init, .. } | ExprKind::ArrayScan { source, init, .. } => {
                self.finalize_expr(source);
                self.finalize_expr(init);
            }
            ExprKind::ArrayMapInto { source, dst, .. } => {
                self.finalize_expr(source);
                self.finalize_expr(dst);
            }
            ExprKind::ArrayDot { a, b, .. } => {
                self.finalize_expr(a);
                self.finalize_expr(b);
            }
            ExprKind::ArrayChunks { source, n, .. } => {
                self.finalize_expr(source);
                self.finalize_expr(n);
            }
            ExprKind::ArrayLit { elems, .. } | ExprKind::VecLit { elems, .. } => {
                for e in elems {
                    self.finalize_expr(e);
                }
            }
            ExprKind::Select { mask, a, b } => {
                self.finalize_expr(mask);
                self.finalize_expr(a);
                self.finalize_expr(b);
            }
            ExprKind::VecSumWhere { vec, mask } => {
                self.finalize_expr(vec);
                self.finalize_expr(mask);
            }
            ExprKind::VecDot { a, b } => {
                self.finalize_expr(a);
                self.finalize_expr(b);
            }
            ExprKind::VecMinMax { vec, .. } => self.finalize_expr(vec),
            ExprKind::VecSum { vec } => self.finalize_expr(vec),
            ExprKind::VecLoad { src, index, .. } => {
                self.finalize_expr(src);
                self.finalize_expr(index);
            }
            ExprKind::VecStore { dst, index, value, .. } => {
                self.finalize_expr(dst);
                self.finalize_expr(index);
                self.finalize_expr(value);
            }
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.finalize_expr(opt);
                self.finalize_expr(fallback);
            }
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Hole(h) | TemplatePart::JsonStr(h) = p {
                        self.finalize_expr(h);
                    }
                }
            }
            ExprKind::JsonDecode { input, .. } | ExprKind::JsonDecodeArray { input, .. } | ExprKind::JsonDecodeStructArray { input, .. } | ExprKind::JsonDecodeSoa { input, .. } => self.finalize_expr(input),
            ExprKind::FsReadFile { path } | ExprKind::ReaderOpen { path } | ExprKind::WriterCreate { path }
            | ExprKind::FsExists { path } | ExprKind::FsRemove { path } | ExprKind::FsReadDir { path }
            | ExprKind::FsReadFileView { path } => self.finalize_expr(path),
            ExprKind::DnsResolve { host } => self.finalize_expr(host),
            ExprKind::TcpConnect { host, port } => {
                self.finalize_expr(host);
                self.finalize_expr(port);
            }
            ExprKind::ConnReader { conn } | ExprKind::ConnWriter { conn } => self.finalize_expr(conn),
            ExprKind::TcpListen { host, port } => {
                self.finalize_expr(host);
                self.finalize_expr(port);
            }
            ExprKind::TcpAccept { listener } => self.finalize_expr(listener),
            ExprKind::UdpBind { host, port } => {
                self.finalize_expr(host);
                self.finalize_expr(port);
            }
            ExprKind::UdpSendTo { sock, data, host, port } => {
                self.finalize_expr(sock);
                self.finalize_expr(data);
                self.finalize_expr(host);
                self.finalize_expr(port);
            }
            ExprKind::UdpRecvFrom { sock, buffer } => {
                self.finalize_expr(sock);
                self.finalize_expr(buffer);
            }
            ExprKind::FsWriteFile { path, data, .. } => {
                self.finalize_expr(path);
                self.finalize_expr(data);
            }
            ExprKind::PathComponent { path, .. } | ExprKind::PathNormalize { path } => self.finalize_expr(path),
            ExprKind::PathJoin { a, b } => {
                self.finalize_expr(a);
                self.finalize_expr(b);
            }
            ExprKind::EnvGet { name } => self.finalize_expr(name),
            ExprKind::EnvSet { name, value } => {
                self.finalize_expr(name);
                self.finalize_expr(value);
            }
            ExprKind::TimeNow | ExprKind::TimeInstant => {}
            ExprKind::TimeSleep { ns } => self.finalize_expr(ns),
            ExprKind::ProcessExit { code } => self.finalize_expr(code),
            ExprKind::ProcessAbort => {}
            ExprKind::ProcessSpawn { cmd, args } => {
                self.finalize_expr(cmd);
                self.finalize_expr(args);
            }
            ExprKind::ChildWait { child } => self.finalize_expr(child),
            ExprKind::ChildKill { child, sig } => {
                self.finalize_expr(child);
                self.finalize_expr(sig);
            }
            ExprKind::ProcessExec { cmd, args } => {
                self.finalize_expr(cmd);
                self.finalize_expr(args);
            }
            ExprKind::EncodingEncode { data, .. } | ExprKind::Utf8Valid { data } => self.finalize_expr(data),
            ExprKind::EncodingDecode { input, .. } => self.finalize_expr(input),
            ExprKind::Compress { data, level, .. } => {
                self.finalize_expr(data);
                self.finalize_expr(level);
            }
            ExprKind::Decompress { data, .. } => self.finalize_expr(data),
            ExprKind::RandSeed => {}
            ExprKind::RandSeedWith { seed } => self.finalize_expr(seed),
            ExprKind::RandNext { rng } => self.finalize_expr(rng),
            ExprKind::RandRange { rng, lo, hi } => {
                self.finalize_expr(rng);
                self.finalize_expr(lo);
                self.finalize_expr(hi);
            }
            ExprKind::RandShuffle { rng, xs, .. } => {
                self.finalize_expr(rng);
                self.finalize_expr(xs);
            }
            ExprKind::RandSample { rng, xs, k, .. } => {
                self.finalize_expr(rng);
                self.finalize_expr(xs);
                self.finalize_expr(k);
            }
            ExprKind::CliCommand { name } => self.finalize_expr(name),
            ExprKind::CliFlag { cmd, name, default, .. } => {
                self.finalize_expr(cmd);
                self.finalize_expr(name);
                if let Some(d) = default {
                    self.finalize_expr(d);
                }
            }
            ExprKind::CliParse { cmd, args } => {
                self.finalize_expr(cmd);
                self.finalize_expr(args);
            }
            ExprKind::CliGetBool { parsed, name } | ExprKind::CliGetI64 { parsed, name } | ExprKind::CliGetStr { parsed, name } => {
                self.finalize_expr(parsed);
                self.finalize_expr(name);
            }
            ExprKind::CliUsage { cmd } => self.finalize_expr(cmd),
            ExprKind::HttpRequest { method, url } => {
                self.finalize_expr(method);
                self.finalize_expr(url);
            }
            ExprKind::HttpHeader { req, name, value } => {
                self.finalize_expr(req);
                self.finalize_expr(name);
                self.finalize_expr(value);
            }
            ExprKind::HttpBody { req, data } => {
                self.finalize_expr(req);
                self.finalize_expr(data);
            }
            ExprKind::HttpParse { data } => self.finalize_expr(data),
            ExprKind::HttpRespStatus { resp } | ExprKind::HttpRespBody { resp } => self.finalize_expr(resp),
            ExprKind::HttpRespHeader { resp, name } => {
                self.finalize_expr(resp);
                self.finalize_expr(name);
            }
            ExprKind::HttpClient => {}
            ExprKind::HttpClientGet { client, url } => {
                self.finalize_expr(client);
                self.finalize_expr(url);
            }
            ExprKind::HttpClientPost { client, url, body } => {
                self.finalize_expr(client);
                self.finalize_expr(url);
                self.finalize_expr(body);
            }
            ExprKind::HttpClientRequest { client, req } => {
                self.finalize_expr(client);
                self.finalize_expr(req);
            }
            ExprKind::CryptoCtEqual { a, b } => {
                self.finalize_expr(a);
                self.finalize_expr(b);
            }
            ExprKind::CryptoRandom { out } => self.finalize_expr(out),
            ExprKind::CryptoHash { data, .. } => self.finalize_expr(data),
            ExprKind::CryptoHmac { key, data } => {
                self.finalize_expr(key);
                self.finalize_expr(data);
            }
            ExprKind::CryptoHkdf { salt, ikm, info, len } => {
                self.finalize_expr(salt);
                self.finalize_expr(ikm);
                self.finalize_expr(info);
                self.finalize_expr(len);
            }
            ExprKind::CryptoAead { key, nonce, input, aad, .. } => {
                self.finalize_expr(key);
                self.finalize_expr(nonce);
                self.finalize_expr(input);
                self.finalize_expr(aad);
            }
            ExprKind::CryptoArgon2 { password, salt, params } => {
                self.finalize_expr(password);
                self.finalize_expr(salt);
                self.finalize_expr(params);
            }
            ExprKind::WriterWrite { writer, arg, .. } => {
                self.finalize_expr(writer);
                self.finalize_expr(arg);
            }
            ExprKind::WriterFlush { writer } => self.finalize_expr(writer),
            ExprKind::ReaderRead { reader, buffer } => {
                self.finalize_expr(reader);
                self.finalize_expr(buffer);
            }
            ExprKind::IoCopy { reader, writer } => {
                self.finalize_expr(reader);
                self.finalize_expr(writer);
            }
            ExprKind::BufferBytes { buffer } | ExprKind::BufferLen { buffer } => self.finalize_expr(buffer),
            ExprKind::BufferNew { capacity } => self.finalize_expr(capacity),
            ExprKind::StrPredicate { haystack, needle, .. } => {
                self.finalize_expr(haystack);
                self.finalize_expr(needle);
            }
            ExprKind::StrTrim { recv, .. } => self.finalize_expr(recv),
            ExprKind::Tuple { elems, .. } => {
                for el in elems {
                    self.finalize_expr(el);
                }
            }
            ExprKind::MathOp { operands, .. } => {
                for o in operands {
                    self.finalize_expr(o);
                }
            }
            ExprKind::TupleIndex { recv, .. } => self.finalize_expr(recv),
            ExprKind::BuilderNew { capacity } => {
                if let Some(c) = capacity {
                    self.finalize_expr(c);
                }
            }
            // A bare (non-negated) integer literal: reject it if its value provably overflows the
            // type inference resolved for it. A negated literal (`-lit`) is handled by the `Unary`
            // arm above (which skips this by finalizing the inner literal itself), so a value that
            // reaches here is always the literal's own effective value.
            ExprKind::Int(v) => self.check_int_lit_range(*v, cur_ty, span),
            ExprKind::Unit
            | ExprKind::Float(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Local(_)
            | ExprKind::OptionNone
            | ExprKind::WriterStd { .. }
            | ExprKind::ReaderStdin
            | ExprKind::Field { .. }
            | ExprKind::SoaColumn { .. }
            | ExprKind::ArrayGroupAgg { .. }
            | ExprKind::ArrayGroupAggMulti { .. }
            | ExprKind::ArrayDictEncode { .. }
            | ExprKind::IndexField { .. } => {}
        }
        if let Some(t) = recomputed {
            e.ty = t;
        }
    }
}

/// Whether a block always diverges (no tail value and its last statement is `return`),
/// so it never yields a value and need not match an expected value type.
fn ast_block_diverges(b: &ast::Block) -> bool {
    b.tail.is_none() && matches!(b.stmts.last(), Some(ast::Stmt::Return(_)))
}

/// Whether a braced `else { … }` fallback diverges (its last statement is `return`),
/// in which case it produces no value and need not match the payload type.
fn block_diverges(e: &ast::Expr) -> bool {
    match &e.kind {
        ast::ExprKind::Block(b) => ast_block_diverges(b),
        _ => false,
    }
}

/// Whether `recv` is an **array-pipeline-shaped** receiver — a `.map()`/`.where()` stage call or a
/// `.field` projection. Such a receiver is an array pipeline and must NOT be type-checked as a value
/// (a pipeline without a terminal is an error); every other receiver (a local, a call, an arithmetic
/// expression) is a value that may be a vector. Used to route `recv.min()`/`recv.max()`. (Struct
/// fields are never vectors, so a `FieldAccess` is always an array projection, not a vector value.)
fn is_array_pipeline_recv(recv: &ast::Expr) -> bool {
    match &recv.kind {
        ast::ExprKind::FieldAccess { .. } => true,
        ast::ExprKind::Call { callee, .. } => matches!(
            &callee.kind,
            ast::ExprKind::FieldAccess { field, .. } if field.name == "map" || field.name == "where"
        ),
        _ => false,
    }
}

fn single_name(p: &ast::Path) -> Option<&str> {
    if p.segments.len() == 1 {
        Some(p.segments[0].name.as_str())
    } else {
        None
    }
}

/// Whether `e` is syntactically a `heap.new(...)` call — the box-allocating builtin. A `heap.new`
/// literal payload defaults to `i64` on its own; recognising the receiver lets `heap.new(x).get()` /
/// `.clone()` thread the caller's expected type into the payload so it infers the right width.
fn is_heap_new_call(e: &ast::Expr) -> bool {
    matches!(&e.kind, ast::ExprKind::Call { callee, .. }
        if matches!(&callee.kind, ast::ExprKind::FieldAccess { recv, field }
            if field.name == "new"
                && matches!(&recv.kind, ast::ExprKind::Path(p) if single_name(p) == Some("heap"))))
}

// --- unused-import lint -----------------------------------------------------------------------
//
// An `import` is **used** if the module's source contains a qualified reference whose dotted prefix
// matches it — `geom.area(...)`, `geom.Point { }`, `geom.Color.Red`, `geom.MAX`, or a type
// `geom.Point` all contribute the prefix `geom` (or `util.math` for a nested module); a builtin
// `core.json` import is matched by the `json.*` namespace. We collect those prefixes with a syntactic
// AST walk (independent of the resolution code, so signatures / bodies / consts are covered
// uniformly), then warn on any import that never appears. The walk over-approximates "used" (a local
// shadowing a module name still counts), so the lint never *wrongly* fires.

/// The dotted module prefix of a path used as a qualified reference (all segments but the last),
/// e.g. `geom.Point` → `geom`, `util.math.Foo` → `util.math`. `None` for a single-segment path.
fn path_module_prefix(p: &ast::Path) -> Option<String> {
    if p.segments.len() < 2 {
        return None;
    }
    Some(p.segments[..p.segments.len() - 1].iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join("."))
}

fn collect_refs(items: &[ast::Item], out: &mut std::collections::HashSet<String>) {
    for item in items {
        match item {
            ast::Item::Fn(f) => {
                for p in &f.params {
                    walk_type(&p.ty, out);
                }
                if let Some(t) = &f.ret {
                    walk_type(t, out);
                }
                match &f.body {
                    ast::FnBody::Block(b) => walk_block(b, out),
                    ast::FnBody::Expr(e) => walk_expr(e, out),
                }
            }
            ast::Item::Struct(s) => {
                for fd in &s.fields {
                    walk_type(&fd.ty, out);
                }
            }
            ast::Item::Enum(e) => {
                for v in &e.variants {
                    for t in &v.payload {
                        walk_type(t, out);
                    }
                }
            }
            ast::Item::Const(c) => {
                if let Some(t) = &c.ty {
                    walk_type(t, out);
                }
                walk_expr(&c.value, out);
            }
            ast::Item::Extern(blk) => {
                for sig in &blk.fns {
                    for p in &sig.params {
                        walk_type(&p.ty, out);
                    }
                    if let Some(t) = &sig.ret {
                        walk_type(t, out);
                    }
                }
            }
        }
    }
}

fn walk_type(t: &ast::Type, out: &mut std::collections::HashSet<String>) {
    match t {
        ast::Type::Named { path, args, .. } => {
            if let Some(prefix) = path_module_prefix(path) {
                out.insert(prefix);
            }
            for a in args {
                walk_type(a, out);
            }
        }
        ast::Type::Tuple { elems, .. } => elems.iter().for_each(|e| walk_type(e, out)),
        ast::Type::Fn { params, ret, .. } => {
            params.iter().for_each(|p| walk_type(p, out));
            walk_type(ret, out);
        }
    }
}

fn walk_block(b: &ast::Block, out: &mut std::collections::HashSet<String>) {
    for s in &b.stmts {
        match s {
            ast::Stmt::Let { ty, init, .. } => {
                if let Some(t) = ty {
                    walk_type(t, out);
                }
                walk_expr(init, out);
            }
            ast::Stmt::LetTuple { init, .. } => walk_expr(init, out),
            ast::Stmt::Assign { place, value } => {
                walk_expr(place, out);
                walk_expr(value, out);
            }
            ast::Stmt::Return(Some(e)) | ast::Stmt::Expr(e) => walk_expr(e, out),
            ast::Stmt::Return(None) => {}
        }
    }
    if let Some(tail) = &b.tail {
        walk_expr(tail, out);
    }
}

fn walk_expr(e: &ast::Expr, out: &mut std::collections::HashSet<String>) {
    use ast::ExprKind as K;
    match &e.kind {
        K::Unit | K::Int(_) | K::Float(_) | K::Char(_) | K::Str(_) | K::Bool(_) | K::FieldShorthand(_) => {}
        K::Path(p) => {
            if let Some(prefix) = path_module_prefix(p) {
                out.insert(prefix);
            }
        }
        K::Unary { expr, .. } | K::Try(expr) | K::TupleIndex { recv: expr, .. } => walk_expr(expr, out),
        K::Cast { expr, ty } => {
            walk_expr(expr, out);
            walk_type(ty, out);
        }
        K::Binary { lhs, rhs, .. } => {
            walk_expr(lhs, out);
            walk_expr(rhs, out);
        }
        K::Call { callee, args } => {
            walk_expr(callee, out);
            args.iter().for_each(|a| walk_expr(a, out));
        }
        K::FieldAccess { recv, .. } => {
            // The whole dotted receiver (`geom`, `util.math`, `geom.Color`) is the qualified prefix.
            if let Some(prefix) = flatten_module_path(recv) {
                out.insert(prefix);
            } else {
                walk_expr(recv, out);
            }
        }
        K::If { cond, then, els } => {
            walk_expr(cond, out);
            walk_block(then, out);
            if let Some(e) = els {
                walk_expr(e, out);
            }
        }
        K::Block(b) | K::Arena(b) | K::TaskGroup(b) | K::Unsafe(b) => walk_block(b, out),
        K::StructLit { name, fields } => {
            if let Some(prefix) = path_module_prefix(name) {
                out.insert(prefix);
            }
            fields.iter().for_each(|f| walk_expr(&f.value, out));
        }
        K::ElseUnwrap { opt, fallback } => {
            walk_expr(opt, out);
            walk_expr(fallback, out);
        }
        K::ArrayLit(es) | K::Tuple(es) => es.iter().for_each(|e| walk_expr(e, out)),
        K::Index { recv, index } => {
            walk_expr(recv, out);
            walk_expr(index, out);
        }
        K::SliceRange { recv, start, end } => {
            walk_expr(recv, out);
            if let Some(s) = start { walk_expr(s, out); }
            if let Some(en) = end { walk_expr(en, out); }
        }
        K::Lambda { params, body } => {
            for p in params {
                if let Some(t) = &p.ty {
                    walk_type(t, out);
                }
            }
            walk_block(body, out);
        }
        K::Match { scrutinee, arms } => {
            walk_expr(scrutinee, out);
            arms.iter().for_each(|a| walk_expr(&a.body, out));
        }
        K::Template(parts) => {
            for p in parts {
                if let ast::TemplatePart::Hole(e) = p {
                    walk_expr(e, out);
                }
            }
        }
    }
}

/// Flatten a receiver that is a pure dotted name into a module path string: `geom` → `"geom"`,
/// `util.math` → `"util.math"` (a `Path` or a chain of field accesses over idents). `None` if the
/// receiver is anything else (a call result, an index, …), so a value-method receiver never matches.
/// Builds the path in a single allocation rather than one per field-access level.
fn flatten_module_path(e: &ast::Expr) -> Option<String> {
    let mut path = String::new();
    flatten_module_path_into(e, &mut path).then_some(path)
}

fn flatten_module_path_into(e: &ast::Expr, out: &mut String) -> bool {
    match &e.kind {
        ast::ExprKind::Path(p) => {
            for (i, s) in p.segments.iter().enumerate() {
                if i > 0 {
                    out.push('.');
                }
                out.push_str(&s.name);
            }
            true
        }
        ast::ExprKind::FieldAccess { recv, field } => {
            flatten_module_path_into(recv, out) && {
                out.push('.');
                out.push_str(&field.name);
                true
            }
        }
        _ => false,
    }
}

/// The leftmost identifier of a dotted receiver (`util.math.cube` → `"util"`); `None` if the
/// receiver root is not a plain name. Used to let an in-scope local shadow a module of that name.
fn leftmost_segment(e: &ast::Expr) -> Option<&str> {
    match &e.kind {
        ast::ExprKind::Path(p) => p.segments.first().map(|s| s.name.as_str()),
        ast::ExprKind::FieldAccess { recv, .. } => leftmost_segment(recv),
        _ => None,
    }
}

/// Every importable builtin module (`draft.md` §18). `core.*` is language-intrinsic; `std.*` is the
/// OS boundary. Both are compiler builtins today (std becomes real Align-over-FFI library code only
/// post-M8). An `import` naming anything outside this set is an error — user-authored modules are a
/// later slice (`open-questions.md` module system).
const BUILTIN_MODULES: &[&str] = &[
    // core — language-intrinsic primitives
    "core.option", "core.result", "core.array", "core.slice", "core.chunks", "core.vec",
    "core.mask", "core.bitset", "core.map", "core.reduce", "core.scan", "core.partition",
    "core.sort", "core.str", "core.string", "core.bytes", "core.buffer", "core.builder",
    "core.arena", "core.json", "core.template", "core.hash", "core.math",
    // std — the OS boundary
    "std.io", "std.fs", "std.path", "std.process", "std.env", "std.time", "std.net",
    "std.cli", "std.encoding", "std.compress", "std.rand", "std.crypto", "std.http",
];

/// A dotted path's segments joined with `.` (`core` `.` `json` → `"core.json"`).
fn path_str(p: &ast::Path) -> String {
    p.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(".")
}

/// Types `print` and a `template` hole can render: integers, floats, `str`, `bool`, `char`
/// (and the error sentinel, to avoid cascading diagnostics).
fn is_printable(ty: Ty) -> bool {
    ty.is_numeric() || matches!(ty, Ty::Str | Ty::String | Ty::Bool | Ty::Char | Ty::Error)
}

/// Map a method name to the builder writer it denotes (MMv2 slice 7c/7d), if any.
fn builder_write_kind(method: &str) -> Option<BuilderWriteKind> {
    Some(match method {
        "write" => BuilderWriteKind::Str,
        "write_int" => BuilderWriteKind::Int,
        "write_bool" => BuilderWriteKind::Bool,
        "write_char" => BuilderWriteKind::Char,
        "write_float" => BuilderWriteKind::Float,
        _ => return None,
    })
}

/// The surface method name of a builder writer (for diagnostics).
fn builder_write_method_name(kind: BuilderWriteKind) -> &'static str {
    match kind {
        BuilderWriteKind::Str => "write",
        BuilderWriteKind::Int => "write_int",
        BuilderWriteKind::Bool => "write_bool",
        BuilderWriteKind::Char => "write_char",
        BuilderWriteKind::Float => "write_float",
    }
}

fn ty_name(ty: Ty) -> String {
    match ty {
        Ty::Int(it) => it.name(),
        Ty::IntVar(_) => "int(undetermined)".to_string(),
        Ty::Float(ft) => ft.name(),
        Ty::FloatVar(_) => "float(undetermined)".to_string(),
        Ty::Bool => "bool".to_string(),
        Ty::Char => "char".to_string(),
        Ty::Option(s) => format!("Option<{}>", scalar_name(s)),
        Ty::Result(o, e) => format!("Result<{}, {}>", scalar_name(o), scalar_name(e)),
        Ty::Box(s) => format!("box<{}>", scalar_name(s)),
        Ty::Array(s, n) => format!("array<{}>[{n}]", scalar_name(s)),
        Ty::Vec(s, n) => format!("vec{n}<{}>", scalar_name(s)),
        Ty::Mask(s, n) => format!("mask{n}<{}>", scalar_name(s)),
        Ty::StructArray(id, n) => format!("array<struct#{id}>[{n}]"),
        Ty::DynStructArray(id, _) => format!("array<struct#{id}>"),
        Ty::DynSliceArray(p) => format!("array<slice<{}>>", scalar_name(prim_to_scalar(p))),
        Ty::Slice(s) => format!("slice<{}>", scalar_name(s)),
        Ty::Soa(id) => format!("soa<struct#{id}>"),
        Ty::DynArray(s) => format!("array<{}>", scalar_name(s)),
        Ty::Str => "str".to_string(),
        Ty::String => "string".to_string(),
        Ty::ArenaHandle => "arena".to_string(),
        Ty::Raw => "raw".to_string(),
        Ty::Builder => "builder".to_string(),
        // The surface type names (`fn f(w: writer)`), so diagnostics match what the user writes.
        Ty::Writer => "writer".to_string(),
        Ty::Reader => "reader".to_string(),
        Ty::Buffer => "buffer".to_string(),
        Ty::Rng => "rng".to_string(),
        Ty::CliCommand => "cli command".to_string(),
        Ty::CliParsed => "cli parsed".to_string(),
        Ty::TcpConn => "tcp_conn".to_string(),
        Ty::TcpListener => "tcp_listener".to_string(),
        Ty::UdpSocket => "udp_socket".to_string(),
        Ty::Child => "child".to_string(),
        Ty::HttpRequest => "http request".to_string(),
        Ty::HttpResponse => "http response".to_string(),
        Ty::HttpClient => "http client".to_string(),
        Ty::Struct(id) => format!("struct#{id}"),
        Ty::Tuple(id) => format!("tuple#{id}"),
        Ty::Fn(id) => format!("fn#{id}"),
        Ty::Enum(id) => format!("enum#{id}"),
        Ty::Task(s) => format!("Task<{}>", scalar_name(s)),
        Ty::DictEncoded(id, _) => format!("dict_encoded<struct#{id}>"),
        Ty::Param(i) => format!("<type param {i}>"),
        Ty::Unit => "()".to_string(),
        Ty::Error => "<error>".to_string(),
    }
}

/// Classify an `as` conversion for the **lossy-conversion** lint (`draft.md` §16, "Numeric
/// conversion — as"). Returns a short reason when the conversion *may* lose information — a
/// defined, non-error conversion the programmer should be aware of — or `None` when it is
/// value-preserving (lossless) and needs no warning. `from`/`to` are the concrete operand /
/// target types (both numeric or `char`); `char` behaves as a 32-bit unsigned code point, exactly
/// as codegen treats it (`gen_cast`). Only *narrowing* / precision-losing / truncating conversions
/// warn — same-width, widening, and same-width sign changes (`u8 as i8`) keep every bit and are
/// treated as lossless (the settled "`as` covers lossless, truncating, and saturating conversions
/// alike" — the lint flags the last two so they are visible, never blocks them). A source still an
/// inference variable (an unconstrained literal like `1 as i8`, which the programmer is explicitly
/// typing) is not a concrete int/float here, so it is silently accepted — the lint targets *typed*
/// values, not literal annotations.
fn cast_loss(from: Ty, to: Ty) -> Option<&'static str> {
    // `char` is a 21-bit code point stored in 32 bits; for capacity analysis treat it as `u32`.
    let int_bits = |t: Ty| -> Option<u8> {
        match t {
            Ty::Int(it) => Some(it.bits),
            Ty::Char => Some(32),
            _ => None,
        }
    };
    let float_bits = |t: Ty| -> Option<u8> {
        if let Ty::Float(ft) = t { Some(ft.bits) } else { None }
    };
    match (float_bits(from), float_bits(to)) {
        // float → float: narrowing (f64 → f32) drops precision; widening / equal is lossless.
        (Some(fb), Some(tb)) => {
            (tb < fb).then_some("narrows a float to fewer bits, which may lose precision")
        }
        // float → int: the fractional part is always dropped and out-of-range values saturate.
        (Some(_), None) => {
            Some("truncates the fractional part (out-of-range values saturate to MIN/MAX)")
        }
        // int/char → float: a source wider than the float's mantissa loses precision on large
        // values. Mantissa bits: f32 = 24, f64 = 53. (`char → float` is a sema error, unreachable.)
        (None, Some(tb)) => {
            let fb = int_bits(from)?;
            let mantissa = if tb <= 32 { 24 } else { 53 };
            (fb > mantissa)
                .then_some("the integer is wider than the float's mantissa, so large values lose precision")
        }
        // int/char → int/char: strict narrowing truncates the high bits. Same-width (incl. a sign
        // change like `u8 as i8`) and widening keep every bit, so they are lossless here.
        (None, None) => {
            let (fb, tb) = (int_bits(from)?, int_bits(to)?);
            (tb < fb).then_some("truncates the high bits")
        }
    }
}

/// Whether `e` is a compile-time numeric literal — an int/float literal, possibly behind a chain of
/// unary `-` (`-1`, `--128`). Such an operand in `x as T` is an explicit annotation on a constant
/// (`1 as i8`, `-1.0 as f32`), not a typed value being narrowed, so the lossy-conversion lint skips
/// it: a bare literal always keeps its own default width and never carries a runtime value that a
/// narrowing would silently lose (a provably-out-of-range constant is the separate out-of-range
/// literal lint's concern, `open-questions.md` M8 lint candidates — not this one).
fn is_numeric_literal(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) => true,
        ExprKind::Unary { op: UnOp::Neg, expr } => is_numeric_literal(expr),
        _ => false,
    }
}

/// Substitute a generic function's type parameters: `Ty::Param(i)` → `args[i]`. The skeleton keeps
/// a type parameter in a **bare** position (never nested inside `Option`/`array`/a tuple — `Scalar`
/// can't hold a `Param`), so this is a single top-level replacement. Used by both call-result typing
/// and monomorphization.
fn subst_param_ty(ty: Ty, args: &[Ty]) -> Ty {
    // A `Param` nested in a scalar-payload composite (`Option<T>` / `Result<T, E>` / `box<T>` /
    // `slice<T>` / `array<T>` fixed / `Task<T>`). Tuples/structs/`array<T>` dynamic carry their
    // element via an interner or `PrimScalar` and are not supported in a nested generic position yet.
    match ty {
        Ty::Param(i) => args.get(i as usize).copied().unwrap_or(Ty::Error),
        Ty::Option(s) => Ty::Option(subst_scalar(s, args)),
        Ty::Result(o, e) => Ty::Result(subst_scalar(o, args), subst_scalar(e, args)),
        Ty::Box(s) => Ty::Box(subst_scalar(s, args)),
        Ty::Slice(s) => Ty::Slice(subst_scalar(s, args)),
        Ty::Array(s, n) => Ty::Array(subst_scalar(s, args), n),
        Ty::Task(s) => Ty::Task(subst_scalar(s, args)),
        other => other,
    }
}

/// Substitute a `Scalar::Param(i)` with the scalar form of `args[i]` (a generic enum's variant
/// payload, or a composite payload). A non-`Param` scalar is unchanged.
fn subst_scalar(s: Scalar, args: &[Ty]) -> Scalar {
    match s {
        Scalar::Param(i) => ty_to_scalar(args.get(i as usize).copied().unwrap_or(Ty::Error)).unwrap_or(s),
        other => other,
    }
}

/// Whether a type carries a generic `Param` — bare, or nested one level in a scalar-payload
/// composite. Used to decide whether a call argument applies a coercion (it does not when the
/// parameter type is generic).
/// Mark every type parameter that appears **nested** in a composite (not a bare `Ty::Param`):
/// such a parameter must resolve to a concrete scalar at the call (a `Scalar` can't hold an
/// inference variable).
fn mark_nested_params(ty: Ty, nested: &mut [bool]) {
    let mut mark = |s: Scalar| {
        if let Scalar::Param(p) = s {
            nested[p as usize] = true;
        }
    };
    match ty {
        Ty::Option(s) | Ty::Box(s) | Ty::Slice(s) | Ty::Array(s, _) | Ty::Task(s) => mark(s),
        Ty::Result(o, e) => {
            mark(o);
            mark(e);
        }
        _ => {}
    }
}

fn ty_mentions_param(ty: Ty) -> bool {
    match ty {
        Ty::Param(_) => true,
        Ty::Option(s) | Ty::Box(s) | Ty::Slice(s) | Ty::Array(s, _) | Ty::Task(s) => matches!(s, Scalar::Param(_)),
        Ty::Result(o, e) => matches!(o, Scalar::Param(_)) || matches!(e, Scalar::Param(_)),
        _ => false,
    }
}

/// The mangled symbol name of a monomorph instance: `name` + `$` + each concrete type argument
/// (`pick` with `[i32]` → `pick$i32`). Deterministic and collision-free across instantiations.
fn mangle_mono(name: &str, args: &[Ty]) -> String {
    let mut s = name.to_string();
    for a in args {
        s.push('$');
        s.push_str(&ty_mangle(*a));
    }
    s
}

/// A compact, identifier-safe spelling of a concrete type for use in a mangled symbol name.
fn ty_mangle(ty: Ty) -> String {
    ty_name(ty).chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect()
}

/// A composite type argument must resolve to a concrete scalar in M2.
fn scalar_arg(ty: Ty, what: &str, allow_param: bool, span: Span, diags: &mut Diagnostics) -> Option<Scalar> {
    // A generic type parameter is a valid payload only where 4c-3 supports it (`Option`/`Result`);
    // `box`/`slice`/`array` over a `T` are not supported yet, so reject `Param` there.
    if matches!(ty, Ty::Param(_)) && !allow_param {
        diags.error(format!("{what} cannot be a generic type parameter yet, got {}", ty_name(ty)), span);
        return None;
    }
    // An owned I/O handle (`reader`/`writer`) is bound to exactly one local and closes its fd once
    // at that binding's `Drop`. It may ride in an `Option`/`Result` payload (`fs.open`/`fs.create`
    // return `Result<reader/writer, Error>`) — the `allow_param` positions — but **never** as an
    // array / slice / vec / box **element**: an element read copies the handle by value (no
    // move-out), so two copies would close/free the same fd (double close + double `Box::from_raw`
    // = UB). A `buffer` is never a payload at all. Reject at the type, matching `is_field_ok` /
    // tuple elements (which also refuse these handles).
    // A `cli command` is never a payload (like `buffer`); a `cli parsed` may ride a `Result` Ok
    // payload (`c.parse(args)`) — the `allow_param` positions — but never as an array/slice/box
    // element (an element read would copy + double-free the handle), same as `reader`/`writer`.
    // A `tcp_conn` / `tcp_listener` / `udp_socket` follows `reader`/`writer` exactly: a `Result` Ok
    // payload (`tcp.connect`/`l.accept` for the conn, `tcp.listen` for the listener, `udp.bind` for
    // the socket) is fine, but never an array/slice/box element (a copied handle would
    // double-`close` its fd).
    if matches!(ty, Ty::Buffer | Ty::CliCommand | Ty::HttpRequest) || (matches!(ty, Ty::Reader | Ty::Writer | Ty::CliParsed | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child | Ty::HttpResponse | Ty::HttpClient) && !allow_param) {
        diags.error(
            format!("{what} cannot be `{}` — an owned I/O handle/buffer is bound to one local, not collected into an array/slice/box (bind it to a local)", ty_name(ty)),
            span,
        );
        return None;
    }
    match ty_to_scalar(ty) {
        Some(s) => Some(s),
        None => {
            if ty != Ty::Error {
                diags.error(format!("{what} must be a scalar (composite payloads are not supported yet), got {}", ty_name(ty)), span);
            }
            None
        }
    }
}

/// An `Option`/`Result` payload may not be a **Move struct** (one that owns a `string`/owned field):
/// the aggregate's drop is scalar-shaped (`payload_is_move` / `move_payload_fields` free a flat
/// `{ptr,len}`) and does not recurse into a struct's owned fields, so an owned-struct payload would
/// leak / double-free. Maps such a payload to `None` (with an error); passes anything else through
/// unchanged (plain-data and `str`-bearing struct payloads keep their pre-Slice-3 behavior).
fn reject_move_struct_payload(s: Option<Scalar>, structs: &[StructDef], what: &str, span: Span, diags: &mut Diagnostics) -> Option<Scalar> {
    match s {
        Some(Scalar::Struct(id)) if struct_is_move(id, structs) => {
            diags.error(
                format!("{what} cannot be the Move struct '{}' yet (its owned fields would not be dropped)", structs[id as usize].name),
                span,
            );
            None
        }
        other => other,
    }
}

/// Intern a tuple type (dedup by element list) into `tuples`, returning its id. Tuples are
/// few, so a linear scan is fine.
/// Match `X.group_by(.key)`, returning the source expr `X` and the key field ident. Used to detect
/// the grouped-sum chain `X.group_by(.key).sum(.value)` from the outer `.sum(.value)` call.
fn as_group_by(recv: &ast::Expr) -> Option<(&ast::Expr, &ast::Ident)> {
    let ast::ExprKind::Call { callee, args } = &recv.kind else { return None };
    let ast::ExprKind::FieldAccess { recv: src, field } = &callee.kind else { return None };
    if field.name != "group_by" {
        return None;
    }
    let [arg] = args.as_slice() else { return None };
    let ast::ExprKind::FieldShorthand(key) = &arg.kind else { return None };
    Some((src, key))
}

fn intern_tuple(tuples: &mut Vec<hir::TupleDef>, elems: Vec<Scalar>) -> u32 {
    if let Some(i) = tuples.iter().position(|t| t.elems == elems) {
        return i as u32;
    }
    tuples.push(hir::TupleDef { elems });
    (tuples.len() - 1) as u32
}

fn intern_fn_type(fn_types: &mut Vec<hir::FnTy>, params: Vec<Scalar>, ret: Scalar) -> u32 {
    let ft = hir::FnTy { params, ret };
    if let Some(i) = fn_types.iter().position(|t| *t == ft) {
        return i as u32;
    }
    fn_types.push(ft);
    (fn_types.len() - 1) as u32
}

/// A generic struct declaration (`Pair<T> { a: T, b: T }`): its type-parameter names and its
/// fields with `Ty::Param` in the parameter positions. Monomorphized on demand by `resolve_type`
/// when a concrete `Pair<i32>` is resolved — kept out of the concrete `structs` table (and thus out
/// of codegen) until then.
#[derive(Clone)]
struct StructTemplate {
    type_params: Vec<String>,
    fields: Vec<hir::FieldDef>,
    /// An `align(N)` over-alignment carried from the template to every monomorph (M6).
    align: Option<u32>,
    /// A `layout(C)` marker carried from the template to every monomorph.
    c_repr: bool,
}

/// A generic sum-type declaration (`Opt<T> { Some(T), None }`): its type-parameter names and its
/// variants with `Scalar::Param` payloads. Monomorphized on demand like a struct template — kept
/// out of the concrete `enums` table (and thus out of codegen) until instantiated.
#[derive(Clone)]
struct EnumTemplate {
    type_params: Vec<String>,
    variants: Vec<hir::EnumVariant>,
}

/// The mutable + shared type-resolution context threaded through [`resolve_type`]: the concrete
/// struct / enum tables (grown with monomorph instances), the generic templates + their monomorph
/// caches, and the tuple / `Ty::Fn` interners.
struct TyCx<'a> {
    /// The module whose body is being resolved — a bare type name resolves here.
    cur_module: &'a str,
    /// The user modules `cur_module` imports — a qualified `mod.Type` must name one (or `cur_module`).
    imports: &'a std::collections::HashSet<String>,
    /// Every module's bare→(canonical, pub?) type names, for qualified-type resolution.
    type_table: &'a ModTypes,
    struct_ids: &'a HashMap<String, u32>,
    enum_ids: &'a HashMap<String, u32>,
    /// Generic struct templates, by name (not in `structs` — they carry `Param` fields).
    struct_templates: &'a HashMap<String, StructTemplate>,
    /// Concrete struct table; monomorph instances of generic structs are appended here.
    structs: &'a mut Vec<StructDef>,
    /// Mangled monomorph name (`Pair$i32`) -> `structs` index — dedups monomorph instances.
    struct_mono: &'a mut HashMap<String, u32>,
    /// Generic sum-type templates, by name (not in `enums` — they carry `Scalar::Param` payloads).
    enum_templates: &'a HashMap<String, EnumTemplate>,
    /// Concrete enum table; monomorph instances of generic sum types are appended here.
    enums: &'a mut Vec<hir::EnumDef>,
    /// Mangled monomorph name (`Opt$i32`) -> `enums` index — dedups monomorph instances.
    enum_mono: &'a mut HashMap<String, u32>,
    tuples: &'a mut Vec<hir::TupleDef>,
    fn_types: &'a mut Vec<hir::FnTy>,
}

fn resolve_type(
    t: &ast::Type,
    cx: &mut TyCx,
    type_params: &[String],
    diags: &mut Diagnostics,
) -> Ty {
    let (path, args, span) = match t {
        ast::Type::Named { path, args, span } => (path, args.as_slice(), *span),
        // `fn(T, U) -> R` — a function-value type. Scalar parameters/return (matching first-class
        // function values); interned into `fn_types` like a tuple type.
        ast::Type::Fn { params, ret, span: _ } => {
            let mut pscalars = Vec::with_capacity(params.len());
            for p in params {
                let pty = resolve_type(p, cx, type_params, diags);
                if pty == Ty::Error {
                    return Ty::Error;
                }
                match ty_to_scalar(pty) {
                    Some(s) => pscalars.push(s),
                    None => {
                        diags.error(format!("a function-type parameter must be a scalar for now, got {}", ty_name(pty)), p.span());
                        return Ty::Error;
                    }
                }
            }
            let rty = resolve_type(ret, cx, type_params, diags);
            if rty == Ty::Error {
                return Ty::Error;
            }
            let Some(rs) = ty_to_scalar(rty) else {
                diags.error(format!("a function-type return must be a scalar for now, got {}", ty_name(rty)), ret.span());
                return Ty::Error;
            };
            return Ty::Fn(intern_fn_type(cx.fn_types, pscalars, rs));
        }
        ast::Type::Tuple { elems, span: _ } => {
            // PR1 cut: tuple elements are primitive scalars (int/float/bool/char) — Copy,
            // `Static`, so the tuple needs no drop/region machinery. `str`/owned elements later.
            let mut scalars = Vec::with_capacity(elems.len());
            for e in elems {
                let ety = resolve_type(e, cx, type_params, diags);
                if ety == Ty::Error {
                    return Ty::Error;
                }
                match ty_to_scalar(ety) {
                    Some(s @ (Scalar::Int(_) | Scalar::Float(_) | Scalar::Bool | Scalar::Char
                    | Scalar::Str | Scalar::String | Scalar::DynArray(_) | Scalar::DynStructArray(_))) => scalars.push(s),
                    _ => {
                        diags.error(
                            format!("tuple elements must be a scalar, str, owned string, or owned array for now, got {}", ty_name(ety)),
                            e.span(),
                        );
                        return Ty::Error;
                    }
                }
            }
            return Ty::Tuple(intern_tuple(cx.tuples, scalars));
        }
    };
    let name = path.segments.last().map(|s| s.name.as_str()).unwrap_or("");
    // A qualified type `mod.Type` (or `a.b.Type`) is always a user type — never a builtin keyword or
    // a generic parameter. Resolve it via the type table (import + `pub` checked) directly.
    if path.segments.len() > 1 {
        return match canonical_type_name(path, cx.cur_module, cx.imports, cx.type_table, true, span, diags) {
            Some(canonical) => resolve_user_type(&canonical, args, cx, type_params, span, diags),
            None => Ty::Error,
        };
    }
    // A generic type parameter (`fn f<T>` → `T`): a bare name (no type arguments) matching a
    // declared parameter resolves to `Ty::Param(i)`.
    if args.is_empty()
        && let Some(i) = type_params.iter().position(|p| p == name) {
            return Ty::Param(i as u32);
        }
    match name {
        "bool" => Ty::Bool,
        "char" => Ty::Char,
        "str" => Ty::Str,
        "string" => Ty::String,
        // `raw` — an opaque raw byte pointer (`raw.alloc` yields one). Nameable so it can be a `let`
        // annotation / function parameter (holding a `raw` is safe; only `raw.*` ops need `unsafe`).
        "raw" => Ty::Raw,
        "f32" => Ty::Float(FloatTy { bits: 32 }),
        "f64" => Ty::Float(FloatTy { bits: 64 }),
        "()" => Ty::Unit,
        // `writer` / `reader` / `buffer` — the std.io / core.buffer Move handles. Surface type names
        // so they can be threaded through functions (each is a Move handle; passed by value).
        "writer" => {
            if !args.is_empty() {
                diags.error("writer takes no type arguments".to_string(), span);
                return Ty::Error;
            }
            Ty::Writer
        }
        "reader" => {
            if !args.is_empty() {
                diags.error("reader takes no type arguments".to_string(), span);
                return Ty::Error;
            }
            Ty::Reader
        }
        "buffer" => {
            if !args.is_empty() {
                diags.error("buffer takes no type arguments".to_string(), span);
                return Ty::Error;
            }
            Ty::Buffer
        }
        // `rng` (`std.rand`) — a Copy state-only random-generator value. A surface type name so it
        // can be a `let` annotation / function parameter (an rng is Copy, passed/returned by value).
        "rng" => {
            if !args.is_empty() {
                diags.error("rng takes no type arguments".to_string(), span);
                return Ty::Error;
            }
            Ty::Rng
        }
        // `tcp_conn` (`std.net`) — a connected TCP socket Move handle (`tcp.connect`). A surface type
        // name so it can be threaded through functions (a Move handle; passed by value).
        "tcp_conn" => {
            if !args.is_empty() {
                diags.error("tcp_conn takes no type arguments".to_string(), span);
                return Ty::Error;
            }
            Ty::TcpConn
        }
        // `tcp_listener` (`std.net`) — a listening TCP socket Move handle (`tcp.listen`). A surface
        // type name so it can be threaded through functions (a Move handle; passed by value).
        "tcp_listener" => {
            if !args.is_empty() {
                diags.error("tcp_listener takes no type arguments".to_string(), span);
                return Ty::Error;
            }
            Ty::TcpListener
        }
        // `udp_socket` (`std.net`) — a bound UDP (`SOCK_DGRAM`) socket Move handle (`udp.bind`). A
        // surface type name so it can be threaded through functions (a Move handle; passed by value).
        "udp_socket" => {
            if !args.is_empty() {
                diags.error("udp_socket takes no type arguments".to_string(), span);
                return Ty::Error;
            }
            Ty::UdpSocket
        }
        // `child` (`std.process`) — a spawned child process Move handle (`process.spawn`). A surface
        // type name so it can be threaded through functions (a Move handle; passed by value).
        "child" => {
            if !args.is_empty() {
                diags.error("child takes no type arguments".to_string(), span);
                return Ty::Error;
            }
            Ty::Child
        }
        // `Error` is the builtin error sum type — resolved via `enum_ids` like any enum name.
        "box" => {
            let inner = match args {
                [a] => resolve_type(a, cx, type_params, diags),
                _ => {
                    diags.error("box takes exactly one type argument".to_string(), span);
                    return Ty::Error;
                }
            };
            // `scalar_arg` accepts structs and owned `string` (valid Option/Result payloads), but
            // a box payload must be a true primitive scalar: codegen can't size a struct box, and
            // a Move payload (`string`) has no `box` drop story. Reject both with a clean
            // diagnostic (else `box<string>`/`box<Struct>` would type-check then panic in codegen).
            match scalar_arg(inner, "box payload", false, span, diags) {
                Some(Scalar::Struct(_)) => {
                    diags.error("a box payload must be a primitive scalar (struct boxes are not supported)".to_string(), span);
                    Ty::Error
                }
                Some(Scalar::Enum(_)) => {
                    diags.error("a box payload must be a primitive scalar (sum-type boxes are not supported)".to_string(), span);
                    Ty::Error
                }
                Some(s) if s.is_move() => {
                    diags.error(format!("a box payload must be a primitive scalar (an owned `{}` cannot be boxed)", scalar_name(s)), span);
                    Ty::Error
                }
                Some(Scalar::Str) => {
                    diags.error("a box payload must be a primitive scalar (a `str` view is not boxable)".to_string(), span);
                    Ty::Error
                }
                Some(s) => Ty::Box(s),
                None => Ty::Error,
            }
        }
        "Option" => {
            let inner = match args {
                [a] => resolve_type(a, cx, type_params, diags),
                _ => {
                    diags.error("Option takes exactly one type argument".to_string(), span);
                    return Ty::Error;
                }
            };
            match reject_move_struct_payload(scalar_arg(inner, "Option payload", true, span, diags), cx.structs, "Option payload", span, diags) {
                Some(s) => Ty::Option(s),
                None => Ty::Error,
            }
        }
        "slice" => {
            let inner = match args {
                [a] => resolve_type(a, cx, type_params, diags),
                _ => {
                    diags.error("slice takes exactly one type argument".to_string(), span);
                    return Ty::Error;
                }
            };
            match scalar_arg(inner, "slice element", false, span, diags) {
                Some(s) => Ty::Slice(s),
                None => Ty::Error,
            }
        }
        // `soa<Struct>` — a struct-of-arrays view (column-major). The element is a struct of
        // primitive scalars and/or `str` columns (no nested/Move fields); a scalar column is a flat
        // array, a `str` column is a flat array of 16-byte `{ptr,len}` views into the decode input.
        "soa" => {
            let inner = match args {
                [a] => resolve_type(a, cx, type_params, diags),
                _ => {
                    diags.error("soa takes exactly one type argument".to_string(), span);
                    return Ty::Error;
                }
            };
            match inner {
                Ty::Struct(id) => {
                    // Fields must be primitive scalars or `str`. Mixed widths are fine: each column's
                    // start is padded to the field's alignment in codegen, so `soa<{active: bool,
                    // pay: i64}>` is well-formed. A `str` column is a 16-byte `{ptr,len}` view column
                    // (zero-copy into the decode input); the soa is then region-tied to that input.
                    let fields = &cx.structs[id as usize].fields;
                    if !fields.is_empty()
                        && fields.iter().all(|f| matches!(f.ty, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Str))
                    {
                        Ty::Soa(id)
                    } else {
                        diags.error("soa<T> requires a non-empty struct of primitive-scalar or `str` fields (no nested/owned fields)".to_string(), span);
                        Ty::Error
                    }
                }
                Ty::Error => Ty::Error,
                other => {
                    diags.error(format!("soa<T> requires a struct element, got {}", ty_name(other)), span);
                    Ty::Error
                }
            }
        }
        // `array<T>` — an owned, dynamic-length array (MMv2). Currently usable as a return
        // type so a function can hand back a free-standing owned array.
        "array" => {
            let inner = match args {
                [a] => resolve_type(a, cx, type_params, diags),
                _ => {
                    diags.error("array takes exactly one type argument".to_string(), span);
                    return Ty::Error;
                }
            };
            // An `array<Struct>` is a dynamic AoS (its own owned type); only a primitive
            // element resolves to the scalar `array<T>` (`DynArray`).
            match inner {
                // An `align(N)` struct element would need its size padded to its alignment for a
                // tight, aligned stride (deferred) — reject embedding it in an array for now.
                Ty::Struct(id) if cx.structs.get(id as usize).and_then(|s| s.align).is_some() => {
                    diags.error(
                        "an `align(N)` struct cannot be an `array` element yet (its over-alignment is not honored when embedded)".to_string(),
                        span,
                    );
                    Ty::Error
                }
                Ty::Struct(id) => Ty::DynStructArray(id, Layout::Aos),
                _ => match scalar_arg(inner, "array element", false, span, diags) {
                    Some(s) => Ty::DynArray(s),
                    None => Ty::Error,
                },
            }
        }
        "Result" => {
            let (ok, err) = match args {
                [a, b] => (
                    resolve_type(a, cx, type_params, diags),
                    resolve_type(b, cx, type_params, diags),
                ),
                _ => {
                    diags.error("Result takes two type arguments".to_string(), span);
                    return Ty::Error;
                }
            };
            match (
                reject_move_struct_payload(scalar_arg(ok, "Result ok payload", true, span, diags), cx.structs, "Result ok payload", span, diags),
                reject_move_struct_payload(scalar_arg(err, "Result err payload", true, span, diags), cx.structs, "Result err payload", span, diags),
            ) {
                (Some(o), Some(e)) => Ty::Result(o, e),
                _ => Ty::Error,
            }
        }
        _ => {
            // `vec2`/`vec4`/`vec8`/`vec16`<T> — a fixed-width SIMD vector of a numeric scalar (M6).
            if let Some(n) = parse_vec_name(name) {
                let inner = match args {
                    [a] => resolve_type(a, cx, type_params, diags),
                    _ => {
                        diags.error(format!("{name} takes exactly one type argument (the element type)"), span);
                        return Ty::Error;
                    }
                };
                return match scalar_arg(inner, "vector element", false, span, diags) {
                    Some(s @ (Scalar::Int(_) | Scalar::Float(_))) => Ty::Vec(s, n),
                    Some(_) => {
                        diags.error("a vector element must be a numeric scalar (an int or float)".to_string(), span);
                        Ty::Error
                    }
                    None => Ty::Error,
                };
            }
            // `mask2`/`mask4`/`mask8`/`mask16`<T> — a comparison mask over a `vecN<T>` (M6), the
            // result type of `a > b`. Spelled like `vecN<T>`; the element matches the source vectors.
            if let Some(n) = parse_mask_name(name) {
                let inner = match args {
                    [a] => resolve_type(a, cx, type_params, diags),
                    _ => {
                        diags.error(format!("{name} takes exactly one type argument (the element type)"), span);
                        return Ty::Error;
                    }
                };
                return match scalar_arg(inner, "mask element", false, span, diags) {
                    Some(s @ (Scalar::Int(_) | Scalar::Float(_))) => Ty::Mask(s, n),
                    Some(_) => {
                        diags.error("a mask element must be a numeric scalar (it mirrors the compared `vecN<T>`)".to_string(), span);
                        Ty::Error
                    }
                    None => Ty::Error,
                };
            }
            // A builtin sized integer (`i8`..`u64`) wins over any user type of the same name.
            if let Some(it) = parse_int_name(name) {
                return Ty::Int(it);
            }
            // Otherwise a user type in the current module (a bare name resolves there). The canonical
            // key handles per-module namespacing; `resolve_user_type` dispatches to struct / enum /
            // generic template.
            match canonical_type_name(path, cx.cur_module, cx.imports, cx.type_table, true, span, diags) {
                Some(canonical) => resolve_user_type(&canonical, args, cx, type_params, span, diags),
                None => Ty::Error,
            }
        }
    }
}

/// Resolve a user type by its canonical (namespaced) key into a `Ty`. A generic template
/// (`Pair<T>` / `Opt<T>`) used with type arguments monomorphizes on demand (substitute + intern a
/// concrete def, deduped); a concrete struct / sum type resolves to its reserved id.
fn resolve_user_type(
    canonical: &str,
    args: &[ast::Type],
    cx: &mut TyCx,
    type_params: &[String],
    span: Span,
    diags: &mut Diagnostics,
) -> Ty {
    if let Some(tmpl) = cx.struct_templates.get(canonical).cloned() {
        return match resolve_generic_args(canonical, "struct", args, tmpl.type_params.len(), cx, type_params, span, diags) {
            Some(arg_tys) => Ty::Struct(instantiate_struct(canonical, &tmpl, &arg_tys, cx, span, diags)),
            None => Ty::Error,
        };
    }
    if let Some(tmpl) = cx.enum_templates.get(canonical).cloned() {
        return match resolve_generic_args(canonical, "sum type", args, tmpl.type_params.len(), cx, type_params, span, diags) {
            Some(arg_tys) => Ty::Enum(instantiate_enum(canonical, &tmpl, &arg_tys, cx, span, diags)),
            None => Ty::Error,
        };
    }
    match cx.struct_ids.get(canonical) {
        Some(&id) => Ty::Struct(id),
        None => match cx.enum_ids.get(canonical) {
            Some(&id) => Ty::Enum(id),
            None => {
                diags.error(format!("unknown type: '{canonical}'"), span);
                Ty::Error
            }
        },
    }
}

/// Whether a resolved type is a valid struct field: a primitive scalar, a `str` borrow, an owned
/// `string`, or a nested struct.
fn is_field_ok(ty: Ty) -> bool {
    // A struct field is a primitive scalar, `str` (a borrow), an owned `string`, or a **nested
    // struct** (validated separately to be acyclic — see `struct_acyclic` / the nested-field pass).
    // An owned (`string` or Move-struct) field makes the enclosing struct a Move type with a
    // recursive `Drop` (Slice 3). Owned *collections* (`array<T>` etc.) as fields are a later slice.
    matches!(ty, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Str | Ty::String | Ty::Struct(_) | Ty::Error)
}

/// Whether struct `id`'s field graph is **acyclic** — no struct contains itself, directly or
/// transitively, without a `box` indirection (which would be an infinite layout). `visiting` is the
/// current DFS path (seeded with the original containing struct). An out-of-range id is a resolution
/// error already reported elsewhere — treated as acyclic so it doesn't emit a spurious cycle error.
fn struct_acyclic(id: u32, structs: &[StructDef], visiting: &mut Vec<u32>) -> bool {
    if visiting.contains(&id) {
        return false; // recursion — forbidden without a `box` indirection
    }
    let Some(def) = structs.get(id as usize) else { return true };
    visiting.push(id);
    let ok = def.fields.iter().all(|f| match f.ty {
        Ty::Struct(nid) => struct_acyclic(nid, structs, visiting),
        _ => true,
    });
    visiting.pop();
    ok
}

/// Whether a scalar is a valid sum-type variant payload — the same rule the non-generic enum pass
/// (0c) enforces on resolved types: a primitive scalar, or a plain-data struct with no `str` field
/// (an enum is neither dropped nor region-tracked, so Move/`str`-bearing payloads are rejected).
fn enum_payload_ok(s: Scalar, structs: &[StructDef]) -> bool {
    match s {
        Scalar::Int(_) | Scalar::Float(_) | Scalar::Bool | Scalar::Char => true,
        // A plain-data struct with no `str` field. A Move struct (owns a `string`/owned field) is
        // rejected — an enum/Option payload is neither dropped recursively nor move-tracked through a
        // struct, so an owned field would leak / double-free (Slice 3).
        Scalar::Struct(id) => structs.get(id as usize).is_some_and(|sd| !struct_is_move(id, structs) && sd.fields.iter().all(|f| f.ty != Ty::Str)),
        _ => false,
    }
}

/// Monomorphize a generic struct: substitute its template fields with `args`, intern a concrete
/// `StructDef` into `cx.structs` (deduped by mangled name), and return its id. Shared by
/// `resolve_type` (a `Pair<i32>` type) and the generic struct-literal path (`Pair { a, b }`).
fn instantiate_struct(name: &str, tmpl: &StructTemplate, args: &[Ty], cx: &mut TyCx, span: Span, diags: &mut Diagnostics) -> u32 {
    let mangled = mangle_mono(name, args);
    if let Some(&id) = cx.struct_mono.get(&mangled) {
        return id;
    }
    let mut fields = Vec::with_capacity(tmpl.fields.len());
    for f in &tmpl.fields {
        let fty = subst_param_ty(f.ty, args);
        if !is_field_ok(fty) {
            diags.error(
                format!("field '{}' of '{name}' resolves to {}, which is not a valid struct field type yet", f.name, ty_name(fty)),
                span,
            );
        }
        fields.push(hir::FieldDef { name: f.name.clone(), ty: fty });
    }
    let id = cx.structs.len() as u32;
    cx.structs.push(StructDef { name: mangled.clone(), fields, align: tmpl.align, c_repr: tmpl.c_repr });
    cx.struct_mono.insert(mangled, id);
    id
}

/// Resolve + validate the type arguments of a generic struct / sum-type use (`Pair<i32>` /
/// `Opt<i32>`): arity, no error args, and not a `Param` arg (a generic def parameterized by a
/// generic *function's* type parameter needs a deferred type — a later slice). `None` on error.
#[allow(clippy::too_many_arguments)]
fn resolve_generic_args(name: &str, kind: &str, args: &[ast::Type], n_params: usize, cx: &mut TyCx, type_params: &[String], span: Span, diags: &mut Diagnostics) -> Option<Vec<Ty>> {
    if args.is_empty() {
        diags.error(format!("'{name}' is a generic {kind} — write `{name}<...>` with type arguments"), span);
        return None;
    }
    let arg_tys: Vec<Ty> = args.iter().map(|a| resolve_type(a, cx, type_params, diags)).collect();
    if arg_tys.len() != n_params {
        diags.error(format!("'{name}' takes {} type argument(s), got {}", n_params, arg_tys.len()), span);
        return None;
    }
    if arg_tys.contains(&Ty::Error) {
        return None;
    }
    if arg_tys.iter().any(|t| ty_mentions_param(*t)) {
        diags.error(
            format!("instantiating a generic {kind} with a type parameter ('{name}<…>' inside a generic function) is not supported yet"),
            span,
        );
        return None;
    }
    Some(arg_tys)
}

/// Monomorphize a generic sum type: substitute its variant payloads with `args`, intern a concrete
/// `EnumDef` into `cx.enums` (deduped by mangled name), and return its id. The enum analogue of
/// [`instantiate_struct`].
fn instantiate_enum(name: &str, tmpl: &EnumTemplate, args: &[Ty], cx: &mut TyCx, span: Span, diags: &mut Diagnostics) -> u32 {
    let mangled = mangle_mono(name, args);
    if let Some(&id) = cx.enum_mono.get(&mangled) {
        return id;
    }
    let mut variants = Vec::with_capacity(tmpl.variants.len());
    for v in &tmpl.variants {
        let payload: Vec<Scalar> = v.payload.iter().map(|&s| subst_scalar(s, args)).collect();
        for &p in &payload {
            // The substituted payload must be a valid sum-type payload — the SAME rule a non-generic
            // enum enforces in Pass 0c: a primitive scalar or a plain-data struct (no `str` field).
            // Without this, `Opt<string>` / `Opt<StructWithStrField>` would slip through, putting a
            // Move/region-tracked value in an enum that is neither dropped nor region-tracked
            // (use-after-free / leak).
            if !enum_payload_ok(p, cx.structs) {
                diags.error(
                    format!("variant '{}' of '{name}' resolves to {}, which is not a valid sum-type payload yet", v.name, scalar_name(p)),
                    span,
                );
            }
        }
        variants.push(hir::EnumVariant { name: v.name.clone(), payload, field_base: v.field_base });
    }
    let id = cx.enums.len() as u32;
    cx.enums.push(hir::EnumDef { name: mangled.clone(), variants });
    cx.enum_mono.insert(mangled, id);
    id
}

fn parse_int_name(name: &str) -> Option<IntTy> {
    let (signed, rest) = match name.as_bytes().first()? {
        b'i' => (true, &name[1..]),
        b'u' => (false, &name[1..]),
        _ => return None,
    };
    let bits: u8 = rest.parse().ok()?;
    matches!(bits, 8 | 16 | 32 | 64).then_some(IntTy { bits, signed })
}

/// The width `N` of a fixed vector type name `vecN` (`vec2`/`vec4`/`vec8`/`vec16`), else `None`.
/// Only powers-of-two 2..16 are valid SIMD widths; any other `vec…` name falls through to a user
/// type (which then errors as unknown).
fn parse_vec_name(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("vec")?;
    let n: u32 = rest.parse().ok()?;
    matches!(n, 2 | 4 | 8 | 16).then_some(n)
}

/// The width `N` of a fixed mask type name `maskN` (`mask2`/`mask4`/`mask8`/`mask16`), else `None`
/// — the mask analogue of [`parse_vec_name`] (a mask mirrors the `vecN<T>` it came from).
fn parse_mask_name(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("mask")?;
    let n: u32 = rest.parse().ok()?;
    matches!(n, 2 | 4 | 8 | 16).then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use align_lexer::tokenize;
    use align_parser::parse_file;

    fn check(src: &str) -> (Program, Diagnostics) {
        let mut d = Diagnostics::new();
        let toks = tokenize(0, src, &mut d);
        let f = parse_file(toks, &mut d);
        let p = check_file(&f, &mut d);
        (p, d)
    }

    #[test]
    fn region_lattice_outlives() {
        // Static ⊐ Frame ⊐ Arena(1) ⊐ Arena(2): longer-lived outlives shorter-lived.
        assert!(Region::Static.outlives(Region::Frame));
        assert!(Region::Static.outlives(Region::Arena(1)));
        assert!(Region::Frame.outlives(Region::Arena(1)));
        assert!(Region::Arena(1).outlives(Region::Arena(2)));
        assert!(Region::Static.outlives(Region::Static));
        // …and not the reverse.
        assert!(!Region::Frame.outlives(Region::Static));
        assert!(!Region::Arena(1).outlives(Region::Frame));
        assert!(!Region::Arena(2).outlives(Region::Arena(1)));
        // `arena(0)` is the leaked / process-lifetime case → Static; deeper = shorter-lived.
        assert_eq!(Region::arena(0), Region::Static);
        assert!(!Region::arena(2).outlives(Region::arena(1)));
        // `shorter` picks the shorter-lived (the one that bounds a view over both).
        assert_eq!(Region::Static.shorter(Region::Arena(1)), Region::Arena(1));
        assert_eq!(Region::Arena(2).shorter(Region::Frame), Region::Arena(2));
    }

    #[test]
    fn fib_checks() {
        let src = "fn fib(n: i64) -> i64 {\n  if n < 2 { return n }\n  return fib(n - 1) + fib(n - 2)\n}\n";
        let (_p, d) = check(src);
        assert!(!d.has_errors(), "fib should type-check");
    }

    #[test]
    fn bool_condition_required() {
        let (_p, d) = check("fn f(n: i32) -> i32 {\n  if n { return 1 }\n  return 0\n}\n");
        assert!(d.has_errors(), "if condition must be bool");
    }

    #[test]
    fn assign_to_immutable_errors() {
        let (_p, d) = check("fn f() -> i32 {\n  x := 1\n  x = 2\n  return x\n}\n");
        assert!(d.has_errors());
    }

    const POINT: &str = "Point {\n  x: i32,\n  y: i32,\n}\n";

    #[test]
    fn struct_construct_and_read_checks() {
        let src = format!(
            "{POINT}fn main() -> i32 {{\n  p := Point {{ x: 1, y: 2 }}\n  return p.x + p.y\n}}\n"
        );
        let (_p, d) = check(&src);
        assert!(!d.has_errors(), "a well-formed struct program should check");
    }

    #[test]
    fn missing_field_errors() {
        let src = format!("{POINT}fn main() -> i32 {{\n  p := Point {{ x: 1 }}\n  return p.x\n}}\n");
        let (_p, d) = check(&src);
        assert!(d.has_errors(), "omitting field y must error");
    }

    #[test]
    fn unknown_field_access_errors() {
        let src = format!("{POINT}fn main() -> i32 {{\n  p := Point {{ x: 1, y: 2 }}\n  return p.z\n}}\n");
        let (_p, d) = check(&src);
        assert!(d.has_errors(), "reading field z must error");
    }

    #[test]
    fn float_program_checks() {
        let (_p, d) = check("fn f(r: f64) -> f64 {\n  return r * r\n}\n");
        assert!(!d.has_errors(), "float arithmetic should check");
    }

    #[test]
    fn no_implicit_int_float_mix() {
        // An integer literal must not silently satisfy a float context.
        let (_p, d) = check("fn f() -> f64 {\n  return 1\n}\n");
        assert!(d.has_errors(), "returning int where f64 is expected must error");
    }

    #[test]
    fn char_is_not_arithmetic() {
        let (_p, d) = check("fn f() -> char {\n  return 'a' + 'b'\n}\n");
        assert!(d.has_errors(), "char does not support arithmetic");
    }

    #[test]
    fn option_program_checks() {
        let (_p, d) = check(
            "fn choose(b: bool) -> Option<i32> {\n  if b { return Some(1) }\n  return None\n}\nfn main() -> i32 {\n  return choose(true) else 0\n}\n",
        );
        assert!(!d.has_errors(), "a well-formed Option program should check");
    }

    #[test]
    fn else_unwrap_requires_option() {
        // `else`-unwrap on a non-Option is an error.
        let (_p, d) = check("fn f() -> i32 {\n  return 1 else 0\n}\n");
        assert!(d.has_errors(), "else-unwrap on a plain int must error");
    }

    #[test]
    fn bare_none_without_context_errors() {
        let (_p, d) = check("fn f() -> i32 {\n  x := None\n  return 0\n}\n");
        assert!(d.has_errors(), "None with no inferable Option type must error");
    }

    #[test]
    fn result_program_checks() {
        let (_p, d) = check(
            "fn g(n: i32) -> Result<i32, Error> {\n  if n < 0 { return Err(error(1)) }\n  return Ok(n)\n}\nfn f() -> Result<i32, Error> {\n  x := g(2)?\n  return Ok(x)\n}\n",
        );
        assert!(!d.has_errors(), "a well-formed Result program should check");
    }

    #[test]
    fn question_requires_result_returning_fn() {
        // `?` in a function that doesn't return Result is an error.
        let (_p, d) = check(
            "fn g() -> Result<i32, Error> {\n  return Ok(1)\n}\nfn f() -> i32 {\n  x := g()?\n  return x\n}\n",
        );
        assert!(d.has_errors(), "`?` in a non-Result function must error");
    }

    #[test]
    fn arena_box_program_checks() {
        let (_p, d) = check(
            "fn main() -> i32 {\n  r: i32 := arena {\n    p: box<i32> := heap.new(5)\n    p.get()\n  }\n  return r\n}\n",
        );
        assert!(!d.has_errors(), "a well-formed arena/box program should check");
    }

    #[test]
    fn array_sum_checks() {
        let (_p, d) = check("fn main() -> i32 {\n  return [10, 20, 12].sum()\n}\n");
        assert!(!d.has_errors(), "a well-formed array sum should check");
    }

    #[test]
    fn fused_pipeline_checks() {
        let (_p, d) = check(
            "fn dbl(x: i32) -> i32 = x * 2\nfn big(x: i32) -> bool = x > 4\nfn main() -> i32 {\n  return [1, 2, 3].map(dbl).where(big).sum()\n}\n",
        );
        assert!(!d.has_errors(), "a well-formed map/where/sum pipeline should check");
    }

    #[test]
    fn struct_array_projection_checks() {
        let (_p, d) = check(
            "Pt { x: i32, y: i32 }\nfn main() -> i32 {\n  return [Pt{x: 1, y: 2}, Pt{x: 3, y: 4}].x.sum()\n}\n",
        );
        assert!(!d.has_errors(), "struct array projection + sum should check");
    }

    #[test]
    fn where_field_predicate_checks() {
        let (_p, d) = check(
            "Emp { pay: i32, active: bool }\nfn main() -> i32 {\n  return [Emp{pay: 1, active: true}].where(.active).pay.sum()\n}\n",
        );
        assert!(!d.has_errors(), "where(.field) + projection should check");
    }

    #[test]
    fn where_field_must_be_bool() {
        let (_p, d) = check(
            "Pt { x: i32, y: i32 }\nfn main() -> i32 {\n  return [Pt{x: 1, y: 2}].where(.x).x.sum()\n}\n",
        );
        assert!(d.has_errors(), "where(.field) on a non-bool field must error");
    }

    #[test]
    fn where_predicate_must_return_bool() {
        let (_p, d) = check(
            "fn dbl(x: i32) -> i32 = x * 2\nfn main() -> i32 {\n  return [1, 2, 3].where(dbl).sum()\n}\n",
        );
        assert!(d.has_errors(), "a where predicate returning non-bool must error");
    }

    #[test]
    fn map_without_terminal_errors() {
        let (_p, d) = check(
            "fn dbl(x: i32) -> i32 = x * 2\nfn main() -> i32 {\n  xs := [1, 2, 3].map(dbl)\n  return 0\n}\n",
        );
        assert!(d.has_errors(), "map without a terminal reduction must error in M4");
    }

    #[test]
    fn string_program_checks() {
        let (_p, d) = check("fn g() -> str = \"hi\"\nfn main() -> i32 {\n  print(g())\n  print(\"x\")\n  return 0\n}\n");
        assert!(!d.has_errors(), "string literals + print(str) should check");
    }

    #[test]
    fn str_equality_checks_but_ordering_errors() {
        let (_p, ok) = check("fn f(s: str) -> bool = s == \"x\"\n");
        assert!(!ok.has_errors(), "str == str should check");
        let (_q, bad) = check("fn f(s: str) -> bool = s < \"x\"\n");
        assert!(bad.has_errors(), "str ordering must error");
    }

    #[test]
    fn nested_struct_fields_and_struct_box() {
        // A scalar-only nested struct field is allowed (Slice 1)...
        let (_p, nested) = check("A { v: i32 }\nB { a: A }\nfn main() -> i32 { return 0 }\n");
        assert!(!nested.has_errors(), "a scalar-only nested struct field is allowed");
        // ...an owned (`string`-bearing) nested struct field is now allowed (Slice 3): the outer
        // struct becomes a Move type with a recursive `Drop`.
        let (_s, owned) = check("A { s: string }\nB { a: A }\nfn main() -> i32 { return 0 }\n");
        assert!(!owned.has_errors(), "an owned (string-bearing) nested struct field is allowed (Slice 3)");
        // ...as is a nested `str` *borrow* field (Copy, region-tracked through the nesting).
        let (_t, borrow) = check("A { s: str }\nB { a: A }\nfn main() -> i32 { return 0 }\n");
        assert!(!borrow.has_errors(), "a nested `str` borrow field is allowed");
        // ...and a self-referential struct field is rejected (infinite layout, no `box` indirection).
        let (_c, cyclic) = check("N { next: N, v: i32 }\nfn main() -> i32 { return 0 }\n");
        assert!(cyclic.has_errors(), "a recursive struct field must be rejected");
        // A struct box payload is still rejected (would panic in codegen).
        let (_q, boxed) = check("P { x: i32 }\nfn main() -> i32 {\n  arena {\n    b := heap.new(P{x: 1})\n  }\n  return 0\n}\n");
        assert!(boxed.has_errors(), "a struct box payload must still be rejected");
        let (_r, boxann) = check("P { x: i32 }\nfn f(b: box<P>) -> i32 = 0\nfn main() -> i32 { return 0 }\n");
        assert!(boxann.has_errors(), "a box<Struct> annotation must still be rejected");
    }

    #[test]
    fn json_decode_checks_and_infers_target() {
        // T is inferred from the binding annotation through `?`.
        let (_p, ok) = check("import core.json\nUser { id: i64, active: bool }\nfn parse(s: str) -> Result<User, Error> {\n  u: User := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> i32 { return 0 }\n");
        assert!(!ok.has_errors(), "json.decode into an annotated struct should check");
        // Without an inferable target type, decode errors.
        let (_q, noty) = check("import core.json\nfn main() -> i32 {\n  x := json.decode(\"{}\")\n  return 0\n}\n");
        assert!(noty.has_errors(), "json.decode needs an inferable target type");
        // A `str` field now decodes as a zero-copy view (MMv2 slice 6); decoding from a param
        // (region Static, the caller owns the buffer) and returning the struct is allowed.
        let (_r, strf) = check("import core.json\nU { name: str }\nfn parse(s: str) -> Result<U, Error> {\n  u: U := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> i32 { return 0 }\n");
        assert!(!strf.has_errors(), "a str field decodes zero-copy and is returnable from a param");
    }

    #[test]
    fn json_decoded_str_view_cannot_escape_arena() {
        // A `str` field decoded from an arena-allocated input is a view into that input; the
        // decoded struct is region-tied to it, so the view cannot escape the arena.
        let (_p, d) = check("import core.json\nU { id: i64, name: str }\nfn bad(key: str) -> Result<i32, Error> {\n  mut outer := \"\"\n  arena {\n    d := key + key\n    u: U := json.decode(d)?\n    outer = u.name\n  }\n  return Ok(0)\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "a decoded str view from arena input must not escape the arena");
    }

    #[test]
    fn json_decode_struct_array_checks_and_escape() {
        // MMv2 slice 8d: `json.decode` into `array<Struct>` infers the target through `?` and is
        // usable as a frame-local when decoded from a param (Static input, caller owns the buffer).
        let (_p, ok) = check("import core.json\nUser { id: i64, name: str }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[]\")?\n  print(users.len())\n  return Ok(())\n}\n");
        assert!(!ok.has_errors(), "json.decode into array<Struct> should check");
        // The decoded array's `str` fields are views into the input, so an array decoded from an
        // arena-allocated input must not escape the arena (use-after-free of the freed buffer).
        let (_q, esc) = check("import core.json\nUser { id: i64, name: str }\nfn bad(key: str) -> Result<i64, Error> {\n  mut total := 0\n  arena {\n    d := key + key\n    users: array<User> := json.decode(d)?\n    total = users.len()\n  }\n  return Ok(total)\n}\nfn main() -> i32 = 0\n");
        assert!(!esc.has_errors(), "reading .len() inside the arena is fine (no escape)");
        // Returning the arena-decoded array (region-tied to the arena input) must be rejected.
        let (_r, ret) = check("import core.json\nUser { id: i64, name: str }\nfn bad(key: str) -> Result<array<User>, Error> {\n  arena {\n    d := key + key\n    users: array<User> := json.decode(d)?\n    return Ok(users)\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(ret.has_errors(), "an arena-tied decoded struct array must not escape via return");
    }

    #[test]
    fn array_index_checks_and_rejects() {
        // Indexing a scalar array / slice / owned array yields the element scalar.
        let (_p, ok) = check("fn main() -> i32 {\n  xs := [10, 20, 30]\n  return xs[1] as i32\n}\n");
        assert!(!ok.has_errors(), "indexing a scalar array should check");
        let (_o, owned) = check("import core.json\nfn main() -> Result<(), Error> {\n  xs: array<i64> := json.decode(\"[1,2]\")?\n  print(xs[0])\n  return Ok(())\n}\n");
        assert!(!owned.has_errors(), "indexing an owned array<i64> should check");
        // A non-integer index is rejected.
        let (_q, badidx) = check("fn main() -> i32 {\n  xs := [10, 20]\n  return xs[true]\n}\n");
        assert!(badidx.has_errors(), "a non-integer index must be rejected");
        // Indexing a non-array is rejected.
        let (_r, nonarr) = check("fn main() -> i32 {\n  x := 5\n  return x[0]\n}\n");
        assert!(nonarr.has_errors(), "indexing a non-array must be rejected");
        // A whole-struct element `ps[0]` is a by-value (Copy) load — supported; the bound struct's
        // field reads fine. (Reading a field directly, `ps[0].x`, also works — see
        // `struct_array_element_field_checks`.)
        let (_s, structarr) = check("P { x: i32 }\nfn main() -> i32 {\n  ps := [P{x: 1}, P{x: 2}]\n  q := ps[0]\n  return q.x\n}\n");
        assert!(!structarr.has_errors(), "a whole-struct element value should check (by-value copy)");
        // Indexing a Move-only element (here a nested owned array) is rejected — copying the
        // element's {ptr,len} without ownership transfer would double-free.
        let (_m, moveelem) = check("fn take(xs: array<array<i64>>) -> i64 {\n  ys := xs[0]\n  return ys.len()\n}\nfn main() -> i32 = 0\n");
        assert!(moveelem.has_errors(), "indexing an array of a Move type must be rejected (double-free)");
        // A `slice<Struct>` element index also yields a whole struct by value — supported (the
        // element resolves to a struct via the slice arm and loads through `SliceIndex`).
        let (_sl, slstruct) = check("P { x: i32 }\nfn first(s: slice<P>) -> i32 {\n  q := s[0]\n  return q.x\n}\nfn main() -> i32 = 0\n");
        assert!(!slstruct.has_errors(), "indexing a slice<Struct> for a whole struct should check");
    }

    #[test]
    fn str_in_composites_checks() {
        // PR-A: `str` is a composite payload (`Scalar::Str`). `Option<str>` / `Result<str,E>`
        // construct and unwrap; a literal-str payload is Static, so it is returnable.
        let (_p, ok) = check("fn mk() -> Option<str> = Some(\"lit\")\nfn r() -> Result<str, Error> = Ok(\"x\")\nfn main() -> i32 {\n  s := mk() else \"no\"\n  print(s)\n  return 0\n}\n");
        assert!(!ok.has_errors(), "Option<str> / Result<str,Error> with literal payloads should check");
        // Region: an arena-built `str` in an `Option<str>` must not escape the arena (the view
        // would dangle) — this falls out of the existing region model, no new logic.
        let (_q, esc) = check("fn bad(a: str, b: str) -> Option<str> {\n  arena {\n    return Some(a + b)\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(esc.has_errors(), "an arena str inside Option<str> must not escape the arena");
        // `box<str>` is rejected (a view is not boxable) — both the annotation and `heap.new`.
        let (_r, bann) = check("fn f(b: box<str>) -> i32 = 0\nfn main() -> i32 = 0\n");
        assert!(bann.has_errors(), "box<str> annotation must be rejected");
        let (_s, bnew) = check("fn main() -> i32 {\n  arena {\n    p: box<str> := heap.new(\"x\")\n    return 0\n  }\n}\n");
        assert!(bnew.has_errors(), "heap.new of a str must be rejected");
        // Un-annotated `heap.new(move_value)` must reject at the scalar level too — else inference
        // forms `box<string>` and codegen's `scalar_bytes` panics (the `box<…>` annotation path is
        // guarded separately, so this exercises the inference path).
        let (_m, bmove) = check("fn mk() -> string = \"x\".clone()\nfn main() -> i32 {\n  arena {\n    p := heap.new(mk())\n    return 0\n  }\n}\n");
        assert!(bmove.has_errors(), "un-annotated heap.new of an owned string must be rejected");
    }

    #[test]
    fn str_array_and_slice_checks() {
        // PR-B: `array<str>` literal + index (→ str) + len.
        let (_p, ok) = check("fn main() -> i32 {\n  xs := [\"a\", \"b\", \"c\"]\n  print(xs[1])\n  print(xs.len())\n  return 0\n}\n");
        assert!(!ok.has_errors(), "array<str> literal + index + len should check");
        // `slice<str>` param: index + len.
        let (_q, sl) = check("fn snd(xs: slice<str>) -> str = xs[1]\nfn len(xs: slice<str>) -> i64 = xs.len()\nfn main() -> i32 = 0\n");
        assert!(!sl.has_errors(), "slice<str> index + len should check");
        // Region: a `slice<str>` viewing a local array must not escape.
        let (_r, esc) = check("fn bad() -> slice<str> {\n  s: slice<str> := [\"a\", \"b\"]\n  return s\n}\nfn main() -> i32 = 0\n");
        assert!(esc.has_errors(), "a slice<str> into a local array must not escape");
        // Region: an `array<str>` of arena strs must not let an element escape via index+return
        // (the fixed array is region-tracked because its `str` element is).
        let (_s, idxesc) = check("fn bad(a: str, b: str) -> str {\n  arena {\n    xs := [a + b, a]\n    return xs[0]\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(idxesc.has_errors(), "a str element of an arena str-array must not escape via index");
        // A literal-str array element is Static → returnable (no false reject); a scalar array
        // stays returnable too (no regression from the new array region-tracking).
        let (_t, lit) = check("fn ok() -> str {\n  xs := [\"lit\", \"lat\"]\n  return xs[0]\n}\nfn n() -> i64 {\n  ys := [1, 2, 3]\n  return ys[0]\n}\nfn main() -> i32 = 0\n");
        assert!(!lit.has_errors(), "literal-str and scalar array element reads stay returnable");
        // A `slice<str>` coerced from an arena str-array must not escape via return — the slice
        // inherits the array's region (`region_of(ArrayToSlice)`), and `slice<str>` is now
        // region-tracked.
        let (_u, slesc) = check("fn bad(a: str, b: str) -> slice<str> {\n  arena {\n    s: slice<str> := [a + b, a]\n    return s\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(slesc.has_errors(), "a slice<str> over an arena str-array must not escape");
        // A scalar `slice<i32>` parameter stays returnable (it borrows the caller) — no regression
        // from adding `Slice` to `tracks_region`.
        let (_v, slok) = check("fn id(xs: slice<i32>) -> slice<i32> = xs\nfn main() -> i32 = 0\n");
        assert!(!slok.has_errors(), "a slice<i32> parameter stays returnable");
    }

    #[test]
    fn fs_read_file_checks() {
        // std.fs: `fs.read_file(path)` yields `Result<string, Error>`; `?` unwraps an owned string.
        let (_p, ok) = check("import std.fs\nfn main() -> Result<(), Error> {\n  data := fs.read_file(\"x.txt\")?\n  print(data.len())\n  return Ok(())\n}\n");
        assert!(!ok.has_errors(), "fs.read_file should check and yield an owned string");
        // The owned string owns a fresh buffer (not a view), so it is returnable.
        let (_q, ret) = check("import std.fs\nfn load(p: str) -> Result<string, Error> {\n  return Ok(fs.read_file(p)?)\n}\nfn main() -> i32 = 0\n");
        assert!(!ret.has_errors(), "an fs.read_file string is owned and returnable");
        // Wrong arity errors cleanly.
        let (_r, ar) = check("import std.fs\nfn main() -> Result<(), Error> {\n  data := fs.read_file()?\n  return Ok(())\n}\n");
        assert!(ar.has_errors(), "fs.read_file needs exactly one argument");
    }

    #[test]
    fn io_stdout_write_checks() {
        // std.io: `io.stdout.write(s)` (s: str / owned string) yields `Result<(), Error>`.
        let (_p, ok) = check("import std.io\nfn main() -> Result<(), Error> {\n  io.stdout.write(\"hi\")?\n  return Ok(())\n}\n");
        assert!(!ok.has_errors(), "io.stdout.write of a str should check");
        // An owned string is accepted (auto-borrowed to str) and stays usable afterwards.
        let (_q, owned) = check("import std.io\nfn mk() -> string = \"x\".clone()\nfn main() -> Result<(), Error> {\n  s := mk()\n  io.stdout.write(s)?\n  print(s.len())\n  return Ok(())\n}\n");
        assert!(!owned.has_errors(), "io.stdout.write borrows an owned string (does not move it)");
        // A `builder` is accepted directly (written, not consumed — still usable / dropped after).
        let (_b, bld) = check("import std.io\nfn main() -> Result<(), Error> {\n  b := builder()\n  b.write(\"hi\")\n  io.stdout.write(b)?\n  print(b.to_string())\n  return Ok(())\n}\n");
        assert!(!bld.has_errors(), "io.stdout.write accepts a builder directly (borrows it)");
        // Wrong arity errors.
        let (_r, ar) = check("import std.io\nfn main() -> Result<(), Error> {\n  io.stdout.write()?\n  return Ok(())\n}\n");
        assert!(ar.has_errors(), "io.stdout.write needs exactly one argument");
    }

    #[test]
    fn struct_array_element_field_checks() {
        // MMv2 slice 8f: `arr[i].field` on a struct array reads one field (scalar or str view),
        // bounds-checked.
        let (_p, ok) = check("import core.json\nUser { id: i64, score: i32 }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[]\")?\n  print(users[0].score)\n  return Ok(())\n}\n");
        assert!(!ok.has_errors(), "arr[i].field on a struct array should check");
        // A whole-struct `arr[i]` value (no field) is a by-value load — supported.
        let (_q, whole) = check("P { x: i32 }\nfn main() -> i32 {\n  ps := [P{x: 1}]\n  q := ps[0]\n  return q.x\n}\n");
        assert!(!whole.has_errors(), "a whole-struct element value should check (by-value copy)");
        // An unknown field on the element is rejected.
        let (_r, badf) = check("P { x: i32 }\nfn main() -> i32 {\n  ps := [P{x: 1}]\n  return ps[0].nope\n}\n");
        assert!(badf.has_errors(), "an unknown element field must be rejected");
        // A `str` field read from an arena-decoded element must not escape the arena.
        let (_s, esc) = check("import core.json\nU { id: i64, name: str }\nfn bad(key: str) -> Result<str, Error> {\n  mut out := \"\"\n  arena {\n    d := key + key\n    users: array<U> := json.decode(d)?\n    out = users[0].name\n  }\n  return Ok(out)\n}\nfn main() -> i32 = 0\n");
        assert!(esc.has_errors(), "a str field of an arena-decoded element must not escape the arena");
    }

    #[test]
    fn pipeline_over_dynamic_struct_array_checks() {
        // MMv2 slice 8d-2: a fused pipeline over a decoded `array<Struct>` variable type-checks
        // (`where(.field)` + projection + reduction).
        let (_p, ok) = check("import core.json\nUser { id: i64, active: bool, score: i32 }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[]\")?\n  print(users.where(.active).score.sum())\n  return Ok(())\n}\n");
        assert!(!ok.has_errors(), "a where(.field).field.sum() pipeline over array<Struct> should check");
        // `where` with a whole-struct predicate over a dynamic struct array now checks (it loads
        // the element by value and keeps it, so the following `.score` projection reads the source).
        let (_q, ok2) = check("import core.json\nUser { id: i64, active: bool, score: i32 }\nfn keep(u: User) -> bool = u.active\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[]\")?\n  print(users.where(keep).score.sum())\n  return Ok(())\n}\n");
        assert!(!ok2.has_errors(), "'where' with a whole-struct predicate should check");
    }

    #[test]
    fn json_decoded_struct_cannot_escape_via_result_local() {
        // The decoded struct's region must survive while wrapped in a `Result`: binding the raw
        // `json.decode(...)` to a `Result`-typed local, unwrapping it with `?`, then returning
        // `Ok(u)` must still be rejected (otherwise the arena-tied str views escape → UAF).
        let (_p, d) = check("import core.json\nU { id: i64, name: str }\nfn bad(key: str) -> Result<U, Error> {\n  arena {\n    d := key + key\n    res: Result<U, Error> := json.decode(d)\n    u: U := res?\n    return Ok(u)\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "a region-tied decoded struct must not escape through a Result-typed local");
    }

    #[test]
    fn result_option_struct_payload_checks() {
        // A struct can be an Ok/Some payload; `?` unwraps to the struct, `else` to it too.
        let (_p, r) = check("Pt { x: i32 }\nfn mk() -> Result<Pt, Error> {\n  p := Pt{x: 1}\n  return Ok(p)\n}\nfn main() -> Result<(), Error> {\n  q := mk()?\n  print(q.x)\n  return Ok(())\n}\n");
        assert!(!r.has_errors(), "Result<Struct, Error> should check");
        let (_q, o) = check("Pt { x: i32 }\nfn pick() -> Option<Pt> {\n  p := Pt{x: 1}\n  return Some(p)\n}\nfn main() -> i32 {\n  q := pick() else { return 9 }\n  return q.x\n}\n");
        assert!(!o.has_errors(), "Option<Struct> should check");
    }

    #[test]
    fn struct_str_field_ok() {
        // A `str` struct field is allowed; reading it back is fine.
        let (_p, d) = check("User { name: str }\nfn main() -> i32 {\n  u := User{name: \"ada\"}\n  print(u.name)\n  return 0\n}\n");
        assert!(!d.has_errors(), "str struct fields are allowed (region-0 strs)");
    }

    #[test]
    fn struct_arena_str_field_ok_when_not_escaping() {
        // MMv2 slice 2: a struct may now hold an arena-backed str. As long as the struct does
        // not escape the arena (here it is only used inside it), this is safe and allowed.
        let (_p, d) = check("P { tag: str }\nfn main() -> i32 {\n  a := \"x\"\n  b := \"y\"\n  arena {\n    p := P{tag: a + b}\n    print(p.tag)\n  }\n  return 0\n}\n");
        assert!(!d.has_errors(), "a struct holding an arena str is fine if it does not escape");
    }

    #[test]
    fn struct_with_arena_str_field_cannot_escape() {
        // The struct carries its field's arena region, so returning it out of the arena (as the
        // arena block's value, which becomes the function result) must be rejected.
        let (_p, d) = check("P { tag: str }\nfn mk(a: str, b: str) -> P {\n  arena {\n    P{tag: a + b}\n  }\n}\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "a struct holding an arena str must not escape its arena");
    }

    #[test]
    fn struct_nested_arena_escape_rejected() {
        // A binding that captures an inner arena's value must keep that arena's region, so it
        // cannot be assigned to an outer-arena binding (which would outlive it → use-after-free).
        let (_p, d) = check("P { tag: str }\nfn main() -> i32 {\n  arena {\n    mut out := P{tag: \"init\"}\n    arena {\n      x := \"a\" + \"b\"\n      p := arena {\n        P{tag: x}\n      }\n      out = p\n    }\n  }\n  return 0\n}\n");
        assert!(d.has_errors(), "a value captured from an inner arena must not escape to an outer one");
    }

    #[test]
    fn struct_with_literal_str_field_returns_ok() {
        // A struct whose str field is a literal (region-0 / Static) stays freely returnable.
        let (_p, d) = check("P { tag: str }\nfn mk() -> P {\n  return P{tag: \"lit\"}\n}\nfn main() -> i32 { return 0 }\n");
        assert!(!d.has_errors(), "a struct with a literal str field is Static and returnable");
    }

    #[test]
    fn arena_str_into_outer_struct_field_rejected() {
        // Assigning an arena str into a field of a struct declared in an outer (longer-lived)
        // scope would let it outlive the arena via that struct.
        let (_p, d) = check("P { tag: str }\nfn main() -> i32 {\n  a := \"x\"\n  b := \"y\"\n  mut p := P{tag: \"init\"}\n  arena {\n    p.tag = a + b\n  }\n  print(p.tag)\n  return 0\n}\n");
        assert!(d.has_errors(), "storing an arena str into an outer struct's field must be rejected");
    }

    #[test]
    fn struct_box_field_still_rejected() {
        // box fields remain unsupported (only scalars and str for now).
        let (_p, d) = check("B { b: box<i32> }\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "box struct fields are still rejected");
    }

    #[test]
    fn struct_float_field_ok() {
        let (_p, d) = check("P { x: f64, y: f64 }\nfn main() -> i32 {\n  p := P{x: 1.5, y: 2.5}\n  if p.x + p.y > 3.0 { return 1 }\n  return 0\n}\n");
        assert!(!d.has_errors(), "float struct fields should check");
    }

    #[test]
    fn struct_by_value_param_return_copy() {
        // Pass a struct by value, copy it, and return it; construct via a struct-literal body.
        let (_p, d) = check("P { x: i32, y: i32 }\nfn sum(p: P) -> i32 = p.x + p.y\nfn dup(p: P) -> P {\n  q := p\n  return q\n}\nfn mk(v: i32) -> P = P{x: v, y: v}\nfn main() -> i32 {\n  a := mk(21)\n  b := dup(a)\n  return sum(b)\n}\n");
        assert!(!d.has_errors(), "struct pass/return/copy + struct-literal expressions should check");
    }

    #[test]
    fn whole_struct_reassign_ok() {
        let (_p, d) = check("P { x: i32 }\nfn mk(v: i32) -> P = P{x: v}\nfn main() -> i32 {\n  mut p := P{x: 1}\n  p = mk(7)\n  return p.x\n}\n");
        assert!(!d.has_errors(), "whole-struct reassignment should check");
    }

    #[test]
    fn arena_backed_str_cannot_escape() {
        let (_p, d) = check("fn f() -> str {\n  arena {\n    \"x\" + \"y\"\n  }\n}\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "an arena-backed str must not escape its arena");
    }

    #[test]
    fn slice_of_local_array_cannot_be_returned() {
        // A slice that views a stack-local array literal dies when the function returns.
        let (_p, d) = check("fn bad() -> slice<i64> {\n  s: slice<i64> := [1, 2, 3]\n  return s\n}\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "a slice into a local array must not escape via return");
    }

    #[test]
    fn slice_borrowing_local_array_via_call_cannot_be_returned() {
        // first() re-borrows its arg; returning it leaks a view into bad()'s temp array.
        let (_p, d) = check("fn first(xs: slice<i64>) -> slice<i64> = xs\nfn bad() -> slice<i64> {\n  return first([1, 2, 3])\n}\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "a slice re-borrowed from a local array must not escape");
    }

    #[test]
    fn slice_local_backed_via_conditional_assign_cannot_escape() {
        // Without a dataflow join we must stay conservative: a binding ever holding a
        // local-backed slice cannot be returned, even if a branch reassigns a param slice.
        let (_p, d) = check("fn pick(p: slice<i32>) -> slice<i32> {\n  mut s: slice<i32> := [1, 2, 3]\n  if true { s = p }\n  return s\n}\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "a conditionally-reassigned local-backed slice must not escape");
    }

    #[test]
    fn slice_array_literal_reassign_cannot_escape() {
        // Reassigning an array literal to a slice local borrows frame-local storage (and is
        // coerced like a `let`), so the binding becomes local-backed and cannot be returned.
        let (_p, d) = check("fn bad(p: slice<i32>) -> slice<i32> {\n  mut s: slice<i32> := p\n  s = [1, 2, 3]\n  return s\n}\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "a slice reassigned from a local array must not escape");
    }

    #[test]
    fn call_result_view_cannot_escape_arena() {
        // A call may return a view borrowing one of its args; calling such a fn with an
        // arena-backed str and returning the result out of the arena must be rejected (the
        // borrowed buffer is freed at arena end → use-after-free). Conservative: the call
        // result lives no longer than its shortest-lived argument.
        let (_p, d) = check("fn dup(s: str) -> str = s\nfn leak() -> str {\n  arena {\n    x := \"a\" + \"b\"\n    return dup(x)\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "a view returned from a call on an arena arg must not escape the arena");
    }

    #[test]
    fn call_result_view_with_static_arg_returns_ok() {
        // The arg propagation only shortens the region by *tracked* args: a call whose str args
        // are literals (Static) yields a Static result, so it stays returnable — no false reject.
        let (_p, d) = check("fn dup(s: str) -> str = s\nfn ok() -> str = dup(\"hi\")\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "a call on a static-region arg should stay returnable");
    }

    #[test]
    fn reduce_str_accumulator_cannot_escape_arena() {
        // `reduce`'s accumulator is folded in the enclosing arena; when it is region-tracked (a
        // `str` built by concatenation), returning it out of the arena must be rejected (the
        // accumulator buffer is freed at arena end → use-after-free).
        let (_p, d) = check("fn build(a: str, e: i64) -> str = a + \"?\"\nfn leak() -> str {\n  arena {\n    ns := [1, 2, 3]\n    return ns.reduce(build, \"\")\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "a str reduce accumulator built in an arena must not escape it");
    }

    #[test]
    fn reduce_scalar_accumulator_returns_ok() {
        // A scalar reduce result carries no region (it is Copy), so folding inside an arena and
        // returning the scalar is fine — the arena region must not leak onto plain scalars.
        let (_p, d) = check("fn add(a: i64, e: i64) -> i64 = a + e\nfn total() -> i64 {\n  arena {\n    ns := [1, 2, 3]\n    return ns.reduce(0, add)\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "a scalar reduce accumulator carries no region and may be returned");
    }

    #[test]
    fn str_clone_produces_returnable_owned_string() {
        // `str.clone()` yields a heap-owned `string` (region `Static`), so it can be returned out
        // of the arena its source was built in — the explicit escape hatch (MMv2 slice 7).
        let (_p, d) = check("fn longer(a: str, b: str) -> string {\n  arena {\n    c := a + b\n    return c.clone()\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "a cloned (owned) string should be returnable from an arena");
    }

    #[test]
    fn arena_str_without_clone_still_cannot_escape() {
        // Without the `.clone()`, the arena-backed `str` view must not escape (regression guard
        // that adding `string` did not loosen the borrow's region check).
        let (_p, d) = check("fn longer(a: str, b: str) -> str {\n  arena {\n    c := a + b\n    return c\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "an arena-backed str view must not escape without an explicit clone");
    }

    #[test]
    fn owned_string_is_move_use_after_move_rejected() {
        // A `string` is a Move type: binding it elsewhere moves it, so a later use is rejected
        // (whereas `print` borrows — covered by the e2e tests).
        let (_p, d) = check("fn mk(a: str) -> string = a.clone()\nfn main() -> i32 {\n  s := mk(\"x\")\n  t := s\n  return t.len() as i32\n}\n");
        // `t := s` moves; but `t.len()` is fine. Now force a use-after-move:
        assert!(!d.has_errors(), "moving a string into a new binding and using the new one is fine");
        let (_q, d2) = check("fn mk(a: str) -> string = a.clone()\nfn main() -> i32 {\n  s := mk(\"x\")\n  t := s\n  return s.len()\n}\n");
        assert!(d2.has_errors(), "using a string after it was moved must be rejected");
    }

    #[test]
    fn string_borrows_as_str_arg_without_moving() {
        // MMv2 slice 7b: passing an owned `string` to a `str` parameter *borrows* it (zero-cost,
        // same `{ptr,len}` layout). The borrow does not consume the string, so a later use is
        // fine — unlike passing it to a `string` parameter (which moves).
        let (_p, d) = check("fn show(s: str) -> i64 = s.len()\nfn mk(a: str) -> string = a.clone()\nfn main() -> i32 {\n  s := mk(\"x\")\n  a := show(s)\n  b := show(s)\n  return 0\n}\n");
        assert!(!d.has_errors(), "borrowing a string as a str arg must not move it");
    }

    #[test]
    fn string_borrows_into_str_let_and_assign() {
        // MMv2 slice 7e: a `str`-annotated let borrows an owned `string` (non-consuming), so the
        // source stays usable.
        let (_p, d) = check("fn mk(a: str) -> string = a.clone()\nfn main() -> i32 {\n  owned := mk(\"x\")\n  view: str := owned\n  print(view)\n  print(owned.len())\n  return 0\n}\n");
        assert!(!d.has_errors(), "borrowing a string into a str let must check and not move it");
        // A `str` place assignment borrows the same way.
        let (_q, d2) = check("fn mk(a: str) -> string = a.clone()\nfn main() -> i32 {\n  owned := mk(\"x\")\n  mut view: str := \"\"\n  view = owned\n  print(view)\n  print(owned.len())\n  return 0\n}\n");
        assert!(!d2.has_errors(), "borrowing a string into a str place assignment must check");
    }

    #[test]
    fn str_let_borrow_returned_escapes() {
        // The let-bound borrow is `Frame`-regioned: returning it (the buffer is freed at exit) is
        // rejected with the borrow-specific diagnostic — both via explicit `return` and as a
        // block's trailing (fall-through) value.
        let (_p, d) = check("fn mk(a: str) -> string = a.clone()\nfn leak() -> str {\n  owned := mk(\"x\")\n  view: str := owned\n  return view\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "returning a str that borrows a local string must be rejected");
        // Fall-through (trailing-value) return path — same rejection.
        let (_q, d2) = check("fn mk(a: str) -> string = a.clone()\nfn leak() -> str {\n  owned := mk(\"x\")\n  view: str := owned\n  view\n}\nfn main() -> i32 = 0\n");
        assert!(d2.has_errors(), "a trailing-value str borrow of a local string must also be rejected");
    }

    #[test]
    fn result_string_payload_checks_and_returns() {
        // MMv2 slice 8a: `Result<string, Error>` is representable; an owned `string` (Static
        // region) is returnable through it, and `?` unwraps to an owned string.
        let (_p, d) = check("fn mk(a: str) -> Result<string, Error> = Ok(a.clone())\nfn use(name: str) -> Result<i64, Error> {\n  s := mk(name)?\n  return Ok(s.len())\n}\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "Result<string,Error> construct/return/unwrap should check");
    }

    #[test]
    fn option_string_payload_checks() {
        let (_p, d) = check("fn first() -> Option<string> = Some(\"x\".clone())\nfn main() -> i32 {\n  s := first() else { return 9 }\n  print(s)\n  return 0\n}\n");
        assert!(!d.has_errors(), "Option<string> construct + else-unwrap should check");
    }

    #[test]
    fn json_decode_scalar_array_checks() {
        // MMv2 slice 8c: `json.decode` into an owned `array<scalar>` checks (target inferred from
        // the `array<T>` annotation threaded through `?`).
        let (_p, d) = check("import core.json\nfn parse(s: str) -> Result<array<i64>, Error> {\n  xs: array<i64> := json.decode(s)?\n  return Ok(xs)\n}\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "json.decode into array<i64> should check");
        // `array<char>` is a representable owned-array type, but the runtime parser only handles
        // int/float/bool elements — `json.decode` rejects it cleanly (exercises the element check).
        let (_q, d2) = check("import core.json\nfn parse(s: str) -> Result<array<char>, Error> {\n  xs: array<char> := json.decode(s)?\n  return Ok(xs)\n}\nfn main() -> i32 = 0\n");
        assert!(d2.has_errors(), "json.decode into array<char> must be rejected for now");
    }

    #[test]
    fn result_and_option_array_payload_checks() {
        // MMv2 slice 8b: `Result<array<i64>, Error>` / `Option<array<i64>>` are representable; an
        // owned array is returnable through them and `?`/`else` unwrap to the owned array.
        let (_p, d) = check("fn mk() -> Result<array<i64>, Error> = Ok([1, 2, 3].to_array())\nfn use() -> Result<i64, Error> {\n  xs := mk()?\n  return Ok(xs.sum())\n}\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "Result<array<i64>,Error> construct/return/unwrap should check");
        let (_q, d2) = check("fn first() -> Option<array<i64>> = Some([1, 2].to_array())\nfn main() -> i32 {\n  xs := first() else { return 9 }\n  print(xs.sum())\n  return 0\n}\n");
        assert!(!d2.has_errors(), "Option<array<i64>> construct + else-unwrap should check");
    }

    #[test]
    fn box_array_payload_rejected_cleanly() {
        // Like `box<string>`, an owned `array` is a Move scalar and cannot be boxed — rejected in
        // sema (not a codegen panic).
        let (_p, d) = check("fn main() -> i32 {\n  arena {\n    p: box<array<i64>> := heap.new([1].to_array())\n    return 0\n  }\n}\n");
        assert!(d.has_errors(), "box<array<T>> must be rejected (an owned array cannot be boxed)");
    }

    #[test]
    fn box_string_payload_rejected_cleanly() {
        // `string` is now a scalar (slice 8a), so `box<string>` must be rejected in sema with a
        // clean diagnostic — not type-check and then panic in codegen (the box payload guard must
        // cover Move scalars, like it already covers structs).
        let (_p, d) = check("fn main() -> i32 {\n  arena {\n    p: box<string> := heap.new(\"x\".clone())\n    return 0\n  }\n}\n");
        assert!(d.has_errors(), "box<string> must be rejected (an owned string cannot be boxed)");
    }

    #[test]
    fn result_string_use_after_try_rejected() {
        // `?` consumes the Result (moves its owned payload out); using the source again is a
        // use-after-move.
        let (_p, d) = check("fn mk() -> Result<string, Error> = Ok(\"x\".clone())\nfn use() -> Result<i64, Error> {\n  r := mk()\n  a := r?\n  b := r?\n  return Ok(a.len())\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "using a Result<string> after `?` consumed it must be rejected");
    }

    #[test]
    fn builder_constructs_string_checks() {
        // MMv2 slice 7c: `builder()` + `write`/`write_int` + `to_string()` yields an owned
        // `string` returnable from the function.
        let (_p, d) = check("fn make(name: str, n: i64) -> string {\n  b := builder()\n  b.write(\"x=\")\n  b.write(name)\n  b.write_int(n)\n  return b.to_string()\n}\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "builder construction should check");
    }

    #[test]
    fn builder_to_string_consumes_use_after_move_rejected() {
        // `to_string()` consumes (moves) the builder; using it afterwards is a use-after-move.
        let (_p, d) = check("fn main() -> i32 {\n  b := builder()\n  b.write(\"a\")\n  s := b.to_string()\n  t := b.to_string()\n  return 0\n}\n");
        assert!(d.has_errors(), "using a builder after to_string() must be rejected");
    }

    #[test]
    fn builder_write_wrong_arg_type_errors() {
        // `.write()` takes a str; an int is rejected (use `.write_int()`).
        let (_p, d) = check("fn main() -> i32 {\n  b := builder()\n  b.write(42)\n  return 0\n}\n");
        assert!(d.has_errors(), "builder.write of a non-str must error");
    }

    #[test]
    fn builder_scalar_writers_check() {
        // MMv2 slice 7d: bool/char/float writers accept their matching scalar.
        let (_p, d) = check("fn main() -> i32 {\n  b := builder()\n  b.write_int(1)\n  b.write_bool(true)\n  b.write_char('z')\n  b.write_float(2.5)\n  s := b.to_string()\n  return 0\n}\n");
        assert!(!d.has_errors(), "builder scalar writers should check");
    }

    #[test]
    fn builder_write_bool_rejects_non_bool() {
        // Each typed writer rejects a mismatched scalar (here `write_bool` of an int).
        let (_p, d) = check("fn main() -> i32 {\n  b := builder()\n  b.write_bool(1)\n  return 0\n}\n");
        assert!(d.has_errors(), "write_bool of a non-bool must error");
    }

    #[test]
    fn builder_write_float_rejects_int() {
        let (_p, d) = check("fn main() -> i32 {\n  b := builder()\n  b.write_float(3)\n  return 0\n}\n");
        assert!(d.has_errors(), "write_float of an int must error (no implicit int->float)");
    }

    #[test]
    fn write_on_non_builder_errors() {
        // The builder methods are builder-only; calling `.write()` on a str is an error.
        let (_p, d) = check("fn main() -> i32 {\n  s := \"x\"\n  s.write(\"y\")\n  return 0\n}\n");
        assert!(d.has_errors(), "'.write()' on a non-builder must error");
    }

    #[test]
    fn string_borrow_returned_as_str_view_escapes() {
        // The borrow is `Frame`-regioned: a function that returns a borrow of its `str` arg, when
        // fed a borrowed `string`, would dangle (the string is freed at frame exit). The
        // call-result region tie (slice 6b) must catch this through the borrow.
        let (_p, d) = check("fn id(s: str) -> str = s\nfn mk(a: str) -> string = a.clone()\nfn leak() -> str {\n  owned := mk(\"x\")\n  return id(owned)\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "returning a str borrow of a frame-owned string must be rejected");
    }

    #[test]
    fn slice_param_passthrough_returns_ok() {
        // A slice parameter borrows the caller, so returning it (directly or re-borrowed) is fine.
        let (_p, d) = check("fn id(xs: slice<i64>) -> slice<i64> = xs\nfn g(ys: slice<i64>) -> slice<i64> = id(ys)\n");
        assert!(!d.has_errors(), "returning a slice parameter is safe (it borrows the caller)");
    }

    #[test]
    fn slice_local_used_in_function_is_ok() {
        // A slice into a local array is fine as long as it does not outlive the frame.
        let (_p, d) = check("fn main() -> i32 {\n  s: slice<i32> := [10, 20, 12]\n  return s.sum()\n}\n");
        assert!(!d.has_errors(), "a non-escaping slice local should check");
    }

    #[test]
    fn non_arena_str_returns_ok() {
        let (_p, d) = check("fn g(a: str, b: str) -> str = a + b\nfn h() -> str = \"lit\"\n");
        assert!(!d.has_errors(), "a non-arena str is returnable (leaked / process-lifetime)");
    }

    #[test]
    fn str_concat_checks_but_other_ops_error() {
        let (_p, ok) = check("fn f(a: str, b: str) -> str = a + b\n");
        assert!(!ok.has_errors(), "str + str should check");
        let (_q, bad) = check("fn f(a: str, b: str) -> str = a - b\n");
        assert!(bad.has_errors(), "str only supports +");
    }

    #[test]
    fn template_checks() {
        let (_p, d) = check("fn main() -> i32 {\n  n := \"x\"\n  k := 1\n  m := template \"{n}={k}\"\n  print(m)\n  return 0\n}\n");
        assert!(!d.has_errors(), "a template with str/int holes should check");
    }

    #[test]
    fn template_undefined_hole_errors() {
        let (_p, d) = check("fn main() -> i32 {\n  m := template \"hi {who}\"\n  return 0\n}\n");
        assert!(d.has_errors(), "an undefined template hole must error");
    }

    #[test]
    fn template_expression_holes_check() {
        // `{expr}` holes: arithmetic and str concatenation are both valid.
        let (_p, d) = check("fn main() -> i32 {\n  a := 20\n  b := 22\n  n := \"x\"\n  m := template \"{a + b} {a * 2} {n + \\\"!\\\"}\"\n  print(m)\n  return 0\n}\n");
        assert!(!d.has_errors(), "arithmetic and str-concat holes should check");
    }

    #[test]
    fn template_bool_and_char_holes_check() {
        // bool and char holes are interpolatable.
        let (_p, d) = check("fn main() -> i32 {\n  c := 'x'\n  print(template \"{1 > 2} {c}\")\n  return 0\n}\n");
        assert!(!d.has_errors(), "bool and char template holes should check");
    }

    #[test]
    fn template_float_hole_checks() {
        // A float hole is interpolatable (rendered via the runtime's shortest round-trip).
        let (_p, d) = check("fn main() -> i32 {\n  print(template \"{1.5} {2.0 + 0.5}\")\n  return 0\n}\n");
        assert!(!d.has_errors(), "a float template hole should check");
    }

    #[test]
    fn print_accepts_bool_char_float() {
        let (_p, d) = check("fn main() -> i32 {\n  print(true)\n  print('a')\n  print(3.14)\n  return 0\n}\n");
        assert!(!d.has_errors(), "print accepts bool, char, and float");
    }

    #[test]
    fn len_checks_on_str_slice_array() {
        let (_p, d) = check("fn slen(xs: slice<i32>) -> i64 = xs.len()\nfn main() -> i32 {\n  s := \"hi\"\n  a := [1, 2, 3]\n  print(s.len())\n  print(a.len())\n  print(slen([4, 5]))\n  return 0\n}\n");
        assert!(!d.has_errors(), ".len() should check on str, array, and slice");
    }

    #[test]
    fn len_rejects_non_sequence() {
        let (_p, d) = check("fn main() -> i32 {\n  x := 5\n  print(x.len())\n  return 0\n}\n");
        assert!(d.has_errors(), ".len() is not defined on an integer");
    }

    #[test]
    fn json_encode_struct_checks() {
        let (_p, d) = check("import core.json\nUser { id: i64, name: str, active: bool }\nfn main() -> i32 {\n  u := User{id: 1, name: \"a\", active: true}\n  print(json.encode(u))\n  return 0\n}\n");
        assert!(!d.has_errors(), "json.encode of a flat struct should check");
    }

    #[test]
    fn json_encode_struct_array_checks() {
        let (_p, d) = check("import core.json\nUser { id: i64, name: str }\nfn main() -> i32 {\n  us := [User{id: 1, name: \"a\"}, User{id: 2, name: \"b\"}]\n  print(json.encode(us))\n  return 0\n}\n");
        assert!(!d.has_errors(), "json.encode of a struct array should check");
    }

    #[test]
    fn json_encode_rejects_non_struct() {
        let (_p, d) = check("import core.json\nfn main() -> i32 {\n  x := 5\n  print(json.encode(x))\n  return 0\n}\n");
        assert!(d.has_errors(), "json.encode requires a struct");
    }

    #[test]
    fn json_encode_rejects_unsupported_field() {
        // A char field is a valid struct field but not encodable yet; json.encode must error
        // (and not return a malformed template).
        let (_p, d) = check("import core.json\nC { ch: char, n: i32 }\nfn main() -> i32 {\n  c := C{ch: 'x', n: 1}\n  print(json.encode(c))\n  return 0\n}\n");
        assert!(d.has_errors(), "json.encode rejects a struct with an unsupported field type");
    }

    #[test]
    fn print_rejects_non_scalar() {
        // An Option is not a printable scalar.
        let (_p, d) = check("fn main() -> i32 {\n  print(Some(1))\n  return 0\n}\n");
        assert!(d.has_errors(), "print rejects non-scalar values like Option");
    }

    #[test]
    fn reduce_checks() {
        let (_p, d) = check(
            "fn add(acc: i32, x: i32) -> i32 = acc + x\nfn main() -> i32 {\n  return [1, 2, 3].reduce(0, add)\n}\n",
        );
        assert!(!d.has_errors(), "reduce with a matching fold should check");
    }

    #[test]
    fn any_all_check_and_require_scalar_element() {
        let (_p, ok) = check("fn big(x: i64) -> bool = x > 4\nfn pos(x: i64) -> bool = x > 0\nfn main() -> i32 {\n  if [1, 2, 3].any(big) { return 1 }\n  if [1, 2, 3].all(pos) { return 2 }\n  return 0\n}\n");
        assert!(!ok.has_errors(), "any/all over a scalar array should check");
        // A struct element (no projection) is rejected — project a field first.
        let (_q, bad) = check("fn f(e: i32) -> bool = e > 0\nE { pay: i32 }\nfn main() -> i32 {\n  if [E{pay: 1}].any(f) { return 1 }\n  return 0\n}\n");
        assert!(bad.has_errors(), "any on a struct element must error");
        // An undefined predicate errors (and returns Ty::Error, not a valid bool node).
        let (_r, undef) = check("fn main() -> i32 {\n  if [1, 2, 3].any(nope) { return 1 }\n  return 0\n}\n");
        assert!(undef.has_errors(), "any with an undefined predicate must error");
    }

    #[test]
    fn count_checks_on_scalar_and_struct_arrays() {
        // count returns i64 and needs no scalar element (a struct element is fine).
        let (_p, d) = check("fn big(x: i64) -> bool = x > 2\nE { active: bool }\nfn main() -> i32 {\n  a := [1, 2, 3].where(big).count()\n  b := [E{active: true}, E{active: false}].where(.active).count()\n  if a + b == 3 { return 1 }\n  return 0\n}\n");
        assert!(!d.has_errors(), "count should check on scalar and struct array pipelines");
    }

    #[test]
    fn field_projection_from_slice_source_rejected() {
        // A `slice<Struct>` parameter is constructible, but a `.field` projection needs a
        // slot-backed source (MIR `IndexField`); projecting from a `{ptr,len}` view would
        // miscompile, so reject it cleanly.
        let (_p, d) = check("P { pay: i32, active: bool }\nfn total(xs: slice<P>) -> i32 = xs.pay.sum()\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "field projection from a slice source must be rejected");
    }

    #[test]
    fn to_array_inside_arena_checks() {
        // MMv2 slice 3: `.to_array()` inside an arena yields an owned array (consumed here).
        let (_p, d) = check("fn double(x: i32) -> i32 = x * 2\nfn main() -> i32 {\n  arena {\n    return [1, 2, 3].map(double).to_array().sum()\n  }\n}\n");
        assert!(!d.has_errors(), "to_array inside an arena should check");
    }

    #[test]
    fn to_array_outside_arena_now_allowed() {
        // MMv2 slice 4: `.to_array()` outside an arena is free-standing (heap + drop), so it
        // checks (the owned array is dropped at function exit).
        let (_p, d) = check("fn double(x: i32) -> i32 = x * 2\nfn main() -> i32 {\n  return [1, 2, 3].map(double).to_array().sum()\n}\n");
        assert!(!d.has_errors(), "to_array outside an arena is now free-standing (heap + drop)");
    }

    #[test]
    fn to_array_owned_cannot_escape_arena() {
        // The owned array is arena-allocated (region Arena(k)); letting it escape as the arena
        // block's value (bound outside the arena) must be rejected.
        let (_p, d) = check("fn double(x: i32) -> i32 = x * 2\nfn main() -> i32 {\n  bad := arena {\n    [1, 2, 3].map(double).to_array()\n  }\n  return 0\n}\n");
        assert!(d.has_errors(), "an arena-allocated owned array must not escape its arena");
    }

    #[test]
    fn move_owned_local_through_if_arm_rejected() {
        // MMv2 slice 4.5: moving a *bound* owned array out through an `if`/`else` arm is a
        // deferred-feature error (codegen only nulls slots at direct move sites). A fresh
        // temporary through an `if` is fine — there is no bound slot to double-free.
        let (_p, d) = check("fn double(x: i32) -> i32 = x * 2\nfn pick(c: bool) -> array<i32> {\n  ys := [1, 2, 3].map(double).to_array()\n  zs := [4, 5, 6].map(double).to_array()\n  return if c { ys } else { zs }\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "moving a bound owned local out through an if/else arm must error");
    }

    #[test]
    fn conditional_move_then_no_later_use_checks() {
        // Moving an owned local on only one path (with no later use of the source) is allowed:
        // MIR nulls the slot at the move site so the not-moved path is still freed at exit.
        let (_p, d) = check("fn double(x: i32) -> i32 = x * 2\nfn run(c: bool) -> i32 {\n  ys := [1, 2, 3].map(double).to_array()\n  mut total := 0\n  if c {\n    zs := ys\n    total = zs.sum()\n  }\n  return total\n}\nfn main() -> i32 = run(true)\n");
        assert!(!d.has_errors(), "a one-path move with no later use of the source should check");
    }

    #[test]
    fn min_over_non_numeric_errors() {
        // `min`/`max` need a numeric element, like `sum`. A bool-producing map is rejected.
        let (_p, d) = check("fn isbig(x: i32) -> bool = x > 1\nfn main() -> i32 {\n  if [1, 2, 3].map(isbig).min() { return 1 }\n  return 0\n}\n");
        assert!(d.has_errors(), "min over a non-numeric element must error");
    }

    #[test]
    fn min_max_inline_checks() {
        let (_p, d) = check("fn id(x: i32) -> i32 = x\nfn main() -> i32 {\n  return [3, 1, 2].map(id).min() + [3, 1, 2].map(id).max()\n}\n");
        assert!(!d.has_errors(), "min + max over an i32 pipeline should check");
    }

    #[test]
    fn scan_inline_checks() {
        // scan(init, f) with f: (i32, i32) -> i32 yields array<i32>; summing it checks.
        let (_p, d) = check("fn add(acc: i32, x: i32) -> i32 = acc + x\nfn id(x: i32) -> i32 = x\nfn main() -> i32 {\n  return [1, 2, 3].map(id).scan(0, add).sum()\n}\n");
        assert!(!d.has_errors(), "scan(0, add) over an i32 pipeline should check");
    }

    #[test]
    fn scan_fn_arity_mismatch_errors() {
        // scan needs a 2-arg fold; a 1-arg function must error.
        let (_p, d) = check("fn bad(x: i32) -> i32 = x\nfn main() -> i32 {\n  return [1, 2, 3].scan(0, bad).sum()\n}\n");
        assert!(d.has_errors(), "scan with a non-binary function must error");
    }

    #[test]
    fn sort_inline_checks() {
        let (_p, d) = check("fn id(x: i32) -> i32 = x\nfn h(acc: i32, x: i32) -> i32 = acc + x\nfn main() -> i32 {\n  return [3, 1, 2].map(id).sort().reduce(0, h)\n}\n");
        assert!(!d.has_errors(), "sort of a numeric pipeline should check");
    }

    #[test]
    fn sort_over_struct_element_rejected() {
        let (_p, d) = check("Point { x: i32, y: i32 }\nfn main() -> i32 {\n  s := [Point { x: 1, y: 2 }].sort()\n  return 0\n}\n");
        assert!(d.has_errors(), "sort over struct elements must error (project a field first)");
    }

    #[test]
    fn dot_length_mismatch_errors() {
        let (_p, d) = check("fn main() -> i32 {\n  xs := [1, 2, 3]\n  ys := [4, 5]\n  return xs.dot(ys)\n}\n");
        assert!(d.has_errors(), "dot of unequal-length arrays must error");
    }

    #[test]
    fn dot_element_type_mismatch_errors() {
        // An int array dotted with a float array must error (no implicit numeric coercion).
        let (_p, d) = check("fn main() -> i32 {\n  xs := [1, 2, 3]\n  ys := [1.0, 2.0, 3.0]\n  if xs.dot(ys) == 0 { return 1 }\n  return 0\n}\n");
        assert!(d.has_errors(), "dot of mismatched element types must error");
    }

    #[test]
    fn dot_arbitrary_right_operand_rejected_not_panicked() {
        // An `if` expression as the right operand is an arbitrary array expr; it must be
        // rejected in sema, not reach `array_source_slot` and panic in MIR.
        let (_p, d) = check("fn main() -> i32 {\n  xs := [1, 2, 3]\n  ys := [4, 5, 6]\n  zs := [7, 8, 9]\n  c := true\n  if xs.dot(if c { ys } else { zs }) == 32 { return 1 }\n  return 0\n}\n");
        assert!(d.has_errors(), "an arbitrary array expr as dot's right operand must error");
    }

    #[test]
    fn dot_inline_checks() {
        let (_p, d) = check("fn main() -> i32 {\n  xs := [2, 3, 4]\n  ys := [5, 6, 7]\n  if xs.dot(ys) == 56 { return 1 }\n  return 0\n}\n");
        assert!(!d.has_errors(), "dot of two equal-length i64 arrays should check");
    }

    #[test]
    fn scan_over_struct_element_rejected_not_panicked() {
        // A struct element (no field projection) must be rejected in sema, not panic in MIR.
        let (_p, d) = check("Point { x: i32, y: i32 }\nfn addx(acc: i32, p: Point) -> i32 = acc + p.x\nfn main() -> i32 {\n  return [Point { x: 1, y: 2 }].scan(0, addx).sum()\n}\n");
        assert!(d.has_errors(), "scan over struct elements must error (project a field first)");
    }

    #[test]
    fn scan_struct_accumulator_rejected() {
        // A struct accumulator (ty_to_scalar succeeds for structs) must be rejected explicitly.
        let (_p, d) = check("Acc { s: i32 }\nfn step(a: Acc, x: i32) -> Acc = Acc { s: a.s + x }\nfn id(x: i32) -> i32 = x\nfn main() -> i32 {\n  return [1, 2, 3].map(id).scan(Acc { s: 0 }, step).len()\n}\n");
        assert!(d.has_errors(), "scan with a struct accumulator must error");
    }

    #[test]
    fn reduce_fold_type_mismatch_errors() {
        // fold that takes the wrong element type.
        let (_p, d) = check(
            "fn add(acc: i32, x: bool) -> i32 = acc\nfn main() -> i32 {\n  return [1, 2, 3].reduce(0, add)\n}\n",
        );
        assert!(d.has_errors(), "a fold whose element param mismatches must error");
    }

    #[test]
    fn slice_param_pipeline_checks() {
        let (_p, d) = check(
            "fn total(xs: slice<i32>) -> i32 = xs.sum()\nfn main() -> i32 {\n  return total([1, 2, 3])\n}\n",
        );
        assert!(!d.has_errors(), "array → slice<i32> + sum over a slice should check");
    }

    #[test]
    fn empty_array_literal_errors() {
        let (_p, d) = check("fn main() -> i32 {\n  return [].sum()\n}\n");
        assert!(d.has_errors(), "an empty array literal needs a type");
    }

    #[test]
    fn sum_on_non_array_errors() {
        let (_p, d) = check("fn main() -> i32 {\n  x := 1\n  return x.sum()\n}\n");
        assert!(d.has_errors(), "`.sum()` on a non-array must error");
    }

    #[test]
    fn use_after_move_errors() {
        let (_p, d) = check(
            "fn main() -> i32 {\n  arena {\n    p: box<i32> := heap.new(7)\n    q: box<i32> := p\n    return p.get()\n  }\n}\n",
        );
        assert!(d.has_errors(), "using a box after it is moved must error");
    }

    #[test]
    fn clone_does_not_move() {
        let (_p, d) = check(
            "fn main() -> i32 {\n  arena {\n    p: box<i32> := heap.new(7)\n    q: box<i32> := p.clone()\n    p.get() + q.get()\n  }\n}\n",
        );
        assert!(!d.has_errors(), "clone borrows; the original stays usable");
    }

    #[test]
    fn arena_box_value_escape_errors() {
        // Yielding a freshly-allocated box as the arena's value escapes the arena.
        let (_p, d) = check("fn main() -> i32 {\n  b := arena {\n    heap.new(7)\n  }\n  return 0\n}\n");
        assert!(d.has_errors(), "a box must not escape as the arena block's value");
    }

    #[test]
    fn return_box_escape_errors() {
        let (_p, d) = check(
            "fn make() -> box<i32> {\n  arena {\n    p: box<i32> := heap.new(7)\n    return p\n  }\n}\n",
        );
        assert!(d.has_errors(), "returning an arena box must error");
    }

    #[test]
    fn assign_box_to_outer_binding_escapes() {
        let (_p, d) = check(
            "fn main() -> i32 {\n  arena {\n    mut saved: box<i32> := heap.new(0)\n    arena {\n      p: box<i32> := heap.new(7)\n      saved = p\n    }\n    saved.get()\n  }\n}\n",
        );
        assert!(d.has_errors(), "binding an inner-arena box to an outer binding must error");
    }

    #[test]
    fn box_escape_via_if_branches_errors() {
        // A box reaching the arena value through `if` branches must still be caught.
        let (_p, d) = check(
            "fn main() -> i32 {\n  b := arena {\n    if true { heap.new(1) } else { heap.new(2) }\n  }\n  return 0\n}\n",
        );
        assert!(d.has_errors(), "a box escaping via if-branch values must error");
    }

    #[test]
    fn box_parameter_and_return_forbidden() {
        let (_p, d) = check("fn id(b: box<i32>) -> box<i32> {\n  return b\n}\nfn main() -> i32 {\n  return 0\n}\n");
        assert!(d.has_errors(), "box params/returns are forbidden in M3");
    }

    #[test]
    fn move_through_block_value_is_tracked() {
        // The block's tail value consumes p, so reusing p afterwards is a move error.
        let (_p, d) = check(
            "fn main() -> i32 {\n  arena {\n    p: box<i32> := heap.new(1)\n    q: box<i32> := {\n      p\n    }\n    return p.get()\n  }\n}\n",
        );
        assert!(d.has_errors(), "a box moved through a block value must be tracked");
    }

    #[test]
    fn heap_new_outside_arena_errors() {
        let (_p, d) = check("fn main() -> i32 {\n  p: box<i32> := heap.new(5)\n  return p.get()\n}\n");
        assert!(d.has_errors(), "heap.new outside an arena must error");
    }

    #[test]
    fn heap_new_payload_infers_from_binding_annotation() {
        // The inline form `v: i32 := heap.new(7).get()` used to miscompile: the boxed literal
        // defaulted to i64 (an i64 box read into an i32 slot), and the width mismatch was not caught.
        // The binding annotation now flows into the `heap.new` payload, so the box is `box<i32>`.
        for ann in ["i8", "u8", "i32", "u64", "i64"] {
            let (_p, d) = check(&format!(
                "fn main() -> i32 {{\n  arena {{\n    v: {ann} := heap.new(7).get()\n    return v as i32\n  }}\n}}\n"
            ));
            assert!(!d.has_errors(), "`v: {ann} := heap.new(7).get()` should infer the payload and check");
        }
        // A float payload infers likewise.
        let (_f, df) = check("fn main() -> i32 {\n  arena {\n    v: f64 := heap.new(3.5).get()\n    return 0\n  }\n}\n");
        assert!(!df.has_errors(), "a float box payload should infer from the annotation");
        // `.clone()` on a fresh `heap.new` receiver threads the expected box type inward too.
        let (_c, dc) = check("fn main() -> i32 {\n  arena {\n    q: box<i32> := heap.new(11).clone()\n    return q.get()\n  }\n}\n");
        assert!(!dc.has_errors(), "`box<i32> := heap.new(11).clone()` should infer the payload and check");
    }

    #[test]
    fn box_get_result_width_mismatch_is_caught_once() {
        // A `box<i64>` variable read into an `i32` slot is a genuine mismatch — it must be a single
        // clean type error (the reconciliation must not double-report alongside any inner check).
        let (_p, d) = check("fn main() -> i32 {\n  arena {\n    p: box<i64> := heap.new(7)\n    v: i32 := p.get()\n    return v\n  }\n}\n");
        assert!(d.has_errors(), "reading an i64 box into an i32 slot must be a type error");
        assert_eq!(d.error_count(), 1, "the box-get width mismatch must be reported exactly once");
    }

    #[test]
    fn value_result_width_mismatch_is_caught_across_contexts() {
        // A value expression whose concrete type differs from its slot must be rejected in every
        // context — not silently narrowed. Each case reports exactly one error (no double-report).
        // let-binding:
        let (_a, da) = check("fn r() -> i64 = 7\nfn main() -> i32 {\n  v: i32 := r()\n  return v\n}\n");
        assert_eq!(da.error_count(), 1, "i64 value into an i32 let must be one error");
        // return position:
        let (_b, db) = check("fn r() -> i64 = 7\nfn main() -> i32 {\n  return r()\n}\n");
        assert_eq!(db.error_count(), 1, "returning i64 from an i32 fn must be one error");
        // call argument:
        let (_c, dc) = check("fn r() -> i64 = 7\nfn id(x: i32) -> i32 = x\nfn main() -> i32 {\n  return id(r())\n}\n");
        assert_eq!(dc.error_count(), 1, "an i64 argument to an i32 param must be one error");
        // struct field initializer:
        let (_e, de) = check("S { x: i32 }\nfn r() -> i64 = 7\nfn main() -> i32 {\n  s := S { x: r() }\n  return s.x\n}\n");
        assert_eq!(de.error_count(), 1, "an i64 struct-field value into an i32 field must be one error");
        // assignment:
        let (_f, df) = check("fn r() -> i64 = 7\nfn main() -> i32 {\n  mut v: i32 := 0\n  v = r()\n  return v\n}\n");
        assert_eq!(df.error_count(), 1, "assigning an i64 value to an i32 var must be one error");
        // a reduction terminal already enforces its result type — must stay a single report:
        let (_g, dg) = check("fn main() -> i32 {\n  xs := [1, 2, 3]\n  s: i32 := xs.sum()\n  return s\n}\n");
        assert_eq!(dg.error_count(), 1, "an i64 array-sum into an i32 slot must be reported exactly once");
    }

    #[test]
    fn get_on_non_box_errors() {
        let (_p, d) = check("fn main() -> i32 {\n  x := 1\n  return x.get()\n}\n");
        assert!(d.has_errors(), "`.get()` on a non-box must error");
    }

    #[test]
    fn main_arguments_only_array_str() {
        // `main(args: array<str>)` with a `Result<(), Error>` return is accepted (PR-C, argv).
        let (_p, ok) = check("pub fn main(args: array<str>) -> Result<(), Error> {\n  print(args.len())\n  return Ok(())\n}\n");
        assert!(!ok.has_errors(), "main(args: array<str>) -> Result should check");
        // Any other main parameter is rejected.
        let (_q, bad) = check("fn main(n: i32) -> i32 {\n  return n\n}\n");
        assert!(bad.has_errors(), "main with a non-`array<str>` argument must error");
        // `main(args)` must return Result (the only form the wrapper marshals argv into).
        let (_r, noresult) = check("fn main(args: array<str>) -> i32 = 0\n");
        assert!(noresult.has_errors(), "main(args) with a non-Result return must error");
    }

    #[test]
    fn main_error_type_restricted_to_builtin() {
        // `main() -> Result<(), Error>` (builtin Error) is the accepted fallible form.
        let (_p, ok) = check("fn main() -> Result<(), Error> {\n  return Ok(())\n}\n");
        assert!(!ok.has_errors(), "main() -> Result<(), Error> must check");
        // A user-defined error enum at `main`'s `E` position is rejected — the C-`main` wrapper's
        // exit-code lowering only knows the builtin `Error` layout, so any other enum would
        // miscompile. Must be a clean diagnostic, not a codegen `extract index out of range`.
        let (_q, userenum) = check("MyErr { Bad, Worse }\nfn main() -> Result<(), MyErr> {\n  return Err(MyErr.Bad)\n}\n");
        assert!(userenum.has_errors(), "main with a user-defined error type must error");
        assert!(
            userenum
                .iter()
                .any(|d| d.message.contains("main's error type must be the builtin `Error`")),
            "the diagnostic must name the builtin-Error restriction",
        );
        // Same restriction on the argv form.
        let (_r, argv) = check("MyErr { Bad }\nfn main(args: array<str>) -> Result<(), MyErr> {\n  return Err(MyErr.Bad)\n}\n");
        assert!(argv.has_errors(), "main(args) with a user-defined error type must error");
        // A fallible main's `Ok` type must be `()` — a non-unit Ok has no exit-code meaning and
        // was silently discarded by the wrapper (return a value via `-> i32` instead).
        let (_t, okint) = check("fn main() -> Result<i32, Error> {\n  return Ok(0)\n}\n");
        assert!(okint.has_errors(), "main() -> Result<i32, Error> (non-unit Ok) must error");
        assert!(
            okint
                .iter()
                .any(|d| d.message.contains("main's Ok type must be `()`")),
            "the diagnostic must name the unit-Ok restriction",
        );
        // The argv form carries the same Ok restriction.
        let (_u, argvok) = check("fn main(args: array<str>) -> Result<i32, Error> {\n  return Ok(0)\n}\n");
        assert!(argvok.has_errors(), "main(args) -> Result<i32, Error> (non-unit Ok) must error");
        // `main() -> i32` stays valid (the C-entry form).
        let (_s, i32main) = check("fn main() -> i32 {\n  return 0\n}\n");
        assert!(!i32main.has_errors(), "main() -> i32 must check");
    }

    #[test]
    fn question_on_non_result_errors() {
        let (_p, d) = check("fn f() -> Result<i32, Error> {\n  x := 1?\n  return Ok(x)\n}\n");
        assert!(d.has_errors(), "`?` on a plain int must error");
    }

    #[test]
    fn field_assign_requires_mut() {
        let src = format!(
            "{POINT}fn main() -> i32 {{\n  p := Point {{ x: 1, y: 2 }}\n  p.x = 5\n  return p.x\n}}\n"
        );
        let (_p, d) = check(&src);
        assert!(d.has_errors(), "assigning a field of an immutable struct must error");
    }

    #[test]
    fn field_assign_after_whole_move_rejected() {
        // `A` is a Move type (owns a `string` field); `t := p` moves it whole out of `p`, so
        // writing into `p.s` afterwards is a use of a moved value — same as reading `p.s` would
        // be. (`Stmt::AssignField` used to skip this check; `Stmt::AssignIndex` already had it.)
        let (_p, d) = check(
            "A { s: string }\nfn main() -> i32 {\n  mut p := A { s: \"x\".clone() }\n  t := p\n  p.s = \"y\".clone()\n  return 0\n}\n",
        );
        assert!(d.has_errors(), "assigning a field of an already-moved struct must error");
    }

    #[test]
    fn field_assign_without_move_still_checks() {
        // Same shape, without the intervening whole-struct move: the field assignment is fine.
        let (_p, d) = check(
            "A { s: string }\nfn main() -> i32 {\n  mut p := A { s: \"x\".clone() }\n  p.s = \"y\".clone()\n  return 0\n}\n",
        );
        assert!(!d.has_errors(), "assigning a field of a not-yet-moved struct must check");
    }

    // `chunks` over a frame-local scalar array yields an `array<slice<T>>` (`DynSliceArray`) whose
    // slice headers borrow the source array's frame storage. The single escape rule (`region_of` +
    // `outlives`) must forbid that borrowing result from outliving the source — a frame-local scalar
    // array is given a `Frame`/arena-depth region (see `EscapeCheck::stmt` `Let`) so the check fires.
    // (Latent until the "array elements are scalar-only" restriction lifts, but reachable today via
    // the arena block value / outer-assign paths, which do not require writing the `array<slice<T>>`
    // type. Was silently accepted before the region was added — a use-after-free of the frame slot.)

    #[test]
    fn chunks_of_arena_local_cannot_escape_as_block_value() {
        // (a) `cs := arena { xs := [...]; xs.chunks(2) }` — the chunks result borrows `xs`, declared
        // inside the arena, so it must not escape as the arena block's value.
        let (_p, d) = check(
            "fn main() -> i32 {\n  cs := arena { xs := [1, 2, 3, 4]\n    xs.chunks(2)\n  }\n  return 0\n}\n",
        );
        assert!(d.has_errors(), "chunks of an arena-local array must not escape as the arena block value");
    }

    #[test]
    fn chunks_of_arena_local_cannot_escape_via_outer_assign() {
        // (b) assigning a chunks of an arena-declared array to an outer (shallower) binding escapes
        // the arena, so it must be rejected.
        let (_p, d) = check(
            "fn main() -> i32 {\n  mut cs := [9, 9].chunks(1)\n  arena { xs := [1, 2, 3, 4]\n    cs = xs.chunks(2)\n  }\n  print(cs.len())\n  return 0\n}\n",
        );
        assert!(d.has_errors(), "chunks of an arena-local array must not escape via assignment to an outer binding");
    }

    #[test]
    fn chunks_used_in_same_scope_ok() {
        // (c) regression: the ordinary use — chunk and consume in the same scope — must still pass.
        let (_p, d) = check(
            "fn main() -> i32 {\n  xs := [1, 2, 3, 4]\n  cs := xs.chunks(2)\n  print(cs.len())\n  return 0\n}\n",
        );
        assert!(!d.has_errors(), "same-scope chunks use must still type-check");
    }

    #[test]
    fn chunks_bound_in_arena_used_locally_ok() {
        // (c) regression: a chunks bound inside an arena and consumed there (a scalar result escapes)
        // must pass — the borrowing result stays within the arena. Also guards the drop set: the
        // chunks header buffer is heap-`malloc`'d, so it must still be freed (not leaked) even though
        // its region is now `Arena(k)` (the drop-set filter drops `DynSliceArray` regardless).
        let (_p, d) = check(
            "fn chunk_sum(c: slice<i64>) -> i64 = c.sum()\nfn main() -> i32 {\n  t := arena { xs := [1, 2, 3, 4]\n    cs := xs.chunks(2)\n    cs.par_map(chunk_sum).sum()\n  }\n  print(t)\n  return 0\n}\n",
        );
        assert!(!d.has_errors(), "a chunks bound and consumed inside its arena must type-check");
    }

    #[test]
    fn chunks_of_local_cannot_be_returned() {
        // (d) the return path is now rejected by the escape check (in addition to the not-yet-liftable
        // `array<slice<T>>` element restriction): the chunks result borrows the frame-local `xs`, so
        // returning it is a use-after-free of the frame slot.
        let (_p, d) = check(
            "fn f() -> array<slice<i64>> {\n  xs := [1, 2, 3, 4]\n  return xs.chunks(2)\n}\nfn main() -> i32 { return 0 }\n",
        );
        assert!(
            d.iter().any(|e| e.message.contains("borrows local storage")),
            "returning a chunks of a local array must raise the escape (local-storage borrow) error"
        );
    }

    // The same hole exists for a `str` array, and is *more* insidious: `array<str>` is
    // region-tracked (`str` tracks), so its `Let` stores its **element** region (`Static` for str
    // literals) in the region map — hiding the frame **storage** region. `chunks` binds to the
    // storage region (`chunks_source_storage_region`), not the element region, so the escape is
    // caught while an element read (`xs[0]`, a `str` view of static data) stays returnable.

    #[test]
    fn chunks_of_str_array_cannot_escape_via_outer_assign() {
        // gemini's exact reproduction: an `array<str>` chunk escaping an arena via an outer binding.
        let (_p, d) = check(
            "fn main() -> i32 {\n  mut cs := [\"x\"].chunks(1)\n  arena {\n    xs := [\"a\", \"b\", \"c\", \"d\"]\n    cs = xs.chunks(2)\n  }\n  print(cs.len())\n  return 0\n}\n",
        );
        assert!(d.has_errors(), "chunks of an arena-local str array must not escape via outer assignment");
    }

    #[test]
    fn chunks_of_str_array_cannot_escape_as_block_value() {
        let (_p, d) = check(
            "fn main() -> i32 {\n  cs := arena { xs := [\"a\", \"b\", \"c\", \"d\"]\n    xs.chunks(2)\n  }\n  return 0\n}\n",
        );
        assert!(d.has_errors(), "chunks of an arena-local str array must not escape as the arena block value");
    }

    #[test]
    fn chunks_of_str_array_cannot_be_returned() {
        // The `array<slice<str>>` return type is not yet expressible (composite array element), so a
        // type error also fires; the point is that the escape (frame-storage borrow) error is present.
        let (_p, d) = check(
            "fn f() -> array<slice<str>> {\n  xs := [\"a\", \"b\", \"c\", \"d\"]\n  return xs.chunks(2)\n}\nfn main() -> i32 { return 0 }\n",
        );
        assert!(
            d.iter().any(|e| e.message.contains("borrows local storage")),
            "returning a chunks of a local str array must raise the escape (local-storage borrow) error"
        );
    }

    #[test]
    fn str_array_element_read_still_returnable() {
        // Regression: reading an element out of a fixed `str` array yields a `str` view whose region
        // is the element's (a literal → `Static`), so it must stay returnable — the storage-region
        // fix for `chunks` must not clobber the element region.
        let (_p, d) = check(
            "fn f() -> str {\n  xs := [\"a\", \"b\"]\n  return xs[0]\n}\nfn main() -> i32 { return 0 }\n",
        );
        assert!(!d.has_errors(), "reading a str element out of a local str array must stay returnable");
    }

    #[test]
    fn chunks_of_struct_array_rejected() {
        // A struct (AoS) array cannot be chunked today (`chunks` requires a scalar/str element view),
        // so there is no escape hole — it is rejected at the type-check with no `array<slice<…>>`
        // ever formed. (If AoS chunking is added later, `chunks_source_storage_region` already covers
        // a `Ty::StructArray` local, so the escape stays closed.)
        let (_p, d) = check(
            "P { x: i64 }\nfn main() -> i32 {\n  xs := [P{x: 1}, P{x: 2}]\n  cs := xs.chunks(1)\n  return 0\n}\n",
        );
        assert!(d.has_errors(), "chunking a struct array must be rejected");
    }
}
