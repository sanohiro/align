このファイルは、下記の `std.fs` 拡張を実装するための設計である。公開契約台帳は
[`../27-fs-exclusive-publication-plan.md`](../27-fs-exclusive-publication-plan.md) と
[`../29-fs-retained-root-plan.md`](../29-fs-retained-root-plan.md) にある。Request 55 の single-link
拡張は [`../34-fs-single-link-plan.md`](../34-fs-single-link-plan.md) が所有する。Request 56 の private
temporary-directory lifecycle は [`../36-fs-private-temp-plan.md`](../36-fs-private-temp-plan.md) が所有する。

# std.fs — 明示的な trusted filesystem 境界

**実装済みの拡張:**
[Plan 54](../../54-r69-r76-prerequisite-batch-plan.md) は、保持した directory の
`read_link(path, max_bytes)`、`metadata_follow(path)`、`access(mode)`、
`access_at(path, mode)`、`create_symlink(path, target)` を定義する。
path/target は bytes、`fs.access_mode` は必須の read/write/execute Bool フィールドを持つ。
結果は順に所有 bytes、既存の Copy metadata、Bool、Bool、unit で、すべて Result/Error を使う。
receiver は共有借用、入力の借用は呼び出し中だけ。実 UID/GID と ACL の意味、最終リンクの扱い、
上限、確保、ネイティブエラー、対応環境は台帳が定める。以下は拡張前の基準となる既存操作であり、
これらの拡張にアプリケーションの木走査ポリシーは含まれない。

> 🌐 [English](../fs.md) · **日本語**

> **ステータス:** Request 14 は 2026-08-19 に実装済み（設計 PR #859 は
> `a21eb8416f2088df68026f10c63a38cd0bd65538`、実装 PR #861 は
> `3c2edd2f399c9e2c9551b4227c61b36d6a041e20` として merge）。align-llm の adoption gate は未完了。
> Request 18 の retained-root regular-file access は実装済みである。Request 55 の retained-root
> single-link open も実装済みであり、align-llm 側の adoption は外部作業として残る。Request 56 の
> private temporary-directory lifecycle は実装済みであり、release と align-llm 側 adoption が残る。

## R65 の封印済みストレージ契約

設計・レビュー指摘の反映を完了した [plan 50](../../50-r65-process-capability-handoff.md) が、明示的な
memory_kind、memory_writer、sealed_file、OS の seal、位置指定の writable-out 読み取り、
所有権、ABI、検証条件を定めます。これらは非対応ホストのエラーを定めた Linux 固有の
OS 機能であり、Mac の通常のファイル・プロセス操作の前提にはしません。データの作成は
明示的なチャンク書き込みで行い、seal は writer を消費します。起動時にファイル全体を
コピーしません。実行イメージの検証と明示的な継承は std.process の責務です。

## 概要

これは既存の M9 ファイルシステム API への狭い拡張であり、競合するディレクトリエントリを置換せずに
result と evidence sidecar を公開するための、次の 2 つのネイティブプリミティブを提供する。

```text
fs.create_exclusive(path: str) -> Result<writer, Error>
fs.rename_no_replace(source: str, destination: str) -> Result<(), Error>
```

操作は独立しており Impure である。2 ファイルのトランザクション、新しい writer 型、既存の `writer`
Move/`Drop` 契約の変更は導入しない。

Request 18 と Request 55 は、1 つの retained root 配下の通常ファイルを扱う別の 3 操作を追加する。

```text
fs.open_beneath(root: str, relative: str) -> Result<reader, Error>
fs.open_beneath_single_link(root: str, relative: str) -> Result<reader, Error>
fs.create_exclusive_beneath(root: str, relative: str) -> Result<writer, Error>
```

これらは root、途中、末尾の symlink を拒否し、保持した directory descriptor から走査する。公開
directory-handle 型、metadata API、canonical path、sandbox、process-global root は追加しない。

Request 56 の最後の 2 操作は platform が選ぶ private staging root を 1 つ作成し、caller が既知の child をすべて
削除した後にだけ明示的に root を削除する。general directory creation や recursive cleanup とは別である。

```text
fs.create_private_temp_dir(prefix: str) -> Result<string, Error>
fs.remove_empty_dir(path: str) -> Result<(), Error>
```

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

### `open_beneath_single_link`

