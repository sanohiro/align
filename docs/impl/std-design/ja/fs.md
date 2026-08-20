このファイルは、下記の `std.fs` 拡張を実装するための設計である。公開契約台帳は
[`../27-fs-exclusive-publication-plan.md`](../27-fs-exclusive-publication-plan.md) と
[`../29-fs-retained-root-plan.md`](../29-fs-retained-root-plan.md) にある。

# std.fs — 明示的な trusted filesystem 境界

> 🌐 [English](../fs.md) · **日本語**

> **ステータス:** Request 14 は 2026-08-19 に実装済み（設計 PR #859 は
> `a21eb8416f2088df68026f10c63a38cd0bd65538`、実装 PR #861 は
> `3c2edd2f399c9e2c9551b4227c61b36d6a041e20` として merge）。align-llm の adoption gate は未完了。
> Request 18 の retained-root regular-file access は実装済みで、align-llm の adoption gate は未完了。

## 概要

これは既存の M9 ファイルシステム API への狭い拡張であり、競合するディレクトリエントリを置換せずに
result と evidence sidecar を公開するための、次の 2 つのネイティブプリミティブを提供する。

```text
fs.create_exclusive(path: str) -> Result<writer, Error>
fs.rename_no_replace(source: str, destination: str) -> Result<(), Error>
```

操作は独立しており Impure である。2 ファイルのトランザクション、新しい writer 型、既存の `writer`
Move/`Drop` 契約の変更は導入しない。

Request 18 は、1 つの retained root 配下の通常ファイルを扱う別の 2 操作を追加する。

```text
fs.open_beneath(root: str, relative: str) -> Result<reader, Error>
fs.create_exclusive_beneath(root: str, relative: str) -> Result<writer, Error>
```

これらは root、途中、末尾の symlink を拒否し、保持した directory descriptor から走査する。公開
directory-handle 型、metadata API、canonical path、sandbox、process-global root は追加しない。

## 公開契約

### `create_exclusive`

`create_exclusive` は、受理する Unix ターゲットで
`O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC|O_NOFOLLOW` 相当の排他的 open を 1 回実行する。末尾の要素は追跡しない。
末尾にファイル、ディレクトリ、シンボリックリンク、FIFO、デバイスなどがすでに存在する場合は
`Error.Code(native EEXIST)` となり、開くこと、切り詰めること、置換、削除を行わない。親要素は通常の OS の
パス解決に従い、`realpath`、親の走査、途中の symlink 拒否は追加しない。

成功時は既存の所有 `writer` を返す。writer は 1 つの descriptor と既存のバッファを所有し、`Drop` は
best effort で flush して descriptor を閉じる。`Drop` はファイルを削除しない。write/flush の失敗で通常
ファイルの一部が残ることがあり、必要な cleanup は既存の明示的な remove を呼び出し側が行う。

### `rename_no_replace`

`rename_no_replace` は、ネイティブの no-replace directory-entry rename を正確に 1 回実行する。Linux では
`renameat2(AT_FDCWD, ..., RENAME_NOREPLACE)`、macOS では
`renameatx_np(AT_FDCWD, ..., RENAME_EXCL)` を用いる。宛先は不在でなければならない。ファイル、ディレクトリ、
symlink、FIFO、デバイスを含む占有済みの宛先は `Error.Code(native EEXIST)` となり、変更されない。ソースは
エントリとして移動されるため、source symlink や特殊ファイルを open したり、事前に型検査したりしない。
C6f2 は `create_exclusive` で作成した通常ファイルに限定する trusted-path/single-writer 前提を別途所有する。

source 不在、別ファイルシステム、未対応 volume、親ディレクトリ不在、権限、長さなどのネイティブエラーは
固定 errno table に従う。通常の置換 rename、`link`+remove による emulation、subprocess、事前の存在検査、
別ファイルシステムへの copy、`fsync`、クラッシュ耐久性の保証はない。成功すると source 名はなくなり、
同じ directory entry が destination 名になる。open descriptor の扱いは OS に従う。

### `open_beneath`

`open_beneath` は 1 つの root path と 1 つの strict relative path を受け取る。root は absolute、既存の
current-directory 規則に従う relative、正確な `.`、または正確な `/` とする。それ以外の root component は
空でなく `.`/`..` ではない。relative path は空でない相対パスで、先頭/末尾 slash と空、`.`、`..` component
を含まない。

