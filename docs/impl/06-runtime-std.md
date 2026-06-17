# ランタイムと core/std ブートストラップ (draft)

`align_runtime` の ABI と、core/std をどう立ち上げるかのたたき台。`05-backend-llvm.md` が呼ぶ `align_rt_*` の実体をここで定義し、source→実行ファイルの設計を閉じる。

方針:

```text
ランタイムは薄く     GC なし。arena / 並列 / panic / 可変バッファ など「言語が要求する最小」だけ
所有はコンパイラ側    寿命・解放点は MIR で確定済み(03/04)。runtime は与えられた通り確保/解放する
core は言語に近い     可能な限り Align 自身で書く。runtime(C ABI)に降りるのは最小の下層だけ
std は OS 境界        std は OS シスコールの薄いラッパ。core を土台にする
```

この文書は **draft**。未決は末尾 + 本文 `// OPEN:`。

---

## 1. ランタイムの構成

`align_runtime` は薄いネイティブライブラリ(実装言語は Rust、`no_std` 寄りで C ABI を公開)。driver が object とリンクする(`01`/`05`)。

```text
align_runtime
  start          エントリ(_start / main 相当)。main を呼び終了コードを返す
  arena          bump allocator + 一括 reset
  heap           明示 heap (malloc 系の薄い包み)
  par            データ並列ランタイム (work-stealing or chunk 分割)
  task           I/O 並行 (task_group)
  buffer/builder 可変バイトバッファ
  panic          アボート + メッセージ
  intrinsics     memcpy/memset 等、SIMD 補助 (端数処理など)
```

`// OPEN:` 静的リンク固定か、libc にどこまで依存するか(`05 §10`)。OS 直叩き(syscall)か libc 経由かは std と共通の判断。

---

## 2. 値の ABI (コンパイラと runtime の契約)

MIR/codegen のレイアウト(`05 §1`)と一致させる。runtime はこの形を前提に受け取る。

```text
slice<T>     { T* ptr, i64 len }
array<T>     { T* ptr, i64 len, i64 cap }
str          { u8* ptr, i64 len }
builder      { u8* ptr, i64 len, i64 cap, Arena* arena? }   // 後述
arena ハンドル  Arena*   (不透明ポインタ。中身は runtime 専有)
```

すべて値渡し可能な小さなヘッダ。実体メモリは ptr の先(arena/heap/static)。

---

## 3. arena allocator

Align のメモリモデルの中心(`draft.md` §6.4)。bump allocator + ブロック出口で一括 reset。

```text
Arena* align_rt_arena_begin(void)
void*  align_rt_arena_alloc(Arena*, i64 size, i64 align)
void   align_rt_arena_reset(Arena*)        // 全確保を一括解放。個別 free なし
void   align_rt_arena_end(Arena*)          // arena 自体を返却
```

実装: 大きなブロックをチャンク単位で OS から取り、ポインタを前進させるだけ(O(1) 確保)。`reset` はポインタを先頭へ戻す(必要なら chunk を解放/プール)。`alloc` は要求 align に切り上げる(SIMD アライン, `draft.md` §3.4)。

codegen の対応(`05 §4`):

```text
arena { .. }  →  a = arena_begin(); ...本体(alloc は arena_alloc(a,..))...; arena_reset(a)/end(a)
```

arena 内 view が外へ出ないことは型検査済み(`03 §7`)なので、runtime は寿命を一切追わない。

`// OPEN:` ネスト arena の chunk 共有/再利用、明示 allocator(`arena a {}`, open-questions)時の API。

---

## 4. heap

```text
void* align_rt_heap_alloc(i64 size, i64 align)
void  align_rt_heap_free(void* ptr)
```

通常コードでは manual free しない(`draft.md` §6.5)。解放点は MIR の `Drop`(move 検査由来, `04 §1`)から codegen が `heap_free` を出す。raw alloc は `unsafe` のみ(`draft.md` §6.5)で、別の薄い API にする。

---

## 5. データ並列 (par)

MIR の `ParLoop`(`04 §6`)が降りる先(`05 §7`)。

```text
void align_rt_par_for(
  void* items, i64 len, i64 elem_size,
  i64 chunk,                       // 0 なら runtime 既定
  void (*body)(void* chunk_ptr, i64 chunk_len, void* ctx),
  void* ctx)
```

- 入力を chunk に分割し、ワーカスレッドへ。並列単位は chunk(`draft.md` §11)。
- `body` は MIR で fusion 済みの本体を切り出した関数(`05 §7`)。`Effect=Pure` 保証済み(`03 §8`)なので競合は起きない。

並列 reduce:

```text
void align_rt_par_reduce(
  ..., void (*body)(.., void* partial),     // chunk ごとの部分結果を partial に
  void (*combine)(void* acc, void* partial),// 部分結果を結合 (結合的)
  void* acc)
```