`open_beneath_single_link` は hard link を拒否する別 constructor である。`open_beneath` の path
grammar、retained-directory traversal、regular-file open、descriptor identity の再検査、error
mapping、nonblocking-clear の全手順を変更せず実行する。reader を構築する直前に、同じ opened
descriptor に対する既存の `fstat` 結果の `st_nlink` を検査する。`st_nlink == 1` のときだけ
成功し、0 または 1 より大きい場合は `Error.Invalid` とする。

返すのは同じ owned `reader` だけであり、descriptor や metadata は公開しない。失敗時は artifact
byte も reader も公開しない。path の検査、directory enumeration、final name の reopen は opened
descriptor identity を失うため代替実装にならない。その後の外部 link-count 変更や byte mutation
は防がず、predicate は最後の descriptor 観測時点の opened inode だけを保証する。

### `create_exclusive_beneath`

`create_exclusive_beneath` は同じ root/relative grammar と retained directory walk を使う。保持した final
parent で close-on-exec と final no-follow を伴う native exclusive create を 1 回実行する。占有済みの末尾は
すべて既存の `Error.Code` mapping による native EEXIST となり変更されない。成功時は既存の owned `writer`
を返し、partial write、flush、Drop、明示的 cleanup は `create_exclusive` と同一である。

parent、temporary name、transaction、rename、rollback、durability state は作らない。これは 1 ファイルの
retained-parent constructor であり、no-replace rename と C6f2 pair publication は引き続き Request 14 が所有する。

### `create_private_temp_dir`

`create_private_temp_dir` は 1..=64 byte の ASCII prefix を 1 つ受け取る。先頭 byte は英数字、残りは
英数字、`_`、`-` に限る。application path や environment variable は読まない。Linux は `/tmp` から、macOS は
platform-provided terminal slash を含む `confstr(_CS_DARWIN_USER_TEMP_DIR)` から開始する。その platform-owned
spelling だけを canonicalize してから strict validation と retained no-follow traversal を行うため、macOS の
`/var` compatibility symlink でも returned canonical absolute path を retained-root API が利用できる。

candidate leaf は prefix、`-`、128 fresh OS-CSPRNG bit を表す 32 桁 lowercase hex である。mode `0700` の
`mkdirat` 1 回で atomically claim し、umask は permission を狭めるだけで広げない。`EEXIST` のときだけ fresh
suffix を生成し、最大 128 回とする。occupant の再利用や parent 作成は行わない。owned absolute path を返し、
最初の create より前に result allocation を完了するため、failure は path も directory も残さない。

### `remove_empty_dir`

`remove_empty_dir` は empty、`.`、`..`、trailing slash component のない absolute strict path を 1 つ受け取る。
すべての ancestor を retain/revalidate し、final directory も symlink を follow せず同じ identity を
observe/open する。parent/final descriptor が live のまま `unlinkat(..., AT_REMOVEDIR)` を 1 回実行し、その
syscall が名指す empty directory だけを削除する。recursive delete は行わず、symlink、file、special entry、
nonempty directory は削除しない。

この native removal が final namespace/type/emptiness の linearization point である。Linux/macOS には portable
な open-directory-descriptor 指定 unlink がないため、final identity revalidation 後に empty directory へ
差し替えられた場合はその名前が削除され得る。constructor が返した path では platform root と `0700` が他 user
と accidental sharing を除外し、同じ OS identity の hostile process は capability 外である。shared non-sticky
parent の任意 path にそれ以上の保証はない。

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