runtime は directory を開く前に 2 つの lexical input 全体を検査する。次に開始 directory を保持し、root と
relative parent の全 component を descriptor-relative no-follow operation で走査する。観測した component と
opened component は同じ directory identity でなければならない。最後の parent では末尾を follow せずに観測し、
regular file を要求し、read-only/nonblocking/no-follow で開き、descriptor の型と identity を再検査してから既存の
owned `reader` を公開する。constructor は artifact byte を読まない。missing は `NotFound`、permission は
`Denied`、unsafe grammar、symlink、non-directory intermediate、non-regular final、identity/type change は
`Invalid` とする。

成功後の read は保持した file descriptor を使い、公開 path の rename/replace で reader の対象は変わらない。
別 descriptor からの byte mutation は防がない。immutable input が必要な caller は明示的な single-writer
precondition を維持する。

### `create_exclusive_beneath`

`create_exclusive_beneath` は同じ root/relative grammar と retained directory walk を使う。保持した final
parent で close-on-exec と final no-follow を伴う native exclusive create を 1 回実行する。占有済みの末尾は
すべて既存の `Error.Code` mapping による native EEXIST となり変更されない。成功時は既存の owned `writer`
を返し、partial write、flush、Drop、明示的 cleanup は `create_exclusive` と同一である。

parent、temporary name、transaction、rename、rollback、durability state は作らない。これは 1 ファイルの
retained-parent constructor であり、no-replace rename と C6f2 pair publication は引き続き Request 14 が所有する。

## パスと ABI の規則

Request 14 の両操作の path view は呼び出し中だけ借用される。path は空でなく、有効な UTF-8 で、NUL を含まず、呼び出し中
有効な読み取り専用 byte range で表現されなければならない。相対パスは既存の `std.fs` と同じ current directory
に対して解決される。runtime は長さ/null、UTF-8、空、内部 NUL を、ネイティブ side effect の前に検査する。
検査可能な `len + 1` capacity overflow は `Error.Invalid` とする。実際の allocation failure は Align の
locked immediate-abort OOM 方針に従い、新しい recoverable filesystem error にはしない。

`create_exclusive` は既存の writer constructor ABI shape を使う。

```text
align_rt_io_writer_create_exclusive(
    path_ptr: ptr, path_len: i64, out_writer: ptr
) -> i32
```

runtime は `out_writer` の null を最初に検査し、その後の検査の前に slot を null にする。caller の slot は
有効な writable `*mut *mut Writer` でなければならず、foreign caller がこの前提に違反した場合は recoverable
ABI 契約の外である。recoverable failure で writer を公開しない。

`rename_no_replace` は既存の 4 引数 path/status ABI shape を使う。

```text
align_rt_fs_rename_no_replace(
    source_ptr: ptr, source_len: i64,
    destination_ptr: ptr, destination_len: i64
) -> i32
```

source の検査と一時 NUL 終端 copy を destination の検査/allocation より先に完了させる。どちらの操作も
native call 後に path を保持しない。compiler は 2 操作に別々の HIR/MIR kind と runtime key を割り当て、
`fs.create` の mode bit や通常の rename として扱わない。

retained-root 操作はそれぞれ 2 つの path view を借用し、A12 ABI shape を使う。

```text
align_rt_io_reader_open_beneath(
    root_ptr: ptr, root_len: i64,
    relative_ptr: ptr, relative_len: i64,
    out_reader: ptr,
) -> i32

align_rt_io_writer_create_exclusive_beneath(
    root_ptr: ptr, root_len: i64,
    relative_ptr: ptr, relative_len: i64,
    out_writer: ptr,
) -> i32
```

検査順は output slot、root 全体の validation/copy/grammar、relative 全体の validation/copy/grammar、
root traversal、relative-parent traversal、final operation とする。したがって不正な root grammar はすべての
relative-view error より先になる。recoverable failure では両 slot とも
null のままである。checked copy-size overflow は `Error.Invalid`、実際の OOM は terminal とする。完全な grammar
検証後にだけ private な full-path copy を NUL 区切りの component storage にし、caller の byte は変更しない。
走査中に live な directory descriptor は最大 2 つで、すべての path/component owner は call とともに終了する。

## pair 公開の consumer

プリミティブ自身は 2 ファイルの atomicity を保証しない。C6f2 consumer は trusted-path/single-writer 前提を
確立した後、次の可視な順序を所有する。

```text
create_exclusive(result_tmp)
write + flush + Drop(result_tmp)
create_exclusive(evidence_tmp)
write + flush + Drop(evidence_tmp)
recheck result_final absent
recheck evidence_final absent
rename_no_replace(result_tmp, result_final)
rename_no_replace(evidence_tmp, evidence_final)
```