部分結果を木状/直列に combine する。`// OPEN:` 結合の順序保証(浮動小数の再現性)、スレッドプールの寿命(プロセス常駐 vs ブロック単位)。

---

## 6. I/O 並行 (task_group)

```text
Task*  align_rt_task_spawn(Result (*fn)(void* ctx), void* ctx)
Result align_rt_task_wait_all(TaskGroup*)
```

I/O 待ちの並行(`draft.md` §11)。`?` は各 spawn 結果に適用され、`wait` 合流点で最初の失敗を伝播。async/await は持たない(`non-goals.md`)ので、runtime 側でブロッキング I/O をスレッド/プールに載せる素朴な実装から始める。

---

## 7. buffer / builder

文字列出力・template 脱糖(`04 §2.5`)の土台。

```text
Builder align_rt_builder_new(Arena* a?)        // arena 紐付け可
void    align_rt_builder_write(Builder*, u8* ptr, i64 len)   // 静的部 (memcpy)
void    align_rt_builder_write_int(Builder*, i64)
void    align_rt_builder_write_f64(Builder*, f64)
str     align_rt_builder_finish(Builder*)
```

`template "Hello {name}"` は静的部 → `builder_write`(長さは文字列 meta で既知, `03`/`05 §6`)、値部 → 型別 `write_*`。静的部長さの合計が既知なら `builder_new` 時に容量事前確保(`Alloc` 1回, `04 §2.5`)。`html`/`json` 用にエスケープ付き write を別に持つ。

---

## 8. panic / トラップ

```text
noreturn void align_rt_panic(str msg, SrcLoc loc)
```

ゼロ除算など溢れ以外の算術エラー(`draft.md` §5)、未回復の不変条件違反で呼ぶ。位置は Span 由来(`05 §9`)。overflow は既定 wrap なので通常呼ばない(開発ビルドの任意チェック時のみ)。`panic` はメッセージ + 位置を stderr に出してアボート。`// OPEN:` panic を Result に変換する捕捉境界を設けるか(現状: 設けない=即アボート)。

---

## 9. エントリポイント

```text
i32 align_rt_start(i32 argc, char** argv):
  args = argv を array<str> に変換 (arena/static)
  r = user_main(args)               // pub fn main(args) -> Result<(), Error>
  match r:
    Ok      => return 0
    Err(e)  => report(e); return 非0
```

`main` の戻り(`draft.md` §17)を終了コードへ。`Error` の表示形式は error 型設計(`03`/M2)で確定。M0 では `fn main() -> i32` 直結の最小形から始め、Result 版は M2 で繋ぐ(`07-roadmap.md`)。

---

## 10. core / std のブートストラップ

```text
core  言語思想に近い基盤 (draft.md §18.1)
      option/result, array/slice/chunks, vec/mask/bitset,
      map/reduce/scan/partition/sort, str/string/bytes/buffer/builder,
      arena, json, template, hash, math
std   OS 境界 (draft.md §18.2)
      io/fs/path/process/env/time/net/cli/encoding/compress/rand/crypto/http
```

### 書く言語の方針
- **core はできる限り Align 自身で書く**。`map`/`where`/`reduce` は MIR の fusion(`04 §3`)が効くよう、特別扱いの組込みではなく**通常の Align ジェネリック関数 + intrinsic フック**として定義する方向。下層(SIMD scan、hash の核)は runtime intrinsic に降りる。
- **std は runtime + OS シスコールの薄いラッパ**を Align で書く。`fs.read_file` 等は runtime の I/O プリミティブを呼ぶ。

### ブートストラップ順 (M と整合, 07)
```text
M0-M1  最小 runtime(start/arena/panic) + 組込み print のみ。core/std はほぼ無し
M2     core.option / core.result。std.fs.read_file (? の実例)
M3     core.arena を言語機能と結線
M4     core.array / slice / reduce 群 (fusion の検証対象)
M5     core.str/string/builder, core.json, core.template
M6     core.vec / mask
M7     並列 (core 側 par_map / std task)
M8+    std 拡充 (path/env/time/net/...), pkg は対象外(draft.md §18.3)
```

`// OPEN:` core を Align で書くために必要な最小ジェネリクス(`03 §9`, M4)が解決するまでは、暫定で組込み実装にしておくものの線引き。

---

## 11. 未決事項 (要決着)

```text
- 静的リンク / libc 依存範囲 / syscall 直叩きの可否 (05 §10 と共通)
- スレッドプールの寿命 (常駐 vs ブロック単位) と par reduce の浮動小数再現性
- panic 捕捉境界を設けるか (現状: 即アボート)
- Error 型の表示・終了コード対応 (M2 error 型設計)
- core を Align で書く境界 (どこまで intrinsic に降ろすか)
- arena ネスト / 明示 allocator の API (03/04 と連動)
```

決着次第 `draft.md`(該当機能)と本書へ反映する。