align_rt_io_reader_open_beneath_single_link(
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

private-directory lifecycle は既存の A08/A04 shape を使う。

```text
align_rt_fs_create_private_temp_dir(
    prefix_ptr: ptr, prefix_len: i64, out_path: ptr,
) -> i32

align_rt_fs_remove_empty_dir(
    path_ptr: ptr, path_len: i64,
) -> i32
```

検査順は output slot、root 全体の validation/copy/grammar、relative 全体の validation/copy/grammar、
root traversal、relative-parent traversal、final operation とする。したがって不正な root grammar はすべての
relative-view error より先になる。recoverable failure では両 slot とも
null のままである。checked copy-size overflow は `Error.Invalid`、実際の OOM は terminal とする。完全な grammar
検証後にだけ private な full-path copy を NUL 区切りの component storage にし、caller の byte は変更しない。
走査中に live な directory descriptor は最大 2 つで、すべての path/component owner は call とともに終了する。

constructor の A08 output は既存の owned-string slot である。runtime は最初に slot を検査して zero にし、
`mkdirat` より前に正確な output を allocate する。removal の A04 input は absolute-only なので、constructor の
output を current-directory race なしで消費する。両操作は別 HIR/MIR kind と runtime key を持つ。

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
`open_beneath_single_link` は descriptor の link count が正確に 1 でない場合も `Error.Invalid` にする。
成功した reader/writer は既存の Move/Drop 経路をそのまま使う。同じ final の open/create pair に hidden exclusion
や snapshot はない。open が不在を観測すれば `NotFound`、create 後なら writer が live の間に新しい regular inode
を取得し得る。immutable input が必要な consumer はこの overlap を拒否しなければならない。

private-directory 操作も `Impure` で、prefix/path operand は借用である。created path は通常の owned `string`
なので move、return、branch/loop join、`?`、replacement、Drop は既存経路を使う。randomness と native
create/remove failure は固定 error mapping に従い、create collision の `EEXIST` だけを retry する。removal
failure は nonempty/mismatched entry を caller-owned cleanup のため観測可能なまま残す。random read の
interrupt は retry し、negative failure は固定 native-error mapping、zero progress は stale `errno` ではなく
`Invalid` とする。

## platform 境界と non-goal

v1 の adoption floor は Linux の controlled local ext4/tmpfs filesystem と macOS の controlled local APFS filesystem である。
runtime は filesystem type を分類しない。NFS、FUSE、overlay、その他 remote/unqualified filesystem、Windows、
portable emulation はこの capability の外にある。adoption fixture は検査前に制御された filesystem 環境を記録し、
unqualified 環境は `std.fs` が暗黙分類するのではなく consumer gate で除外する。

Request 14/18/55 は transaction、journal、recovery daemon、process-global lock、temporary-name generator、公開 directory-handle
capability、sandbox、replacement/exchange operation、durability guarantee は提供しない。Request 14 の path-only
操作は通常の parent resolution を維持し、Request 18 の 2 constructor だけが上記の明示的 no-symlink
regular-file 境界を提供する。

Request 55 は metadata surface や永続的な immutability guarantee を追加しない。hard link を意図的に
許可する caller のために `open_beneath` は変更しない。

Request 56 は environment-sensitive temp root、caller root、directory handle、recursive create/remove、hidden
cleanup、exit hook、quarantine registry、same-identity hostile-process defense を追加しない。general directory
creation/listing/type predicate は Request 53 のままである。

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

Request 55 の確定契約と implementation closure matrix は
[`34-fs-single-link-plan.md`](../34-fs-single-link-plan.md) にある。既存の A12 lowering と reader identity
を再利用し、別 operation/runtime key と既存 stat record に対する descriptor-only link-count
predicate を追加する。

Request 56 は A08 owned-string constructor と A04 unit-result remover を 1 つずつ追加し、別々の
HIR/MIR/runtime identity と完全な checked-HIR/whole/per-unit/export coverage を要求する。prefix、platform
root、randomness、allocation-before-mutation、no-follow removal、race boundary、ownership の正確な matrix は
[`36-fs-private-temp-plan.md`](../36-fs-private-temp-plan.md) にある。

Request 18 も同じ cross-stage 規則を使う。`ReaderOpenBeneath` と `CreateExclusiveBeneath` の別 node、完全な
visitor/validator/replay/MIR closure、正確な A12 runtime row と export parity、既存 handle Drop、
whole/per-unit/cache parity、Linux/macOS descriptor-walk owner、および align-llm の実 consumer
`c6d-request18-adoption` が必要である。完全な matrix は
[`29-fs-retained-root-plan.md`](../29-fs-retained-root-plan.md) にある。新契約は throughput ではなく safety と
ownership なので benchmark は不要である。

### 通常のディレクトリ作成と型の観測

`fs.create_dir(path: str) -> Result<(), Error>` はディレクトリを一つだけ作る。
Unix mode 0777 を現在の umask と OS の ACL 規則で制限する。umask は変更せず、
欠落した親は作らない。既存ディレクトリ・ファイル・symlink はいずれもエラー。

`fs.is_dir(path: str) -> Result<bool, Error>` は通常のパスと symlink を辿る。
metadata の取得が成功したディレクトリなら true、他の種類なら false。
欠落・権限不足・非ディレクトリの祖先・壊れたリンク・リンクループなどの失敗はエラー。
書き込み可能性・安定した識別子・将来のアクセスは保証しない。

両方とも `import std.fs` が必要な Impure 操作で、必須のパス引数を一つだけ借用し、
保持しない。string は str として自動借用する。空パス・NUL・不正な UTF-8 は
I/O 前に拒否する。相対パスは現在の cwd、dot/dot-dot・連続区切り・末尾区切りは
通常の OS 規則を使う。正規化・環境変数展開・再帰処理・キャッシュ・cwd 変更はない。
ネイティブ形式への変換は明示パス長に比例する割り当てを行いうる。OOM は既存の即時停止。
既存の errno 対応（既存エントリなら Code(EEXIST)）を使い、新しい所有型や Error variant
は追加しない。[正本と検証計画](../../43-ordinary-directory-plan.md) を参照。

### 保持したディレクトリと生のエントリ名

`import std.fs` は不透明な Move 所有型 `fs.directory` と `fs.dir_cursor` を提供する。
`fs.open_directory(path: str) -> Result<fs.directory, Error>` はシンボリックリンクを
たどらずディレクトリを保持する。`directory.cursor() -> Result<fs.dir_cursor, Error>`
は独立した列挙位置を持ち、ディレクトリの Drop 後も有効なカーソルを作る。排他的な
`cursor.next() -> Result<Option<fs.dir_entry>, Error>` は所有した生のベース名を返す。
EOF の None とネイティブ読み取り失敗の Error は終端状態として保存する。省く名前は
`.` と `..` だけで、ソート、UTF-8 フィルタ、暗黙のメタデータ取得は行わない。
`fs.dir_entry` は通常の Move レコード `{ name: array<u8> }` である。

メソッドは名前付きローカルのレシーバーを呼び出し中だけ借用し、入力を保持しない。
所有ローカルのカーソルは内部状態を進められる。借用ヘルパーで next を呼ぶには
`borrow mut` が必要である。一時値とフィールドパスを直接レシーバーにすることは
認めない。通常のレコード・直和・Option・Result の受け渡しは既存の Move 解放規則を
使う。不透明型を直接要素とするコレクション、タプル、box、グローバル、キャプチャ、
並列処理、FFI での受け渡しは対象外。Drop は所有資源を閉じ、エントリを削除しない。

ディレクトリの相対メソッドは `bytes` を受け取る。`metadata_at` は `fs.metadata`、
`open_dir` は `fs.directory`、`open_read` と `open_read_single_link` は `reader`、
`create_new` は `writer` を、それぞれ `Result<_, Error>` で返す。
`create_dir(path: bytes, mode: u32)`、`remove_file(path: bytes)`、
`remove_dir(path: bytes)` は `Result<(), Error>` を返す。作成は排他的かつ一段階で、
create_new は通常の umask/ACL の下で 0666 を要求する。公開前に失敗した場合は新しい
記述子を閉じるが、ロールバックの unlink は行わない。削除は非再帰的であり、
remove_file はシンボリックリンク自体を削除し、remove_dir は空ディレクトリだけを削除する。

ルート・相対パスとも NUL、空の構成要素、連続・末尾の区切り、`.`・`..` の構成要素を
I/O 前に拒否する。ルート単体の `.` と `/` は例外として有効で、先頭の `/` も許可する。
ルートは UTF-8 が必要。相対パスは絶対パスを許可せず、NUL 以外の任意の生バイトを
受け入れる。祖先をシンボリックリンクなしで検証し、reader はさらに通常ファイルの
種類と記述子の同一性を再検証する。single-link 版は公開前のリンク数が 1 であることも要求する。

ディレクトリ、reader、writer、file はそれぞれ記述子に対する
`metadata() -> Result<fs.metadata, Error>` と
`set_mode(mode: u32) -> Result<(), Error>` を提供する。mode は 0..07777 で、
無効ビットは I/O 前に拒否する。名前付き作成ではパスを mode より先に検証する。
metadata は位置を変えず、バッファを flush しない。通常の Copy レコードの宣言順は
`kind: fs.entry_kind`、`device: u64`、`inode: u64`、`links: u64`、`mode: u32`、
`size: i64`、`modified_seconds: i64`、`modified_nanoseconds: u32`、
`changed_seconds: i64`、`changed_nanoseconds: u32`。
Copy 直和 `fs.entry_kind` はペイロードなしの Regular、Directory、Symlink、Other を
この順で持つ。メタデータの変換は検証付きで、ナノ秒は 1,000,000,000 未満、changed は
作成日時ではなく ctime を表す。全操作は Impure で既存の Error モデルを使う。
観測結果はスナップショット、書き込み可能性、ソースの不変性、同一性条件付き削除を
保証しない。検証順序、所有権、プラットフォーム・競合の限界、ABI の厳密な定義は
[生バイトツリーの台帳](../../45-retained-byte-tree-plan.md) にある。