再確認は診断用に過ぎず、競合の境界は no-replace rename である。公開順は result then evidence とする。
clean な staging/finalization failure は、自分の残骸を削除した後に C6f2 が `OUTPUT_WRITE` とする。所有する
cleanup または必要な recheck が失敗した場合は、正確に残っている evaluator-owned path だけを示して
`OUTPUT_PAIR_CLEANUP_FAILED` とする。競合する final destination を削除しない。2 回目の rename 前に 1 回目が
成功した場合、その final は明示的な consumer cleanup まで残る。割り込みでは 0 個または 1 個の final と temp
残骸が残り得る。

## エラー、effect、所有権

Request 14 の両操作は directory state を変更するため `Impure` である。既存の errno table を使い、`ENOENT` →
`Error.NotFound`、`EACCES`/`EPERM` → `Error.Denied`、`EINVAL` → `Error.Invalid`、それ以外（`EEXIST` と
`EXDEV` を含む）→ `Error.Code(errno)` とする。`AlreadyExists` variant は追加しない。pair-level の
`OUTPUT_*` status は C6f2 に属し、プリミティブの error model を変更しない。

path operand は借用された `str` view であり、move も保持もされない。`create_exclusive` の result は既存の
`writer` Move value なので、write、flush、`?`、`map_err`、branch/loop join、return、early exit、Drop は
既存の所有権経路を使う。partial write 後の暗黙の rollback/delete は行わない。

retained-root 操作も `Impure` である。同じ固定 error model を使い、unsafe grammar、symlink/non-directory
traversal component、non-regular input、identity change を `Error.Invalid` にする。2 つの path operand は借用で、
成功した reader/writer は既存の Move/Drop 経路をそのまま使う。同じ final の open/create pair に hidden exclusion
や snapshot はない。open が不在を観測すれば `NotFound`、create 後なら writer が live の間に新しい regular inode
を取得し得る。immutable input が必要な consumer はこの overlap を拒否しなければならない。

## platform 境界と non-goal

v1 の adoption floor は Linux の controlled local ext4/tmpfs filesystem と macOS の controlled local APFS filesystem である。
runtime は filesystem type を分類しない。NFS、FUSE、overlay、その他 remote/unqualified filesystem、Windows、
portable emulation はこの capability の外にある。adoption fixture は検査前に制御された filesystem 環境を記録し、
unqualified 環境は `std.fs` が暗黙分類するのではなく consumer gate で除外する。

transaction、journal、recovery daemon、process-global lock、temporary-name generator、公開 directory-handle
capability、sandbox、replacement/exchange operation、durability guarantee は提供しない。Request 14 の path-only
操作は通常の parent resolution を維持し、Request 18 の 2 constructor だけが上記の明示的 no-symlink
regular-file 境界を提供する。

## 実装と acceptance の境界

実装では、semantic/HIR、checked-HIR、replay、MIR、LLVM、runtime-key、ABI declaration、native-runtime の各経路を
別々に追加しなければならない。whole-program/per-unit identity と既存の reader/writer nominal type は維持する。予定する
ABI row は constructor が A08、2-path rename が A09 であり、runtime ABI golden と key↔symbol/export parity は
実装と同時に更新する。

owner evidence の境界は次のとおりである。

- `crates/align_driver/tests/m9_fs.rs` が formation、import、実行、readback、control flow、type diagnostic を所有する。
- `crates/align_runtime` が malformed ABI view、native flag、errno mapping、partial write、Drop、fd cleanup、platform control を所有する。
- runtime ABI declaration golden が正確な symbol、shape、parity を所有する。
- generic、interface、cache、whole/per-unit、cleanup owner は新操作が到達する境界だけを再実行する。
- align-llm の `c6f2-request14-adoption` が pair の race、cleanup、interrupt、filesystem、forbidden workaround 全体を所有する。

保証は atomic no-replace と明示的な所有権であり throughput ではないため、benchmark は不要である。

完全な closure matrix、acceptance table、review finding の処理は
[`27-fs-exclusive-publication-plan.md`](../27-fs-exclusive-publication-plan.md) にある。

Request 18 も同じ cross-stage 規則を使う。`ReaderOpenBeneath` と `CreateExclusiveBeneath` の別 node、完全な
visitor/validator/replay/MIR closure、正確な A12 runtime row と export parity、既存 handle Drop、
whole/per-unit/cache parity、Linux/macOS descriptor-walk owner、および align-llm の実 consumer
`c6d-request18-adoption` が必要である。完全な matrix は
[`29-fs-retained-root-plan.md`](../29-fs-retained-root-plan.md) にある。新契約は throughput ではなく safety と
ownership なので benchmark は不要である。
