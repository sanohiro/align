このディレクトリには、ファーストパーティー `pkg` ライブラリの日本語設計ミラーを
収める。英語版が規範であり、この文書は
[`../db.md`](../db.md) のAPI、所有権、実装境界、ロードマップを同期して説明する。

# pkg — db

> 🌐 [English](../db.md) · **日本語**
>
> 公開呼び出しは `pkg.db.*`、`pkg.db.sqlite.*`、`pkg.db.postgres.*` と完全修飾する。
> 例中の `db.*`、`sqlite.*`、`postgres.*` は読みやすさのための略記である。

## 状態

**設計確定 — SQL NativeかつQuery中心のデータベースパッケージ。**

初期ドライバはSQLiteとPostgreSQLの両方である。

この文書の意味論とAPI形状が契約である。ただし、一般的なライブラリ境界の前提機能
[`../../17-library-boundary-prerequisites.md`](../../17-library-boundary-prerequisites.md)
が実装されるまでDBドライバ実装を開始してはならない。これらはDB専用builtinでも、
後から直せばよいcleanupでもない。

## 1. 結論

AlignのDBサポートはSQL NativeかつQuery中心とする。モデル中心、ORM中心にはしない。

> SQLがリレーショナル処理を定義する。名前付きQueryがbindするParamsとDBから返る
> 正確な物理Rowを定義する。通常のAlignコードが、その1本のrow streamを1回の
> 可視なpassでアプリケーションOutputへ整形できる。

主単位は名前付きQuery moduleである。

```text
Query module
  SQL source          可視なアプリケーションSQL statementを正確に1つ
  Params              statementへbindする値
  Row                 DBが返す正確でflatな列
  Output              任意の論理アプリケーション結果
  query()             静的で型付きのQuery descriptor
  run()/shape()       任意の1-pass整形。追加SQLは禁止
```

次を必須とする。

- SQLはread/writeのsource of truthである。
- migrationはSQL fileのまま扱う。
- SQLiteとPostgreSQLはどちらも初期ドライバである。
- 共通SQL、型、optionだけなら同一の共通Align interfaceを使う。
- DB固有機能は接続、Query、prepare、実行、transaction、metadata、planの各scopeで
  明示的に使える。
- 通常の `alignc build` / `alignc check` は完全にofflineである。
- DBが生成したchecked metadataは明示的に作り、CIで必須化できる。
- 通常のstructはデータ形状であり、Model/Entityではない。
- JOIN、CTE、grouping、window、native operator、DB固有最適化はSQLに見える。
- 1つの名前付きQueryは見えているstatementを正確に1回だけ実行する。
- Output整形はrelationship queryや隠れたSELECTを発行しない。

transactionとmaster projection、集約report、一対多readを通常ケースとして扱う。
単一table形状のloadは単純な特殊ケースであり、概念の中心ではない。

## 2. 設計原則

### 2.1 SQLはSQLのまま

LINQ風DSL、Active Record、ORM relationship言語、汎用SQL AST builderは導入しない。

```sql
SELECT
    c.id AS customer_id,
    c.name AS customer_name,
    COUNT(o.id) AS order_count,
    SUM(o.amount) AS total_amount
FROM customers AS c
JOIN orders AS o ON o.customer_id = c.id
WHERE o.created_at >= :from_date
GROUP BY c.id, c.name
HAVING COUNT(o.id) >= :minimum_orders
ORDER BY total_amount DESC
```

pkg.dbが追加するのはtyped params、typed rows、予測可能なownership、明示的実行、
migration、metadataである。SQLの前に第2のquery言語を置かない。

### 2.2 Queryが主で、structはデータ形状

特別な `model` 宣言はない。structはParams、Row、domain value、Query固有projection、
rowsから組み立てるcompound Outputのどれにも使える。table、primary key、relationship、
INSERT/UPDATE、migration、lazy loadを暗示しない。

```align
pub User {
  id: i64,
  name: str,
  email: Option<str>,
}
```

同じUserを複数Queryで使ってよい。JOIN/reportが別のRow/Outputを持ってもよい。
異なるprojectionに型が増えることは、すべてを1つのEntityに見せかけるより望ましい。

### 2.3 隠さないもの

```text
SQL statement
SQL実行回数
parameter binding
allocation先
error境界
transaction境界
materialization mode
native DB依存
result shaping pass
```

field accessやshaperがSQLを実行してはならない。driverがretry、pagination、追加SELECT、
複数round tripへの分割、native optionの無視を隠してはならない。

### 2.4 機構は隠せるがcost classは隠さない

libsqlite3/libpq、binary decode、buffer reuseなどの内部機構は選択できる。ただし、
`one`/`maybe_one` がdecodeするのは最大2 delivered rowsだが、transport/bufferingの共通上限
ではない。SQLiteは最大2回stepする。初期PostgreSQLはlibpq `BufferedFull` なので全resultを
transport/bufferした後に最大2行をdecodeする。`rows` はone-pass consumptionでありnetwork
streaming保証ではない。test/bench observationは `Step` / `BufferedFull` / `SingleRow` /
`PortalBatch` とtransported/buffered/decoded countを固定する。D13のsingle-row/portalは明示
optionで選ぶ。optionなしは`BufferedFull`、`SingleRow`と`PortalBatch(n)`はlibpqの
single-row/chunked-row modeへexactly mapし、適用不能ならerrorで黙ってdowngradeしない。

### 2.5 薄い共通層と明示的native extension

共通層はconnection/execution handle、typed Query/command、binding、prepared statement、
execution mode、transaction、error、common metadataを持つ。SQLite/PostgreSQLを
least-common-denominatorへ削らない。

native controlを提供するscope:

```text
connection
Query definition
prepare
execution
transaction
metadata
EXPLAIN / query plan
```

### 2.6 通常buildはoffline

通常build/check/module checkからlive DBやnetworkへ接続してはならない。権威あるSQL
parse/type resolutionは明示的prepare commandでreal engineへ問い合わせ、その結果を
checked metadataとして保存する。compilerがSQLite/PostgreSQL SQL全体を再実装しない。

## 3. Non-goals

v1に含めないもの:

- Active Record、Hibernate風persistence、relationship宣言、lazy loading;
- identity map、change tracking、自動join、暗黙N+1;
- structからschema/migrationを生成する機能;
- runtime reflectionによるRow mapping;
- 汎用Query-builder DSLや自動dialect rewrite;
- stored procedure abstraction、distributed transaction;
- transparent retry、transparent pooling（D13は§23の明示的 `pkg.db.pool` を提供する）;
- 「最適」なrelational fetch strategyの自動選択。

D13 poolは§23で確定する明示的 `pkg.db.pool` moduleである。Query意味論を変えず、
acquisition、exhaustion、physical-connection costを隠さない。

## 4. Package layout

### 4.1 初期public module

```text
pkg.db
pkg.db.sqlite
pkg.db.postgres
```

D13はsame package subtreeへ4つ目のpublic moduleを追加する:

```text
pkg.db.pool
```

initial listは独立versionの3 packageではない。Alignのpackage規則に従う、1つのvendorable
`pkg/db` subtree内の3つのpublic module境界であり、D13はsame subtreeへ4つ目を追加する。
`pkg.db.pool` はpublic driver constructorをimportするがroot/internalへupward edgeを作らない。
root/common moduleはpublic driver moduleをimportせず、driver moduleがcommon/internalへ下向きに
呼ぶ。この向きでmodule graphをacyclicに保つ。

将来候補:

```text
pkg.db.odbc
pkg.db.mysql
pkg.db.duckdb
```

common moduleが意味論契約とclosed internal resource dispatchを所有する。driver
submoduleがconnection construction、native type/option、authoritative prepare/describe、
native metadataを所有する。外部driver registration ABIを設計するまでは、third-party
driverは別package rootを使う。

### 4.2 必須の一般言語基盤

DB driverより先に次を実装する。

- structured owned errorとMove Outputを扱うrecursive tagged Move payload;
- `borrow` / `borrow mut` parameterとinterface-visible return borrow summary;
- package-defined opaque/dependent Move resource、exactly-once Drop、`resource_ref<R>`、
  owner-tied native view;
- `arena name {}` とscope-bound `region`;
- deterministic static source inputとQuery/command artifact;
- region-backed `array_builder<PlainStruct>`。

同じ機構は `std.http`、`std.net`、`std.process` などでも使える。`pkg.db` は
`std.http` に依存しない。compilerのownership/borrow safetyがpackage名で分岐しては
ならない。

不要な言語機能:

- `database` keyword、annotation/decorator、reflection;
- user-defined trait、row polymorphism、structural record;
- operator overloading、macro、第2のcompile-time言語。

`db.query_file([])` などのstatic Query constructorだけは、expected Params/Row、
tagged file/inline source identity、source SQL hash、build dependencyを必要とするため
compiler-knownである。これは一般metaprogrammingではなく、限定されたstatic-data機能である。

### 4.3 共通の具体型

```text
db.conn
db.tx
db.exec
db.query<P, R>
db.command<P>
db.stmt<P, R>
db.rows<R>
db.batch<R>
db.exec_result
db.Driver
db.row
db.value
```

- `db.conn`: SQLite/PostgreSQL connectionを1つ所有するopaque Move resource。
- `db.tx`: transactionへmoveされたconnectionを所有するopaque Move resource。
  activeなままDropするとfail-safe rollbackしcommitしない。direct-driver connectionはcloseする。
  D13はpool-origin connectionをpool ledgerのexact rollback-and-native-idle proof後だけreturnし、
  それ以外はcloseしてslotをretireする。
- `db.exec`: conn/txのどちらからも生成する短命なborrowed execution view。
- `db.query<P,R>`: SQL identity、Params/Row contract、driver restriction、hash、
  static optionを持つCopy static descriptor。
- `db.command<P>`: Rowを返さないdescriptor。`RETURNING` はQueryである。
- `db.stmt<P,R>`: prepareしたconnectionへのdependencyを持つMove statement。
- `db.rows<R>`: 1実行だけのone-pass typed stream。native bufferが必要な間、
  statement/connectionへdependencyを持つ。
- `db.batch<R>`: 1つのnonempty bounded column batchを独立所有するMove resource。publication後は
  rows dependencyを持たず、row/eligible SoA viewがbatch generationをborrowする。
- `db.exec_result`: §6.1のallocation-freeなCopy affected-row record。
- `db.Driver { SQLite, PostgreSQL }`: error/metadata/delivery observation/D14 dynamic SQL
  restrictionで使うexact public identity。`Any` variantはない。
- `db.row` / `db.value`: 明示的dynamic escape hatch。

common rootがresource typeを宣言し、rawだけを受ける `pub` Drop hookを
`pkg.db.internal.resource` に置く。hookは通常のfnと `unsafe {}` bodyで書き、
resource宣言からproducer-owned hidden support thunkを生成する。consumer cleanupは
internal moduleをimportせずそのthunkへlinkする。resource表現intrinsicは宣言moduleの
descendant subtreeだけが使える。public driver descendantは `pkg.db` をimportし、raw-only internal FFI
helperの結果を検査して、expected `db.conn` として `resource.from_raw` を直接呼ぶ。
internal Drop-hook moduleは `pkg.db` をimportしない。

```text
pkg.db                    -> pkg.db.internal.resource
pkg.db.sqlite/postgres    -> pkg.db + pkg.db.internal
pkg.db.internal.*         -X-> pkg.db
```

したがってpublic raw constructorもmodule cycleもない。public trait/trait objectや外部driver
resource ABIもない。

```align
pub fn exec_conn(borrow c: db.conn) -> db.exec
pub fn exec_tx(borrow t: db.tx) -> db.exec
```

`db.exec` は `resource_ref<db.conn>` / `resource_ref<db.tx>` のCopy sumである。
source generationを保持するため、connのmove/dropやtxのcommit/rollbackで派生viewは
無効になる。

### 4.4 実装境界

L1a〜L7後に通常のファーストパーティーAlign package codeで実装するもの:

- public handle/descriptor/option/error/metadata形状;
- common/SQLite/PostgreSQL module API;
- private sqlite3/libpq FFI宣言を包むsafe wrapper。supported libpqのOpenSSL依存closureは
  `link("ssl")` と `link("crypto")` で明示する;
- connection/transaction/statement/rows lifecycle;
- bind/step/result/metadata driver operation;
- Query-local `run`、Pure shaping step、builder、Output;
- SQL migrationと明示的tool orchestration。

compiler/frontendが所有するもの:

- L1a〜L7のlanguage/ownership/region/generics/interface/MIR;
- recognized static Query/command constructorと入力追跡;
- expected Params/Rowによるdescriptor check;
- dialect-aware placeholder occurrence/source mapとstatement screening;
- versioned artifactとinterface/implementation hash;
- direct field-offset binderとordinal decoder thunk生成;
- `.sql` spanへ対応するdiagnostic。

runtimeはregion-builder chunk/compact、checked owner-tied native view/UTF-8 validation、
既存arena/allocation helperだけを提供する。DB Query意味論、SQL parser、reflection、
DB handle typeをruntimeへ置かない。基本driver callはgenerated package codeから
libsqlite3/libpqへ直接行う。common PostgreSQL dispatchもlibssl/libcryptoを明示して
static ELF linkでlibpqのTLS dependency closureを保持する。`pq`を含む場合、driverは
unitごとのfirst-seen discoveryを保持したまま、最終linkで`pq`、`ssl`、`crypto`、supportedな
`zstd`/`z`のclosure tailを1回追加する。suffix library自身がnative referenceを導入する
場合もこのtailで解決できる。

`align_driver` は明示tool (`alignc db prepare`、migrate/status/check)、deterministic
artifact I/O、tool-only schema setupを所有する。

## 5. 名前付きQuery module

### 5.1 file convention

```text
user_with_groups.align
user_with_groups.sql
```

```align
pub Params {
  user_id: i64,
}

pub Row {
  user_id: i64,
  user_name: str,
  group_id: Option<i64>,
  group_name: Option<str>,
}

pub fn query() -> db.query<Params, Row> = db.query_file([])
```

path-free `query_file([])` は定義moduleと同basenameの `.sql` を解決する。Query module
自体がapplication identityであり、callerはfilesystem pathを渡さない。

### 5.2 明示path override

```align
pub fn query() -> db.query<Params, Row> =
  db.query_file("legacy/user_lookup.sql", [])
```

pathはcompile-time literalで、定義module directoryからのrelative pathである。
absolute path、lexical `..`、project/package root外へのsymlink escapeを拒否する。
UTF-8 exact source bytesをSourceMapへ登録し、newline normalizationなしでhashする。
runtimeは選択driver用に決定的に生成したwire entryを送る。

### 5.3 inline SQL

```align
pub fn query() -> db.query<Params, User> =
  db.query("SELECT id, name, email FROM users WHERE id = :id", [])
```

static expressionだけを許可し、runtime `string` からtyped Queryを作らない。複雑なSQLは
review/tool/EXPLAIN/diffしやすい `.sql` を推奨する。

inline SQLは `SqlSourceIdentity::Inline { query_id }` を使い、fake `.sql` pathを作らない。
`source_sql` はAlign escape decode後のexact UTF-8 valueで、artifactはdecoded byteから
defining `.align` literal spanへのmapを持つ。decoded bytes/hashとtagged identityが
producer/artifact identityへ入り、diagnosticはliteralへ戻る。

### 5.4 descriptor、artifact、incremental identity

`db.query<P,R>` はCopyのcompiler-known descriptorであり、runtime reflection object
ではない。

```text
StaticQueryArtifact       producer implementation
  query id
  structural Params/Row contract/fingerprint
  source identity: File(logical path) | Inline(query_id)
  exact source SQL bytes/hash
  driver別wire SQL bytes/hashとsource map
  driver restriction
  canonical static options
  named-parameter occurrence/source maps
  driver別binding plan/parameter retention class
  driver別checked metadata policy/state/reference/digest
  declared QueryMeta planとdriver別checked evidence
  generated binder/decoder bodies
  generated QueryMeta materialization plan

IStaticQuery              public interface contract
  fully qualified query id
  Params/Row identities and public layout contracts
  driver restriction
  public static options
```

static constructorは、named・zero-argument・non-generic descriptor functionの
single-expression body全体として正確に1回だけ書ける。conditional、multiple、block、
nested、helper wrapper、通常expressionでの使用はcompile errorである。Query IDは
fully-qualified module path + descriptor function nameとし、同じmodule内の2 descriptorは
別artifact/thunk slotを持つ。private descriptorはmodule内だけ、`pub` descriptorはinterface
外へ公開できる。

SQL-only/private metadata変更はproducer object/artifactをinvalidateするが、public contractが
同じconsumerは再type-checkしない。Params/Row/restriction/public option/required metadata
policy変更はconsumerをinvalidateする。runtime descriptorはimmutable dataとdirect
function pointer/thunkだけを持つ。
producer-owned QueryMeta planはD12まではinert descriptor dataである。D12が最初のnative
metadata consumerと同時にexact materialization thunk ABIとexecution-header versionを導入し、
Q2はdormant function-pointer slotを予約しない。将来のthunkもdecoder codeをinspectしたり
`.align-db`を開いたりせずcaller regionへmaterializeする。

### 5.5 1 descriptor = 1 statement

1 Queryはapplication SQL statementを正確に1つ所有する。semicolon、comment、stringを
理解するdialect-aware scannerでmulti-statementを拒否する。driver preparationでも
tail SQLを検査する。trigger内部のDB動作はschemaの明示的意味論であり、packageが
隠した追加Queryとは区別する。

### 5.6 Params

Paramsは名前付きの通常structで、public field名がSQL parameter contractになる。
SQL内の名前付きparameterをscannerで抽出し、missing/unused/duplicate ambiguityを
diagnosticにする。

SQLiteはnative named token/indexへbindする。PostgreSQLはsource上の初出順に
`:name` を `$1`、`$2` ...へrewriteし、同名再出現は同じordinalを再利用する。
string literal、quoted identifier、line/block comment、dollar quote、`::` castを
parameterと誤認しない。named/positionalの混在は拒否する。

source SQLとwire SQLは別identityである。`source_sql` はreview対象のexact file/inline bytes
とstatic-input hashを持つ。SQLite wireはsourceと同一。PostgreSQL wireは認識済みparameter
token spanだけを `$n` へ置換し、それ以外のbyteを保存する。artifactは両hash、
rewrite-format version、source-to-wire span mapを持つ。metadata keyはdriver/source hash/
wire hash/rewrite version/static optionを含み、runtimeは選択したwire bytesを正確に送り、
engine位置を可能な範囲でsource spanへ戻す。
NUL-terminated native entry point用storageはrecorded pointer/length/hash domain外にsentinelを
1 byte追加する。これはtransport storageでありsource/wire SQLの一部ではない。

SQL source byteにU+0000を許可しない。static Query/commandはartifact生成前にexact spanへ
diagnosticし、dynamic SQL/migration toolは最初のnative call前に拒否する。SQLiteとlibpqで
C string/length boundaryが異なるため、NULを受け入れてstatement count/source identity/
送信SQLをdriver間で不一致にしない。

#### 5.6.1 bind storageの保持

common execution contractがparameter storageをborrowするのはoperationがreturnするまでだけ。
`rows`/`rows_stmt` もstream resourceを返す前にbindを完了するか、execution-owned storageへ
全valueを保持する。返された `db.rows<R>` はParams由来provenanceを持たず、statement/
connection/native result ownerだけへdependentである。callerはreturn後、最初の `next`
より前でも元text/blob ownerをdrop/reassign/mutateできる。

v1 SQLiteはscalarをby-valueで渡し、`str`/`slice<u8>` とowned `string`/`array<u8>` のbytesを
`SQLITE_TRANSIENT` semanticsでbindする。SQLiteはreturn前に必要なcopyを所有する。
prepared statementはreset/rebind/finalizeまでnative copyを保持し、live rows dependencyが
resetを禁止する。partial-bind/native errorもtemporaryを全て解放し、moved Params ownerを
exactly onceでconsume/dropする。

初期PostgreSQL `BufferedFull` pathは同期libpq callのreturn前にparameter transmissionを完了
する。D13 `SingleRow`/`PortalBatch`は`PQgetResult`がnullを返してprotocol synchronizationが
完了するまでexecution-owned contextに全parameter bytesを保持する。将来のpipeline/async pathも
同じ義務を持つ。これはper-execution bind copyでありper-row copyではない。
v1 PostgreSQL Text binderはNUL-terminated execution-owned copyを作り、text/varchar/nameの
embedded U+0000をSQL送信前に `db.Error.Encode` とする。Text formatのbyteaはPostgreSQL
`\x` formへinput byteごとにlowercase hex 2桁でencodeし、raw byteをlibpq Text parameterへ
渡さない。Binary formatのbyteaだけがraw byteとexplicit lengthを使う。
test/benchmarkはtext/blob copy bytesとallocationをrow decodeと分けて報告する。zero-copy
bindは明示driver-qualified surface、全source ownerへ結ぶreturn provenance、独立measurement
を必要とし、黙ってborrowed bindへ切り替えない。generated driver binder planは各fieldの
retention classを記録し、Query/execution inspectionは `BindValue` / `BindCopy` を表示する。

### 5.7 Row

RowはDBが返す正確でflatな列契約である。compilerがordinal decoder thunkを生成し、
1行ごとのname lookup/reflectionは行わない。column count、type/storage class、
NULL/non-NULLをruntimeでも検証してからRowを構築する。

v1のstatic Rowは構造的に `RegionPlain` とする。scalar、`str`/`slice<u8>` view、
それらから再帰的に作るOption/fixed array/plain fieldだけを許可し、独立owned `string`、
dynamic `array`、resource、raw、function、builder fieldを拒否する。owned
`string`/`array<u8>` はParams/Outputでは使えるが、v1 generated Rowの別storage formには
しない。

Rowにnested relationshipやlazy collectionを入れない。SQL aliasとRow fieldの対応は
checked metadataで強化する。Declared modeでもexact ordinal契約をruntimeで守る。

### 5.8 Output

Outputは任意で、通常のAlign struct/array/Option/Resultからなる。Rowと同じ型である必要は
ない。DBアクセスを受け取らない通常コードが1-passで組み立てる。

## 6. Execution surface

### 6.1 command

Rowを返さないstatement:

```align
pub Params {
  id: i64,
  name: str,
}

pub fn command() -> db.command<Params> = db.command_file([])
```

commandはQueryと同じwhole-body item制限、static source identity、source/wire hash、
parameter occurrence/source map、driver別bind/retention plan、checked-policy map、
interface/implementation split、cache invalidationを持つ。`StaticCommandArtifact` /
`CommandStatic` が省くのはRow contract、result-column metadata、decode thunkだけである。
Params binderはgeneratedで、reflection/runtime field-name lookupへfallbackしない。

`execute` はallocation-freeなCopy record
`db.exec_result { rows_affected: Option<i64> }` を返す。engineがnon-negativeかつi64に収まる
affected-row countを報告するときだけ `Some(n)`、それ以外は `None` とし、native result解放前に
変換する。native textual command tag/statusはowner/destinationなしでviewを返せないため初期
common resultに含めない。後続driver-qualified APIはowned storageまたは明示regionとallocationを
surfaceへ出す。`RETURNING` を含むstatementはQueryとして定義する。

### 6.2 result mode

```text
execute    Rowをdecodeしない
one        正確に1行。0/2+はerror
maybe_one  0または1行。2+はerror
all        supplied regionへ全行materialize
rows       one-pass stream
next_batch bounded owned column batch
```

`one`/`maybe_one` はcardinality判定に最大2 delivered rowsをdecodeするが、§2.4の
driver-specific transport/buffering costは別である。`one`/`maybe_one`/`all` のexact package
definitionは `P, R: RegionPlain` のgeneric functionである。`all` はregion builderのchunk
growthと1回のcompact passを使う。`RegionPlain` は
L7のclosed builtin structural boundであり、public/user-defined trait hierarchyではない。
v1 static Rowはすべて満たす。
bounded `next_batch` はD13の追加APIである。exact surfaceは
`next_batch<R>(borrow mut rows<R>, i64) -> Result<Option<batch<R>>, Error>`、
`batch_len<R>(borrow batch<R>) -> Result<i64, Error>`、
`batch_row<R>(borrow batch<R>, i64) -> Result<Option<R>, Error>`、
`batch_soa<R: SoaPlain>(borrow batch<R>) -> Result<soa<R>, Error>` である。batchはcolumn/child storageを
独立所有し、row/SoA viewはそのresource generationをborrowする。region、追加Query execution、
hidden pagination、optionless overloadはない。D1〜D12の初期common operationには含めない。
名前からmaterialize/streamが分からない
convenience APIは作らない。

### 6.3 Query-local run helper

```align
pub fn run(
  exec: db.exec,
  params: Params,
  out: region,
) -> Result<Option<User>, db.Error> {
  return db.maybe_one(exec, query(), params, out, [])
}
```

compound OutputではImpure `run` が1本のrowsを作り、可視な `loop` を1つ持つ。
genericな `db.fold` は提供しない。L7は通常package functionに必要なnested generic typeだけを
提供し、higher-order DB execution modelは追加しない。

### 6.4 prepared statement

```align
mut stmt := db.prepare(exec, query(), [])?
rows := db.rows_stmt(stmt, params, [])?
```

stmtはprepare元connectionへdependentである。rowsはstmtのfresh generationへdependentで、
rows Dropまでstmtを再利用/reset/finalizeできない。global implicit statement cacheはない。
PostgreSQL D13 native deliveryは同じdependency/cleanup ruleを持つ
`postgres.rows_stmt_native(borrow mut stmt, params, common_options, native_options)`だけを追加する。
これは`postgres.rows_native`のprepared counterpartで、common `db.rows_stmt`やstatement cacheを
変更しない。

### 6.5 connection/transaction reuse

Queryは `db.exec` を受けるため、conn/txの両方で同じsurfaceを使う。conn/txを抽象化する
public traitは導入しない。

## 7. Compound Query pattern

### 7.1 many-to-one/master lookup

JOINで必要なmaster値を同じRowへ投影し、1回のstatementでdecode/shapeする。master tableを
暗黙に追加SELECTしない。

### 7.2 一対多: 1 parent

SQLはparent列とchild列をflatに返す。nullable childの全列がNULLならchildなしと解釈する。
partial NULLはcontract errorにする。

```align
State {
  seen_parent: bool,
  parent_id: i64,
  parent_name: str,
}

pub fn step(
  borrow mut state: State,
  borrow mut groups: array_builder<Group>,
  row: Row,
  out: region,
) -> Result<(), db.Error> {
  // parent identityを検証し、childがあればclone_in(out)してpushする
  return Ok(())
}
```

`step` はPureでDB handleを受けない。stateとbuilderへのmutationは明示的 `borrow mut`
inputだけにrootedする。builderはstruct fieldへ入れず、runの独立した
`mut groups := array_builder(out)` localとして保持する。runはCopy/view stateの検証を先に
終え、`groups_out := groups.build()` として、Output/Resultを同じrun内で直接構築する。
arena-owned arrayを通常functionへby-valueで渡さない。runのrows loopがSQLを1回だけ実行する。

### 7.3 一対多: 複数parent

SQLのORDER BY contractによりparent key単位でgroupを閉じる。canonicalな多数parent表現は
並行する `users: array<User>`、`groups: array<Group>`、`group_offsets: array<i64>` の3本を
独立したregion builderで作るsegmented表現である。各parentを閉じるときuserと次の
group offsetだけをpushし、childは1本のgroups builderへ連続してpushする。parentごとの
`array<Group>` を持つOutputをouter builderへby-valueで渡さない。hash groupingを選ぶ場合は
そのallocation/costを通常Alignコードに見せる。

### 7.4 複数child collection

1つのJOINがCartesian multiplicationを起こす場合、SQL側で事前集約/CTE/native aggregation
を使うか、visible dedup stateをshaperへ持たせる。packageが隠れたSELECTへ分割しない。

### 7.5 native nested aggregation

PostgreSQL JSON/array aggregation、SQLite JSON1などはdriver-qualified Queryとして使える。
native type/decoderが必要ならQueryがdriver-pinnedになる。common layerがdialect変換しない。

### 7.6 複数の明示Query

applicationが明示的に複数Queryを呼ぶことは禁止しない。transaction境界と各実行がsourceに
見え、1つの名前付きQueryを複数statementへ偽装しないことが条件である。

## 8. Shaping contract

### 8.1 Pure exclusive-state transition

canonical shaperは次の分割を使う。

```text
run   Impure: Queryを正確に1回実行し、rows loopを所有
step  Pure: borrow mut State + Row + region、DB handleなし
```

explicit `borrow mut` parameterにrootedするmutationだけならPureである。captured mutation、
unsafe、FFI、I/OはImpureのまま。したがってstepから追加SQLを構造的に実行できない。

### 8.2 既定は1-pass

行を読みながらOutputを構築する。SQLがorderingを保証しないのにgroupingを仮定しない。
必要なsort/hashはsourceに明示する。

### 8.3 ordering contract

shaperがorderingに依存するならSQL `ORDER BY` とQuery contractへ記録する。

### 8.4 duplicate

JOIN multiplicity、重複child、dedup policyを明示する。自動identity mapはない。

### 8.5 Outputは通常のAlign data

OutputへDB entity意味論、dirty tracking、lazy handleを付加しない。

## 9. Memoryとownership

### 9.1 GC/per-row reflectionなし

descriptorごとのgenerated binder/decoderを使う。hot row pathでfield名検索、hash map、
schema reflection、不要なallocationを行わない。

### 9.2 Row view

SQLite text/blob pointerは次の `step/reset/finalize` まで、libpqはresult/row modeの契約に
従う。`next(borrow mut rows)` は前row generationを終了し、返すRow viewをfresh current-row
generationへ結ぶ。旧viewをnext後に使うことをcompile errorにする。

### 9.3 Materialized result

`one`、`maybe_one`、`all` とarrayを作るshaperはcallerの `region` を受ける。

```align
arena out {
  rows := db.all(exec, query(), params, out, [])?
  use(rows)
}
```

短命Row viewを保持するには `clone_in(out)` をsourceに書く。

### 9.4 builder前提

`array_builder<T>(out)` は`RegionPlain`だけを受け、region内でchunk成長し、最後に1回だけ
compactする。hidden heapは使わない。heap builderのzero-copy freezeは別の既存契約として
維持する。

### 9.5 batch/SoA

D13はdatabase protocol rowをtyped column bufferへ直接decodeし、eligibleな`soa<Row>`へ
projectする。intermediate `array<Row>`、AoS、transposeは作らない。nullable columnは
value/header columnとvalidity bitmap、text/blobはbatch-owned segmented child bufferを使う。
exact operation、ownership、validation order、generated-plan ABI、cleanup matrixは§23 A1 ledger
をsource of truthとする。`SoaPlain`でないvalid static Rowも`batch_row`で利用でき、silent
downgradeしない。

## 10. SQL type

### 10.1 common logical type

初期共通型はbool、符号付き整数、float、UTF-8 text、bytes、nullable `Option<T>` を中心に
する。Paramsではtext/bytesのowned formも使えるが、Rowは `str` / `slice<u8>` viewを使う。
decimal、UUID、temporal、JSON、array/range/domainは明示型mappingを設計してから追加する。
曖昧なimplicit conversionを避ける。

### 10.2 SQLite

SQLite storage classとdeclared affinity/STRICT情報を区別する。runtime値の
INTEGER/REAL/TEXT/BLOB/NULLを必ず検証し、lossy conversionを暗黙に行わない。
TEXT/BLOBはParamsで `str|string` / `slice<u8>|array<u8>`、Rowで `str` /
`slice<u8>` にmappingする。

### 10.3 PostgreSQL

checked metadataはtype name、OID evidence、format、nullability/origin evidenceを持つ。
text formatで開始してもbinary pathをsurfaceから閉ざさない。
初期releaseのexact common mappingは
`int2/int4/int8 -> i16/i32/i64`、`float4/float8 -> f32/f64`、`bool -> bool`、
`text/varchar/name -> Params str|string / Row str`、`bytea -> Params slice<u8>|array<u8> /
Row slice<u8>`、`NULL -> Option<T>` である。date/time/timestamp、numeric、UUID、
JSON/JSONB、array/range/domain/user-defined typeはこのsetへ黙ってcoerceしない。D12〜D14の
consumer decisionでexact logical/native representationを確定し、使用前に明示mappingを追加する。

### 10.4 NULL

nullable parameter/resultは `Option<T>`。NULLをzero/emptyへ変換しない。nullable resultを
non-Optionへdecodeするとcheck/runtime errorにする。non-nullをOptionへ入れるpolicyも
明示的かつone-wayでなければならず、v1はexact matchを優先する。

## 11. SQLite driver

### 11.1 connection

```align
conn := sqlite.connect("app.db", [
  sqlite.ConnectOption.OpenReadWrite,
  sqlite.ConnectOption.Create,
  sqlite.ConnectOption.BusyTimeoutNs(5_000_000_000),
  sqlite.ConnectOption.Pragma("journal_mode", "WAL"),
  sqlite.ConnectOption.Pragma("foreign_keys", "ON"),
])?
```

native designはopen flag、URI mode、busy timeout/handler policy、安全に表現できる任意の
明示PRAGMA、shared/private cache、thread/open mode、extension loading policyの明示extension
pointを保持する。初期releaseのfinite subsetは§11.2で固定し、callback busy handlerと
extension loadingはproved callback後でv1 constructorではない。extension loadingはdisabled。
要求optionを黙って無視しない。

### 11.2 Query/prepare/execute option

```text
sqlite.QueryOption.RequireVersionAtLeast(major, minor, patch)
sqlite.CommandOption.RequireVersionAtLeast(major, minor, patch)
sqlite.PrepareOption.Persistent
sqlite.PrepareOption.Normalize
sqlite.ExecuteOption.BusyTimeoutNs(ns)
sqlite.TxOption.Deferred | Immediate | Exclusive
sqlite.MetaOption.IncludeInternalObjects | IncludeHiddenColumns
sqlite.ExplainOption.QueryPlan | Bytecode
```

`major`、`minor`、`patch`は`u32`である。

初期releaseのconnection sumも次で固定する。

```text
OpenReadOnly | OpenReadWrite | Create | Uri
PrivateCache | SharedCache | NoMutex | FullMutex
BusyTimeoutNs(ns) | Pragma(name, value)
```

`[]` はcreate/URI/cache/mutex/busy/PRAGMA指定なしのread-write openと、transactionの
Deferred、EXPLAINのQueryPlanを意味する。read-onlyとread-write/create、privateとshared、
no-mutexとfull-mutex、重複busy timeout/PRAGMA名はconflict。durationは正でなければならない。
DB pathはU+0000を含めない。Pragma nameはASCII `[A-Za-z_][A-Za-z0-9_]*`、valueはUTF-8かつ
U+0000なしで、raw SQL pasteではなくdeterministic SQLite string-literal quotingを使う。
invalid/unsupported PRAGMAをopen/setup前errorまたはnative errorにし、ignoreしない。
`RequireVersionAtLeast` はstatic artifact/public contractへ入りSQLiteへpinする。
Persistent/Normalizeはprepareだけ、execution BusyTimeoutはそのexecution中だけ適用して
restoreする。`execute`/`one`/`maybe_one`/`all` はreturn前、`rows`/`rows_stmt` はrows resourceが
prior package-tracked valueを保持し、exhaustion/terminal step error/Drop時にconnection/
statement dependency解放前にrestoreする。restore失敗はunknown policyのconnectionを返さず
poison/closeする。SQLite v1はnative connectionごとにpackage-tracked active-execution
leaseを1つだけ持つ。synchronous executionはreturnまで、`rows`/`rows_stmt`は
exhaustion/error/Dropまでleaseを保持する。lease ownerの`next`/Drop以外に同じnative
connectionへ到達するoperationは、bind/timeout変更/native call前に
`Unsupported(ContractError { item: "sqlite.connection.active_execution",
message: "SQLite connection already has an active execution" })`でrejectする。attempt対象が
Query/commandならそのID、Query-lessならNoneを使う。Copy `db.exec`からtimeout/statement stateを
overlapさせない意図的v1制限であり、failed second attemptはfirst leaseのsaved valueを
read/restoreしない。ordinary/timeout streamの両方向overlap、second override失敗後のfirst
Drop、exhaustion/error cleanup、restore失敗poisonをtestする。capability不足はUnsupportedで
ありfallback/ignoreしない。extension loadingは
v1で公開せずdisabledのまま。SQLite optionをPostgreSQLへ渡すことは型エラーである。

### 11.3 native feature

`RETURNING`、STRICT、WITHOUT ROWID、JSON1、FTS、attached database、backup、incremental
blob、安全なcallback modelができた後のcustom function/collationを妨げない。初期release
はconnection、typed Query、transaction、migration、必要metadataに絞り、残りはD13/D14へ
明示的に置く。

## 12. PostgreSQL driver

### 12.1 connection

```align
conn := postgres.connect(url, [
  postgres.ConnectOption.ApplicationName("align-service"),
  postgres.ConnectOption.ConnectTimeoutNs(5_000_000_000),
  postgres.ConnectOption.Parameter("options", "-c statement_timeout=5000"),
])?
```

host/port/database/user、application name、connect timeout、TLS/SSL mode、target session
attributes、任意のsupported connection key/value、notice/error detailを扱える。
secretはruntime valueでありstatic Query metadataへ入れない。URL、application name、
parameter name/valueはUTF-8かつU+0000なしでなければならず、embedded NULはlibpq call前に
Query identityなしの `db.Error.Encode` として拒否し、truncateしない。

client encodingはpackage所有でUTF-8に固定する。`client_encoding`をreserved semantic keyとし、
URL/keywordや`Parameter("client_encoding", ...)`でのexplicit指定はopen前のconflictである。
`options`値はdocumentedなbackslash escape/space-separated startup-option grammarでtokenizeし、
`-c name=value`、`-cname=value`、`--name=value`のASCII case-insensitive
`client_encoding` assignmentを拒否する。long name比較では`-`と`_`を同一視する。trailing escapeと
assignmentなし`-c`はopen前のEncode error。expanded URLと全user overrideの後へexact
`client_encoding=UTF8`を`PQconnectdbParams`でappendしてambient `PGCLIENTENCODING`を排除し、
non-null `PGconn`では最初に`PQstatus`をcheckする。`CONNECTION_OK`以外はnative connection errorを
copyしてcloseし`PQclientEncoding`を呼ばない。OK後だけ`PQclientEncoding(conn) == PG_UTF8`を要求する。
mismatch（`-1`を含む）は常にcloseし、
libpq error messageに依存せずexact
`db.Error.Unsupported(db.ContractError { query_id: None,
item: "postgres.connection.client_encoding",
message: "PostgreSQL client encoding is not UTF-8" })`を返す。このinvariant前にはSQLを実行しない。

### 12.2 parameter/result format

初期D1--D12 PostgreSQL sumは次で固定する。

```text
postgres.QueryOption.ParameterType(name, canonical_type_name)
postgres.CommandOption.ParameterType(name, canonical_type_name)
postgres.PrepareOption.ParameterOid(name, oid)
postgres.ExecuteOption.ParameterFormat(name, Text|Binary)
postgres.ExecuteOption.ResultFormat(Text|Binary)
postgres.TxOption.Isolation(ReadCommitted|RepeatableRead|Serializable)
postgres.TxOption.Access(ReadOnly|ReadWrite)
postgres.TxOption.Deferrable(bool)
postgres.MetaOption.SearchPathOnly | IncludeSystemCatalogs
postgres.ExplainOption.Analyze
postgres.ExplainOption.Format(Text|Json)
postgres.ExplainOption.Verbose/Costs/Buffers/Timing/Settings/Wal(bool)
```

post-release D13はinitial-release gateを変えず次の1 execution variantを追加する。

```text
postgres.ExecuteOption.Delivery(SingleRow|PortalBatch(max_rows))
```

`name`と`canonical_type_name`は`str` literalである。

Text formatのbyteaは§5.6.1のexact hex encodingを使い、Binary formatだけがraw byteと
explicit libpq lengthを使う。D1--D12 implementationはBinary mapping要求をSQL送信前に
`Unsupported`としたが、§23 D13 binary-format ledgerのexact proof/mapping tableがそのtemporary
unavailable dispositionをsupersedeする。

unknown/conflicting OIDをignored hintにせず
errorにする。ParameterTypeはstatic artifact/public contractへ入りPostgreSQLへpinする。
field別controlのunknown/duplicateもerror。
`Delivery`はrow-producing executionだけのoptionで、operation set/default/size bound/
streaming/error/ABIは§23のA1 PostgreSQL delivery ledgerが固定する。commandには適用しない。
prepared rowsはexplicit `postgres.rows_stmt_native`を使い、common `db.rows_stmt`はcommon
optionだけを受け続ける。

connection sumは `ApplicationName`、`ConnectTimeoutNs`、`SslMode`、
`TargetSessionAttrs`、`Parameter(name,value)` で固定する。URLとoptionのsemantic key重複は
conflict、secretはartifactへ入れない。transactionの `[]` はReadCommitted/ReadWrite/
non-deferrableで、deferrableはserializable read-only以外ではBEGIN前にrejectする。
SearchPathOnlyとIncludeSystemCatalogsはconflict。Buffers/Timing/Walはnative Analyzeが
なければ実行前にconflictする。

### 12.3 native feature

初期release後の最初のPostgreSQL-native D13 railはsingle-row/chunked-row deliveryを追加する。
COPYは§25がconcrete measured consumerを記録するまでdeferredである。pipeline modeとLISTEN/NOTIFYは
独立してspecifyする後続D13 railである。

PostgreSQL array、UUID、JSONB、enum/domain、range、explicit composite mapping、COPY、
pipeline/single-row、LISTEN/NOTIFY、LATERAL、DISTINCT ON、custom operator/extension、
詳細EXPLAIN formatを将来追加できる。初期releaseではcommon Query/transaction path、
native option、checked metadata、basic plan accessを必須とし、広いnative pathはD13。

## 13. Native optionとportability

### 13.1 scope

optionは1つのuntyped bagではなくscope別のpublic sum typeである。

```text
db.QueryOption             static common Query semantics
db.CommandOption           static common command semantics
db.PrepareOption           common prepare controls
db.ExecuteOption           common one-execution controls
db.TxOption                common transaction semantics
db.MetaOption              common metadata categories
db.ExplainOption           common plan controls

sqlite.ConnectOption       SQLite connection controls
sqlite.QueryOption         SQLite static Query controls
sqlite.CommandOption       SQLite static command controls
sqlite.PrepareOption       SQLite prepare controls
sqlite.ExecuteOption       SQLite execution controls
sqlite.TxOption            SQLite transaction controls
sqlite.MetaOption          SQLite metadata controls
sqlite.ExplainOption       SQLite plan controls

postgres.*Option           対応するPostgreSQL-native scope
```

scopeが違うoptionはcompile-time type error。common operationはcommon optionだけを受ける。
driver native operationはcommon sliceとnative sliceを別に受ける。

```align
db.execute(exec, command, params, common_options)
sqlite.execute_native(exec, command, params, common_options, sqlite_options)
postgres.execute_native(exec, command, params, common_options, postgres_options)
```

driver static Query optionはdescriptorをそのdriverへpinし、public semantic artifactへ入る。
runtime connection/prepare/execution optionはstatic Query identityを変えない。

### 13.2 option valueの表現

各 `*Option` は有限のpublic sum typeで、operation引数は
`slice<ThatScopeOption>`。`[]` が明示的なoptionなしを意味する。payloadはCopy scalar、
call中だけ消費する `str` view、static type/Query identityに限定する。driverがcall後も
runtime stringを保持するならcopyする。untyped map/reflectionは使わない。

common初期variantは次で固定する。

```text
db.QueryOption.Check(DeclaredOnly|CheckedOptional|CheckedRequired)
db.CommandOption.Check(DeclaredOnly|CheckedOptional|CheckedRequired)
db.PrepareOption.TimeoutNs(ns)
db.ExecuteOption.TimeoutNs(ns)
db.TxOption.BeginTimeoutNs(ns)
db.MetaOption.TimeoutNs(ns) | IncludeSystem
db.ExplainOption.TimeoutNs(ns)
```

`[]` はstatic descriptorのDeclaredOnly、runtime deadlineなし、system metadata除外を
意味する。common db.explainはinspection-onlyで、PostgreSQL native AnalyzeだけがQueryを
実行してexecution-countへ入る。SQLiteはv1 Analyze optionを持たず、PostgreSQL optionを
reinterpretしない。durationは正。重複tag/check policyやcommon/native conflictはSQL送信前の
error。これは例ではなく必須minimumで、unknown tagやsilent extensionは禁止する。

static Query/command optionはさらに限定する。recognized constructorへ渡せるのは、literal、
type identity、compiler-known constantをpayloadとする固定literal option listだけである。
runtime local、environment read、FFI result、任意function callはcompile error。
canonical tag/payload encodingは `StaticQueryArtifact` / `StaticCommandArtifact` に入り、
driver pinningは `IStaticQuery` / `IStaticCommand` にも入る。

primitive operationは1つのoption-bearing形式を持つ。

```text
connection   driver.connect(input, slice<driver.ConnectOption>)
Query        db.query_file(slice<db.QueryOption>)
             driver.query_file(slice<db.QueryOption>, slice<driver.QueryOption>)
command      db.command_file(slice<db.CommandOption>)
             driver.command_file(slice<db.CommandOption>, slice<driver.CommandOption>)
prepare      db.prepare(exec, query, slice<db.PrepareOption>)
execution    db.execute/one/maybe_one/all/rows(..., slice<db.ExecuteOption>)
transaction  db.begin(conn, slice<db.TxOption>)
metadata     db.meta_database/meta_schemas/...(..., out: region, slice<db.MetaOption>)
EXPLAIN      db.explain(exec, query, params, out: region, slice<db.ExplainOption>)
```

default argument、optionless overload、fluent builder、string-key map、process-global optionは
ない。Query-local run helperはoptionをcallerから受けるか `[]` を渡すかsourceに書く。
option ownershipはD1 static Query/command、D2 SQLite connection/execute、D4 PostgreSQL
connection/execute、D6 prepare、D7 transaction、D9 deadline enforcement/native cancellation
cleanupと全scope matrix、D12 metadata/EXPLAINである。D9までpreliminary APIを待たせず、
別表現も作らない。

### 13.3 silent ignore禁止

全optionは次のいずれかになる。

```text
applied
rejected as unsupported
rejected as conflicting
```

portabilityのためにignoreすることは禁止。

### 13.4 precedence

validation orderはobservableかつexactである。connection formationは次の順で行う。

1. explicit connection inputをvalidate/parseする。
2. optionをsource orderで訪れ、current payload/native conversionをvalidateしてからtag/semantic
   keyをregisterする。duplicate/conflictはsecond occurrenceがownerになる。
3. complete list後にcross-option/capability constraintをvalidateする。
4. native connectionをopenしpost-connect invariantを確立する。

最初に失敗したphaseが勝つ。phase 2では後続の別errorに関係なく最初のsource-order invalid optionが
勝つ。PostgreSQL URL keyは最初のoptionより先に存在するため、最初のcolliding optionがconflict
ownerである。

Query/command executionはnative work前に次の順で行う。

1. state取得前にexecution-header pointer、ABI、reserved、kind、mask/slot/thunk agreement、
   `descriptor_id` view、Q1-plan pointerをvalidateし、complete success後だけidentityをtrustする。
2. common optionをsource orderでpayload、duplicateの順にvalidateする。
3. driver-native optionも同じ規則でvalidateする。
4. driver restrictionをvalidateする。
5. `db.exec` discriminator、generation、open/poison stateをvalidateする。
6. generated static-option validatorをinvokeする。
7. driver execution leaseを取得する。

invalid phase-1 descriptorはuntrusted identityを読まずexact
`db.Error.InvalidQuery(db.ContractError { query_id: None, item: "db.descriptor.header",
message: "invalid static database descriptor" })`を返す。したがってmalformedは全descriptor-dependent
errorより先、timeoutはmismatch/closedより先、mismatchはclosed/static-option/overlapより先、closedは
static-option/overlapより先に勝つ。bind/nativeはphase 7後だけ。cleanupは別のfirst-error ruleに従う。

- static Query optionはQuery意味論を表し、incompatible override不可。
- prepare optionはprepareだけに作用。
- execute optionは1回のexecutionだけに作用。
- connection defaultは明示的にoverride可能なpropertyのfallbackだけ。
- duplicate/conflicting non-overrideable optionはerror。

### 13.5 driver restriction

descriptorは次を記録する。

```text
AnySupportedDriver
SQLiteOnly
PostgreSQLOnly
```

`db.query_file([])` はAPI上portableに始まるが、SQL自体はprepareで失敗し得る。
`sqlite.query_file([], [])` / `postgres.query_file([], [])` とnative static optionはdriverを
pinする。wrong driverでの実行はSQL送信前に `DriverMismatch`。portabilityはdialect translationでは
なく、「同一common Align interfaceで両engineのprepareに成功するSQL」を意味する。

## 14. Transaction

### 14.1 明示lifecycle

```align
tx := db.begin(conn, common_tx_options)?
result := update_account.run(db.exec_tx(tx), params, out)?
conn := db.commit(tx)?
```

rollback:

```align
conn := db.rollback(tx)?
```

`begin` はconnをconsumeし、active tx中にcallerがconnを使えない。commit/rollbackはtxを
consumeし、成功時にconnを返す。明示end失敗またはDropはfail-safe rollbackしcommitしない。
direct-driver connectionはcloseする。D13 pool-origin exceptionはpool ledgerのexact native
rollback-and-idle proof後だけreturnし、proof missing/failureはcloseしてslotをretireする。
どちらも状態不明connを返さない。

### 14.2 native transaction option

SQLite:

```text
DEFERRED
IMMEDIATE
EXCLUSIVE
```

PostgreSQL:

```text
isolation level
read only/read write
deferrable
```

native optionはdriver-qualifiedで、unsupported combinationはtransaction開始前に失敗する。

### 14.3 nested transaction

v1のcommon APIでportable nested transactionを装わない。savepointは明示native APIとして
追加できる。

## 15. Error

DB errorはstructured owned Move sumである。文字列を唯一のerror modelにしない。

```align
db.NativeError {
  driver: db.Driver,
  code: Option<string>,
  extended_code: Option<i64>,
  sqlstate: Option<string>,
  message: string,
  detail: Option<string>,
  constraint: Option<string>,
  table: Option<string>,
  column: Option<string>,
}

db.CardinalityError {
  expected_min: i64,
  expected_max: i64,
  observed_at_least: i64,
}

db.ContractError {
  query_id: Option<string>,
  item: string,
  message: string,
}

db.Error {
  Connection(NativeError),
  Timeout(NativeError),
  Cancelled(NativeError),
  NotFound,
  PoolExhausted,
  Cardinality(CardinalityError),
  Constraint(NativeError),
  Serialization(NativeError),
  Deadlock(NativeError),
  SchemaMismatch(ContractError),
  DriverMismatch(ContractError),
  Decode(ContractError),
  Encode(ContractError),
  InvalidQuery(ContractError),
  Unsupported(ContractError),
  Native(NativeError),
}
```

SQLite primary/extended code、PostgreSQL SQLSTATE/detailを所有して保持する。native buffer
viewをerrorへ残さない。`ContractError.query_id` はQuery/command contractなら `Some(id)`、
connection/transaction/metadataなどQueryなしのoperation/input validationと、identity trust前に
malformed descriptor headerがfailする場合は `None` とし、
どちらも `item` がexact operation/inputを示す。success hot pathはerror stringをallocateしない。
`PoolExhausted` はvalidなnon-waiting pool acquisitionにidle connectionがない場合の
allocation-free、Query-less categoryである。`pkg.db.pool.info` により、全slotがcheckout中か、
poisoned connectionの後にcapacityがretireしたかを区別できる。

この形はL1a/L1bのrecursive tagged Move payloadを必須とする。compiler-known例外や
opaque integerだけへ縮退しない。low-level builtin `Error` への変換が必要なら
`map_err` による明示的boundary conversionを使う。

## 16. Offline checkとdatabase prepare

### 16.1 check level

Query/commandは許可driverごとに
`DriverVerification { driver, Declared|DatabaseChecked, metadata_fingerprint }` を持つ。
SQLiteOnlyはSQLite、PostgreSQLOnlyはPostgreSQL、AnySupportedDriverは両方がstate setになる。

```text
Declared         Align Params/Row（commandはParams）とruntime contractだけ
CheckedOptional  driverごとにmatching metadataを使い、missing/stale driverはDeclared
CheckedRequired  許可driver全てにmatching metadataがなければbuild失敗
```

AnySupportedDriverをSQLiteだけprepareしてもCheckedRequiredは満たさない。PostgreSQLも
prepareするかdescriptorをSQLiteへpinする。mixed stateを1つのchecked boolへ畳まない。
inspectionは全driver map、runtimeは選択connectionのentryを報告する。stale metadataを
CheckedOptionalで黙って使ってはならず、使うのはdigest一致したdriver entryだけ。

### 16.2 明示prepare command

```text
alignc db prepare app.align --driver sqlite --database dev.sqlite --schema-id dev-v1
alignc db prepare app.align --driver sqlite --memory --migrations db/migrations
alignc db prepare app.align --driver postgres --url-env APP_DATABASE_URL --schema-id dev-v1
alignc db prepare app.align --driver postgres --url-env APP_DATABASE_URL --schema-id dev-v1 --query app.user
alignc db prepare app.align --driver postgres --url-env APP_DATABASE_URL --schema-id dev-v1 --check
```

entry moduleとdriverは必須。Query discoveryはreachable static Query/command graphだけを
対象にし、directoryをscanしない。SQLiteで明示した `--migrations <dir>` だけは§16.6の
catalogをtool action内で列挙する。`--query` は対象をさらに絞る。prepare regeneration
modeだけがmissing/stale artifactを許し、`--check` は何も書かない。normal buildは
environmentを読まずDBへ接続しない。mutableなSQLite DB fileとPostgreSQL targetは
non-empty/non-secret UTF-8かつU+0000なしの `--schema-id` を必須にする。
PostgreSQL URLはnon-empty user/password、exactly one host、nonzero port、exactly one
databaseを明示する。target override、service expansion、`client_encoding`、startup
`options`を拒否し、toolが`PQconnectdbParams`でpackage-owned UTF-8とempty startup-option
sequenceを渡すため、ambient `PG*` defaultがtarget/encoding/SQL settingを選ばない。
`--memory [--migrations]` は§16.6からidentityをderiveして `--schema-id` を禁止する。
1 preparation batchは1 schema snapshotだけを観測する。SQLiteはmigration transaction後に
read transactionを開始して`sqlite_schema`を読み、PostgreSQLはenvironment capture前に
read-only repeatable-read transactionを開始する。全selected prepare/describeまで保持し、
preparation connectionと一緒にcloseする。

### 16.3 metadata location

```text
.align-db/
  .publication.lock
  sqlite/<descriptor-id-hash>.json
  postgres/<descriptor-id-hash>.json
```

`.publication.lock` はemptyなimplementation-owned cross-process lockで、build inputでも
artifact identityでもない。normal compilationはchecked metadata snapshot全体でshared OS
lockを保持し、preparationはcomparison、staging、replacement、rollback全体でexclusive
lockを保持する。最初のpublication後もfileを残すため、process exitだけでsynchronizationが
解放され、stale-lock recoveryを必要としない。

`descriptor-id-hash` はQueryのquery_idまたはcommand_idである`descriptor_id` exact bytesの
`Hash128::of(...).to_hex()`。directoryはexactに
`sqlite|postgres`、filenameは32 lowercase hexと`.json`。compilerはdirectory scanをせず
descriptor/driverから1 pathをderiveし、内部ID/driver不一致やhash collisionをdiagnosticにする。

v1 JSONは1 objectの1行とexact LF、BOM/他whitespaceなし。keyは下記orderで、
duplicate/unknown/missing/out-of-orderをrejectする。string escapeはexactに
`\"`、`\\`、`\b`、`\t`、`\n`、`\f`、`\r`、残りU+0000〜U+001Fはlowercase
`\u00xx`、slashはescapeせず、他Unicodeはraw UTF-8。integerはshortest decimalで
leading zero/plus/negative zeroなし。Optionはpayloadまたは`null`。decode後のre-encodeが
byte-identicalでなければnoncanonicalとしてrejectする。

```text
format_version: 1
descriptor_id: string
module: string
item: string
driver: "sqlite" | "postgres"
driver_restriction: "any_supported_driver" | "sqlite_only" | "postgres_only"
statement_kind: "query" | "command"
statement_class: "select" | "dml" | "ddl" | "native" | "unknown"
source_identity:
  File   => { kind: "file", logical_path: string }
  Inline => { kind: "inline", descriptor_id: string }
source_sql_hash: Hash128Hex
wire_sql_hash: Hash128Hex
rewrite_format_version: u32
static_options_hash: Hash128Hex
params_fingerprint: Hash128Hex
row_fingerprint: Hash128Hex | null
schema_fingerprint: Hash128Hex
engine_version: string
driver_version: string
search_path: array<string>
extensions: array<{ schema: string, name: string, version: string | null }>
parameters: array<{
  source_name: string, protocol_ordinal: u32, logical_type: string,
  native_type: string | null, native_type_id: i64 | null
}>
columns: array<{
  ordinal: u32, source_alias: string, logical_type: string,
  native_type: string | null, native_type_id: i64 | null,
  nullable: "yes" | "no" | "unknown",
  origin_schema: string | null, origin_table: string | null,
  origin_column: string | null
}>
```

各nested objectのkey orderも表示順。Hash128Hexは`lo`、`hi`順の32 lowercase hex。
parameterはone-based protocol ordinal、columnはzero-based decoder ordinal順。
search_pathはsemantic order、extensionは`(schema,name,version Option tag/bytes)`の
UTF-8 byte順。SQLiteのsearch_path/extensionsはempty。commandはrow fingerprint null、
columns empty。`static_options_hash`はu32 count prefixを含むcomplete
`sequence<StaticOption>` encodingのHash128。hash/fingerprint/ordinal/nameはL5 artifactと
一致必須。`logical_type`はfieldのsubstituted `CanonicalType` rootに対するformatterの
canonical fully-qualified Align spellingで、alias/source layout spellingを残さない。
`driver_restriction`はL5 artifactと一致し、selected driverを許可しなければならない。

complete file bytesの`Hash128::of`が`metadata_fingerprint`。`schema_identity`は
`schema_fingerprint`。`server_identity`は
`"ALIGNSRV", u32 1, Driver tag, engine_version, driver_version,
sequence<search_path>, sequence<{schema string,name string,version Option<string>}>`のL5 binary
codec digest。`prepare_identity`は
`"ALIGNPRP", u32 1, descriptor_id, Driver tag, metadata_fingerprint, server_identity`のdigest。
全identityは`to_hex()`で、JSON自身へ埋めずcompilerがderiveする。compilerはordered
native evidenceとidentityをproducer-owned QueryMeta planへcopyし、runtimeはfileを開かない。

invalid UTF-8/JSON、noncanonical escape/number/key order、wrong type/tag、non-dense ordinal、
count/name/hash/source mismatch、trailing byteをpanicせずrejectする。malformed fileは
CheckedOptionalでもhard diagnostic、well-formed staleだけ§16.4に従う。
`checked_metadata_{sqlite_query,postgres_command}_v1.json`と`.digest`のindependent-reference
goldenをD3/D5で固定し、全Option/escape/native ID/origin/nullability/source identityを含める。
secret/URLを保存しない。

### 16.4 stale判定

SQL、static option、Params/Row、driver restriction、metadata policy、relevant schema
fingerprintの変更でstaleになる。各descriptor/許可driverについて、exact metadata logical
pathと存在状態を `StaticInputManifest` / action keyへ
`Missing` / `Present(content_hash, format_version)` として入れる。file作成/変更/削除が
directory scanなしでinvalidateする。

### 16.5 custom SQL engineを作らない

通常buildではstatement screeningとplaceholder scanだけを行う。権威あるsyntax/type/
result metadataはSQLite/libpqへprepare toolで問い合わせる。compiler内に不完全な
PostgreSQL parser/type checkerを作らない。

### 16.6 SQLite prepare environment

明示DB fileを使うか、temporary/in-memory DBへcanonical migration sequenceを適用して
prepareする。`--migrations <dir>` は明示tool actionだけが行うdirectory列挙で、normal
build/checkやQuery discoveryは行わない。v1 rule:

- immediate entryだけを非recursiveに列挙し、non-UTF-8名と全symlinkを拒否;
- exact `[0-9]{4}_[a-z][a-z0-9_]*[.]sql` のregular fileだけを選択し、他の`.sql`名はerror;
- `--migrations` 指定時は1つ以上のmigrationを要求;
- version `0001`〜`9999`、duplicateなし、`0001`開始のcontiguous sequenceを要求;
- filesystem orderではなくnumeric version ascending;
- fileはUTF-8 exact bytes、newline normalizationなし;
- 全selected fileのU+0000を最初のmigration適用前に拒否;
- 各screened file全体をその順でmigration scriptとして適用（Queryの1-statement rule対象外）;
- 次のexact bytesをencodeしてfingerprintする。

```text
magic "ALIGNMIG", u32 format_version 1, u32 entry_count
numeric version順の各entry:
  version u32
  filename string                 # L5 u32 length + exact UTF-8 bytes
  content_hash Hash128            # exact file bytesのHash128
catalog_fingerprint = complete bytesのHash128.to_hex()
```

`migration_catalog_{empty,nonempty}_v1.hex`と`.digest`のindependent-reference goldenを持ち、
nonempty fixtureはnon-ASCII filenameとnewline非正規化SQLを含む。

name/version/gap/symlink/UTF-8 errorは1件目を適用する前に報告する。D11のmigrateも同じ
catalog ruleを再利用する。schema identity bytesは
`"ALIGNSID", u32 1, Driver tag, source tag`に続け、
SQLite memoryは`Option<catalog_fingerprint>`、SQLite databaseは明示`schema_id`、
PostgreSQLは明示`schema_id`とsemantic-order search_path、canonical-order extensionsを
encodeし、そのHash128を`schema_fingerprint`にする。v1 memory prepareはPRAGMA/attachment
inputを公開せず、追加時はversioned stream fieldが必要。undeclared ambient stateを使わない。
`schema_identity_{sqlite_empty,sqlite_migrations,sqlite_database,postgres}_v1.hex`と`.digest`の
independent reference fixtureで全source tag、catalog Option両state、non-ASCII
schema_id/search_path、extension canonical orderを固定し、production codecを共有しない。

### 16.7 PostgreSQL prepare environment

URLは指定environment variableからprepare toolだけが読む。search_path、server version、
extension/type OID evidence、schema fingerprintを記録する。equivalentに再作成したschemaで
canonical outputが一致するようにする。

### 16.3.1 nullability/origin evidence

query固有のevidenceをfail-closedに扱う。

```text
Yes      engineがこのexact result expressionをnullableと記述した
No       engineがこのexact result expressionをnon-nullと記述した
Unknown  authoritativeなquery-level answerがない、またはambiguous
```

catalog `NOT NULL`、source column declaration、origin lookupだけでは `No` にしない。outer
join、expression、function、rewriteでresult nullabilityは変わる。Align v1はSQL nullability
analyzerを追加しない。originはengineがexact result entryについてunambiguousなschema/table/
columnを返した場合だけ記録し、nameやsearch pathから推測しない。

```text
SQLite     exact reported originだけ記録し、probed APIにquery-level evidenceがなければ Unknown
PostgreSQL RowDescriptionのtable/attribute originだけ記録し、catalog nullabilityは Unknown
```

D0がAPI/version evidenceを記録し、D3/D5は各driver/version matrixとtestをmerge gateにする。
これはD12〜D14へ残す設計判断ではない。`Yes` はexact Rowに `Option<T>`、`No` はv1で
non-`Option<T>` を要求する。`Unknown` はどちらも許すが何も証明せず、SQL NULLは必ず
`Option<T>` の `None` またはnon-Optionのstructured decode errorになる。Declared/
DatabaseChecked/evidence stateに関係なくruntime NULL guardを残し、optimizationで除去しない。
type/ordinal等が一致すれば `Unknown` でもDatabaseCheckedになれる。

## 17. Migration

### 17.1 SQL file

```text
migrations/
  0001_create_users.sql
  0002_add_groups.sql
```

順序、name、exact content hashがidentityである。structから生成しない。
prepareとD11 migrateは§16.6の同じfilename/version/order/symlink/hash catalog ruleを使う。

### 17.2 commandとexact input

D11 live-database commandはentry graph、migration catalog、driver、targetを全て明示する。

```text
alignc db migrate --entry ENTRY --migrations DIR --driver sqlite --sqlite-path PATH
alignc db status  --entry ENTRY --migrations DIR --driver sqlite --sqlite-path PATH
alignc db check   --entry ENTRY --migrations DIR --driver sqlite --sqlite-path PATH

alignc db migrate --entry ENTRY --migrations DIR --driver postgres --postgres-url-env NAME
alignc db status  --entry ENTRY --migrations DIR --driver postgres --postgres-url-env NAME
alignc db check   --entry ENTRY --migrations DIR --driver postgres --postgres-url-env NAME

alignc db repair  --entry ENTRY --migrations DIR --driver sqlite --sqlite-path PATH
                  --version N (--accept-applied | --clear-dirty) --expect-checksum HASH
alignc db repair  --entry ENTRY --migrations DIR --driver postgres --postgres-url-env NAME
                  --version N (--accept-applied | --clear-dirty) --expect-checksum HASH
```

ENTRYはproject/package rootを決めるexplicit `.align` entry、DIRは§16.6のpath/symlink/catalog
ruleを満たすproject-root-relative directory。targetはdriverと一致する `--sqlite-path PATH`
または `--postgres-url-env NAME` のexactly oneで、missing/duplicate/mismatchはDB open前に
失敗する。SQLiteはmigrateでread-write/create、status/checkでread-only/no-create、
repairでread-write/no-createとし、missing fileはmigrate以外で失敗する。implicit PRAGMAは
ない。PostgreSQLはNAMEをenv identifierとして検証しcommand parse後にそのURL valueだけを
読む。secret valueはargv/artifact/log/normal buildへ入れない。default `DATABASE_URL`、
config discovery、cwd migration scan、driver推測はない。status/checkは適用・repairせず、
repairは§17.5のchecksum-bound actionだけを行う。`alignc db prepare` は§16.5〜§16.7の別の
explicit metadata workflowである。通常buildにはどのcommandも組み込まない。

### 17.3 checksum/history

適用済みmigrationのversion/name/checksumをDB内history tableへ記録し、変更済み過去file、
重複version、gap policy違反をerrorにする。

### 17.4 transaction

各fileは先頭physical lineに0個または1個のexact ASCII directiveを持つ。

```sql
-- align:migration transaction=required
-- align:migration transaction=forbidden
```

省略時はrequired。directive byteはchecksumに入り、CLI/ambient overrideはない。
BEGIN/COMMIT/ROLLBACK/SAVEPOINTなどnative transaction-control statementは禁止し、runnerが
boundaryを所有する。PostgreSQL migrationはtop-level first tokenが`COPY`のstatementも禁止する。
migration executorはCOPY data/termination protocolをownせずlibpq COPY modeへ入れないためである。
driver-authoritativeなscript preparation/screeningでtarget open/mutation前に両classをrejectする。
requiredはmigration lock取得後、native write transactionをbeginしてdatabase-native history lockを
取得し、exact history schema/already-Applied prefixを再検証してから全statementを実行する。
全statement後に同じschema/prefixを再検証し、Applied rowをinsertして
rereadする。commit前に観測したerrorは全体rollbackし、不確定commit responseは下記reconciliation
ruleを使う。transaction内で拒否されたstatementを外へ出してretryしない。

forbiddenはtransaction外が必須なnative statement用で、v1はdatabase-authoritativeに
正確に1 statementのfileだけ許可する。実行前にApplying history rowを入れ、成功後だけ
Appliedへ更新する。errorはbest-effortでFailed、process lossはApplyingを残す。両方をdirty
stateとして後続migrationをblockし、status/checkが報告し、自動retryしない。

recoveryは§17.2のentry/catalog/driver/target全入力に加えてexact checksumを要求する。

```text
alignc db repair --entry ENTRY --migrations DIR --driver sqlite --sqlite-path PATH
                --version N --accept-applied --expect-checksum HASH
alignc db repair --entry ENTRY --migrations DIR --driver sqlite --sqlite-path PATH
                --version N --clear-dirty --expect-checksum HASH

alignc db repair --entry ENTRY --migrations DIR --driver postgres --postgres-url-env NAME
                --version N --accept-applied --expect-checksum HASH
alignc db repair --entry ENTRY --migrations DIR --driver postgres --postgres-url-env NAME
                --version N --clear-dirty --expect-checksum HASH
```

`--accept-applied` はoperatorがnative stateを確認してAppliedにし、`--clear-dirty` はsafe
retryを確認してdirty rowだけを削除する。DB effectをundoせず、Applied rowは対象外。

database commitまたはconnection-loss errorはclientが観測できないoutcomeを持ち得る。runnerはその
不確実性をrollback済みと主張しない。failed native connectionをcloseし、fresh connection 1つをopen、
上記native lock下でexact historyを1回rereadする。SQLiteはconnection replacement中もseparate OS-lock
descriptorを保持し、PostgreSQLはnative table lock/reread前にconnection-owned advisory lockを再取得する。
Required transactionではexact Applied
rowがSQLとhistory両方のcommitを証明し、absentならどちらもcommitしておらず、そのinvocationでは
retryせずnot-appliedを報告する。Forbiddenはnative execution前にApplying insertのdurable observationを
必須とする。不確定なfinal updateはAppliedまたはdirty Applying rowとしてreconcileする。reconnect、
relock、許可state取得に失敗すればexplicit outcome-unknown errorを返す。後続invocationは通常のcomplete
history reconciliationから始める。

### 17.6 version 1 history、lock、command contract

D11はpersistent history formatを1つだけ所有する。これはpackageのoperational stateであり、
Query artifactでもnormal-build inputでもない。SQLiteは `__align_migrations_v1` table、
PostgreSQLは `align_internal` schemaと `align_internal.migrations_v1` tableを所有する。
`migrate` だけがこれらを作成でき、`status`、`check`、`repair` は作成しない。history objectが
なければempty historyとする。既存objectやrowがこのexact contract外ならin-place upgradeせず
invalid historyにする。

logical recordはexactに次である。

```text
format_version: u32 = 1
version: u32                 # 1 through 9999, primary key
filename: str                # exact canonical catalog filename
checksum: str                # lowercase 32-hex Hash128::of(exact file bytes)
policy: u8                   # 0 Required, 1 Forbidden
state: u8                    # 0 Applying, 1 Applied, 2 Failed
completed_statements: u32
```

creation DDLはcanonicalである。SQLiteは次のexact statementを実行する。

```sql
CREATE TABLE "__align_migrations_v1" (
  "format_version" INTEGER NOT NULL CHECK (typeof("format_version") = 'integer' AND "format_version" = 1),
  "version" INTEGER NOT NULL PRIMARY KEY CHECK (typeof("version") = 'integer' AND "version" BETWEEN 1 AND 9999),
  "filename" TEXT NOT NULL CHECK (typeof("filename") = 'text'),
  "checksum" TEXT NOT NULL CHECK (typeof("checksum") = 'text' AND length("checksum") = 32),
  "policy" INTEGER NOT NULL CHECK (typeof("policy") = 'integer' AND "policy" IN (0, 1)),
  "state" INTEGER NOT NULL CHECK (typeof("state") = 'integer' AND "state" IN (0, 1, 2)),
  "completed_statements" INTEGER NOT NULL CHECK (typeof("completed_statements") = 'integer' AND "completed_statements" BETWEEN 0 AND 4294967295)
)
```

PostgreSQLはbootstrap transaction 1つでschemaとtableを作り、
`CREATE SCHEMA "align_internal"` の後に次のexact table statementを実行する。

```sql
CREATE TABLE "align_internal"."migrations_v1" (
  "format_version" integer NOT NULL CHECK ("format_version" = 1),
  "version" integer NOT NULL PRIMARY KEY CHECK ("version" BETWEEN 1 AND 9999),
  "filename" text NOT NULL,
  "checksum" text NOT NULL CHECK (length("checksum") = 32),
  "policy" integer NOT NULL CHECK ("policy" IN (0, 1)),
  "state" integer NOT NULL CHECK ("state" IN (0, 1, 2)),
  "completed_statements" bigint NOT NULL CHECK ("completed_statements" BETWEEN 0 AND 4294967295)
)
```

PostgreSQL objectが両方absentのときだけempty historyである。一方だけ存在すればinvalid historyで、
既存schemaをadoptしない。SQLiteは1つのtableの `sqlite_schema.sql` が上のcanonical DDL byteと
一致することを要求し、`tbl_name` が `__align_migrations_v1` の他の `sqlite_schema` rowを許可しない。
さらにそのtableを `tbl_name` とする `sqlite_temp_schema` rowを許可せず、persistent/connection-local
index、trigger、view、rewritten table formを除外する。他のmain-database tableからhistory tableを
参照するforeign keyも許可せず、必要なexact-snapshot restoreがapplication dataへcascadeすることを
防ぐ。全history query/mutationは
`main.__align_migrations_v1` をexplicit qualifyし、temporary objectによるowned table shadowを防ぐ。
PostgreSQLはcurrent role所有の
permanent ordinary heap tableを要求し、partition/inheritance relation、row security、forced row
security、inbound foreign keyのinternal triggerを含むtrigger、rewrite rule、policy、extra index、
generated/default/identity expression、non-owner table/column ACL、history tableへattachされた
他のbehavior-affecting objectを許可しない。唯一のindexは
`version` 上のvalid/ready/immediate/default-btree primary-key indexで、constraintはそのprimary keyと
DDL中の6個のimmediate validated checkだけである。schemaはcurrent role所有である。fully qualified
history DMLへ影響しないunrelated schema object/schema grantはこのtable invariantに含めない。

readerはこのcomplete table-attached ancillary-object inventoryに加え、1本のjoined driver-catalog
queryでexact column order、declared type、nullability、primary key、check expressionを検証し、
さらに全rowのstorage/type、
exact lowercase checksum、filename/version agreement、field combinationを検証する。PostgreSQLでは
`completed_statements` だけが `bigint` で、contractは上記のfull unsigned 32-bit rangeである。
schema disagreement、unrepresentable native value、semantic row violationはmutation前にcommandを
失敗させる。replaced/weakened tableはfail closedする。
`Applying` と `Failed` は `policy = Forbidden` かつ `completed_statements = 0`、`Applied` は
current catalog fileがあればdriver-screened statement countと一致しなければならない。
HistoryOnly Applied rowはrescreenするfileがないためForbiddenは1、Requiredはnonzero `u32` を要求し、
そのversionを再導入するとexact-count checkを戻す。Requiredはmigration transaction内で
Applied rowだけをpublishする。ForbiddenはApplying(0)、native success後のApplied(1)、
best-effort error recordのFailed(0)だけを持つ。

lockはhistory bootstrap/read/validationとoperation全体を覆う。SQLiteはexact database pathに
`.align-migrate.lock` をappendしたpersistent empty sibling fileを使う。全commandはその1 fileを
replacementなしでatomicにopen/createし、`fstat` 後にsymlink、non-regular、nonempty fileをrejectする。
その後 `migrate`/`repair` はexclusive OS lock、`status`/`check` はshared OS lockを保持する。
absent pathのcreatorはmode `0600` のexclusive creationを使い、`AlreadyExists` raceではwinnerを
truncateせずreopenする。全cooperating operationは同じpersistent inodeのlock acquisitionで
linearizeする。このoperational lock作成だけが `status`/`check` に許されるfilesystem writeであり、
databaseはread-onlyでopenしhistory/schema objectは作成しない。
PostgreSQLはsession advisory lock key `(1095518535, 1296647985)` (`ALIG`, `MIG1`) を、
writeではexclusive、readではshared modeで保持する。process/connection lossはOS/advisory lockを
解放するがSQLite fileは削除しない。

OS/advisory lockはcooperating Align commandをserializeし、database-native lockはnon-cooperating
database connectionに対するvalidation-to-history-DML intervalを閉じる。SQLite bootstrap、Required、
repair、Forbiddenの各history transitionは `BEGIN IMMEDIATE` を使い、write reservation取得後だけ
validateする。SQLite `status`/`check` はread transaction 1つを使い、最初の
`main.sqlite_schema` readでschema/row validation前にsnapshotを固定する。PostgreSQLの各history phaseは
explicit `READ COMMITTED` transaction 1つを使う。`BEGIN` 後のfirst SQLは存在queryなしで、全history
validation/mutation前に `LOCK TABLE "align_internal"."migrations_v1" IN ACCESS EXCLUSIVE MODE`、
`status`/`check` validation前に `IN SHARE ROW EXCLUSIVE MODE` を取得する。後者はordinary readerを
許可しつつhistory DMLとordinary/concurrent index/DDL lock modeに競合する。このlockはfirst
catalog/history read前に
prior writerを待ち、transaction endまでlater readへのtable writerを止める。

SQLSTATE `42P01` (undefined table)または `3F000` (invalid schema name)だけがfailed transaction
rollback後にabsent-owned-object pathを選ぶ。
`migrate` はnew transaction 1つでexact schema/table bootstrapをattemptし、new objectをvalidate/commit後、
blind-lock phaseから再開する。pre-existing schema/creation raceはadoptせずfailする。`status`/`check` は
new transaction 1つとcatalog query 1本を使い、両object absentをempty snapshot、一方だけpresentを
invalidとする。`repair` はmissing historyを報告し、他のlock errorは直接failする。これによりbootstrap/
first-reader raceをdeterministicにし、`migrate` だけがhistoryを作る規則を保つ。

Forbidden user SQLはApplying commit後にseparate worker connectionで実行する。workerはhistory DMLを
行わず、history connectionがfinal native-lock/revalidation/Applied-or-Failed transactionを始める前に
closeする。このためworker-local temporary object/settingはhistory mutationへ影響せず、recorded
history-connection invariantの外である。persistent changeはfinal validationから可視である。
runnerはworker前に公開したexact Applied-prefix-plus-Applying history snapshotを保持する。final native
lock下でowned schemaをvalidateし、complete historyをsnapshotと比較する。unchanged snapshotは
rewriteせず、current Applying rowだけをAppliedまたはFailedへupdateする。row changeはexact snapshotを
restoreしてrereadした後、Applyingを保ったままvisibleに失敗する。workerがowned tableを削除した場合、このmigrate
invocationはexact table
（PostgreSQLでは両owned objectがabsentの場合のみschemaも）を再作成し、Applyingを復元してvisibleに
失敗する。malformed replacementはdrop/adoptせずlater migrationをblockするため、user SQLまたは
non-cooperating writerはdirty checkpointをerase/forgeしてautomatic retryへ変えられない。
Required user SQLは1 transaction connection上に残り、その
connection-local inventoryをrow insert前に検証する。lock acquisitionは常にvalidationより先で、
対応read/history mutation完了後だけnative transaction/lockを解放する。

migration directoryはproject-root-relativeである。entryはexisting regular non-symlink `.align`
fileで、そのlexical parentをproject rootとする。absolute migration path、`..`、symlink directory、
canonical escapeはenumeration前にrejectする。relative SQLite targetはproject root基準、absolute
targetはそのままexplicitである。final targetとlock fileはsymlink不可。全catalog/policy validation後、
全commandはmissing lockだけを作成でき、`migrate` だけがfinal databaseも作成できる。database missingは
`migrate` 以外でerror。PostgreSQL env nameは `[A-Za-z_][A-Za-z0-9_]*` に一致し、`PG` で
始まらず、§16.2のcomplete URLとambient `PG*` rejectionに従う。

validation precedenceはdeterministicに次の1列である。

1. argv tokenをsource orderで訪れ、各tokenをnon-empty UTF-8かつU+0000なしとしてdecodeしてから
   そのtokenのunknown/duplicate optionをrejectする。
2. operation固有required field、exactly one matching target、repair action、version、lowercase
   expected checksumを検証する。
3. entry、project-relative catalog containment、§16.6のcomplete catalogの順で検証する。
4. 全first-line policyを分類し、全complete statementをscreenし、empty file、最初の
   transaction-control、最初のPostgreSQL top-level `COPY`、database-authoritative countが1でない
   Forbidden fileをrejectする。
5. PostgreSQL選択時はそのURL env value 1つを読み、全connection inputを検証する。
6. exact targetを検証しmigration lockを取得し、targetをopen/readしてcomplete historyをvalidate後に
   operationを行う。SQLiteはdatabase open前にfile lockを取得し、PostgreSQLはconnect直後かつ
   history/user-schema request前にadvisory lockを取得する。

phase 1ではoperation nameを最初に検証し、次にtokenをsource orderで訪れる。各tokenはUTF-8 decode、
empty、U+0000の順で検証する。missing option value、unknown option、duplicate optionの2回目はその
token位置で失敗する。phase 2は `--entry`、`--migrations`、`--driver`、matching targetの順。
repairは続けて `--version`、exactly one action、`--expect-checksum` を検証し、non-repair commandは
同じ順でそれらのfieldをrejectする。

phase 3はentryをmigration pathより先に検証する。全immediate directory entryをsnapshotし、
enumeration errorを優先し、その後いずれかのnon-UTF-8 nameをpath-independent errorにする。残るnameを
UTF-8 byte順にsortする。その順で同じentryのmetadata-read failure、symlink、invalid regular `.sql`
nameの順に優先し、unrelated non-SQL regular file、directory、他のnon-regular entryはignoreする。
selected nameはnumeric順で
version、duplicate、gap ruleを検証する。selected contentもnumeric順で読み、per-file precedenceは
read/count overflow、invalid UTF-8、U+0000である。phase 4もnumeric file順で、1 file内ではpolicy
classification、lexical completeness、empty scriptを先に検証する。その後1つのclassification passが
各complete statementをsource orderでexactly once訪れ、同じstatement内はtransaction-controlを
PostgreSQL `COPY`より先に分類し、最初のprohibited statementで停止する。Forbidden countはその後に検証する。
phase 5は§16.2のambient `PG*`、selected variable presence/value、complete URLの順。
phase 6はtarget、lock、schema shape、history row、selected-operation errorの順で、history内はversion順と
上記record-field順を使う。両driverがこのtraversalで全multi-invalid inputのwinnerを1つ選ぶ。

policy directiveはLFまたはEOFで終わるexact first physical lineだけを認識する。CRLFやleading byteは
matchせずRequired既定になる。SQL screeningはquoted string/identifier、line/block comment、
PostgreSQL dollar-quoted bodyを無視する。top-level first tokenが `BEGIN`、`START TRANSACTION`、
`COMMIT`、`END`、`ROLLBACK`、`ABORT`、`SAVEPOINT`、`RELEASE`、`PREPARE TRANSACTION`、
`SET TRANSACTION`、`SET LOCAL TRANSACTION`、`SET SESSION TRANSACTION`、`SET SESSION
CHARACTERISTICS AS TRANSACTION` のstatementをrejectする。PostgreSQLはさらにfirst token `COPY`を
exact diagnostic `migration \`<filename>\` contains a PostgreSQL COPY statement`でrejectする。1 numeric
file内は1つのstatement-ordered classification passがsource-order最初のprohibited statementを選び、
各statementではtransaction-controlをCOPYより先にclassifyし、その後Forbidden countを検査する。
SQLite boundaryはtrigger bodyを含め `sqlite3_complete`、PostgreSQLは同じdollar-quote-aware
driver scannerを使う。両driverともtarget openやApplying publish前にscreeningを完了し、SQL validityは
native executionがauthoritativeである。
user SQL後かつ全history insert/update前に、上記operation lockとdatabase-native lockの両方の下で
complete owned persistent/applicable history-connection-local schema/ancillary-object inventoryと
expected prefixを再検証する。Requiredはcommit前にnew rowもrereadする。Forbiddenはworkerをcloseし、
history connection上でfinal native lockを取得してからfinal update前の同じ検証を行う。historyを
alter/replace/shadowしたりbehaviorをattachするSQLはRequired fileをrollbackするかForbidden rowを
visible dirtyに残し、progressをsilent erase/forgeできない。

history comparisonはversion order。current catalog versionごとにexactly oneの `Pending`、`Applied`、
`NameMismatch`、`ChecksumMismatch`、`PolicyMismatch`、`DirtyApplying`、`DirtyFailed` を生成し、
catalogにないhistory versionは `HistoryOnly` とする。current/history rowがversionを共有するときの
precedenceはName、Checksum、Policy、Applying、Failed、Appliedである。earlier Applied rowの欠落、
mismatch/dirty/history-only、non-prefix
Applied setはpending file実行前に `migrate` をblockする。

print tagは順に `pending`、`applied`、`name_mismatch`、`checksum_mismatch`、`policy_mismatch`、
`dirty_applying`、`dirty_failed`、`history_only` である。`mismatched` はchecksum/name/policy mismatch、
他のsummary fieldはsame-named stateをcountし、5 summary countが全printed rowをexactly once覆う。
全commandはcurrent catalog rowをversion順、その後HistoryOnly rowをversion順に出す。全fieldは常に
存在し、`catalog_*` はcurrent catalog、`history_*` はstored rowだけをsourceにする。missing sideは
exact unavailable token `-`、`history_state` は `applying`、`applied`、`failed`、`-` のいずれか。
`history_completed` はstored decimal `u32` または `-` である。

```text
migration version=0001 catalog_name=0001_create_users.sql catalog_checksum=<32hex> catalog_policy=required history_name=0001_create_users.sql history_checksum=<32hex> history_policy=required history_state=applied history_completed=1 state=applied
summary driver=sqlite applied=1 pending=0 dirty=0 mismatched=0 history_only=0
```

`status` はsummaryがcurrentでなくてもcomplete read後にsuccess。`check` は全catalog rowがAppliedで
extra rowがないときだけsuccess。successful `migrate` はfinal all-Applied view、successful `repair` は
exactly one checksum-matching dirty row変更後のfinal viewを出す。errorとPostgreSQL outputはURL/env
valueを含めない。`--accept-applied` はscreened statement countを記録し、`--clear-dirty` はそのrowだけを
削除する。Applied、non-current version、current/history checksumが `--expect-checksum` と異なる対象は
受理しない。

## 18. Metadataとintrospection

### 18.1 detail level

metadataは要求categoryだけを取得する。万能snapshotを既定にしない。

### 18.2 operation分離

```text
database
schema / attached database
table / view
column
key / constraint
index
static Query
query plan
```

request typeとoptionをcategoryごとに分け、1 category requestが無関係なcatalogをfetch
しないことをquery-count testで固定する。

common結果はflatな `RegionPlain` recordで、型とfield contractは次で固定する。

```text
db.MetaTableKind      Table | View | MaterializedView | Native
db.MetaNullability    Yes | No | Unknown
db.MetaKeyKind        Primary | Unique | Foreign | Check | Exclusion | Native
db.MetaForeignKeyMatch Simple | Full | Partial
db.MetaReferentialAction NoAction | Restrict | Cascade | SetNull | SetDefault
db.MetaIndexTermKind  Key | Included
db.MetaSortOrder      Asc | Desc
db.MetaNullOrder      First | Last
db.MetaQueryState     Declared | DatabaseChecked
db.MetaQueryEntry     Summary | Parameter | Column
db.MetaStatementClass Select | Dml | Ddl | Native | Unknown
db.PlanFormat         Text | Json | Native

db.SchemaRef
  name: str

db.TableRef
  schema, name: str

db.DatabaseMeta
  driver: Driver
  name, engine_version: str
  default_schema, encoding, collation: Option<str>
  read_only, transactional_ddl: Option<bool>

db.SchemaMeta
  name: str
  owner: Option<str>
  visible, system: bool

db.TableMeta
  schema, name: str
  kind: MetaTableKind
  native_kind, owner, comment: Option<str>
  estimated_rows: Option<f64>

db.ColumnMeta
  schema, table, name: str
  ordinal: i64
  logical_type, native_type: Option<str>
  native_type_id: Option<i64>
  nullable: MetaNullability
  default_sql, generated_sql, identity_kind, collation, comment: Option<str>
  origin_schema, origin_table, origin_column: Option<str>

db.KeyMeta
  schema, table: str
  name: Option<str>
  kind: MetaKeyKind
  key_ordinal: i64
  term_ordinal: i64
  local_column, referenced_schema, referenced_table, referenced_column, expression: Option<str>
  match_policy: Option<MetaForeignKeyMatch>
  on_update, on_delete: Option<MetaReferentialAction>
  deferrable, initially_deferred, validated: Option<bool>

db.IndexMeta
  schema, table, name: str
  unique, primary_backed: Option<bool>
  term_ordinal: i64
  term_kind: MetaIndexTermKind
  column, expression, predicate, native_method, native_opclass: Option<str>
  sort_order: Option<MetaSortOrder>
  null_order: Option<MetaNullOrder>
  valid, ready: Option<bool>

db.QueryMeta
  query_id: str
  driver: Driver
  driver_restriction: DriverRestriction
  statement_class: MetaStatementClass
  artifact_digest: str
  state: MetaQueryState
  metadata_fingerprint: Option<str>
  source_sql_hash, driver_wire_sql_hash: str
  rewrite_format_version: i64
  prepare_identity, schema_identity, server_identity: Option<str>
  entry: MetaQueryEntry
  ordinal: Option<i64>
  source_name, source_alias, logical_type, native_type: Option<str>
  native_type_id: Option<i64>
  origin_schema, origin_table, origin_column: Option<str>
  nullable: MetaNullability

db.QueryPlan
  driver: Driver
  format: PlanFormat
  analyzed: bool
  body: str
```

#### 18.2.1 detail projection、row、ordinal

selected detailの外にあるoptional fieldは `None`。selected detailでもengine evidenceが
unavailableなら `None`。empty text、zero、`false` は実際のreported valueでabsence sentinelに
しない。record discriminatorにinapplicableなfieldはFullでも `None`。required evidence
enumはevidenceがsuppressed/unavailableならexplicit `Unknown`/`Native` stateを使い推測しない。
required identityは全detailで存在する。matrixでexplicitに指定したoptional identityは
全detailで要求し、engineがvalueを公開しない場合だけ `None`。結果は下記category keyの順に
`str` byte-lexicographic orderを使い、engine/catalog iteration orderを外へ出さない。

| Category | 全detailのrowとrequired field | `Names` | `Summary` | `Full` |
|---|---|---|---|---|
| `DatabaseMeta` | exactly one; `driver`, `name`, `engine_version` | optionalは全て `None` | `default_schema`, `read_only`, `transactional_ddl` を要求 | Summaryに `encoding`, `collation` を追加 |
| `SchemaMeta` | selected schemaごとに1行、`name`順; `name`, `visible`, `system` | `owner = None` | `owner` を要求 | common fieldはSummaryと同じ |
| `TableMeta` | table/viewごとに1行、`(schema, name)`順; `schema`, `name`, `kind` | optionalは全て `None` | `native_kind`, `owner`, `estimated_rows` を要求 | Summaryに `comment` を追加 |
| `ColumnMeta` | columnごとにphysical/result順; `ordinal`はzero-based; `schema`, `table`, `name`, `ordinal` | optionalは全て `None`; `nullable = Unknown` | `logical_type`, `native_type`, catalog-column `nullable` を要求 | Summaryに `native_type_id`、default/generated/identity/collation/comment、view-originを追加 |
| `KeyMeta` | key/constraint termごとに1行、`(key_ordinal, term_ordinal)`順; 両ordinalはzero-based; schema/table, `kind`, ordinal必須; optional identity `name`は全detailで要求 | `name`だけ `Some`になり得て他optionalは `None` | Namesにlocal/referenced nameと `expression` を追加 | Summaryにmatch/update/delete、deferral、validation evidenceを追加 |
| `IndexMeta` | termごとに1行、key termをinclude termより前にして`(name, term_ordinal)`順; 全体の`term_ordinal`はzero-based; identity, ordinal, `term_kind`必須 | optionalは全て `None` | unique/primary backing、column/expression/predicate、sort/null orderを要求 | Summaryにnative method/opclass、valid/ready evidenceを追加 |
| `QueryMeta` | 下記ordering/discriminator rule; Query/driver identity、class、artifact/state、source/wire hash、rewrite version、`entry`は全行必須 | `Summary` 1行だけ | exactにSummary、全Parameter、全Columnの順; source name/aliasとlogical type | 同じrow groupにchecked native/origin/nullabilityとprepare/schema/server evidenceを追加 |

`ColumnMeta.nullable` は要求時のcatalog column declarationで、Query result nullabilityを
証明しない。NamesおよびSummary/Fullでcatalog evidenceがunavailableなら `Unknown`。
`QueryMeta.nullable` は§16.3.1に従う。

constraint nameはoptionalかつuniqueと仮定しない。`None` はengineがnameを公開しない意味で
synthetic nameを作らない。tableごとにcomplete common key groupを作り、
`(kind declaration tag, name Option tag/UTF-8 bytes, ordered term sequenceの全
local/reference/expression field, match_policy, on_update, on_delete, deferrable,
initially_deferred, validated)` からなるFull-detailの完全なsignatureでcanonical sortする。
enum/Option tagはdeclaration order、stringはbyte-lexicographic order、boolはfalseを先に
する。driverはsort前にgroup-level policy/evidenceを1値へnormalizeし、矛盾するengine
rowをrejectする。同順位になれるのはbyte-identicalな完全common groupだけなので、
physical orderはcommon outputへ影響しない。sort後にzeroから`key_ordinal`を付け、
groupの全termが同じ`key_ordinal`を繰り返し、`term_ordinal`はgroup内でzeroから始める。
Names/Summaryの
non-identity field suppressionはこのidentity/order計算後に行う。

`QueryMeta` discriminatorは次で固定する。

| `entry` | row presence/order | `ordinal` | applicable optional field |
|---|---|---|---|
| `Summary` | exactly one、常に先頭 | `None` | checkedならSummary/Fullで `metadata_fingerprint`; checkedかつFullだけ `prepare_identity`, `schema_identity`, `server_identity` |
| `Parameter` | Namesではabsent; Summary/Fullでdistinct source parameterごとに1行、protocol ordinal順 | one-based protocol ordinal（`$1` は1） | `source_name`, `logical_type`; Fullでchecked `native_type`/`native_type_id` |
| `Column` | Namesではabsent; Summary/FullでRow fieldごとに1行、decoder position順 | zero-based decoder ordinal | `source_alias`, `logical_type`; Fullでchecked `native_type`/`native_type_id`、structured origin、§16.3.1 `nullable` |

complete row orderはSummary 1行、protocol ordinal昇順の全Parameter、decoder ordinal昇順の
全Columnで、groupをinterleaveしない。Summary/Parameter entryの `nullable` は `Unknown`。
Column entryもSummary、Fullでchecked evidenceがunavailable/ambiguous、またはDeclaredの
全detailで `Unknown`。Parameterに `source_alias`、Columnに `source_name`、Parameterにorigin
fieldはinapplicable。`metadata_fingerprint` とprepare/schema/server identityはSummary
entryだけに置き、Parameter/Columnへ複製しない。Declared Queryはどのdetailでもchecked-only
fieldを持たない。

`artifact_digest` はこのQuery descriptor用にemitしたexact versioned D1
`StaticQueryArtifact` bytes（digest自体はbytesに含めない）に対する
`Hash128::of(...).to_hex()`（`lo`、`hi` の順）の32文字lowercase hexadecimal値。bytesは
Query identity、driver restriction、source SQL、static
Query option、Params/Row fingerprint、binder/decoder ABI version、全permitted driverの
wire SQL/rewrite/binding/checked-metadata entryを `Driver` enum順で含む。同じdescriptorの
driver-specific `QueryMeta` rowは同じdigestを繰り返す。runtime option、connection/secret、
requested `MetaDetail`、metadata output orderingは含めない。

multi-term key/indexはordered flat rowとして返す。key groupはnameがuniqueとは限らないため
`key_ordinal`、termは `term_ordinal` を使う。index key termはincluded termより前に置き、
`term_kind` で区別し、nested allocationを作らない。
QueryMetaはSummary 1行の後にparameter/column行をordinal順で返す。detail/engineにないfieldは
Option.Noneだがbase identityは常に存在する。`source_sql_hash`、`driver_wire_sql_hash`、
`rewrite_format_version` はDeclaredでもD1が全static Query/driverに生成するため常に存在する。

common declarationは全て明示destination `out: region` をoption sliceの直前に持つ。

```text
pub fn meta_database(
  exec: db.exec, detail: db.MetaDetail, out: region, options: slice<db.MetaOption>,
) -> Result<db.DatabaseMeta, db.Error>
pub fn meta_schemas(
  exec: db.exec, detail: db.MetaDetail, out: region, options: slice<db.MetaOption>,
) -> Result<array<db.SchemaMeta>, db.Error>
pub fn meta_tables(
  exec: db.exec, schema_filter: Option<db.SchemaRef>, detail: db.MetaDetail,
  out: region, options: slice<db.MetaOption>,
) -> Result<array<db.TableMeta>, db.Error>
pub fn meta_table(
  exec: db.exec, table_ref: db.TableRef, detail: db.MetaDetail,
  out: region, options: slice<db.MetaOption>,
) -> Result<db.TableMeta, db.Error>
pub fn meta_columns(
  exec: db.exec, table_ref: db.TableRef, detail: db.MetaDetail,
  out: region, options: slice<db.MetaOption>,
) -> Result<array<db.ColumnMeta>, db.Error>
pub fn meta_keys(
  exec: db.exec, table_ref: db.TableRef, detail: db.MetaDetail,
  out: region, options: slice<db.MetaOption>,
) -> Result<array<db.KeyMeta>, db.Error>
pub fn meta_indexes(
  exec: db.exec, table_ref: db.TableRef, detail: db.MetaDetail,
  out: region, options: slice<db.MetaOption>,
) -> Result<array<db.IndexMeta>, db.Error>
pub fn meta_query<P, R>(
  exec: db.exec, query: db.query<P, R>, detail: db.MetaDetail,
  out: region, options: slice<db.MetaOption>,
) -> Result<array<db.QueryMeta>, db.Error>
pub fn explain<P, R>(
  exec: db.exec, query: db.query<P, R>, params: P,
  out: region, options: slice<db.ExplainOption>,
) -> Result<db.QueryPlan, db.Error>
```

上はbodyless Align sourceではなくexact API signature notation。下のcall argumentは
syntax-checkする通常のpositional Align syntaxで、typeはbinding/declarationにだけ書く。

```align
schema_filter: Option<db.SchemaRef> = None
table_ref: db.TableRef = db.TableRef { schema: "main", name: "users" }
q := query()
database: db.DatabaseMeta = db.meta_database(exec, detail, out, [])?
schemas: array<db.SchemaMeta> = db.meta_schemas(exec, detail, out, [])?
tables: array<db.TableMeta> =
  db.meta_tables(exec, schema_filter, detail, out, [])?
table: db.TableMeta =
  db.meta_table(exec, table_ref, detail, out, [])?
columns: array<db.ColumnMeta> =
  db.meta_columns(exec, table_ref, detail, out, [])?
keys: array<db.KeyMeta> =
  db.meta_keys(exec, table_ref, detail, out, [])?
indexes: array<db.IndexMeta> =
  db.meta_indexes(exec, table_ref, detail, out, [])?
query_meta: array<db.QueryMeta> = db.meta_query(exec, q, detail, out, [])?
plan: db.QueryPlan = db.explain(exec, q, params, out, options)?
```

対応する `sqlite.meta_*_native` / `postgres.meta_*_native` はcommon sliceとdriver-native
option sliceを別々に受け、同じoutを先に受ける。optionless/hidden-heap overloadはない。
全string/array/bodyをnative result解放前にoutへcopyし、connection/row bufferをborrowしない。
arrayはL6 builderの1 compact passを使う。meta_tableの欠落はNotFoundでありpartial recordや
Optionを返さない。

`SchemaRef` と `TableRef` はmetadata callが返るまでだけborrowされるCopy view inputで、
driverは保持しない。`SchemaRef.name` はexact engine schema/attached-database名を選ぶ。
`meta_tables` の `None` はaccessibleなnon-system schema全てを意味し、system schemaは
explicit optionが要求した場合だけ含む。`TableRef` は常にexact schema/nameを持つため、
PostgreSQL search pathやSQLite `main`を推測しない。identifierはdriver metadata APIで
bind/escapeし、SQLへ文字列連結しない。

`SchemaRef.name`、`TableRef.schema`、`TableRef.name` はdriver/native metadata call前に
U+0000を検査する。rejectは
`db.Error.Encode(db.ContractError { query_id: None, item, message:
"metadata identifier contains U+0000" })` を返す。対応する `item` はexactに
`"metadata.schema"`、`"metadata.table.schema"`、`"metadata.table.name"` とする。このerror
ではSQL/catalog requestを送らない。
validationはpublic record declaration順。`SchemaRef` は `name`、`TableRef` は `schema`
の後に `name` を検査する。TableRefの両componentがU+0000なら
`item = "metadata.table.schema"`。両driver testでdual-invalid precedenceを固定する。

### 18.3 database

database名、driver、engine version、encoding/collationなど要求した基本情報だけ。

### 18.4 schema

PostgreSQL schema/search path、SQLite attached databaseを共通categoryへ写し、native detailを
別に保持する。

### 18.5 table/view

kind、qualified name、必要なvisibility/native flags。全column/indexを暗黙に展開しない。

### 18.6 column

ordinal、name、declared/native type、nullability evidence、default/generated/origin evidence。

### 18.7 key/constraint

primary、unique、foreign key、check/exclusionなど、engineが提供する意味を失わず共通部分と
native detailを分ける。constraint nameはoptionalで、absent/duplicateでも `key_ordinal` が
composite term groupを保持する。

### 18.8 index

name、unique、key/include column、predicate/expression、method/opclass/originなどを要求scopeで
取得する。

### 18.9 Query metadata

static Query id、parameter/result contract、driver restriction、checked evidence、artifact digest、
statement classificationを返す。runtime SQL reflection結果と混同しない。

### 18.10 query plan

`db.explain(exec, query(), params, out, options)?` は明示のinspection-only操作。
`postgres.explain_native(..., [postgres.ExplainOption.Analyze, ...])` はstatementを実行し
execution-countへ入る。read-only plan取得のように見せない。

### 18.11 PostgreSQL native metadata

OID、domain/base type、array/range/composite、opclass、index method、JSON planなどを
driver-qualified dataで提供する。

### 18.12 SQLite native metadata

STRICT、WITHOUT ROWID、hidden/generated column、index origin/partial、query-plan/bytecode
detailなどをdriver-qualified dataで提供する。

## 19. Dynamic SQL escape hatch

typed Queryがprimary pathである。runtime SQLが本当に必要な場合だけ、別surfaceで明示する。

```text
db.dynamic_execute(exec, db.Driver.SQLite, sql, params, execute_options)
db.dynamic_rows(exec, db.Driver.SQLite, sql, params, out, execute_options)
  -> db.rows<db.row>
db.row.get(index) -> db.value
```

dynamic APIはtyped Query descriptor、checked artifact、struct decodeを装わない。SQL string、
exact `db.Driver` restriction、parameter value slice、result access、末尾の
`slice<db.ExecuteOption>` がsourceに見える。restrictionは `exec` から推測せず、SQL送信前に
handleと比較してmismatchをerrorにする。driver-native formは
`sqlite.dynamic_*_native` / `postgres.dynamic_*_native` のmodule path自体をexact restriction
とし、common/native option sliceを別々に受ける。`db.value` は
ownedかself-containedな最小sumとし、native pointerを保持しない。name-based struct writeや
runtime reflectionは導入しない。exact surfaceはD14で実装するが、typed Queryをdynamic
engine経由で実装してはならない。

## 20. Concurrency、cancellation、timeout

connection/tx/stmt/rows resourceは既定でtask間共有しない。driverのthread ruleをresourceの
sendabilityへ正確に反映し、`spawn` へ入れない型をlintではなくtype/ownershipで拒否する。

timeoutはD9のexact operation-scoped `TimeoutNs` / `BeginTimeoutNs` optionで扱う。要求deadlineは
enforceするかSQL送信前にrejectし、silent ignoreしない。expiryはdriver-owned native
interrupt/cancel machineryを使い、hidden SQLを発行せず `Timeout` へnative detail付きで
mappingする。local deadline以外のengine-reported cancelだけを `Cancelled` とする。
PostgreSQL v1はnonblocking libpq waitとnative cancelを使う。SQLite v1はcommon operation
deadlineをSQL送信前に `Unsupported` とし、native `BusyTimeoutNs` はlock waitだけを制御して
whole-query deadlineを装わない。後続SQLite deadlineにはgeneral noncapturing C-callback
boundaryまたは別途証明したnative mechanismが必要で、DB-specific compiler例外を作らない。
L3 resource/refはnon-Sendで同期executionにsoundなconcurrent callerがないため、
v1にexternally shareable cancel resourceはない。user-triggered cancel handleにはgeneral
Send/thread-safe-resource前提と専用roadmap sliceを先にscheduleする新しい具体的proposalが
必要で、accepted v1 designやD9から暗黙には生えない。
timeout/cancel後はrequired resultをdrainしてprotocol/transaction stateの同期を証明できた
connectionだけを再利用可能とする。それ以外はpoison/closeし、state不明のconnectionをpoolや
callerへ返さない。transparent retryはない。

D13 `pkg.db.pool` は明示的なfixed-capacity moduleである。acquisitionはnon-waiting
`try_acquire` のみで、exhaustionを即時に返しacquisition timeoutを持たず、constructorがexact
sizeを公開する。acquired `db.conn` はtransaction conversionからDropまでphysical connectionと
pool affinityを保持し、operationが黙って別connectionへ移さない。

## 21. Performance contract

### 21.1 必須invariant

- generated binder/decoder。per-row reflection/name lookupなし。
- 1 Query = 1 visible statement = 1 execution。
- `one`/`maybe_one` は2 delivered rowsでdecodeを停止し、driver delivery costを別に測る。
- row viewは可能な範囲でzero-copy、retention copyは `clone_in` に見える。
- parameter bind retention/copy classとcopied bytes/allocationを可視・計測可能にする。
- materialization allocation先は明示region。
- region builderはhidden heapなし、compact passは正確に1回。
- all handle Drop/finalize/rollback/closeは成功/全error pathでexactly once。
- execution count、allocation/copy、prepare count、metadata query countをtest hookで測れる。

### 21.2 1 statementが常に最速とは限らない

巨大Cartesian JOINより複数の明示Queryが速い場合は、applicationが可視な複数呼び出しを選べる。
packageが自動でsplit/追加SELECTしない。native aggregation、batch、COPY/SoAは測定して
driver-qualified pathとして追加する。

### 21.3 local measurement anchor

次のnamed local measurementを保つ。named pathが最初にlandする時、実質的に変わる時、または
明示的なperformance investigationで実行する。regression、integration、PR、release、
milestone gateではない。

- generated binder/decoder対hand-written native loop;
- SQLite package path対direct libsqlite3;
- SQLite streamed text/blob transient bindのcopy bytes/allocation;
- PostgreSQL package path対direct libpq;
- file/inline Query/command artifact generationとcold/warm rebuild;
- structural contract/artifactとQueryMeta plan（1/10/100 reachable definitions）;
- canonical checked-metadata JSON encode/decode（10/100/1000 columns）;
- SQLite canonical migration catalog fingerprint/replay（10/100/1000 files）;
- SQLite active-execution lease acquire/release/rejected-overlap;
- prepare reuse対reprepare;
- rows iteration/decode;
- region builder push/freeze/copy count;
- one-to-many one-pass shaping;
- metadata categoryごとのquery count/latency、destination region bytes/compact count、
  native-buffer copy bytes;
- batch/SoA/native path。

### 21.4 execution-count test

各Query実行にtest hook/counterを置き、Query-local runとcompound shaperが正確に1回だけ
executeし、hidden follow-upが0であることを固定する。hook自体はproduction semanticsを
変更せず、driver call境界で数える。

## 22. Diagnostic

diagnosticは `.align` と `.sql` の両spanを使い分ける。

```text
missing Params field       SQL parameter span + Params declaration
unused Params field        Params field span
duplicate/mixed parameter  SQL occurrences
Row column mismatch        Row field + checked metadata/ordinal
multiple statements        second statement SQL span
stale metadata             Query declaration + artifact path/reason
wrong driver               Query restriction + connection origin
old row view after next    view origin + mutable next call
dependent child alive      child construction + attempted parent move/drop
unsupported option         exact option constructor + operation scope
```

path/source hashだけの内部値をuser-facing identityにしない。Query module名を主に表示する。
malformed artifact/interfaceはpanicやfail-openではなくdiagnosticでfail closedする。

## 23. Roadmap

実装は少数の前提capability PRとvertical capability PRで進める。roadmap labelは
acceptance closureのownerであり、labelごとに別PRを要求しない。database PRが不足する
前提をpackage名special caseで代替してはならない。

以下のD1〜D14 contractは変更しない。deliveryはusefulなconsumer outcomeでまとめる。

| Wave | Acceptance owner | Mergeable outcome | Default publication boundary |
|---|---|---|---|
| P0 native evidence | D0 | public APIなしでSQLite/libpq behaviorを記録 | product PRにせず前提実装と並行 |
| Q1 static Query | D1 | generated Query/commandがfake driverでend-to-end実行 | 1 capability PR |
| Q2 dual-driver scalar | D2 + D4 | 同じscalar Query/command surfaceがSQLite/PostgreSQLで動く | 1 coordinated capability PR |
| Q3 checked/offline parity | D3 + D5 | 両driverが1つのoffline checked-metadata/invalidation contractを共有 | 1 coordinated capability PR |
| Q4a reusable execution | D6 + D7 | prepared statementとtransactionが1つのreusable execution/ownership modelを共有 | Q2後、Q3と並行する1 capability PR |
| Q4b streaming resilience | D8 + D9 | typed streaming、deadline、cancellation、cleanupが1つのresilient lifecycleになる | Q4a後の1 capability PR |
| Q5 schema tooling/inspection | D11 + D12 | migrationとread-only metadata/EXPLAINでschema-facing productを完成 | mutationとinspectionは独立failure domainなので2 parallel capability PRを許可 |
| Q6 compound product | D10 | many-to-one/one-to-many Outputを1 executionでend-to-end実行 | Q4b後の1 capability PR |
| A1 throughput/native train | D13 | batch/SoAとdriver-native throughput surface | independently usefulなcommon/driver railを並行merge可能 |
| A2 dynamic/callback train | D14 | dynamic rowとproved native callback | dynamic SQL/driver callback railを並行merge可能 |

Q1〜Q4b/Q6は内部D labelで分割しない。両側がend-to-end実行し、独立に有用で、同じmatrix、
review、broad gateを繰り返さない時だけ分割する。Q5はmigrationがexternal stateを変更し、
metadata/EXPLAINがread-onlyであるため意図的な例外とする。A1/A2はadditive release trainで
あり、独立native railを直列化しない。ただしcomplete-roadmap statusはD13/D14の全
acceptance cellを待つ。

A1のdefault railはcommon batch/SoA、PostgreSQL native throughput、SQLite native service、
explicit poolの4つとする。A2はdynamic SQL/value/row railの後、独立にproveしたSQLite/
PostgreSQL callback railを進める。rail内は複数commitを使えるが、useful surfaceがstableに
なった時にreviewとselected broad gateを1回だけ行う。未指定の追加driverはcompletionに
必須ではなく、common contract実証後にconsumer-backed railとして追加する。

active implementationでは8時間でcompiling focused-owner-backed source checkpoint、24時間で
whole capability PR-ready、またはindependently usefulなA1/A2 railを残す。達しない場合は
dominant costを記録し、最寄りのconsumer boundaryで区切り直す。dormant seam、document拡張、
repeated broad review、変更pathと無関係なbenchmark/full suiteで埋め合わせない。reviewと
broad verificationはstable wave candidateで1回だけ行い、public contractが変わる時だけ
documentを更新する。

### L1a〜L7 — 必須Align前提

詳細scope、file、acceptanceは
[`../../17-library-boundary-prerequisites.md`](../../17-library-boundary-prerequisites.md)。

```text
L1a recursive DropPlan framework + Option<string> field
L1b Move sum/Option/Result payload completion
L2a parameter modeとborrow/region summaryの表現およびinterface identity
L2b recursive parameter/capture return provenanceとfunction-value join
L2c cleanup ABI recordとrecursively Move returnのdynamic bit
L2d stable bound Copy/Move storageへのshared borrow
L2e borrow mut/out、all-peer alias、Copy/Move replacement、Pure shaping
L3  opaque/dependent resource + linkable Drop thunk + resource_ref/native view
L4  named arena region + clone_in
L5  deterministic tagged file/inline input + one-item Query/command identity/artifact
L6  RegionPlain region array_builder
L7  nested generic package API + closed structural RegionPlain bound
```

依存DAGに従い、独立なL3/L4/L5を直列化しない。全L1a〜L7 gateの完了前にsafe DB driver
APIを始めない。L7により通常package codeで
`rows_stmt<P,R>`、`all<P,R: RegionPlain>`、`query<P,R>`、`rows<R>`、`array<R>`を表現できる。

### D0 — native feasibility probe

production APIを作らず、SQLite pointer validity、libpq full/single-row result、extended
protocolの1 statement性、parameter/result metadata、nullability evidence、cancel/cleanupを
実測して記録する。

#### D0実測 — 2026-08-07

Q2 author probeはApple Silicon macOS 26.5.2でconsumed C signatureを
`-Wall -Wextra -Werror`によりcompileし、Homebrew SQLite 3.53.3とlibpq 18.4をPostgreSQL
18.4に対して実行した。選択したarm64 dylibはcompatibility version 9の
`libsqlite3.0.dylib`とversion 5の`libpq.5.dylib`だった。unqualified local `pkg-config`は
SQLite 3.51.0を報告した一方、明示選択したHomebrew libraryは3.53.3だったため、Q2は
discovery metadataから推測せずlinked runtime versionをtest/reportする。
libpqのTLS依存はsupported targetでlibssl/libcryptoへ解決するため、generated package link
contractではこのdependency closureをtransitive linker任せにせず明示する。`pq`を含む最終link
では、unitごとのfirst-seen discoveryに依存せず、driverが`pq`、`ssl`、`crypto`、続いて
supportedな`zstd`/`z`の順に正規化する。

SQLiteでは`sqlite3_prepare_v3` tailが最初のterminator直後のbyte 74を指し、comment付き
tailを再prepareするとsecond statementになった。従ってcomplete tailを走査しwhitespace/comment
だけを許す。2-row resultは`INTEGER, NULL, INTEGER, TEXT`を報告し、base columnはdeclaration/
originを持つがexpressionは持たなかった。current-row text pointerは同じrowのread中だけ保持され、
次のstepでsecond row用に同じaddressがreuseされた。`step`、`reset`、`finalize`は全て失効境界であり、
Q2は次のstep前にscalarをcacheしnative pointerを保持しない。cross-thread
`sqlite3_interrupt`はactive stepを`SQLITE_INTERRUPT` (9)にし、finalize後の同じautocommit
connectionで`SELECT 42`が成功した。

libpqでは`PQexecParams`が`SELECT $1::bigint; SELECT 2`を`PGRES_FATAL_ERROR`/SQLSTATE
`42601`で拒否した。buffered row bytesはowning `PGresult`をclearするまで別result取得後も有効で、
single-row modeでもfirst `PGRES_SINGLE_TUPLE` resultを保持したままsecond resultを取得すると
first bytesは有効だった。terminal resultは0 row、同じ2 fieldの`PGRES_TUPLES_OK`だった。
base `bigint`はOID 20、nonzero table OID、attribute ordinal 1を返し、expressionはtable/attribute
zeroだった。`PQgetisnull`はruntime NULLを返すがordinary result APIに完全なdeclared-nullability
factはないため、D5がorigin/catalog evidenceを結合しproof不能時はfail closedする。
`PQcancelBlocking`成功後、drainしたresultは`PGRES_FATAL_ERROR`/SQLSTATE `57014`となり、全result
clear後のidle connectionで`SELECT 42`が成功した。Q2はsynchronous cleanupを所有し、後のpublic
cancellationはcomplete drain後だけconnectionを再利用する。

### D1 — fake driver上のgenerated Query/command

#### Q1/D1 implementation closure matrix

Q1は1つの実行可能capabilityである。public package declaration、compiler-produced
artifact、generated binder/decoder plan、QueryMeta plan、fake-driver consumerは1つのdescriptor
ABIとcache identityを共有する。最初のconsumerより前でproducerを分割すると、実行不能な
static valueを公開するか、同じstructural-contract proofを重複させる。
このcapabilityは約1,000 hand-written lineのthresholdを意図的に超える。descriptor、artifact、
runtime data、最初のconsumerを一体にするとdormant seamをなくし、1つのABI/cache boundaryを
一度だけproveできるため、producer-only PR間で同じproofを繰り返すよりintegration riskが低い。

| Closure cell | 必要なimplementation closure | Exact owner evidence |
|---|---|---|
| public source surface | exact generic signatureを持つ`pkg.db`、`pkg.db.sqlite`、`pkg.db.postgres`のQuery/command descriptorとD1 option sumをcheck inする。constructorはcomplete single-expression descriptor bodyのままでraw/native stateを公開しない。file constructorはcompiler-owned constructor signature ruleによりimplicit sibling pathとleading explicit relative pathの両方を受理する。 | `pkg_db_q1::public_surface_whole_and_per_unit`、`pkg_db_q1::file_constructors_accept_explicit_paths_on_the_shipped_surface` |
| typed semantic descriptor | concrete generic return typeからQuery Params/Rowとcommand Paramsを解決し、post-compaction HIR identityを保持し、literal static optionだけをdecodeする。wrong kind/arity、command Row contract、unresolved type、duplicate/conflicting option、runtime option valueはpublication前にrejectする。 | `pkg_db_q1::typed_descriptor_contract_matrix`、`pkg_db_q1::static_option_rejection_matrix` |
| artifact formation | resolved static input、reachable structural contract、source identity/bytes、placeholder occurrence、SQLite identity wire SQL、PostgreSQL `$n` rewrite/span、binding ordinal/retention、declared metadata plan、ABI version、checked-metadata snapshotからexact versioned Query/command artifactを作る。current semantic snapshotは`DatabaseChecked`となり、stale Optional evidenceはDeclaredのまま、stale/missing Required evidenceはfailureとなる。byte publication前にvalidateする。 | `pkg_db_q1::artifact_semantics_and_checked_in_goldens`、checked Query/command metadata promotion test、独立なQuery/command byte/digest golden |
| generated runtime data | artifact formation時にcanonical type nameをclosed value tag、nullability、declaration-order field ordinalへ1回だけ解決する。artifact digest、driver別wire/bind plan、direct binder thunk planを含むproducer-owned immutable `ALIGNQST`/`ALIGNCST` descriptor dataをemitする。Queryだけがordinal decoder thunk planとDeclared QueryMeta materialization planを持つ。runtime field-name lookup、source/artifact I/O、map、dictionary、reflection、consumer-side generic instantiationは禁止する。 | `pkg_db_q1::generated_runtime_data_is_producer_owned`、`pkg_db_q1::fake_driver_query_and_command_end_to_end` |
| fake-driver execution | 同じgenerated binderでinline/sibling-file Queryとinline/sibling-file commandを各1つ実行し、admitted first-release scalar/nullable Query row shapeを全てdecodeし、1 executionをcountし、Query/commandを区別し、DBなしでbind/decode/cardinality errorを返す。 | `pkg_db_q1::fake_driver_query_and_command_end_to_end`のwhole-program/per-unit modeと`pkg_db_q1::scalar_bind_and_decode_shape_matrix` |
| interface, implementation, and cache identity | public Params/Row/restriction/static-option editはinterfaceを変える。SQL、rewrite、checked metadata、binder/decoder ABI、private descriptor editはunchanged public consumerをrecompileせずproducer implementation/artifact identityを変える。Query/commandは同じstatement-artifact identity ruleを使い、commandはRow/decoder/QueryMetaだけを省く。 | `pkg_db_q1::interface_impl_cache_invalidation_matrix` |
| fail-closed and Q1 ownership boundary | malformed SQL UTF-8/NUL/statement shape、malformed placeholder、unmatched Params field、unsupported field type、overflowed offset/count、malformed checked metadata、全artifact corruptionをcodegen/fake execution前にrejectする。Q1はfake inputをborrowしowned fake observationを返し、native resourceは所有しない。generated planはD2/D4 native cleanupを実行したと装わず、各future native driverのexact `BindValue`/`BindCopy` retentionを記録する。 | `pkg_db_q1::malformed_static_query_matrix`、`pkg_db_q1::inline_nul_diagnostic_points_at_the_exact_source_bytes`、fake-driver invalid-plan/error case、既存static-input/artifact corruption suite |

#### Q1 review finding-to-fix ledger

| Finding | Root-cause closure | Owner evidence |
|---|---|---|
| checked snapshotがDeclaredのままだった | unchanged L5 input snapshotと並べてparsed semantic recordを保持し、全artifact-bound fieldを比較し、promotion前にexact server/prepare identityをderiveする。Optional/Requiredは指定されたstale/missing branchを通る。 | `pkg_db_q1::checked_metadata_promotes_current_snapshots_and_obeys_policy_on_stale_data`、`pkg_db_q1::checked_command_metadata_promotes_without_query_evidence`、checked-metadata parser/revalidation suite |
| explicit file pathがordinary arity checkで失敗した | compiler-owned static-constructor signature ruleがexplicit file formだけにexact leading `str` parameterを挿入する。general overloadやpackage declarationの重複は導入しない。 | `pkg_db_q1::file_constructors_accept_explicit_paths_on_the_shipped_surface`、`align_sema::static_file_descriptor_preserves_explicit_decoded_path` |
| PostgreSQL escape string内をplaceholderとして誤認した | token boundaryの`E'...'`/`e'...'`をbackslash escapeとdoubled quoteを含むescape-aware opaque SQL tokenとして扱う。 | `static_artifacts::tests::scanner_keeps_postgres_escape_strings_opaque` |
| 全`WITH` statementをSelectに分類した | top-level CTE bodyを追跡し、recursive/multiple CTEを含め、final CTE後の最初のstatement keywordを分類する。 | `static_artifacts::tests::scanner_classifies_the_main_statement_after_ctes` |

author-side matrix-to-diff passでは、全runtime descriptor fieldをartifact producerとfake-driver
consumerへ対応付け、全accepted Params/Row field classをdirect binder/decoder ownerとmalformed
twinへ対応付ける。validated artifact identityなしでdescriptorが実行できる場合、またはgenerated
codeがreflection/name lookupへfallbackする場合はこのmatrixを再度開く。

D1 private runtime-plan prefixはexactであり、別配布のartifact codecではない。両recordは8 byte
magic（`ALIGNQST`または`ALIGNCST`）、`format_version: u32 = 1`、`u32` length UTF-8
descriptor ID、artifact `Hash128`（`lo`、次に`hi`）、static-option count/record、driver countで始まる。
static optionはowner `u8`、value tag `u8`、exact payload（Check policy `u8`、SQLite versionの
3つの`u32`、またはPostgreSQLの2つの`u32` length UTF-8 string）を格納する。driverはSQLite、
PostgreSQL順で、`u8` tag、`u32` length wire bytes、dense bind fieldを格納する。bind fieldは
Params ordinal `u32`、protocol ordinal `u32`、retention `u8`、shape
`(kind: u8, nullable: u8)`を格納する。Queryは同じshapeのdense decoder field、statement-class
`u8`、dense declared parameter row、dense declared column rowを続け、commandはdriver record後に
終わる。全text/byte fieldとsequenceはdescriptor MIR install前に`u32` boundedである。artifact
validationとruntime-plan formationはcodegen前に完了するため、emitted constantはtrusted producer
dataである。fake consumerもnon-dense ordinalとzero protocol ordinalをrejectする。

inline/sibling SQLからQueryとcommandのdescriptor/artifactを作り、exact source identityと
SQLite source/PostgreSQL `$n` wire entry・reverse span map、named occurrence table、
両kindのbinder/Queryだけのdecoder thunk、driver別 `BindValue` / `BindCopy`、flat scalar Row、
Query/command別interface/implementation/cache invalidationをDBなしで証明する。commandは
Row/decodeを持たず、それ以外のidentity/hash/checked/binding schemaを共有する。static
common/native Query/Command option sumもここで実装する。L5のversioned artifact schema通り
reachable definitionを含むstructural Params/Row contract/fingerprintとbinder/decoder ABI
versionをserializeし、independent byte/digest goldenとsame-path field
name/order/type/Option変更のinvalidationを固定する。producer-owned Declared QueryMeta
planを生成し、D12が最初のconsumerと同時にmaterialization thunkを追加する。separate compiled
Queryでruntime artifact/source I/Oなしをtestする。reflectionとper-row name lookupがないことをtest/IRで固定し、このpathが最初に
landする時または実質的に変わる時だけlocal measurementを行う。

#### Q2/D2+D4 implementation closure matrix

Q2は1つのdual-driver capabilityである。common `execute`/`one` surface、producer-owned
native descriptor ABI、connection resource、両driver consumerを一緒にlandする。SQLiteと
PostgreSQLを分けると、最初のdriverだけでcommon option、error、cardinality、descriptor、
cleanup behaviorを定義できてしまいportability peerが欠ける。descriptor consumerとnative
packageを分けると、compiler側に別のdormant seamを公開してしまう。このcapabilityは約1,000
hand-written lineを超える見込みだが、1つのcoordinated boundaryでdescriptor/thunk ABI、
resource cleanup、common error model、同一scalar surfaceを一度だけproveする方が、2つのdriver
PRとproducer-only bridgeにproofを重複させるよりintegration riskが低い。

Q1 canonical `ALIGNQST`/`ALIGNCST` bytesは変更しない。Q2はdescriptorの1つのcompiler-private
raw fieldが指すtarget-native、producer-owned `QueryStatic`/`CommandStatic` execution headerを
追加する。現在supportするDB targetは全て64-bitである。execution-header v1はalignment 8、
size 96で、exact layoutは英語originalのoffset 0 version、4 kind、5 mask、6 reserved、8 Q1
plan、16 ID、32 SQLite SQL、48 PostgreSQL SQL、64 binder、72 static validator、80 row validator、
88 decoder、96 endとする。両driver slotは常にSQLite/PostgreSQL順で存在し、mask外slotはexact
`(null, 0)`、present slotはnon-null/positive lengthでlength外にNUL sentinelを1つ持つ。
Queryは4 thunk全てを要求し、commandはbinder/static validatorだけnon-nullで他2つをnullにする。publication前に
mask/slot/kind/thunk/reservedをcheckし、consumerは最初のload前にversion/kindをcheckする。
mask 1/2/3についてoffset、relocation、null slot、alignment、total sizeをindependent
LLVM/object goldenで固定する。Q1 QueryMeta planはこのheader外のままにし、D12がnative metadata
consumerと同時にnew header versionとexact materializer ABIを追加する。

Binder ABI v1は`fn(context: raw, borrow params: P) -> i32`である。producerはcontextをopaqueな
call-scoped tokenとして扱い、exact package callback
`pkg.db.internal.bind_i64_v1(context, protocol_ordinal: u32, value: i64) -> i32`へ渡す以外に
load/store/retain/return/freeしない。packageがcontextを作成・所有し、private
version/driver/ordinal stateをcallback内でvalidateし、synchronous operation後にdestroyする。
全status callbackはsuccessでexact `0_i32`、context-owned first failureをrecordした後にexact
`1_i32`だけを返しnegative valueを返さない。generated thunkはcallbackの全nonzeroを`1_i32`へ
mapし、`-1_i32`をcompiler-generated unsupported-shape path専用に保つ。packageがnative cleanup前に
selected owned `db.Error`を1つmaterializeし、untaken recordはcontext Dropがfreeし、binderは
最初のfailureで停止する。

`P`はgeneral shared-borrow ruleによりCopy/Moveのどちらでもよい。generated callbackはcallerの
stable storageを読み、by-value aggregate copyを作らない。`P`またはQueryの`R`がQ2のclosed
non-null `i64` subset外のshapeを含む場合、`validate_static_thunk`はfield callback前にexact
`-1_i32`を返す。このreserved statusはcontext failure recordを選択せず、事前allocationも
行わない。packageはそのstatusを受け取った後にだけexact owned
`db.Error.Unsupported(db.ContractError { query_id: Some(id), item: "db.descriptor.shape",
message: "static database descriptor uses a field shape unsupported by this execution milestone" })`
をconstructする。supported executionはerrorをallocateしない。exact `1_i32`だけが
context-owned first-failure recordを選び、exact `-1_i32`だけが上記unsupported shapeを表す。
valid v1 thunkは他statusを生成しない。phase 6はlease/native send前にfailする。
Queryのbinder/row-validator/decoder pointerはnon-nullのままunreachableであり、commandの
row-validator/decoder slotはfixed headerどおりnullである。

Static-option validation ABI v1は`fn(context: raw) -> i32`でQ1 canonical sorted option順に訪れる。
artifact formationで閉じたcommon Checkにはruntime callbackを出さない。SQLiteはexact
`pkg.db.internal.require_sqlite_version_v1(context: raw, major: u32, minor: u32, patch: u32) -> i32`
を呼び、packageは`sqlite3_libversion_number()`をarithmetic overflowなしのcomponent tupleで比較する。
PostgreSQL i64はexact
`pkg.db.internal.set_postgres_i64_type_v1(context: raw, protocol_ordinal: u32,
canonical_type_name: str) -> i32`を呼ぶ。Q2はexact `int8`だけをacceptしてcall-scoped
`PQexecParams` type vectorへOID 20を記録し、他mappingはsend前にUnsupported。status/error/context/stop
ruleはbinderと同じで、compiler publicationがQ1 option/thunk agreementをvalidateする。

Query row-validation ABI v1は`fn(context: raw) -> i32`であり、最初にexact
`pkg.db.internal.validate_row_count_v1(context: raw, expected: u32) -> i32`、次にdeclared ordinal順で
exact `pkg.db.internal.validate_i64_v1(context: raw, ordinal: u32, expected_name: str) -> i32`を呼ぶ。
per-column callbackはexact UTF-8 name bytes、NULL、driver-native representation、full-range i64
parseの順にcheckし、success時だけparsed scalarをpackage contextへcacheする。0はsuccess、exact
1はbinderと同じcontext-owned first-failure recordを選びvalidatorは即停止する。Decoder ABI v1は
`fn(context: raw) -> R`で、successful validation後だけ
`pkg.db.internal.read_i64_v1(context: raw, ordinal: u32) -> i64`を呼びcached validated scalarを
direct field offsetへwriteする。validation/decodeでもcontextはcall-scoped/non-retainedである。
header/thunk/callback contract変更時はartifact ABIとexecution-header/callback versionをincrementする。

このopaque headerのprojection/thunk invocationはcompilerがvalidateした`pkg.db`所有generic
bodyだけに許可する。trusted operationはconcrete `P`/`R`とcomplete function signatureを持つ
明示的HIR/MIRであり、fixed header loadとindirect callへlowerする。name/reflection lookupでは
なくapplication sourceからは利用できない。backendはこのMIR contractをlowerするだけであり、
SQLite/PostgreSQL option、connection、error、lease、bind、step/result、cleanup semanticsは
ordinary first-party Align package codeとdirect `sqlite3`/`pq` FFIに置く。`align_runtime`へDB
semantic helperやhandleを追加しない。

| Closure cell | 必要なimplementation closure | Exact owner evidence |
|---|---|---|
| public common surface | §4/§6/§13/§15のexact `db.conn`、`db.exec`、`db.Driver`、`db.exec_result`、structured owned error、`db.ExecuteOption`、`exec_conn`、`execute`、`one` shapeと、common/native option sliceを分離したSQLite/PostgreSQL `execute_native`/`one_native`を定義する。Q2 Rowがi64だけでも`one`のexplicit output `region`を維持する。`db.exec`はsettled conn/tx sum shapeを保ち、Q2ではD7までconstruct不能なtransaction armをnative work前にrejectする。 | package whole/per-unit interface golden、両driverのcompiled common/native command/portable Query、wrong option scope/arityとescaped execution viewのcompile-fail |
| namespaced builtin type identity | `db.Error`をrenameせず一般type lookupを直す。closed alias/explicit tableは `Error`/`core.Error`（常時利用可能なsyntactic core）、`argon2_params`/`crypto.argon2_params`（`import std.crypto`）、`regex_match`/`regex.regex_match`（`import std.regex`）。non-entry same-module declarationをbare lookupが優先し、local missはaliasへfallbackする。exact explicit spellingはbuiltin identityを保持し、std-qualified type useはunused-import lintを満たす。`error(c)`は常に`core.Error.Code(c)`を対象とし、entry canonical collisionはrejectする。DB名のspecial caseは作らない。 | 3 aliasすべてのparameterized local signature/constructor、qualified importer construct/match、exact explicit builtinとmissing-import negative、bare builtin fallback、local Error下の`error(c)`、entry reject、whole/per-unit interface/executable parity |
| native descriptor ABI | exact Q1 plan bytesとfixed 96-byte v1 layoutを保ちdescriptorごとに1つのheaderをemitする。publication前にversion/reserved/kind/mask/slot/thunkとQ1 static-option agreementをvalidateしone-pointer Copyを保つ。Q2はdormant QueryMeta pointerをemitしない。 | masks 1/2/3 header/relocation、ABI/null/size/alignment、kind omission、static-option thunk、QueryMeta absence、runtime selection、whole/per-unit |
| generated binder/decoder | Query/commandのdirect ordinal `i64` binderとQueryだけのvalidation/decoder thunkを生成する。binderはcontextをopaqueに扱い`bind_i64_v1`だけを呼び最初のfailureで停止する。validationはexact count、declared-order UTF-8 field-name bytes、NULL、driver-native `i64` representationをdecoder前にcheckする。decoderは`read_i64_v1`だけを使いname/map/boxing/reflectionなしで`R`をconstructする。Q2未対応shapeはsend前にfailし後続mappingはD8所有。 | 両driverのportable bind/validate/decode、repeated PG ordinal、same-typed alias reorder/name/type/count twin、callback version/context misuse rejection、unsupported no-send、thunk MIR/LLVM inspection |
| type and monomorph closure | generic instantiation、function-value signature、interface serialization、whole-program/per-unit compilation、cache/implementation identityを通じてconcrete `P`/`R`を保つ。unresolved type、wrong header kind、absent decoder、mismatched thunk signatureをMIR/codegenへ到達させない。 | whole/per-unit Query/command execution、generic mono-key/header-signature golden、malformed HIR/MIR/header rejection owner |
| connection formation and ownership | SQLite/PostgreSQL descendantがinput/optionsをvalidateし、1つのphysical native connectionと1つのtagged package stateを作りroot `db.conn`をconstructする。ownerのmove/return/replacementでstateは1つのまま、`db.exec`はgeneration-checked `resource_ref`だけを持つ。source nulling、branch/loop join、early `?`、Dropでclose/free exactly once。 | construction、move-in/out、return、replacement、malformed null、early return/`?`、branch/loop join、use-after-move、whole/per-unit producer Drop thunk linkageのresource owner matrix |
| common validation precedence | §13.4 exact phase、つまりfail-closed header/identity、common/native option、restriction、connection state、static option、leaseを適用する。malformedはidentityを読まず全errorより先、timeoutはmismatch/closedより先、mismatchはclosed/static/overlapより先、closedはstatic/overlapより先。losing phaseはstate取得/sendなし。 | pairwise winner、source-order twin、malformed no-identity/embedded-deref、no-send/lease/state、error allocation/drop |
| static descriptor options | valid matching open connection後かつlease/send前にgenerated static-option thunkを呼ぶ。Common CheckはQ1 artifact-timeで閉じる。SQLite versionとPG i64 exact int8/OID20をenforceしunsupported mappingをsend前にfailする。 | Check no-call、SQLite version boundary/large-u32/no-send、PG absent/int8/int4/reorder/repeated OID、malformed agreement、whole/per-unit |
| SQLite connection options | §11.2の全option/default/conflict/NUL/PRAGMA/capability/setup ruleを実装する。positive nsはoverflow-safe ceilingでnative msへ変換し、`1..=1_000_000` nsは1 ms、`i32::MAX` ms超はSQLite前に`Unsupported`。flag/PRAGMAをdegrade/ignoreしない。 | duration 1/1_000_000/1_000_001 ns、`i32::MAX` ms、overflow、option/multi-invalid、open/setup、PRAGMA、capability、failed-open cleanup |
| SQLite execution lease/options | bind/timeout/native前にleaseを取得する。BusyTimeoutNsはpositive/uniqueでconnectionと同じceiling/overflow ruleを使いtracked native-ms valueを一時置換し、全return/Drop pathでrestoreする。second operationはfirst stateへ触れる前にfailし、restore failureはpoison/close。first operation errorを保存し、cleanup/restore failureは先行errorがないsuccessだけを置換する。 | duration boundary、overlap、apply/restore、failed second、全cleanup、first/cleanup error precedence、poison、execution count |
| SQLite command/query lifecycle | oneは0 rowならCardinality、row0があればvalidate/cacheしinvalidなら即そのerror、valid後にsecondをprobeし存在すればCardinality、DONEならcached row0をdecodeする。row1はvalidateしない。全pathでfinalize exactly onceしfirst-error ruleを守る。 | 0/1/2+、malformed-first validation winner、valid-first/malformed-second Cardinality winner、step/validator/decoder count、command row、fault/cleanup/direct |
| PostgreSQL connection options | §12.2の全option/default/source-order conflict/SSL/target/parameterを実装する。positive nsへcross-version floorを適用し、`1..=2_000_000_000`は2秒、`2_000_000_001`は3秒。NUL/secret/client_encodingを守り、`CONNECTION_OK`後だけPG_UTF8をverifyする。 | pairwise/source-order、duration floor/overflow、URL/encoding/ambient、null/BAD/OK-mismatch precedenceとencoding-call count、exact error/close/no-execute、NUL/secret |
| PostgreSQL execution options/binding | Text `i64` bindingとexact baseline `postgres.ExecuteOption` validationを実装する。unknown/duplicate parameter name、Binary `i64`、unavailable result formatはsend前に`Unsupported`。repeated source nameは1つの`$n`をreuseする。synchronous callがreturnまでparameter transportを所有する。shared bytea codecもここで閉じる。Textは`\\x`とbyteごとのlowercase hex 2桁を生成し、recorded length外にNUL sentinelを置く。Binaryだけがraw bytesとexplicit lengthを公開する。D8まではbyteaをexecutable descriptor shapeにしない。 | format disposition、no-send counter、`CAST($1 AS BIGINT)` execution、repeated-placeholder、embedded zero/high byteを含むindependent Text/Binary bytea golden、parameter buffer allocation/free counter |
| PostgreSQL result/cardinality | `BufferedFull`でもSQLiteと同順に、0 row Cardinality、row0 validate/cache、invalidなら即error、validかつrow1存在ならCardinality、singletonだけdecodeする。row1以降はvalidateしない。CommandはPGRES_COMMAND_OKとPQcmdTuples ruleを守りclear exactly once。 | 0/1/2+、malformed-first/valid-first-malformed-second winner、validator/decoder count、name/NULL/type/range/count、command/affected/clear/direct |
| native error ownership | cleanup前にnative detailをexact owned `db.NativeError`へcopyしmessage parseなしでstable categoryへmapする。Native errorはQuery IDを持たない。`db.ContractError`だけがquery_idを持ち、trusted statementは`Some(id)`、Query-less inputまたはidentity trust前のinvalid headerは`None`。 | cleanup後field、SQLSTATE/SQLite、identity twin、malformed/Query-less error、allocation/drop |
| FFI/ABI and malformed input | SQLite/libpq declaration/status/pointer/length/destructor/libraryをpinする。descriptor rawはfirst header load前にnull-checkし、complete fixed headerをvalidateしてからembedded pointerをfollow/thunk invokeする。invalid length/text/header/thunkをnative side effect前にrejectする。 | D0、C signature、Rust/Align inventory、null/header/embedded-pointer malformed test、ASan等、x86_64/ARM64/macOS CI |
| native library dependency closure | native link contractを明示する。SQLiteは`sqlite3`、PostgreSQLは`pq`、supported libpq TLS closureは`ssl`と`crypto`を使う。common dispatchは両driver pathを保持し得るため、SQLite-only callでもELF上のordered closureをtransitive `DT_NEEDED` discoveryに依存せずlinkする。別unitが先に`crypto`を要求しても、最終linkは元のlistを保持し、`pq`/`ssl`/`crypto`/`zstd`/`z`のordered closure tailを追加してsuffix libraryのreferenceも解決する。 | `pkg_db_q2::common_surface_dispatches_to_sqlite_without_driver_cycle`のwhole/per-unit executableとordered link inventory、Linux required PostgreSQL integration |
| allocation parity | scalar connect/execute/one successはvisible connection/execution/native objectとPostgreSQL Text parameter storageだけをallocateする。per-row heap allocation、error allocation、runtime dictionary、artifact/source I/Oは禁止する。partial allocationごとにownerとcleanup edgeを1つ持つ。 | success/各injected partial failureのallocation/copy counter、DB runtime helperを含まないemitted-symbol inventory、package対direct driver measurement |
| required PostgreSQL gate | pinned provisioned `db-postgres` CI jobを追加する。jobはephemeral connectionを`ALIGN_DB_POSTGRES_URL`で渡し、`ALIGN_DB_POSTGRES_REQUIRED=1`を設定する。missing/unreachable configはfailureとし、同じportable Queryを両driverで実行する。local absenceだけは理由付きskip可。native library/server versionをevidenceとして表示し、URLはsource・artifact・logへ埋め込まない。 | missing URLのrequired-mode self-test、provisioned PostgreSQL job、portable dual-driver integration target、unconditional/required-mode skip branch不在 |

author-side matrix-to-diff passでは、全acquired native pointer、active lease、timeout override、
statement/result、parameter buffer、owned error stringを、success、各native error、cardinality exit、
early `?`、Dropそれぞれの1つのcleanup ownerへ対応付ける。header/thunk strategyを変える指摘、
second SQLite operationがconnection-global stateへ触れる経路、complete validation前のPostgreSQL
send、driver semanticsのruntime移動が見つかった場合はこのmatrixを再度開き、high-risk review
pathを要求する。

#### Q2 plan-review finding-to-fix ledger

| Finding | Root-cause closure | Owner evidence |
|---|---|---|
| driver maskでheader offsetが変わり得た | both ordered slotを常設したfixed 96-byte/8-aligned layout、canonical absence、thunk nullability、mask 1/2/3 relocation goldenで固定する。 | native descriptor ABI |
| binder `raw` context layoutが未定義だった | contextをopaque call-scoped tokenにし、generated codeはversioned package callbackだけを呼ぶ。packageだけがown/validate/error materialize/destroyする。 | binder/decoder、FFI/ABI |
| same-typed reordered aliasをordinal decodeできた | Query validation thunkで両driverのexact count/name bytes/order/NULL/native representationをdecode前に要求する。 | binder/decoder、result lifecycle |
| PostgreSQL command result semanticsがなかった | `PGRES_COMMAND_OK`、tuple rejection、`PQcmdTuples` parse/range-checkをclear前に固定する。 | PostgreSQL result/cardinality |
| native errorへ存在しないQuery IDを要求した | settled shapeを保ち、native errorはdetailだけ、contract errorだけがsubject時`Some(query_id)`を持つ。 | native error ownership |
| zero-allocation validationがowned ContractErrorと矛盾した | no-send/no-lease/native-stateは維持しrequired owned error allocationをcount/freeする。successだけno-error-allocation。 | common precedence、allocation parity |
| ns conversionがnative timeoutをdisable/overflowし得た | SQLite ms/libpq secへoverflow-safe ceilingしsigned native bound超をlibrary前にreject、全edgeをpinする。 | SQLite/PostgreSQL option |
| row-validation callback ABIがimplicitだった | exact count/i64 callbackをname/typeし、count/ordinal/name/NULL/native/parse order、shared first-failure status、validated scalar cacheを固定する。 | binder/decoder、FFI/ABI |
| Q2がunused QueryMeta pointerをcalling contractなしで予約した | dormant QueryMeta slotを除き、consumed static-option validatorをfixed 96-byte v1の第4 thunk位置に置く。Q1 metadata planはD12がfirst consumerとmaterializer ABIを導入するまでinertに保つ。 | native descriptor ABI、monomorph closure |
| multi-invalidのwinner ruleがなかった | connection/execution phase orderとsource-order option ownershipを§13.4でnormativeにしpairwise winnerを要求する。 | common precedence、driver connection option |
| PostgreSQL client encodingがUTF-8からdriftできた | direct/startup-option client_encodingをreserveしpackage UTF8を最後にappend、ambientを排除しreturn前にPG_UTF8をverifyする。 | PostgreSQL connection、FFI/ABI |
| option errorがuntrusted descriptor identityを必要とした | complete safety headerを先にvalidateし、malformedはquery-less exact error、後続errorだけがvalidated IDを使う。 | common precedence、FFI/ABI |
| static descriptor optionにQ2 consumerがなかった | generated static-option thunkとSQLite version/PG int8 callbackをlease/send前に追加し、Checkはartifact-time decisionのままにする。 | static option、descriptor ABI |
| libpqが1秒timeoutを2秒にre-interpretできた | ceiling後にdocumented cross-version 2秒floorを適用しboundaryをpinする。 | PostgreSQL connection |
| post-connect encoding mismatch errorが未定義だった | libpq error bufferに依存しないexact query-less Unsupportedを返してexactly-once closeする。 | PostgreSQL connection、native error |
| QueryMeta thunk ownershipがplan間でdriftした | producer planはD1、materializer ABI/codeとheader versionだけをD12へ移し全current ledger/planへ伝播する。 | descriptor ABI、D12 metadata |
| cardinality/row-validation precedenceがdriverでdivergeできた | row0 validate/cache、second detection、singleton decodeの順にし、first invalidをmultiplicityより優先してlater rowをvalidateしない。 | SQLite/PostgreSQL result |
| encoding checkがfailed connectionをhideできた | PQstatusを先にcheckしnon-OK native errorをown、CONNECTION_OK後だけPQclientEncodingを呼ぶ。 | PostgreSQL connection、native error |
| Q1 proseにdormant metadata thunk obligationが残った | Q1 capability/measurementをbinder/decoder/QueryMeta planへ統一しmaterializer codeはD12だけにする。 | D12 owner、current ledger |
| exact binder ABIがCopy `P`のborrowを必要としたがshared borrowはCopyをredundantとしてrejectした | shared borrowをstable bound Copy/Move placeへgeneralizeしpointer-to-caller-storage ABIとtemporary rejectionを保つ。source/function value/interface/generated MIR/whole/per-unit codegenへDB exceptionなしで同じruleを適用する。 | binder/decoder、monomorph closure、language borrow owner |
| Q2 scalar subset外のQ1 descriptorにpublish可能なexecution headerがなかった | unsupported `P`/Query `R`にphase 6の`validate_static_thunk`からreserved `-1_i32`を返しfield callback/lease/sendより先にfailする。そのstatus後だけpackageがexact `db.descriptor.shape` owned `Unsupported`をconstructし、supported successはerror allocationを行わない。Query thunkはnon-null/unreachable、command row/decode slotはnull。 | binder/decoder、descriptor ABI、unsupported no-send、supported success zero-error-allocation |
| global builtin Error reservationがmodule identityとsettled pkg.db.Error APIに矛盾した | non-entry bare lookupをlocal-firstにしexplicit core.Errorを残す。entry canonical collisionだけrejectしDB special caseを作らない。 | public common surface、namespaced builtin type identity |
| 最初のnamespace revisionがcore/interface contractと矛盾した | core EN/JAとL2 interface contractを更新する。imported unitのsame-spelled local definitionを受理しsemantic importをlocal-firstにし、producer entry collisionはpublication前にrejectする。 | namespaced builtin type identity、whole/per-unit parity |
| `core.Error`とcapability import ruleの関係が未定義だった | `core.Error`を常時利用可能なlanguage-syntactic-core pathとし、`import core`は不要かつ無効とする。std-owned explicit spellingは通常のimportを必要としunused-import lintのuseに数える。 | namespaced builtin type identity、explicit import owner |
| `error(c)`がlocal `Error`へtextual bindできた | sugarを`core.Error.Code(c)`のdirect constructionと定義しcolliding moduleでtestする。 | namespaced builtin type identity、error owner |
| general alias ruleにcomplete provider mapがなく`Error`しかtestしなかった | `Error`、`argon2_params`、`regex_match`のclosed tableをexact provider spellingとparameterized owner coverage込みで定義する。 | namespaced builtin type identity、core/std EN/JA |

### D2 — 最小SQLite vertical

- in-memory SQLite connection;
- i64をinsertする1つの `db.command<Params>`;
- i64を1つselectするsibling-file `db.query<Params,Row>`;
- Params/Rowはself-contained scalarだけ;
- `execute` と `one`;
- 0/1/2+ cardinality;
- structured SQLite primary/extended error;
- §11のexact SQLite connection/baseline execute option sumと全conflict/unsupported branch;
- connection-wide active-execution leaseとpre-native overlap rejection/cleanup;
- execution-count hook;
- 全pathでclose/finalize exactly once。

text view、all、stream、transaction、migration、dynamic row、metadata catalog、追加native
breadthは含めない。このpathが最初にlandする時または変わる時だけ、named local
measurementでdirect libsqlite3 loopと比較する。

### D3 — checked metadata core + SQLite

#### Q3/D3+D5 implementation closure matrix

Q3は1つのchecked/offline capabilityである。regeneration command、canonical metadata
codec、SQLite/PostgreSQL describer、normal-build consumerを一緒にlandする。片方のdriverを
分割するとshared path/identity/stale/diagnostic contractがportability peerなしで決まり、
writerとconsumerを分割すると安全に消費できないmetadataを公開する。このcapabilityは
1,000 hand-written linesを超える見込みだが、tool/native/codec/compiler境界をdriver別に
重複証明するより1回で閉じる方がintegration riskは低い。

既存v1 JSONと `ALIGNMIG`/`ALIGNSID`/`ALIGNSRV`/`ALIGNPRP` streamはexact public contractの
ままである。DBを開き明示migration directoryを列挙できるのは `alignc db prepare` だけで、
normal check/buildはderived metadata pathだけを読む。

| closure cell | required implementation closure | exact owner evidence |
|---|---|---|
| command/input grammar | §16.2のexact command、repeatable `--query`、`--check`、driver別environment formを実装し、invalid/duplicate/cross-driver inputをcompile/native work前に拒否する。 | `pkg_db_q3::prepare_cli_input_and_precedence_matrix` |
| regeneration/selection | reachable graphだけをregeneration modeでcompileし、descriptorをUTF-8 ID順にselectする。unknown/duplicate/excluded driver/path hash collisionはDB open前に拒否する。 | `pkg_db_q3::regeneration_ignores_missing_required_metadata_and_is_deterministic` と `pkg_db_q3::selection_rejects_unknown_and_duplicate_ids_before_native_open` |
| canonical codec | §16.3のone-line-LF JSONをproduction reader/writerと独立goldenで固定し、malformed/noncanonical inputはpanic/partial publicationなしで拒否する。 | `pkg_db_q3::checked_metadata_sqlite_query_and_postgres_command_goldens` とstatic-input malformed matrix |
| schema/server identity | exact `ALIGNMIG`/`ALIGNSID` とderived identityを実装し、migrationの全validationを最初のSQLite apply前に終える。 | `pkg_db_q3::schema_identity_goldens_match_an_independent_encoder` とmigration owners |
| SQLite describe | explicit targetだけをopenし、validated migrationをtransactionally applyし、descriptorごとの全`RequireVersionAtLeast`をlinked SQLite componentsとstatement preparation前に比較してから、exact wire SQLのcount/name/type/originを記録する。nullabilityはowned query-level evidenceがなければ`Unknown`。 | `pkg_db_q3::sqlite_native_prepare_describes_the_selected_query`、`pkg_db_q3::sqlite_prepare_enforces_static_version_options` と `pkg_db_q3::migration_catalog_validates_before_sqlite_open_and_applies_atomically` |
| PostgreSQL describe | library load前に全ambient `PG*`を拒否し、selected non-`PG*` env valueだけでUTF-8 connectionを開く。supported `ParameterType`は対応する`i64`または`Option<i64>` fieldだけをbinding protocol ordinalごとのexact `PQprepare` OID vectorへ写し、collision-free nameでprepare/describeしてOID/name/origin/search path/extensions/versionを記録する。unsupported mappingはstatement preparation前に失敗する。zero-column Queryはempty Rowと一致し、native result columnを持つcommandだけを拒否する。prepared state/result/connectionを全pathでexactly once cleanupする。 | `pkg_db_q3::postgres_rejects_ambient_connection_defaults_before_native_load`、`pkg_db_q3::prepare_rejects_unsupported_static_options_before_native_work`、`pkg_db_q3::postgres_native_prepare_describes_the_selected_query` とrequired PostgreSQL CI |
| publication/offline consumption | 全canonical recordをmemory形成してからsame-directory staging/atomic replacementを行う。`--check`は同じnative workを行うがwriteしない。1つのshared publication guardをwhole-programまたはmulti-unit build全体で保持してgeneration混在を禁止する。normal buildはcurrent evidenceだけを`DatabaseChecked`へ昇格しDB/network/env/directory scanを行わない。 | `pkg_db_q3::schema_identities_and_publication_are_exact_and_check_is_read_only`、`pkg_db_q3::generated_metadata_is_consumed_offline_and_stale_required_evidence_fails` とpublication lock owner |

Q3のchecked-in native evidence matrixは次である。

| Driver | required environment | owned observation | fail-closed rule | owner |
|---|---|---|---|---|
| SQLite | macOS arm64 Homebrew SQLite 3.53.3、Ubuntu CI system SQLite ABI 3 | prepare tail、parameter/result order/name、declaration/origin API、migration transaction、runtime library version | expression declaration/query nullabilityが得られなければ`null`/`Unknown`を記録しruntime storage/NULL validationを残す | SQLite native/migration owner tests |
| PostgreSQL | required CI PostgreSQL 16.4 + libpq ABI 5。client versionはCIで表示 | UTF-8 connection、parameter/result OID/name、table/attribute origin、search path、extension、server/client version | §10.3のclosed OID mappingだけを受理し、catalog `NOT NULL`からresult nullabilityを`Unknown`より強くしない | PostgreSQL native owner test |

macOSのPostgreSQL testはserver URL未設定なら理由付きskipできる。required `db-postgres` CIは
`ALIGN_DB_POSTGRES_REQUIRED=1` とPostgreSQL 16.4を使い、未設定/接続不能をfailureにする。

最初のfull-diff reviewで、deterministic stateに関する3つのgapについてQ3 matrixを再度
開いた。finding-to-fix closureは次である。

| finding | root-cause closure | owner evidence |
|---|---|---|
| ambient libpq default | library load前にuser/password/single host/port/databaseを明示したcomplete URLを要求し、target/startup overrideを拒否する。`PQconnectdbParams`でpackage-owned `client_encoding=UTF8`とempty startup-option sequenceを最後に渡す。 | `pkg_db_q3::postgres_rejects_ambient_connection_defaults_before_native_load` とrequired PostgreSQL CI |
| statement間のschema drift | SQLiteは`sqlite_schema`を読む1つのread transaction、PostgreSQLは1つのread-only repeatable-read transactionを開始する。environment captureから全selected describeまで維持し、connection Dropで解放する。 | SQLite native/migration owners と `pkg_db_q3::postgres_native_prepare_describes_the_selected_query` |
| filesystem generationの混在 | 永続するimplementation-owned `.align-db/.publication.lock` はbuild inputではない。normal readerはmetadata snapshot全体でshared OS lockを保持し、publicationはcomparison/staging/replacement/rollback全体でexclusive lockを保持する。最初のlock fileより前に開始したreaderはresolution中にfileが現れたら拒否する。process exitはlockを自動解放する。 | `static_inputs::tests::metadata_publication_lock_closes_first_publish_and_overlap_races`、`pkg_db_q3::schema_identities_and_publication_are_exact_and_check_is_read_only` とoffline whole/per-unit consumption |

required second full-diff reviewは、最初のconnection closureがURL keyを列挙した一方で
libpq独自のenvironment lookupを閉じておらず、regenerationがnative static optionをhashへ
含めても適用していないことを検出した。個別keywordの追加ではなく境界を次のように再設計する。

| finding | root-cause closure | owner evidence |
|---|---|---|
| ambient libpq target selector | `PG`で始まる`--url-env`名を拒否し、library load前に全ambient `PG*` variableの存在を拒否する。process-global environmentを変更せず、`PGHOSTADDR`、`PGTARGETSESSIONATTRS`、`PGLOADBALANCEHOSTS`と将来のlibpq追加を一括して閉じる。complete URL、package UTF-8、empty startup-option ruleは引き続き必須である。 | `pkg_db_q3::postgres_rejects_ambient_connection_defaults_before_native_load` とrequired PostgreSQL CI |
| prepare/runtime static-option drift | regenerationでQ3 native-option set全体を適用する。SQLiteはstatement preparation前に全要求version tupleを比較する。PostgreSQLはprotocol ordinalごとにsupported `int8`だけをOID 20へ写してdense vectorを`PQprepare`へ渡し、unsupported mappingはstatement preparation前に失敗する。 | `pkg_db_q3::sqlite_prepare_enforces_static_version_options`、`pkg_db_q3::postgres_native_prepare_describes_the_selected_query` とrequired PostgreSQL CI |
| per-unit snapshot scope | 最初のstatic producerからper-unit walk全体の終了までouter shared publication guardを保持し、legacy absent-file guardは最後のunit後にvalidateする。unit内resolutionのsnapshot checkも維持し、producer unit間へpublicationが割り込んでgenerationを混在させることを禁止する。 | `pkg_db_q3::generated_metadata_is_consumed_offline_and_stale_required_evidence_fails` と `static_inputs::tests::metadata_publication_lock_closes_first_publish_and_overlap_races` |

required redesign re-reviewは2つのlocal closure errorを検出した。strategyを変えずexact ownerで
次のように閉じる。

| finding | root-cause closure | owner evidence |
|---|---|---|
| logical pairを確認せずnative typeを受理 | native work前にnamed Params fieldをresolveし、Q2 `int8` mappingは`i64`または`Option<i64>`だけを受理する。同じvalidationがconnection/environment captureとOID-vector constructionの両方より先にある。 | `pkg_db_q3::prepare_rejects_unsupported_static_options_before_native_work` |
| zero-column Queryをcommandと分類 | Query/commandはnative column vectorのempty/nonemptyではなくartifact discriminatorで決める。両describerはempty Rowに対するzero columnsを受理し、native result columnを持つcommandだけを拒否する。 | `pkg_db_q3::postgres_native_prepare_describes_the_selected_query` のPostgreSQL zero-column caseとexact result-count validation |

`.align-db/sqlite` exact derived path/canonical fail-closed JSONとindependent byte/digest golden、
`alignc db prepare`/`--check`、producer-owned checked QueryMeta evidence、explicit
temp/in-memory schema setupと`ALIGNMIG`/`ALIGNSID` catalog/order/fingerprint golden、
Declared/CheckedOptional/CheckedRequired、stale/missing診断、offline normal build、runtime
storage-class/NULL validation、§16.3.1のSQLite origin/nullability matrixを所有する。
ambiguous/outer-join resultは `Unknown` のままにする。

### D4 — 最小PostgreSQL vertical

D2と同じcommon Query module形状、libpq connection、dialect-aware named scanと `$n` rewrite、
同名ordinal reuse、scalar bind/decode、SQLSTATE/owned detail、send前driver mismatch、
初期 `BufferedFull` delivery（`one` decodeは最大2行でもtransportは全result）、
両driverでportable `CAST(:value AS BIGINT)`、execution count/cleanup。明示設定された
local/ephemeral PostgreSQLでintegration testする。§12のexact PostgreSQL
connection/baseline execute option sumと全conflict/unsupported branchも含む。
§10.3のexact初期mappingを固定して未所有mappingを拒否するが、D4 executable verticalは
`i64`に保ち、complete runtime type matrixはD8が所有する。SQL/Text Params/URL/connection
option stringのU+0000をlibpq前に拒否する。Text-format byteaはexact lowercase hexへencodeし、
raw byteaはBinary formatだけでexplicit length付きにする。
local開発では未設定時に理由付きskipできるが、D4 merge/DB releaseでは
`ALIGN_DB_POSTGRES_REQUIRED=1` のrequired `db-postgres` CIがpinned ephemeral serverを
provisionし、skip/接続不能をfailureにする。同じjobでportable Queryを両driverに実行する。
direct libpq comparisonはD4が最初にlandする時またはその実行pathが変わる時のlocal
measurementであり、無関係なPR、integration suite、database release gateには含めない。

### D5 — PostgreSQL checked metadata

`.align-db/postgres`の同じexact JSON/path/derived-identity codecとPostgreSQL command golden、
engine/search path/extension/schema fingerprint、type name/OID evidence、
§16.3.1のPostgreSQL origin/nullability matrix（catalog `NOT NULL`だけではarbitrary
expression/outer joinをnon-nullにしない）、equivalent recreated schemaでreproducible
`--check`、runtime describe comparison。

### D6 — prepared statement lifecycle

dependent `db.stmt<P,R>`、connection/driver check、§11〜§13のexact common/両driver prepare
option sumとdisposition test、
`rows_stmt` の `borrow mut` statement parameter、rows Drop後のsequential reuse、
text/blob rebind時の旧transient copy解放、partial-bind failureの全binding/Params cleanup、
全path finalize/close、
implicit global cacheなし。pathが最初にlandする時または変わる時だけ
prepared/common/reprepare costをlocalに別々に測る。

### D7 — transaction/common exec view

`db.begin` がconnをconsumeし、`exec_conn`/`exec_tx` が同じ `db.exec` を返す。
commit/rollback consume、direct connectionのDrop rollback+close、D13 pool-originのproved
rollback+return-or-retire、success/error/panic相当exitのexact cleanup、
public traitなし。§11〜§14のexact common/両driver transaction option sum、SQLite begin
mode、PostgreSQL isolation/access/deferrableのBEGIN前conflict rejectionもここで実装する。

#### Q4a/D6+D7 implementation closure matrix

Q4aはprepared reuseとtransactionを一緒にpublishする。両方が同じphysical-connection resource
prefix、dependent generation、generic execution dispatch、native cleanup、whole/per-unit ABIを
拡張するためである。このcapabilityはおよそ1,000 hand-written lineを超える見込みだが、分割すると
connection move-in/move-out、child-before-parent、driver dispatch、Drop proofを反復し、prepared
またはtransactional reuseの片方がsettled common execution modelなしで残る。

| Closure cell | Required implementation closure | Exact owner evidence |
|---|---|---|
| exact public surface | §§6、11〜14のexact option-slice順で`stmt<P,R>`、`rows<R>`、`PrepareOption`、`TxOption`、`prepare`、`rows_stmt`、`begin`、`commit`、`rollback`と対応するSQLite/PostgreSQL native prepare/begin formだけをpublishする。execution view constructorは`exec_conn`/`exec_tx`だけとする。 | Q4a public whole/per-unit interface golden、wrong-scope/wrong-arity/sealed-helper negative |
| statement formationとvalidation | complete Query descriptor、common option、native option、driver restriction、live targetをnative prepare前にvalidateする。exact conn/tx generationにdependentなMove `stmt<P,R>`を1つconstructし、package-owned native state、generated binder identity、owned diagnostic identityだけを保持する。 | malformed descriptor/no-send、wrong driver/no-send、conn/tx parent-move negative、whole/per-unit prepare owner |
| prepared bindとexecution | `rows_stmt(borrow mut stmt, params, [])`はfresh dependent `rows<R>` generationを1つ作り、reflectionなしでproducer-owned Params thunkを呼び、全native bind/result allocationをrows Dropまで保持する。partial bind failureは全installed bindingをclearし、moved Paramsをexactly once Dropする。 | sequential prepare/reuse、text/blob source invalidation/rebind、injected partial-bind cleanup count |
| SQLite prepared lifecycle | `Persistent`/`Normalize`を各最大1回native prepare flagへ適用し、`rows_stmt`からrows Dropまでconnection execution leaseを保持する。全end pathでreset後clear bindingsし、stmt Dropでexactly once finalizeする。cleanup failureはdependency解放前にconnectionをpoison/closeする。 | SQLite option disposition、overlap、reset/clear/finalize counter、error/early-Drop owner |
| PostgreSQL prepared lifecycle | concrete Params contractに対して各`ParameterOid(name, oid)`を`PQprepare`前にvalidateし、unknown/duplicate/zero OIDをrejectする。connection-local collision-free statement nameを1つallocateし、`PQexecPrepared`を使い、全result/contextを1回clearし、stmt Dropでbest-effort `DEALLOCATE`する。process/global cacheはない。 | PostgreSQL required prepare/reuse/option ownerとnative stub lifecycle counter |
| transaction formationとoption | common option、次にdriver optionをsource orderで`BEGIN`前にvalidateする。`begin`はnative begin成功後だけconnをconsumeし、exact SQLite mode SQLとexact PostgreSQL isolation/access/deferrable clauseを使う。invalid PostgreSQL combinationはsend前にrejectする。 | common/native option precedence、SQLite 3 mode owner、required PostgreSQL combination/no-send owner |
| transaction executionとjoin | live txはconnと同じvalidated connection prefixを持つ。既存common/native execution/inspection pathはsecond trait/ABIなしで`exec.Tx`を受ける。use-after-end、conn/tx alias、live stmt/rows child中のcommit/rollbackはcompile-time errorにする。 | 両driver transaction内common command/Query、metadata/EXPLAIN tx owner、compile-fail alias/child matrix |
| commit、rollback、Drop | `commit`/`rollback`はtxをconsumeし、certain native success後だけconnを返す。end errorはfail-safe rollbackのためtx ownerをliveに保つ。implicit Dropはcommitせずrollbackし、transferred state null、wrapper freeをnative orderで行う。direct-driver stateはonce close。D13 pool-origin stateはexact driver rollback-and-idle proof後だけreturnし、それ以外はclose/retire。 | success/error/early-return/branch/`?` end matrixとclose/rollback/commit/pool-return counter |
| generic/ABI closure | fixed 8-aligned execution descriptorを104-byte v2から120-byte v3へbumpし、offset 0〜103は不変とする。offset 104はQuery/commandでnon-nullのproducer-owned `fn(name: str) -> i32` parameter-ordinal resolver、offset 112はexact `u32` distinct-parameter count、offset 116はreserved zeroである。generic stmt binder bridgeはpackage-privateで、concrete `stmt<P,R>` referenceとmatching Pを要求し、exact borrow modeでretained producer thunkへlowerする。application sourceまたはmalformed HIRからの使用をrejectする。whole/per-unit linkageはreachableな時だけproducer thunkとnative libraryを保持する。 | sema/MIR fail-closed bridge owner、exact descriptor size/offset/signature owner、whole/per-unit runtime parity |
| explicit later cells | Q4aは`next`、borrowed current-row view、common deadline enforcement、cancellation、portal、statement cacheをpublishしない。D8がtyped row delivery/generationとfull bind/decode type matrix、D9がdeadline/cancel completionを所有する。Q4aはそのconsumerに必要なrows cleanupとprepared text/blob copy lifetimeをcloseする。 | absence/interface goldenとQ4b/D8+D9 matrix |

candidate review前にauthor-side matrix-to-diff passで全applicable rowを1 source pathと1 ownerへ
対応させる。bind/end/cleanupの1 branchにfindingが出た場合、coherent fix commit前に両driverと
conn/tx parentの同じroot-cause classをauditする。

### D8 — typed row streaming

`db.rows<Row>`、`next` generation、text/blob owner-tied view、old-view compile rejection、
`clone_in` retention、bounded batchの前提、SQLite/libpq両driverのpointer-validity test。
SQLite text/blob Params sourceを`rows` return後/最初の`next`前にdrop/mutateするtest、
transient bind copy bytes/allocation/partial-error cleanup、per-row parameter copy 0を固定する。
exact初期common type mapping全てのruntime bind/decode/nullability matrixもここで完成する。
PostgreSQL初期pathは `BufferedFull` 上のone-pass decodeで、single-row/portal deliveryはD13。
SQLite ordinary/timeout stream overlapの両方向reject、failed second attemptがfirstのsaved
stateをrestoreしないこと、firstのexhaustion/error/Drop lease cleanupも固定する。

### D9 — scoped native option、deadline enforcement、cancellation cleanup

D1/D2/D4/D6/D7で既に公開したoption API上へcommon deadline/native cancellation machineryを
完成させ、全scopeのapplied/unsupported/conflicting/precedence matrix、timeout/cancel後
connection stateをauditする。SQLite common deadlineのpre-send Unsupported、
PostgreSQL deadline/cancel、BusyTimeoutをcommon deadline扱いしないことをhidden SQLなしで
固定する。v1 public external cancel resourceがないこと、cancel後にdrain/resynchronizeを
証明できなければconnectionをpoison/closeすることも固定する。D9で
preliminary APIや別表現を作らない。要求optionのsilent ignoreを全driverでnegative testする。

v1の最終dispositionはexactである。PostgreSQLはdirect Query/command executionとprepared
Query executionの`db.ExecuteOption.TimeoutNs`を1つのmonotonic deadlineとnative cancellationで適用する。
SQLiteはこのcommon execution deadlineをsend前にrejectし、native `BusyTimeoutNs`はlock-wait
controlのままとする。両driverともcommon prepare、transaction-begin、metadata、EXPLAIN
deadlineをsend前にrejectする。PostgreSQL `ConnectTimeoutNs`とSQLite `BusyTimeoutNs`は既に
settledなdriver-native behaviorを維持する。enforceしないtimeoutを受理するscopeはない。

#### Q4b/D8+D9 implementation closure matrix

Q4bはstreamingのpublishとdeadline/cancellation behaviorの完成を一緒に行う。current-row
generation、native execution lease、timeout state、result drain、connectionをreuse可能とするか
poisonするかの決定が1つのlifecycleだからである。このcapabilityはおよそ1,000 hand-written
lineを超える見込みだが、分割するとcomplete end-path proofなしでrowsを公開するか、bind/decode、
lease、native wait、cleanup matrixを2つのdormant producer/consumer seamで重複させることになる。

| Closure cell | Required implementation closure | Exact owner evidence |
|---|---|---|
| exact public surface | Q4a prepared surfaceへapplication-callableなcommon `rows(exec, query, params, options)`と`next(borrow mut rows)`だけを追加する。exact option-slice順と`Result<Option<R>, Error>`を保つ。sealed cross-module validator adapterはapplication-unconstructible internal controlを要求し、callable application surfaceを追加しない。cancel handle、portal/single-row selector、cache、iterator trait、materializing rows helperは公開しない。 | `pkg_db_q4b::public_streaming_surface_is_exact`、whole/per-unit interface、wrong-scope/wrong-arity、sealed-bypass、explicit absence golden |
| descriptor/generic ABI | fixed 8-aligned execution descriptorを120-byte v3から128-byte v4へbumpし、offset 0〜119は不変とする。Queryのoffset 120はexact dependent `resource_ref<rows<R>>` current generationを受け取り、そのownerだけにrootedしたrecursive borrow provenanceを持つ`R`を返すnon-null producer-owned streaming decoder thunkであり、commandのoffset 120はnullである。complete v4 field checkは1つのshared query/command header validatorが所有し、common/native entry pointはapplication-unconstructibleな`pkg.db.internal.DescriptorHeaderControl`だけで到達する。malformed descriptor/header/function-signature HIRはfail-closedのままにし、whole-program/per-unit compileで生成binder/validator/materializing decoder/streaming decoderとselected native driverをreachable時だけretainする。 | `pkg_db_q4b::public_streaming_surface_is_exact`、`direct_rows_and_next_typecheck_whole_and_per_unit`、`streamed_views_cannot_cross_generation_or_escape`、Q4a shared-validator/delegation/sealed-bypass owner、sema/MIR malformed-HIR owner、exact 128-byte/offset/signature golden |
| direct/prepared stream formation | settled orderでdescriptor、common option、native option、restriction、live target/statement、leaseをsend前にvalidateする。rows return前に全Params fieldをnative transmission完了までcopyまたはretainし、partial bind failureでは全installed/package-owned copyをexactly once dropする。`rows`/`rows_stmt`はnative success後だけdependent rows resourceを1つconstructし、resultをmaterializeしない。 | `pkg_db_q4b::owned_text_and_bytes_params_bind_before_their_sources_drop`、`sqlite_direct_stream_retains_binds_and_releases_each_native_phase_once`、両driver lifecycle ownerによるconn/tx・direct/prepared path |
| complete bind/type matrix | non-null/`Option`のbool、全signed integer width、`f32`/`f64`、UTF-8 text、bytesにdirect ordinal binderを生成する。SQLiteはexact INTEGER/REAL/TEXT/BLOB/NULL mappingとtransient text/blob copyを使う。PostgreSQLはsettled OID/typeとText/Binary format combinationだけを受け付け、bytea Textはexact lowercase `\\x`、bytea Binaryはraw payloadを使い、C-string Text parameterのembedded NULをsend前にrejectする。 | `pkg_db_q4b::complete_sqlite_parameter_and_row_matrix_is_exact`と`complete_postgres_parameter_and_row_matrix_is_exact`、bounds/nullability/embedded zero/high byte/source mutation・Drop/copied byte/no-per-row-parameter-copy path |
| complete row validation/decode | decoder前にexact column count、declared-order UTF-8 name bytes、exact driver-native type metadata、NULL disposition、全first-release typeのvalue representationをvalidateする。ordinalごとのgenerated typed callbackでreflection/boxing/map/artifact/source I/O/post-validation fallible conversionなしにdecodeする。multi-invalid rowは最初のdeclared ordinalをreportし、native pointer/length/UTF-8 malformed valueはsafe view形成前に失敗する。D13 binary ledgerはこの順序を、zero-row/cardinality処理前にcountと全declared name/OID/formatを検査するresult-generation metadata phaseと、その後に各delivered rowのNULL disposition/representationを検査するvalue phaseへsupersedeする。 | 両driver complete matrix owner、`pkg_db_q4b::malformed_native_view_values_fail_before_safe_view_formation`、cumulative Q2 name/type/count/null twin、generated MIR inspection、callback version/context misuse negative、D13 metadata-before-value winner owner |
| row generation/view safety | `next(borrow mut rows)`はnative advance前にprevious generationを終了し、1回だけadvanceしてcurrent rowをvalidateし、clean exhaustionだけで`None`を返す。scalar fieldはcopyする。`str`/`slice<u8>` fieldはfresh rows generationをownerに`resource.view_from_raw`だけで形成し、visible retention pathは`clone_in`である。次のmutable borrow後のview使用、storage、return、branch/loop generation joinを拒否する。 | `pkg_db_q4b::streamed_views_cannot_cross_generation_or_escape`、両driver complete matrix owner、malformed-native-view owner、cumulative stmt/conn parent-move case |
| `one` cardinality compatibility | common/driver-qualified `one`はstream formationを再利用する。row 0だけをvalidate/decodeして`out`へcloneし、malformed first rowはmultiplicityより先に失敗する。D13はolder second-row winnerをsupersedeし、driver-private probeはsecond row有無の判定前にeach newly acquired result generationのmetadataをvalidateする。したがってmalformed second-generation count/name/OID/formatはCardinality前にDecodeを返し、valid metadata+任意のsecond rowはValue mode/row decodeなしにCardinalityを返す。exhaustion/multiplicityは同じstream cleanup pathでcloseする。 | `pkg_db_q2::sqlite_native_command_and_one_execute_generated_i64_thunks`、`postgres_native_command_and_one_own_buffered_results`、両common-surface dispatch owner、D13 second-generation malformed-metadata vs valid-metadata Cardinality twin |
| SQLite streaming lifecycle | stream formationからexhaustion/error/Dropまでexecution leaseを保持する。successful `next`はSQLite current rowだけを公開する。exhaustion/error/Dropはowned direct statementをfinalizeするかborrowed prepared statementをreset後clearし、適用したnative busy timeoutのrestore、bind state free、lease releaseを続ける。ordinary/timeout stream overlapを両方向でrejectし、failed second attemptはfirst streamのsaved global stateをrestoreもmutateもしない。cleanup failureはdependency解放前にpoison/closeする。 | `pkg_db_q4b::sqlite_stream_lifecycle_and_overlap_are_exact`、step/finalize/reset/clear/busy state/lease/close counterとsuccess/decode error/native error/early Drop/failed second attempt path |
| PostgreSQL buffered streaming lifecycle | settled synchronous `BufferedFull` resultをv1 baselineとしてrowsにretainし、`next`ごとにexactly one ordinal rowをdecodeし、exhaustion/error/Dropでclearする。parameter transportはsynchronous send完了後にfreeする。1つのlibpq result以外のhidden buffering、portal/single-row modeはない。 | `pkg_db_q4b::postgres_buffered_stream_lifecycle_is_exact`、delivery/PQclear/allocation counter、clean exhaustion/malformed row/early Drop、conn/tx・prepared/direct reuse |
| common deadline disposition | 既存の全common TimeoutNs/BeginTimeoutNs scopeでpositive value、duplicate/source-order conflict、driver capability、state、native-option precedenceをauditする。SQLiteはrequested common operation deadlineをpre-send `Unsupported`にする。BusyTimeoutNsはstream lifetime中activeなlock-wait controlのままでcommon deadlineとしてreportしない。requested optionをsilent ignoreしない。 | connect/execute/one/rows/prepare/begin/metadata/EXPLAINとcommon/driver-qualified entry point両方をcrossするparameterized `pkg_db_q4b::deadline_disposition_and_precedence_are_exact` |
| PostgreSQL timeout/cancel recovery | requested common deadlineにはnonblocking libpq send/flush/consume waitと1つのmonotonic absolute deadlineを使う。expiryはnative cancellationを発行して`Timeout`、別のengine cancellationは`Cancelled`を返す。その後protocol/transaction synchronizationまでdrainしてからreuseし、証明できなければpoison/closeする。hidden SQLを発行せずpublic cancellation resourceを作らない。 | `pkg_db_q4b::postgres_deadline_cancel_drain_and_poisoning_are_exact`、`postgres_deadline_fault_phases_are_exact`、`postgres_prepared_deadline_recovers_for_reuse`、`postgres_command_deadline_is_enforced_and_recovers_for_reuse` |
| cleanup/allocation/scale closure | formation/bind/advance/decode/timeout/cancellation/exhaustion/early exit/`?`/Dropの全pathでtransferred pointerをnullにし、result/context/saved native state/lease/parent dependencyをproducer orderでexactly once解放する。MIR scope cleanupはordinary return、early exit、loop back-edge、`break`の全てでsettledなreverse-declaration orderに従い、dependent rows/stmt resourceをparent connectionより先にDropする。success rowのerror allocationとloop内reflection/artifact I/Oは0。local one-million-row scalar/borrowed-text iteration・decode・delivery countとdeadline/cancellation overheadを記録し、correctness counter mismatchを示す場合以外はmeasurementをgateにしない。 | cross-driver fault-injection cleanup table、`resource_ownership::dependent_resources_drop_child_before_parent_on_every_scope_exit`、sanitizer-compatible stub owner、allocation/copy/delivery counter、recorded local D8/D9 measurement |

2026-08-09のLinux/ARM64 Docker local recordではSQLiteの両pathがexactly 1,000,000 rowをdeliveryし、
scalar streamingは301.85 ns/row、borrowed-text streamingは353.63 ns/rowだった。deterministic
PostgreSQL stubではordinary commandが1,092.17 ns/op、expiryしないenforced deadline付きが
1,415.34 ns/op、1 ms expiry/cancel/drain pathが1,991,807.25 ns/opだった。これらはnon-gatingな
machine-local observationであり、exact delivery/native-call countをcorrectness ownerとする。

candidate review前にauthor-side matrix-to-diff passで全applicable rowを1 source pathと1 ownerへ
対応させる。1 type、parent、timeout phase、cleanup branchのdefectは、coherent fix commit前に全type、
両driver、direct/prepared execution、conn/tx parentの同じroot-cause classをauditする。

### D10 — compound Output

many-to-one/master projectionと一対多Outputをend-to-endで実装する。Query-local visible loop、
Pure step（`borrow mut state`、0個以上の独立した `borrow mut` builder、row、out）、
1 execution、hidden SQL 0、copy/allocation countを固定する。builderはState fieldにしない。
初期DB releaseの必須項目。

#### Q6/D10 implementation closure matrix

Q6は新しいdatabase/compiler primitiveを公開しない。既にreview済みのcompound contractを、shipped
typed stream、exclusive borrow、region、builder、`clone_in` surfaceをordinary Query-local Align codeで
合成して閉じる。transaction/master projectionとUser + Groups exampleは、visible execution、Pure
shaping、row-view retention、explicit destination allocationの1つのproofを共有する必須compound consumer
2つであるため一緒にlandする。
このcapabilityは手書き約1,000行を超える可能性がある。first consumerがL2e/L6 seamに1つの不足した
analysis-local proofを露出したためである。現compilerは全view-bearing argumentが全`borrow mut`
destinationへ保持され得ると仮定する。このproofをcompound consumerから分割すると唯一のend-to-end
ownerなしにdormant compiler relaxationを公開し、exampleを2つに分割すると同じprovenance、execution、
builder matrixを重複させる。

| Closure cell | Required implementation closure | Exact owner evidence |
|---|---|---|
| exact Query-local surface | transaction/master projectionとUser + GroupsのQuery moduleを1つずつ追加する。各moduleはflat `Params`/`Row`、logical output record、static Query constructor 1つ、既存public `rows`/`next` surfaceだけで構成するImpure `run(exec, params, out)`を公開し、private Pure `step` 1つを定義する。`db.fold`、relationship、lazy-load、iterator、hidden materializer、package-private execution pathは追加しない。 | `pkg_db_q6::compound_query_modules_are_exact_and_typecheck_whole_and_per_unit` とexplicit absence/source-shape assertion |
| exact mutable-retention proof | same-program direct callだけで、各`borrow mut` destinationからwhole-value/field replacement、builder push/append、transitive direct callによって実際にstoreされるexact parameter rootへのrelationをleast fixed pointでinferする。各retained parameter rootはcontained provenanceとborrowed owned parameterから到達するstorageを区別し、そのtyped edgeをborrow livenessとescape analysisの両方へ通し、前者をargumentのprojected contained fact、後者をstorage rootとexact lexical storage regionへtranslateする。したがって`clone_in(out)`はsource row generationではなく`out`をretainし、borrowed owned arrayのretained sliceはarray ownerをliveに保ち、ownerをdropするforwarding helperからescapeできない。imported/indirect/missing-body/malformed/unresolved callは1つのshared conservative expansionを使い、compatibleな全argumentのcontained/storage両edgeを全consumerへ渡す。builderまたはimmutable-view aggregate placeのmutationとslice/resource dependencyを介して到達するmutationを区別し、同じallocation regionの共有をstorage aliasと見なさず、実際のnested mutable aliasはrejectする。mutable `resource_ref`はmutable place自体が別のCopy slotでもpeer-alias checkではowner generationにrootedしたままにする。checked-HIR replayでこのanalysis-local factを再計算し、HIR/interface/MIR/ABI fieldとruntime workは追加しない。 | direct/wrapped/recursive/control-flow store、clone対raw-row retention、borrowed-owned storage対contained aggregate field、direct/forwarding storage-region escape、same-region distinct place、nested slice/resource/resource-reference alias、owned-storage forwardingを含むimported/indirect fallback、malformed index、whole/per-unit parityをcrossする `pkg_db_q6::borrow_mut_shaper_retention` ownerモジュール(cellごとに1つのnamed test)、およびcumulative L2e all-peer/L6 wrong-region owner |
| Pure shaping boundary | database handleとrow advancementは`run`に置く。`step`は`borrow mut` state、0個以上の独立した`borrow mut` region builder、current `Row` 1つ、`out`だけを受け取り、inferred Pureを維持する。同shapeでdatabase I/Oを呼ぶstepはwhole-program/per-unit checkでrejectされる。builderはState fieldに入れずordinary by-value callをcrossしない。 | `pkg_db_q6::shapers_are_pure_and_cannot_reach_database_io` とcumulative L2e/L6 effect/arena-call owner |
| one-parent consistency/nullable child | first rowでparent nameを`out`へcopyし、later rowではchild append前に同一parent ID/nameを要求する。`(None,None)`だけをno childとし、partial `(Some,None)`/`(None,Some)`を同じdeterministic field orderでrejectする。zero rowは`None`を返し、全errorはnormal rows/builder cleanupを通る。 | zero/one/repeated/inconsistent/complete-null/both-partial inputをrejectionごとのexact `Decode` contract-item identity付きでcrossする `pkg_db_q6::sqlite_user_groups_one_parent_and_segmented_matrices_are_exact`。`pkg_db_q6::postgres_compound_shaping_dispatches_once_on_connection_and_transaction` はwell-formed inputでの両common driver dispatchのみをcoverする — shapingはbackend非依存のQuery-localコードなので、malformed-input classはSQLite ownerで一度だけ走る |
| segmented many-parent output | parent key、child key順のSQLからadjacent parent groupだけをconsumeする。`out`内にusers/groups/offsetsの独立arrayをbuildし、offsetは0で始めfinal offsetを1つ追加する。`(None,None)`だけをabsent childとし、`(Some,None)`を`(None,Some)`より先にdeclared child-field orderでrejectしてchild appendもincorrect final offset確定も行わない。later key後にparent keyが再出現する場合とrepeated-parent field disagreementをrejectし、sort/hash/deduplicate/per-parent child arrayを作らない。 | empty/empty-child/both partial-child directionとprecedence/high-fanout/parent transition/disagreement/non-adjacent-key caseを持つ `pkg_db_q6::sqlite_user_groups_one_parent_and_segmented_matrices_are_exact` |
| transaction/master projection | flat transaction + status-master rowを1 passでnested region-owned output rowへmapする。同じ`run`が`exec_conn`と`exec_tx`で実行され、transaction ownershipとcommit/rollbackはcallerに残り、relationship fieldはfollow-up readを発行できない。 | exact nested value、transaction lifecycle counter、callごとの1 executionを持つ `pkg_db_q6::sqlite_transaction_master_runs_on_connection_and_transaction` および `pkg_db_q6::postgres_compound_shaping_dispatches_once_on_connection_and_transaction` |
| ownership/allocation/compilation parity | retained streamed text fieldを各1回だけ`out`へcloneし、全aggregateをregion builderへpushし、各final arrayを`run`内でinline buildする。heap array builder、reflection、row materialization、generated extra Queryはloopに入らない。whole-program/per-unit compilationは同じQuery descriptor、step、stream ownership、cleanup、generic instantiationをretainする。 | `pkg_db_q6::compound_shaping_uses_region_builders_and_exact_visible_copies`、MIR/LLVM call count、whole/per-unit execution、cumulative Q4b row-generation/Drop owner |
| execution/scale closure | stream formation成功後、各`run`はrows resourceをexactly 1つ所有し、row/fanout countによらずnative SQL executionをexactly 1回完了する。send前のvalidation/binding failureはexecution 0、native send failureはexecution attempt最大1回、rows resource 0、retryなしとする。row delivery、child push、parent push、text copy、builder build、rows-resource、execution-attempt、completed-execution countを固定する。local high-fanout one-to-many measurementを記録する。timingはnon-gatingだが全countはcorrectness evidenceである。 | 全Q6 runtime owner、pre-send/send-failure counter owner、ignored `pkg_db_q6::high_fanout_shaping_measurement_reports_one_execution` |

candidate review前にauthor-side matrix-to-diff passで上の全rowをQuery-local sourceとexact owner 1つへ
対応させる。parent consistency、nullable-child、adjacency、copy/allocation、execution count、cleanupの
defectは、両compound example、common driver dispatch、connection/transaction parent、whole/per-unit
compilationを跨いで同じroot-cause classをauditする。

pre-implementation adversarial reviewはsource work前に次のcontract gapを閉じた。

| Finding | Contract closure |
|---|---|
| P1 exact direct retentionがL6のall-argument ruleと衝突した | L2e/L6は同じanalysis-local exact same-program relationを所有し、bodyを証明できない場合はconservative fallbackを維持する。 |
| P2 segmented outputにpartial-child rowがなかった | segmented ownerはpartial-NULL両方向、declared-field precedence、reject時にchild/offsetをmutateしないことをcoverする。 |
| P2 one-execution wordingがpre-stream failureを含んだ | successful stream formationはrows resource 1つとcompleted execution 1回を所有し、pre-send/send-failure pathはzero/at-most-one attemptとno-retry countを別に持つ。 |

implementation reviewは1つのmissed invariantを中心にmutable-retention cellを再開した。call
effectはoptimized traversal pathを含め、argument評価後のatomic transition 1つである。

| Finding | Root-cause closure | Owner evidence |
|---|---|---|
| P1 transparent `?`/return pathがdestination retentionを欠いた | child call effect後のtransparent error/return edgeで全mutable destinationをcollectし、general HIR walkと一致させる。 | `pkg_db_q6::borrow_mut_shaper_retention` ownerモジュールのtry-error/direct-return forwarding case |
| P1 eager argument factをcontrol-expression completion前にcaptureした | 全eager argument完了後だけcall effectをapplyし、その時点で各argumentのcontained factをsnapshotする。 | 同ownerのinline `if` argument case |
| P1 複数mutable destinationが順次変更済みregionを読んだ | destination update前に全exact source regionとdestination storage regionをsnapshotし、1つのpre-call stateからvalidateしてupdateをjoinする。 | 同ownerのcross-arena two-destination swap case |
| P2 unary direct callがexact summaryをbypassした | unary transparent-spine post actionをeager worklistと同じatomic direct-call transitionへrouteする。 | 同ownerのunary whole-field clear case |

required P1 re-reviewはこのcellを再度開き、call-site exception追加ではなくanalysis boundaryを
変更した。exact retentionは`contained(parameter)`または`storage(parameter)`というtyped source
edgeをtransportする。

| Finding | Root-cause closure | Owner evidence |
|---|---|---|
| P1 exact transferがowned source storageをdiscardした | caller-owned parameter storageをborrowして導入されたrootをparameter valueにcontainedするrootと別にmarkする。storage edgeだけを`storage_roots`へtranslateし、contained edgeはprojection precisionを維持する。 | 同ownerのborrowed owned-array slice retentionとaggregate-contained-field non-pinning control |
| P1 mutable `resource_ref` peerが共通ownerをdiscardした | exclusive alias rootで`resource_ref`をreachable resource dependencyとして扱い、distinct mutable-place exceptionはowning resourceだけに限定する。 | 同ownerの1 resource owner由来の2 mutable reference slot |

次のrequired reviewでtyped edgeがescape solve前に終了していることが判明した。closure ruleは
end-to-endであり、liveness factとlexical storage regionの両方をselectする前に
`storage(parameter)` edgeをargument indexへ縮退させてはならない。

| Finding | Root-cause closure | Owner evidence |
|---|---|---|
| P1 escape checkingがretained storage lifetimeをeraseした | typed sourceをescape-flow operation全体で維持する。contained edgeはargument value regionを使い、storage edgeはdestinationのvalidate/update前にvalue regionとexact caller-place lexical storage regionをintersectする。 | 同ownerのlocal owned arrayを持つforwarding helper、およびdirect-call/fixed-array storage control |
| P1 unavailable-call fallbackがcontained edgeだけをemitした | liveness/escape両consumerが使う1つのshared operationでexact/conservative source edgeをselectする。conservative resultはmalformed-summary fallbackを含め、全argumentをcontained/storage両edgeへexpandする。 | forwarding helperのlocal owned-array storageをretainし得るinterface-only/indirect call、および既存cloned-string fallback control |

2026-08-10のmacOS/ARM64 local Q6 recordでは、User + Groups child 10,000件を
16,253,542 ns（約1.63 us/child）でshapeし、native execution 1回、delivered row 10,000件、
child push 10,000回、rows-resource finalization 1回をexactに記録した。timingはnon-gatingな
machine-local observationであり、execution、delivery、push、cleanupのexact countがcorrectness evidenceである。

2026-08-10のfollow-up reviewは、non-gatingな既知のfidelity limitを3件記録した:

- SQLite stubはprepare失敗時にallocated statementをout-pointerへstoreし、
  `sqlite3_finalize(NULL)`をprotocol violation扱いする。実SQLiteはerror時のNULL
  out-statementを保証し、NULL finalizeをno-opとして受理する。どちらのpathも
  現在のownerからは到達不能である。
- PostgreSQLのsend-failure ownerはNULL-result pathのみをmodelする。`PQclear`を
  依然要求するnon-NULLの`PGRES_FATAL_ERROR` resultはQ6ではなくQ2 sqlstate
  suiteがownする。
- `rows_resource_names_for_descriptor`は`.`と`$`をどちらも`_`へmapするため、
  reconstructed per-unit rows-resource spellingはinjectiveでない。contrivedな
  programでは別のrow nominalが1つのshared-drop resource recordへmergeし得る。
  injective escapeはupstreamのper-unit generic-body spellingに属する。

### D11 — SQL migration

ordered SQL file、D3と共有するexact `ALIGNMIG`/`ALIGNSID` byte/digest golden、
checksum/history、先頭lineのexact required/forbidden policy、
required default atomicity、forbidden 1-statement制限、Applying/Failed dirty state、
checksum-bound repair、全file適用前のU+0000拒否、
明示 `alignc db migrate/status/check/repair`。

#### Q5a/D11 implementation closure matrix

Q5aはCLI、canonical catalog reuse、driver-owned screening/locking、persistent history、migration
execution、inspection、explicit repairを跨ぐ1つのexternal-state capabilityである。SQLiteと
PostgreSQLを同時に入れ、history/state/repair semanticsのdriver driftを防ぐ。これはQ5の意図的な
mutation halfであり、D12は独立したread-only capabilityとして残す。
このcapabilityは2 FFI adapterとfault/concurrency ownerを含むため手書き約1,000行を超える見込みである。
driver/commandで分割するとdormant producer/consumer seamまたは同じpersistent-state proofの重複を
残す。shared state machine 1つとthin driver adapterの方がproof重複とintegration riskを下げる。

| Closure cell | Required implementation closure | Exact owner evidence |
|---|---|---|
| CLI/target identity | §17.6のexact migrate/status/check/repair formとvalidation precedenceを実装する。cwd discoveryなしでentry、project-relative catalog、relative SQLite targetをresolveし、non-secret validation後にselected non-`PG*` PostgreSQL URL variableだけを読む。 | `pkg_db_q5a::migration_cli_rejects_invalid_forms_before_catalog_environment_or_native_work` とpath/symlink matrix |
| catalog/policy screening | Q3の `ALIGNMIG` catalog byte/digestを再利用する。exact first physical lineをparseしcomplete driver statementをcountし、target mutation前にempty、transaction-control、NUL、invalid UTF-8、multi-statement Forbidden fileをrejectする。lexical completeness後、1 source-ordered classification passが最初のtransaction-controlまたはPostgreSQL top-level `COPY`をtarget open前にrejectする。1 statement内はtransaction-controlがCOPYより先で、COPYは§17.6 exact diagnosticを使い、quoted/comment/dollar-body内COPY textはdataのままにする。 | cumulative Q3 catalog goldenと `pkg_db_q5a::migration_policy_and_statement_screening_is_exact`、両prohibited statement orderを含む`postgres_migration_copy_is_rejected_before_native_work` |
| history codec/state reconciliation | migrate中だけ§17.6のexact owned objectを作成し、complete persistent/session-local history-table/attached-object inventoryと全row/state combinationを検証し、version順でreconcileし、complete current/extra/mismatch/dirty matrixをpanic/silent upgradeなしでclassifyする。1本のjoined PostgreSQL catalog queryがtable invariantを所有し、unrelated schema objectを除外する。malformed schema/row stateはmutation前にrejectする。 | SQLite TEMP-trigger/shadow/inbound-FKとPostgreSQL user/inbound-FK trigger/rule/RLS/default/index/table/column ACL negativeを含む両driverの `pkg_db_q5a::migration_history_state_matrix_is_fail_closed` |
| overlap exclusion/cleanup | 全SQLite commandはexact persistent OS-lock inodeを作成可能かつ保持し、全PostgreSQL commandはoperation全体でexact advisory keyを保持する。そのcooperating lock後、SQLite read snapshot/`BEGIN IMMEDIATE` とblind-first-SQL PostgreSQL `READ COMMITTED` SHARE ROW EXCLUSIVE/ACCESS EXCLUSIVE table lockがnon-cooperating DB writerに対してvalidation/history accessをatomicにする。SQLSTATE-bound rollback/bootstrapがexistence raceなしでabsent tableを扱う。Forbiddenはhistoryをmutateしないworkerへuser SQLを分離する。全success/error/Drop/process-loss edgeでworker、native transaction/table lock、operation lockの順に解放する。 | SQLite absent-lock creation/external-writer race/TEMP-trigger/two-process ownerとrequired PostgreSQL concurrent-session/external-DDL-DML/bootstrap-race owner |
| Required execution | migration lock下で各Required fileとApplied history insertを1 transactionで行う。statement/history failureはcomplete fileをrollbackする。不確定commitはclose/reconnect/relockしexact Applied/absentをclassifyし、same-invocation retryしない。current Applied prefixを再実行しない。 | SQLiteとrequired PostgreSQL atomic/multi-statement/error/restart/outcome-unknown owner |
| Forbidden execution/dirty state | screened statement 1つを要求し、transaction外execution前にApplying(0)とそのexact history snapshotをdurably observeする。final native lock下でsnapshotをcompare/restoreし、observed snapshotがunchangedの場合だけApplied(1)またはFailed(0)をrecordする。row changeまたはabsent owned history objectはApplyingをrestoreしてvisibleに失敗し、malformed replacementはfail closedする。native errorはbest-effortでFailed(0)、不確定final publicationはAppliedまたはdirty Applyingへreconcileする。dirty stateは継続をblockし自動retryしない。 | 両driverのbefore/after/error-recording/history-forgery/outcome-unknown fault matrixとexecution counter |
| status/check | operational lock file以外を作らず、schema/history creation、migration、repairを行わない。catalog/history provenance付きexact ordered row/summaryを出し、statusはinspection後success、checkはexact current Applied setだけsuccess。missing historyはempty、missing SQLite targetはinput error。 | empty/current/compound-mismatch/dirty/history-onlyを跨ぐ `pkg_db_q5a::status_and_check_are_read_only_and_ordered` |
| repair | current version 1つ、action 1つ、argv/catalog/dirty historyに一致するexact lowercase checksumを要求する。acceptはscreened countでAppliedを記録し、clearはdirty rowだけを削除する。Applied/absent/stale/mismatchは何も変更しない。 | 両action/driverの `pkg_db_q5a::repair_is_dirty_and_checksum_bound` |
| secret/allocation/diagnostic | result cleanup前にnative error/history stringをcopyし、PostgreSQL URL/valueをprintせず、allocation前に全native countをboundし、malformed rowをindex/panicせずrejectし、statement/result/connectionをexactly once closeする。 | malformed native/history owner、URL redaction owner、self-review FFI checklist、required PostgreSQL CI |

author-side matrix-to-diff passは各cellを1 implementation ownerとtestへ対応させる。D11のexplicit
scaling record promiseによりlocal 10/100/1000 catalog/history measurementも行う。normal compiler
pathはD11 moduleをimport/callしない。

pre-implementation adversarial reviewはsource work前に次のroot-cause classを閉じた。

| Finding | Contract closure |
|---|---|
| P1 absent-lock readerがfirst writerとraceできた | 全commandが同じpersistent SQLite inodeをatomic create/openしてlockする。read-onlyはdatabase/historyを指し、operational lockだけをfilesystem writeとする。 |
| P1 commit response lossをrollback/dirtyと断定していた | Required/Forbidden publicationはfresh-connection history reconciliation 1回とexact permitted outcomeを使い、same-invocation automatic retryをしない。 |
| P1 column/check検証がbehavior-changing attached objectを除外しなかった | SQLite schema-row inventoryとPostgreSQL history-table relation/index/constraint/trigger/rule/RLS/ACL inventoryを閉じ、fail closedする。 |
| P2 compound mismatchにtotal order/value provenanceがなかった | Name、checksum、policy、native stateを1 total orderとし、全output rowがexact unavailable marker付きcatalog/history fieldを常に持つ。 |
| P2 validation phase内部のwinnerが未規定だった | token、field、directory、file、statement、connection、schema、historyのtraversal/error precedenceをexactにした。 |
| P1 revised reviewがSQLite connection-local behaviorを `sqlite_schema` 外に発見した | matrixをpersistent/session-local modifier全体へ再openし、exact `main` qualificationと `sqlite_temp_schema` rejectionでTEMP trigger/shadow pathを閉じた。 |
| P1 focused continuationがnon-cooperating DB writerとのvalidation-to-DML raceを発見した | matrixを2 lock layerへ再設計した。OS/advisory lockがAlignをserializeし、SQLite transaction/PostgreSQL table lockがnative validation/history accessを覆い、Forbidden workerはhistory DMLを行わない。 |
| focused lock reviewがunprotected schema-wide stateとracy PostgreSQL existence branchを含んでいた | invariantを1 catalog queryで検証するhistory-table-attached behaviorだけにし、blind first-SQL lockとSQLSTATE-bound rollback/bootstrapでexistence raceを除去した。worker-local stateはhistory-connection invariant外で、Applyingはrepair必須/no-retry dirty stateのままである。 |
| final inspection reviewがPostgreSQL `SHARE` とindex creationのcompatibilityを発見した | inspectionはordinary readerを許可し、DMLとordinary/concurrent index/DDL modeに競合する `SHARE ROW EXCLUSIVE` を使う。 |
| author-side implementation passがForbidden user SQL自身によるApplying rowのerase/forgeをfinal validation前に許していた | runnerはexact pre-worker history snapshotを保持し、final native lock下でcompare/restoreし、row change時はrestored Applyingとvisible failureを残す。absent owned tableはApplyingをrestoreしてfailするためだけに再作成し、malformed replacementはblocking errorのままにする。 |
| focused implementation reviewがmalformed rowをrestorable changeとして扱い、不確定restore commitでcurrent Applying rowだけを確認していた | 両adapterはrestore前にcomplete row semanticsをvalidateし、malformed replacementを変更しない。全Applying commit reconciliationはcomplete expected history snapshotを比較するため、partial/unapplied restoreをexactと報告できない。 |
| Align compiler self-reviewがpublic native PostgreSQL entryのCLI-only input check依存、preparation固有diagnostic、2つの `SET ... TRANSACTION` spelling漏れ、attached behaviorからinbound-FK triggerとcolumn ACLの欠落を発見した | native entryはcontext-specific shared validatorでlibpq load前にambient/complete-URL validationを再実行する。screeningは `SET LOCAL TRANSACTION` と `SET SESSION TRANSACTION` をrejectする。joined inventoryは全table triggerとnon-owner column ACLを数え、owner testで固定する。 |
| full-diff reviewがtoken-free SQLite tailのrejectとunchanged Forbidden historyのdestructive rewriteを発見した | SQLite screeningは全complete statementを確認した後のtoken-free trailing byteを無視する。両adapterはunchanged Applying rowをin-place updateし、changed/missing snapshotだけをrestoreする。SQLiteはinbound foreign keyをrejectするため、必要なrestoreがapplication dataへcascadeしない。 |
| required PostgreSQL CIがfirst-run bootstrapでabsent tableだけをmissing historyとして扱い、到達したinventory queryがambiguousなinternal-`"char"` concatenationとdimensionless empty ACL arrayに依存していたことを発見した | 全blind history lockはPostgreSQLの両absent-object SQLSTATEを認識する。missing tableは `42P01`、missing schemaは `3F000` とし、後続のexact inventory queryが両owned objectのabsentを判定する。signature構築前に全internal-character discriminatorをtextへcastし、NULL column ACLは `aclexplode` 前に `acldefault('c', owner)` で補う。 |
| A1 status-closure reviewがPostgreSQL migration user SQLはCOPY modeへ入り同connectionへrollbackし得ると発見した | PostgreSQL migration screeningは全top-level first-token `COPY`をstatement順でURL read、target open、lock、history publication、libpq前にrejectする。exact diagnostic/lexical near-missはA1 status prerequisiteがretainするQ5a ownerとする。 |

### D12 — category metadataとEXPLAIN

database/schema/table/column/key/constraint/index/Query/planの分離、各common categoryの
明示region、exact flat result shape、`MetaOption` slice、native formの追加native option
slice、§11〜§13のexact metadata/EXPLAIN option sum、PostgreSQL native detail、
SQLite native detail、1 categoryが無関係categoryをfetchしないtest。native result解放後も
out arenaまでstringが有効、hidden heapなし、§18.2.1の全category/detail/entry projection、
ordering、ordinal base、unavailable field、artifact digest、multi-term flat ordering、NotFoundを
固定する。両driverで全 `SchemaRef`/`TableRef` componentのU+0000をnative/catalog request前に
exact Query-less Encode itemとしてrejectする。matrixはDeclared/checked/unknown-nullability
cell、ParameterとColumnを両方持つQuery、schema/nameが両方invalidなTableRef、同名constraint
2個についてtermが同一でaction/deferral policyだけが異なる場合もcanonical
`key_ordinal` groupを固定する。矛盾するnative policy rowはordinalを与えずfailする。
unnamed constraintは `name = None` を返す。
§18.2のpositional call exampleをparse/formatし、signature notationはowning API tableと
比較する。separate compiled Queryのmetadata rowがproducer-owned plan/thunkだけからruntime
artifact I/Oなしで得られることを固定する。
`EXPLAIN ANALYZE` は実行を明示する。

#### Q5b1/D12 static Query metadata implementation closure matrix

Q5b1は独立して有用な最初のD12 consumerである。complete common `meta_query` operationを
publishし、immutable Query descriptorをexact generated materializer thunkで拡張する。
database/schema/table/column/key/index catalog inspectionとEXPLAINはQ5b2に残す。このboundaryが
およそ1,000 hand-written lineを超えるのは、public record surface、producer thunk、descriptor
ABI、common package consumer、separate-compilation linkage、region owner testが1つのsafety
chainだからである。どれかを分割すると、読めないproducerまたはreflection/runtime-artifact
fallbackをpublishする。Q5b2はnative catalogとstatement executionという別failure domainで、
その下に有用でstableなQ5b1 consumerを持つ。

| Closure cell | Required implementation closure | Exact owner evidence |
|---|---|---|
| public typeとoption formation | §18の全`MetaDetail`、metadata discriminator、ref、exact flat common record declarationと、D12-owned common/SQLite/PostgreSQL metadata/EXPLAIN option sumを追加する。Q5b1 boundaryは`meta_query`だけをpublishし、残るexact signatureはstubやhidden overloadを置かずQ5b2へまとめて残した。 | `pkg_db_q5b1::q5b1_query_surface_remains_exact_after_catalog_consumers_land` |
| descriptor formationとvalidation | fixed 8-aligned execution descriptorを96-byte v1から104-byte v2へbumpする。offset 96はexactly 1 native pointerで、Queryではnon-null、commandではnull、既存offsetは全て不変とする。Query identityをtrustする前にversion/reserved/kind/mask/string/thunk agreementを検証する。全execution pathをatomicに更新し、malformed headerはoption、driver state、thunk invocation前にQuery-lessでfailする。 | `pkg_db_q1::generated_runtime_data_is_producer_owned`、既存`pkg_db_q2` SQLite/PostgreSQL execution owner |
| producer planからgenerated code | D1 planと`StaticQueryArtifact`に既にあるD3/D5 checked evidenceから、Queryごとにmonomorph-free thunkを1つ生成する。exact native signatureは`fn(driver_tag: u8, detail_tag: u8, row_index: i64) -> Option<QueryMeta>`で、driver/detail tagはpublic declaration order、nonnegative indexは§18 row orderを`None`まで辿る。thunkはimmutable literalをembedし、runtime source/interface/artifact/metadata-file/decoder/reflection/native/allocation workを行わない。 | `pkg_db_q5b1::static_query_metadata_thunk_links_from_its_producer_unit` |
| detail、state、discriminator、order | NamesではSummaryをexactly 1つ、Summary/FullではSummary、distinct parameterをone-based protocol order、columnをzero-based decoder orderでmaterializeする。Declared/DatabaseCheckedとavailable/ambiguous evidenceについて§18.2.1の全field-presence ruleを適用し、non-applicable fieldは`None`、必要なcellではnullableを`Unknown`に保つ。 | `pkg_db_q5b1::static_query_metadata_materializes_exact_declared_projections`、`checked_query_metadata_projection_uses_only_selected_driver_evidence` |
| identityとdigest | exact Query/driver restriction/class/artifact/state/source hash/wire hash/rewrite versionを全rowでrepeatする。exact D1 byteのartifact digest、selected driver entryだけ、checked metadata digestをfingerprintとして使い、checked prepare/schema/server identityは許されたSummary/Full cellだけに置く。 | Q5b1の2 projection ownerと`pkg_db_q1::artifact_semantics_and_checked_in_goldens` |
| package dispatchとerror | `meta_query`はexplicit `exec`、Query、detail、destination region、common option sliceを1つずつ取る。complete descriptor、source-order option、driver restriction、live exec stateの順に検証してからthunkを呼ぶ。Query-specific failureは`Some(query_id)`、untrusted descriptorは`None`を保ち、SQL/catalog requestを行わない。 | `pkg_db_q5b1::meta_query_rejects_non_live_execution_targets_before_materialization`、Q5b1 declared-projection runtime owner、既存`pkg_db_q2::common_surface_dispatches_to_*` owner |
| region ownershipとallocation | 全returned string leafを`out`へcloneし、exact flat recordを`array_builder<QueryMeta>(out)`でpushし、one compacting buildを行う。generated thunkはstatic viewを返しallocationしない。final arrayと全string byteは`out`だけを使い、source thunk/native cleanup後も生存し、region外へescapeできない。 | Q5b1 declared-projection runtime ownerとthunk MIR whitelist、`align_mir::tagged_copy_fields_from_dynamic_struct_arrays_survive_the_hir_gate` |
| separateとgeneric compilation | materializerをdescriptor producer objectに置き、immutable static dataからreferenceし、descriptor relocationを通してexact thunk targetだけをimportする。consumer-side Query-body instantiationなしでconcrete common package codeから呼ぶ。whole-program/per-unit outputはpublic/private Queryで一致する。 | `pkg_db_q5b1::static_query_metadata_thunk_links_from_its_producer_unit`、whole-program ownerのentry-private Query |
| construction、move、return、cleanup | Query descriptorはconstruction、local copy、argument、branch join、returnを通じてCopy immutable valueのままにする。`meta_query`はQuery/exec ownerをmoveせず、return後にreferenceをretainせず、success pathごとにregion builderをexactly once consume/buildする。failureはtemporary stateをreleaseし`out`を終了しない。 | `pkg_db_q1::typed_descriptor_contract_matrix`、Q5b1 whole/per-unit runtime owner |
| malformed compiler input | handcrafted HIR/MIRのwrong materializer offset/signature、command use、return type、parameter type/mode、non-static descriptor source、header relocationをfail closedにする。malformed formをunchecked raw callとしてcodegenへ到達させない。 | `pkg_db_q5b1::materializer_call_is_query_only_and_exactly_typed`、`align_mir::static_descriptor_bridge_retains_concrete_bare_call_abis` |
| explicit Q5b2 deferral | Q5b1 boundaryはdatabase/schema/table/column/key/index catalog operation、typed identifier U+0000 precedence、native option application、canonical constraint/index ordering、common/native EXPLAINとvisible PostgreSQL ANALYZEを後続Q5b2 capabilityへ意図的に残した。これらのD12 acceptance cellを完了扱いせずplaceholder implementationも追加しなかった。 | 下記Q5b2 matrixでcloseし、`pkg_db_q5b1::q5b1_query_surface_remains_exact_after_catalog_consumers_land`がQ5b1-owned typeを保持する。 |

#### Q5b2/D12 native catalogとEXPLAIN implementation closure matrix

Q5b2は残るD12 read-only failure domain、すなわちcommon catalog operation、両driver-qualified
option path、common/native EXPLAINを1 capabilityで閉じる。identifier validation、native row
ownership、canonical constraint/index reconstruction、static Query binding、両driver cleanup pathが
1つのpublic result contractを形成するため、約1,000 hand-written lineを超えるboundaryである。
category分割はincomplete inspection surfaceをpublishし同じnative cursor/region proofを反復する。
EXPLAIN分割もlive-exec、option precedence、descriptor、binding、cleanup boundaryを反復する。
Q5b1はこのconsumerの下でindependently usefulなstatic producerとして残る。

driver-qualified metadata functionは対応するcommon return recordと同じcommon argument orderを使い、
その後に1つのnative option sliceを受け取る。driver-qualified EXPLAIN functionはcommon signatureの
後に1つのnative EXPLAIN option sliceを受け取る。最初のD12 implementationではnative-only recordを
導入しない。後続のadditive native detailはこれらcommon-record formを変えずdistinct recordと
operationを追加できる。

| Closure cell | Required implementation closure | Exact owner evidence |
|---|---|---|
| exact public operation surface | §18.2でdeferした7 common catalog signatureとcommon `explain`を全てpublishする。対応する`sqlite.meta_*_native`/`postgres.meta_*_native`は同じcommon recordを返し、`out`、common option、native optionの順とする。両generic `explain_native`も同じ順でpublishする。optionless overload、hidden destination、catalog stubを残さない。 | `apps/db/pkg/db.align`、`apps/db/pkg/db/sqlite.align`、`apps/db/pkg/db/postgres.align`、`pkg_db_q5b2::q5b2_publishes_exact_common_and_native_surface` |
| complete validation phaseとprecedence | category callはcommon optionをsource order、native optionをsource order、typed identifier fieldをdeclaration order、applicableなdriver restriction、complete live exec prefixの順に検証し、その後だけlease/native callへ進む。EXPLAINはidentityを信頼する前にcomplete descriptor、その後common option、native option、restriction、live state、generated static option/bind、lease、sendの順とする。malformed headerはQuery-less、category failureはQuery-less、Query-specific failureは`Some(query_id)`を保つ。全losing phaseをno-send/no-lease ownerで固定する。 | `apps/db/pkg/db.align`と両public driver module、`pkg_db_q5b2::sqlite_native_option_matrix_and_validation_precedence_are_exact`、`common_sqlite_explain_is_bound_and_inspection_only`、`postgres_native_generic_bridge_compiles_and_rejects_wrong_driver` |
| typed identifier encoding | `SchemaRef.name`、次に`TableRef.schema`、次に`TableRef.name`のU+0000を§18.2 exact Encode item/messageでnative work前にrejectする。accepted UTF-8 identifierはdriver adapterでquoteまたはbindし、unchecked identifierをSQLへpasteしない。 | `apps/db/pkg/db.align`のshared validator/bound catalog parameter、`pkg_db_q5b2::sqlite_database_schema_table_and_column_projection_is_exact`、`sqlite_native_option_matrix_and_validation_precedence_are_exact` |
| common option disposition | positive metadata/EXPLAIN timeoutは最大1つ、`IncludeSystem`も最大1つだけ受け付け、duplicateはsource orderでrejectする。v1 metadata/EXPLAIN deadlineの最終D9 dispositionは両driverともpre-send `Unsupported`であり、silent ignoreしない。`IncludeSystem`はsystem-object selectionだけへ作用し、static Query metadataには作用しない。 | `apps/db/pkg/db.align`、`pkg_db_q5b2::sqlite_native_option_matrix_and_validation_precedence_are_exact`、`common_sqlite_explain_is_bound_and_inspection_only`、`postgres_required_catalog_and_explain_contract_is_exact` |
| native option disposition | SQLiteは各metadata flagを最大1つ受け付け、internal-object/hidden-column flagをowning categoryだけへ適用し、EXPLAIN modeをexactly 1つ選択する。empty defaultはQueryPlan。PostgreSQLは`SearchPathOnly`と`IncludeSystemCatalogs`をconflictにし、duplicate metadata tagをrejectする。各EXPLAIN fieldは最大1つ、text/server-default booleanがdefault。Buffers/Timing/Wal without Analyzeはexecution前にrejectする。 | `apps/db/pkg/db/{sqlite,postgres}.align`、SQLite native/plan owner、PostgreSQL bridge owner、required PostgreSQL owner |
| live executionとoverlap | selected driverのexact versioned、unpoisoned connection prefixだけ受け付ける。SQLite/PostgreSQL catalog/EXPLAINはfirst native call前にtyped executionと同じconnection-wide leaseを取得し、success/errorの全pathでstatement/result/context finalization後だけreleaseする。PostgreSQL overlapは`Unsupported`、item `postgres.connection.active_execution`、message `PostgreSQL connection already has an active execution`、catalogはquery_id None、EXPLAINはSome(query_id)でlibpq callを行わない。transaction contractはmalformed stateをreinterpretしない。 | `apps/db/pkg/db.align`と`apps/db/pkg/db/internal/{postgres,resource}.align`、SQLite lease owner、`postgres_catalog_and_explain_share_the_execution_lease`、PostgreSQL bridge owner |
| database/schema/table projection | requested categoryだけfetchする。§18.2.1 required identity、detail-selected optional、system/visibility semantics、byte-lexicographic category order、exactly 1つの`DatabaseMeta`を生成する。`meta_table` absenceは`NotFound`を返しcolumns/keys/indexesをfetchしない。SQLiteはsearch-path ownershipをinventせずattached databaseをmapし、PostgreSQLはserver catalogからvisibilityを計算する。 | `apps/db/pkg/db.align`のcatalog scanner、`pkg_db_q5b2::sqlite_database_schema_table_and_column_projection_is_exact`、required `postgres_required_catalog_and_explain_contract_is_exact` |
| column projection | physical zero-based orderを保つ。Namesは全optionalをsuppressし`Unknown`を強制、Summaryはlogical/native typeとcatalog nullability、Fullはavailableなnative/default/generated/identity/collation/comment/origin evidenceだけを追加する。SQLite hidden columnはnative flag時だけ出す。malformed native ordinal/countはallocation indexing前にfailする。 | `apps/db/pkg/db.align`のcolumn scanner、SQLite catalog/native/malformed-result owner、required PostgreSQL owner |
| key groupingとcanonical order | complete primary/unique/foreign/check/exclusion/native groupをprojection前に構築する。全group-level valueをnormalizeし、contradictory native rowをrejectし、complete Full signatureでsort後zero-based `key_ordinal`を割り当てzero-based term orderを保つ。same-named/unnamed constraintをfabricated nameなしで区別する。 | `apps/db/pkg/db.align`のkey scanner/canonical SQL、SQLite catalog owner、malformed-result owner、required PostgreSQL owner |
| index reconstructionとorder | complete indexをreconstructし、key termをincluded termより前に置き、両方を通じて1つのzero-based ordinalを割り当て、byte-lexicographic nameとordinalでrow sortする。identity/order construction後にNames/Summary/Full presenceを適用する。expression/predicateはguessせずoptional evidenceとする。 | `apps/db/pkg/db.align`のindex scanner、SQLite catalog owner、required PostgreSQL owner |
| native text、count、cursor safety | allocation/pointer arithmetic前に全native row/column count/lengthをboundし、`str`構築前にrequired UTF-8をvalidateし、SQL NULLとempty textを区別する。finalizationがerrorを置換する前にerrorをcopyし、全non-null native resultをexactly once finalize/clearする。1つのmalformed-row guardを変更した場合は全sibling catalog scannerをauditする。 | `apps/db/pkg/db.align`、`apps/db/pkg/db/internal/{sqlite,postgres,resource}.align`、`pkg_db_q5b2::malformed_native_catalog_rows_close_once_and_sqlite_releases_the_lease` |
| explicit-region ownership | 全result arrayを`array_builder<T>(out)`で構築し、native storage finalize/clear前に全string leafを`out`へcloneする。QueryPlan bodyも同じ。temporary grouping storageはexplicit nested regionを使いescapeしない。success pathはheapを選ばない。 | `apps/db/pkg/db.align`と両internal adapterの全catalog/plan builder、SQLite runtime owner、malformed-result owner、required PostgreSQL owner |
| common EXPLAIN | v2 Query descriptorとselected driverをexecution同様にvalidateし、producer-owned binderでParamsをbindし、inspection-only EXPLAINを1回issueする。SQLite common outputはQueryPlan text、PostgreSQL common outputはtextかつ`analyzed = false`。common formはnamed statementを実行しない。 | `apps/db/pkg/db.align`、両internal adapter、`crates/align_sema/src/lib.rs`、SQLite common/per-unit owner、required PostgreSQL owner |
| native EXPLAINとvisible execution | SQLite QueryPlan/Bytecodeはいずれもinspection-only。PostgreSQLは1つのfixed option clauseを構築し、exact Text/Json formatを返し、explicit Analyzeだけがnamed Queryをexactly once実行し`analyzed = true`にする。execution-count instrumentationに含める。option valueはambient configurationからinterpolateしない。 | `apps/db/pkg/db/{sqlite,postgres}.align`と両internal adapter、SQLite plan owner、PostgreSQL bridge owner、required PostgreSQL execution-counter owner |
| construction、return、cleanup | metadata refとQuery/exec valueはsynchronous call中だけborrowする。successはfully initialized RegionPlain value/arrayを1つ返す。全prepare/bind/step/result errorはfirst owned errorを保ち、statement/result/context/leaseをnative orderでreleaseする。PostgreSQL catalogはbuffered-result cursor clearまで、EXPLAINはresult/context clearまでleaseをownする。early `?`、branch join、malformed rowはcleanup skip/double release/partial publicationを起こさない。 | `apps/db/pkg/db.align`と`apps/db/pkg/db/internal/{sqlite,postgres,resource}.align`、malformed-result owner、`postgres_catalog_and_explain_share_the_execution_lease`、SQLite catalog/plan owner |
| separate compilationとlinkage | common generic EXPLAINはP/R concrete後、execution同様にavailableなselected native engineだけをqueueする。catalog codeはlive connectionのlinked driverを使いsource/artifact/reflection I/Oを行わない。whole-program/per-unit behaviorとABIは一致し、affected link pathでは各native libraryをconsumerより前に置く。 | `crates/align_sema/src/lib.rs`のgeneric bridgeとpublic/internal adapter、`pkg_db_q5b2::sqlite_explain_generic_bridge_links_per_unit`と`postgres_native_generic_bridge_compiles_and_rejects_wrong_driver`のwhole/per-unit native link closure |

candidate review前にauthor-side matrix-to-diff passで全applicable cellへ1 source pathと1 ownerを
対応させる。validation、native-row、cleanup pathのfindingはcoherent fix commit前に同じroot-cause
classのsibling-path auditを1回行う。

### D13 — batch、SoA、高価値native path

#### A1 common batch/SoA public-contract ledger

これは最初のindependently useful D13 railのsource of truthである。generic type formation、
generated Query artifact、両driver、resource cleanupを横断するが、どのproducer seamも他を
欠くとstable consumerを持たず、分割は同じownership/malformed-input proofを重複させるため
1 capability boundaryとする。1,000 hand-written lines超を見込む理由もこれである。
PostgreSQL deliveryはこのrailでは`BufferedFull`のままで、native delivery optionは別rail。

```align
pub resource batch<R> = pkg.db.internal.resource.drop_batch
pub fn next_batch<R>(borrow mut stream: rows<R>, max_rows: i64)
  -> Result<Option<batch<R>>, Error>
pub fn batch_len<R>(borrow values: batch<R>) -> Result<i64, Error>
pub fn batch_row<R>(borrow values: batch<R>, index: i64) -> Result<Option<R>, Error>
pub fn batch_soa<R: SoaPlain>(borrow values: batch<R>) -> Result<soa<R>, Error>
```

`SoaPlain`は`RegionPlain`より狭いclosed builtin structural boundで、nonempty concrete structの
fieldがdeclaration orderで`bool`、`char`、integer、float、`str`だけの場合にexactly成立する。
generic template内のsymbolic `soa<R>` formation/returnだけを許可し、user trait、dictionary、
reflection、implicit conversion、abstract `R` operationを追加しない。public generic template
interfaceはcanonical symbolic `soa<Param(R)>`と`SoaPlain` boundをserializeし、separate consumerが
signatureをreconstruct/instantiateできる。concrete call時に既存`Ty::Soa(struct_id)`へsubstituteし、
MoveCheck/EscapeCheck/emitted HIR/MIR/codegen前に通常のSoA field ruleを再検証する。abstract SoAは
emitted HIR/MIRへ到達しない。non-`SoaPlain` Rowは他のbatch operationを使えるが`batch_soa`は
compile-time rejectされ、runtime downgradeしない。

このfirst-batch ledgerのstatement-v2/rows-v2 ABI entryは、そのlanding boundaryを記述する。
cumulative current recordは後続PostgreSQL delivery ledger §23が固定するstatement-v3/rows-v3である。

| Public record | Exact contract |
|---|---|
| input/default | `max_rows`はdefaultなしで`1..=2_147_483_647`。supplied live `rows<R>`だけをadvanceし、追加SQLをsendしない。first railはSQLite `Step`とPostgreSQL `BufferedFull`をshipし、後続delivery ledgerがcommon callを変えず`SingleRow`/`PortalBatch`を追加する。 |
| result/order | native delivery orderの連続rowを最大`max_rows` decodeする。endで0 rowなら`Ok(None)`、`Some`は必ずnonempty。capを超えてprobeしないためSQLite/streamed PostgreSQLのexact-cap exhaustionは次callで観測可能。PostgreSQL `BufferedFull`は既知buffered countを利用できる。 |
| validation/error precedence | complete rows wrapper、Query identity、producer batch-plan headerを最初に検証し、malformedは`Error.InvalidQuery`、query_id None、item `db.rows.header`、message `invalid database rows resource`。次に`max_rows`を検証し、範囲外はquery-specific `Unsupported` item `db.batch.max_rows`、message `database batch size must be between 1 and 2147483647 rows`。terminalは3番目で、exhaustedは`Ok(None)`、failedはquery-specific `Error.InvalidQuery` item `db.rows.state`、message `database rows cannot advance after a failed execution`。その後layout arithmetic/native advancement。delivered rowごとに既存generated row validatorをdeclaration orderで先に実行し、そのcached contextとexact driver-specific `Decode`/`Native` protocolを維持する。成功後だけappendする。layout/storage overflowはそれぞれitem `db.batch.layout` / `db.batch.storage`とEnglish ledgerのexact messageを返す。各accessorは最初のload/thunk call前にcomplete batch/planを検証し、failureはQuery-less `InvalidQuery` item `db.batch.header`、message `invalid database batch resource`。この順序がmulti-invalidにも適用される。 |
| ownership/lifetime | `batch<R>`はindependent Move resource。text/blob bytesをbatch-owned storageへcopyしてからnative rowをadvanceするので複数batch coexistとrows advance/Dropが可能。header成功後、`batch_row`はnegative/out-of-rangeで`Ok(None)`、それ以外でdeclaration-orderの`R`を再構築し、全viewをbatch generationだけへrootする。`batch_soa`も同じroot。batch move/Drop後は使用不可。 |
| allocation/layout | 1つのgeometric fixed-column blockとvariable fieldごとのgeometric child chainをresourceが所有する。primitive/value/headerはtyped column、nullableはpacked validity bitmap。null laneのvalue/header bytesを読まない。child bytesはappend後に再copy/compactしない。final lenがcapacity未満ならfixed columnだけを既存exact-length SoA offset ruleへin-placeで1回compactする。AoS/transposeなし。checked arithmeticはallocation/write前。OOMは通常のAlign allocation abort。 |
| failure/cleanup | 既存row validatorがnative valueを検証し、generated appendはaggregate child growthをlane mutation前に検証する。error時はpartial batchを非公開のままdestroy後、`next`と同じfirst-error-preserving orderでrows/nativeをclose/poisonする。0-row/partial-final exhaustionはpublication前にdriver cleanupを完了する。SQLite failureはunpublished batchをdestroyし既存query-specific `db.rows.cleanup` errorを返す。PostgreSQL `BufferedFull` cleanupはinfallibleのまま、streamed modeは後続ledgerのfallible restoreとexact PostgreSQL cleanup errorに従う。poisoned connectionをrestoreしない。Dropはchild chain/fixed block/payload/wrapperをexactly once解放しpartial constructionも同順。 |
| producer/cache | static Query producerだけがlayout/decodeを生成し、packageはproducer-owned typed thunk経由でdispatchする。field-name lookup/reflection/source/artifact/cache I/O/consumer instantiationなし。descriptor/plan bytes、field kind/nullability/order、thunk body、row structural fingerprint、compiler/dependency hashesは既存artifact/object/cache identityへ入る。whole/per-unitでplan/thunk/selected driverをretainする。 |
| ABI | Query descriptorは136-byte、8-align v5。offset 0--127はv4と同じ、query offset 128はnonnull batch-plan v1 pointer、commandはnull。planは72-byte、8-align: version u32=1@0、flags u8@4(bit0=`SoaPlain`のみ)、reserved 5--7=0、exact field_count u32@8（既存のzero-column Query/empty Rowでは0）、reserved u32@12=0、nonnull `create`/`append`/`finish`/`row`/`drop` pointer@16/24/32/40/56、`soa`@48はbit0とexactly一致、tail_reserved u64@64=0。empty Rowはflags clearかつ`soa` null。prepared statementは80-byte v2で0--71 unchanged、plan@72。rowsは96-byte v2で0--87 unchanged、plan@88。batch stateは48-byte、8-align v1: version u32=1@0、state u8@4 (`0=live`、`1=closed`、他はmalformed)、copied plan flags u8@5、reserved u16@6=0、nonnull plan/payload@8/16、nonzero len i64@24、requested max_rows i64@32 (`len <= max_rows <= 2_147_483_647`)、tail_reserved u64@40=0。flagsはvalidated planと一致する。accessorはstate 0だけを受け、Dropは0を受けてpayload Drop thunk前に1を書き、他tagではplan thunkを呼ばない。template interfaceはsymbolic `soa<Param(R)>`+`SoaPlain`を記録する。mono key/Row fingerprintはcomplete reachable Row graphのstructural identity、resource/functionはcanonical producer nominal identity。independent semantic-to-byte/byte-to-semantic goldenが両record、field ordinal/tag、malformed reserved/tag/pointer rejection、sequence orderを固定する。 |
| thunk ABI | `create(i64)->raw`はcomplete fixed-layout representabilityをallocation前にcheckし、そのfailureだけnull。既存row validatorが成功しtrusted current-row contextを作った後だけ`append(raw,raw,resource_ref<rows<R>>)->i32`を呼び、atomic direct-column append成功で0、aggregate child overflowで1。appendはnative valueをclassifyしない。`finish(raw,i64)`はfixed compactを最大1回。`row(raw,resource_ref<batch<R>>,i64)->R`、eligible時だけ`soa(raw,resource_ref<batch<R>>)->soa<R>`、`drop(raw)`。symbolic SoAはtemplate/interfaceだけで、abstract `R`/generic thunk/unvalidated signatureはemitted HIR/MIRへ到達しない。 |
| prerequisite/acceptance | shipped L2/L3/L7/D8とconcrete SoA ABIが前提。ownerはexact public surface/bound whole-per-unit、descriptor/plan byte+signature golden、fake direct-column/layout、SQLite/PostgreSQL lifecycle/error/cleanup、malformed HIR/state、alloc-count Drop balance。direct SoA/batch measurementはcorrectness後にallocation/copy/compact countを記録するだけでgateではない。 |

Implementation closure matrix:

| Cell | Closure | Owner evidence |
|---|---|---|
| formation/validation | `R: SoaPlain`時だけsymbolic SoAを形成しanalysis前にconcrete化。新しいrecursive generic-container例外はexact abstract producer resource `Option<pkg.db.batch<R>>`だけで、他の`Option`内abstract nominalはL7 rejectを維持する。descriptor/plan/statement/rows/batchのversion/reserved/pointerをdispatch前に検証する。zero-field planはSoA bit clearかつSoA thunk null、SoA-capable planはfield count nonzeroでなければならず、commandはplanを持たない。 | accepted `Option<batch<R>>` surfaceとcumulative rejected `Option<Wrap<T>>` owner、malformed HIR、zero-field/SoA-bit/thunk v5/plan golden |
| construction/move-in | rows/max/terminal後に1 unpublished payloadをcreateし、既存row validator成功後にtrusted contextをtyped columnへ直接decodeし、aggregate growthを検証してcommit。textはnative length-aware bytesを使いvalid UTF-8を要求し、embedded U+0000をbyte-exactに保持する。blobはzeroを含む全byteを保持。invalid pointer/length/UTF-8はgrowth/lane mutation前にfail。 | fake all-kind/nullable/empty/partial/exact-cap、両driver direct-column owner |
| move-out/view | row gatherとeligible SoA viewをbatch rootで返す。`batch_soa`はSoA thunk load前にcomplete live batchをvalidateし、copied plan flagsがSoA capabilityを示すことを要求する。valid non-SoA planはindirect dispatchせず`InvalidQuery`を返す。Option/generic/branch/`?`を通してprovenanceを保持。 | whole/per-unit lifetime、post-move/post-Drop rejection、malformed non-SoA batch no-dispatch owner |
| owner transfer/Drop | block/`if`/`match`/`else`/`?`/replacement/returnで1 ownerだけtransferし、empty/partial/compact/uncompact/terminalの全stateをexactly once cleanup。 | L3 matrix、batch branch/replacement/failpoint/alloc-count owner |
| driver path | SQLiteはcap超過stepなし、PostgreSQLはcap超過decode/sendなし。partial errorはbatch後native rowsをfirst-error orderでclose。 | zero/partial/exact/multi/decode/native/Drop counter owner |
| nullable/variable layout | 全value kind×nullable、empty/nonempty text/blob、null/empty、child/fixed growth、compact有無をcrossし、gather/SoA order一致。 | plan matrix、bitmap/header/offset golden、UTF-8/length malformed owner |
| generic/separate compile | 全operationで`batch<R>`からR inference、public template interfaceだけにcanonical symbolic `soa<Param(R)>`+`SoaPlain`をserializeし、genericから別genericへのforward時はparameter indexが異なってもcallee-to-caller type-argument mapでsymbolic parameterをremapする。concrete bound reject、HIR/MIRへabstract SoA publicationなし、whole/per-unit retain。 | interface golden、wrong-bound diagnostic、different-index direct/tagged-result symbolic-SoA forwarding owner、whole/per-unit executable |
| ABI/allocation parity | direct/preparedが同planをv2 stateへ運び、fake/SQLite/PostgreSQLが同じv1 batch stateを使う。producer/package/HIR/MIR/LLVMでsize/offset/signature/cleanup provenance一致。LLVM preflightはstatic recordをexact shapeとは独立にversion/kind headerからv5 Query descriptor candidateと分類し、size/alignmentがexact 136 bytes/8でなければrejectした後、offset 128のnested planを1 semantic recordとして認識する。そこでexact 72-byte image、callback ordinal set、stored declaration signature、Row/rows/batch resource pairing、field count、SoA flagをemission前に検証する。さらに全callbackを同じRowのexact producer bodyへbindする。`create`/`finish`/`row`/`soa`/`drop`はsingle column-batch operationより前にprefixを持たず、`append`はcontext argument 1と一致するfield ordinalを読むdeclared Row順の`pkg.db.internal` reader callだけを持ち、そのresultをfinal append inputへ1対1で接続し、他のprefixを持たない。全operationの`struct_id`とtyped callbackのproducer resourceも一致させるため、別descriptor callbackもpayload use前のraw mutation/freeもspliceできない。semaと同じnonempty concrete `SoaPlain` structだけに`ColumnBatchSoa`を許可し、vacuous field iterationでzero fieldsを通さない。 | byte/relocation/HIR ABI golden、short/long/misaligned v5 Query descriptorのparameterized rejection、全plan callback ordinalのparameterized wrong-signature test、cross-Row raw-only callback splice rejection、parameterized callback-prefix mutation rejection、exact append reader/input topology、empty-struct `ColumnBatchSoa` rejection、alloc-count parity |

`7bddab67` independent design reviewの7 findingはledger-firstの1 passで閉じた。symbolic SoAは
template interfaceに保存しemitted HIR/MIRだけから除外、native validationは既存row validatorを
append前に再利用、state tagは`0=live`/`1=closed`、terminal cleanup failureはpartial batchを
destroyしてexact `db.rows.cleanup`、failed stateはexact `db.rows.state`、common inventoryへ
`db.batch<R>`を追加し、`docs/design-notes.md`へ`SoaPlain`/resource-root rationaleを記録した。
各closureはEnglish tableのinterface/native-malformed/tag/cleanup/precedence/surface/consistency
ownerで固定する。

required author-side ledger-to-prose/matrix passとfresh independent adversarial design reviewは
`7bddab67`で完了し、上記complete finding setをimplementation前に閉じた。後続code review前は
matrix-to-diff passだけを再実施し、全cellへimplementation pathとownerを対応させるか別A1
railへのexplicit deferを記す。

#### A1 PostgreSQL streamed-delivery public-contract ledger

これは2番目のindependently useful D13 railのsource of truthである。common `db.rows<R>` /
`db.batch<R>`を変えず、explicit PostgreSQL single-rowとbounded chunk deliveryを追加する。
PostgreSQL binary parameter/result formatはPGresult lifetime/cancellation state machineを共有しない
後続railとし、このimplementation boundaryへ含めない。COPY、pipeline、LISTEN/NOTIFYも別railである。

direct/preparedは最終的に1 connection-owned libpq result sequenceと同じnext/batch/timeout/cancel/
Drop engineを使うがformation closureは異なる。prepared native option validationにはproducerの
parameter-name resolverをstatement stateへretainする必要がある。よってcomplete direct consumerを
先にshipし、prepared parityは同engineを再利用するsmaller independent state/formation extensionとする。
どちらもcompiler IR variantやdriver-independent abstractionを追加しない。

implementationにはさらに2 independently correct prerequisitesがある。first PRはQ5b2 matrixに残る
PostgreSQL libpq-consumer lease gapを閉じ、全catalogとcommon/native EXPLAINがtyped executionと
同じconnection leaseを取得しresult/context cleanupまで保持し、overlap時libpqを呼ばないようにする。
second PRは全shipped package-owned PGresult consumerを1 sealed status authorityへrouteし、COPY、
pipeline、unknown numeric statusをfail closedにする。同prerequisiteは唯一のshipped compiler-side
user-SQL gapも閉じ、全top-level PostgreSQL migration `COPY`をtarget open前にrejectする。さらに全Rust
prepare/migration PGresult consumerも1 private exhaustive tool-status authorityへrouteし、nullまたは
deferred resultならrollback、deallocation、row access、later libpq前にconnectionをclose+null化する。
両prerequisiteともapplication-callable package surface/ABIを変えずこの順にmergeする。third PRはdirect
delivery/rows ABI/stream protocol、fourth PRは`rows_stmt_native`とprepared
parameter-name authorityを追加する。各prerequisiteはindependently usefulなshipped safety gapを閉じる。
direct boundaryが約1,000行のcheckpointを超えるのは意図的である。rows-v3 formation、result-sequence
consumer、cleanup authorityを分割するとdormant producer/consumer chainが残り、ownershipと
fail-closed proofが重複してintegration riskが高くなる。
prepared parityもstatement-v3 formation、guarded compiler bridge、conn/tx delivery ownerを1 safety
closureとして扱うため、このcheckpointを超え得る。分割するとdormant resolver ABIをpublishするか、
malformed-HIRとretained-resolver proofを欠くcallable delivery pathを残すことになる。

direct/prepared rail完了後のexact cumulative application-callable surfaceを次に示す。各宣言を
どのPRがpublishするかは直後のstaged boundaryが固定する。

```align
pub Delivery {
  SingleRow
  PortalBatch(i64)
}

pub ExecuteOption {
  ParameterFormat(str, Format)
  ResultFormat(Format)
  Delivery(Delivery)
}

pub fn rows_stmt_native<P, R>(
  borrow mut statement: db.stmt<P, R>,
  params: P,
  options: slice<db.ExecuteOption>,
  native: slice<postgres.ExecuteOption>,
) -> Result<db.rows<R>, db.Error>
```

direct PRで既存signatureの`postgres.rows_native`/`postgres.one_native`がnew optionを受理し、
`postgres.execute_native`はcommandなのでrejectする。prepared parity PRがexactly
`postgres.rows_stmt_native`を追加する。common operationへのnative option、optionless overload、
new `maybe_one_native`/`all_native`/cursor/portal/cancel resourceはない。Deliveryなしはexactly shipped
caller-synchronous `BufferedFull`で、TimeoutNs時の既存nonblocking deadline実装も保持する。
`SingleRow`は`PQsetSingleRowMode`、`PortalBatch(max_rows)`は`PQsetChunkedRowsMode`へmapする。
libpq側の名称はchunked-row modeだがpublic observation labelは`PortalBatch`である。

`PQsetChunkedRowsMode`とnonblocking cancel-connection APIを必要とするためsupported PostgreSQL
client baselineはlibpq 17.0へ上がる。direct linkを使い、`dlsym` fallback、older-client downgrade、
ambient feature probeは行わない。required CI/local evidenceはclient >=17を表示し、PostgreSQL server
16.4をcompatibility floorとして維持する。stable libpq ABI majorは5のままである。

| Public record | Exact contract |
|---|---|
| input/default | direct PRでは`Delivery`を`postgres.rows_native`/`postgres.one_native`だけ、prepared parity後は`postgres.rows_stmt_native`も受理する。最大1個。PortalBatchはdefaultなし、`1..=2_147_483_647`をnative intへ渡す。common Timeout/Text optionは不変。Delivery absenceはzero-tag asyncでなくexact existing `BufferedFull` implementation。ambient heuristicなし。 |
| operation/result | Delivery absenceはshipped caller-synchronous BufferedFullへdelegateしprotocol completion、blocking restore、server failureをcomplete rows publication前に終える。Timeoutなしは既存1 `PQexecParams`/`PQexecPrepared`でnonblocking/send/selector zero。Timeoutありは既存`PQsetnonblocking`+`PQsendQueryParams`/`PQsendQueryPrepared`+wait/cancel/drainでselector zero、completion前rows publicationなし。explicit direct/preparedは同send後immediate selectorを呼びfirst rowを待たずlive rowsを返す。`one_native`はfirst Rowだけをvalidate/decodeし、multiplicity probe前にcaller-supplied regionへexactly one ordinary `row.clone_in(out)`を行う。second mode-valid rowはvalidate/decodeせずpending Cardinality 2として記録しoriginal deadlineでnormal drainする。clean completion後だけCardinality 0/2またはone Rowをpublishし、late native/sequence/timeout/cleanup failureが勝つ時Rowはreturnしない。caller regionはmonotonicなのでfirst clone後のCardinality 2/later failureではsuccessful singletonとexact同じclone bytesがarena scope終了までunreachableのまま残り、packageはrewind/reuse/Dropしない。zero rowとfirst-row validation/decode failureは`out`へzero allocation。Cardinalityはcancelしない。 |
| physical delivery/order | `SingleRow`はexactly 1 rowの`PGRES_SINGLE_TUPLE`、`PortalBatch(n)`は`1..=n` rowsの`PGRES_TUPLES_CHUNK`だけを受理しserver orderを保持する。0個以上のdata後zero-row `PGRES_TUPLES_OK`を要求し、nullまで`PQgetResult`を呼ぶ。[libpq 17 §32.6](https://www.postgresql.org/docs/17/libpq-single-row-mode.html)のexact contractである。後続nonnull resultごとにprotocol-state validationをstatus-to-error mappingより先に行う。zero-row terminal観測後の全resultはstatusをcleanup用にclassifyする前にexact invalid-sequence errorを記録するため、post-terminal BAD/NONFATAL/FATALも`execution_error`で上書きしない。terminal前のBAD/NONFATAL/FATALだけが既存native mappingを使う。EMPTY/COMMAND/other-mode/invalid count/nonempty-or-missing terminalもinvalid-sequenceを記録しcurrent resultをonce clearしてordinary no-decode drainを続ける。`PGRES_COPY_OUT`/`PGRES_COPY_IN`/`PGRES_COPY_BOTH`、`PGRES_PIPELINE_SYNC`/`PGRES_PIPELINE_ABORTED`、complete known libpq17 set外のnumeric statusは、deferred COPY/pipeline APIなしでprotocolまたはconnection-global stateをdrain/reuse可能と仮定できないためfail-closedとする。earlier owned errorを保持し、なければ既に記録したpost-terminal invalid-sequenceを使うかその場で記録し、current resultをexactly once clearしてphysical connectionを即poison/closeする。観測後はそのconnectionへ`PQgetResult`、COPY API、`PQexitPipelineMode`、cancel API、`PQtransactionStatus`、blocking restoreを呼ばない。main-result classification開始時にcancel handleは残らない。close後にwrapper mutable native state、unpublished package-owned batchをdestroyし、cloned Rowは`out`をrewindせずunreachable化し、context、unpublished direct Query ID、leaseをcleanupする。published valueは残り、earlier row/storage/mode/timeout errorを保持しDropはsilent。COPY/unknown/command/pipeline/multi-statementをrowsへ再解釈しない。 |
| `next`/view lifetime | `next`は最大1 validated Rowを返し、current resultにunread rowがある間new PGresultをfetchしない。mutating advanceはそのresultをclearする前にprevious rows generationをinvalidateするため、previous `str`/`slice<u8>`/enclosing viewは次advance後に使用不可。data result endでexactly once clearしてから次をwaitする。terminal resultとnull protocol markerをconsumeしconnection synchronized後だけ`Ok(None)`。row validation/decode errorはfirst owned errorとなりRowをpublishせず、return前に下記error-drain ruleへ入る。 |
| `next_batch`/atomicity | common A1 contractを維持する。1 resultの一部または複数をconsumeできるがcaller boundでexactly stopしnext rowをprobeしない。source clear前にview-bearing fieldをunpublished batchへcopyし、bound前native/decode/storage errorはwhole batchをdestroyしてfirst errorを保持しreturn前に下記error-drain ruleへ入る。bound到達後のlater errorはsubsequent advanceへ返す。clean EOFをbound前に見る場合はresult drain+blocking restore後だけpartial batchをpublishする。earlier errorなしでcleanup failureならunpublished batchをdestroy、poison/closeし、`Error.Unsupported(ContractError { query_id: Some(query_id), item: "db.rows.cleanup", message: "PostgreSQL streamed rows cleanup failed and the connection was closed" })`を返す。zero-row EOFも`Ok(None)`でなく同error。published batchはretractしない。 |
| partial server failure | data後fatalが来得る。観測advanceは既存exact SQLSTATE/native detailをcopy、result clear、nullまでdrain、rows failed化する。以前returnしたrow/batchはcaller-visibleでhidden compensation/rollback/mutationなし。`one_native`はclean completion前に何もpublishせず、one-row/two-rows-then-fatalはいずれもnative failure、two-or-more-then-cleanだけCardinality 2を返す。 |
| parameter ownership/allocation | generated binderは§5.6.1どおりparameterごとに1 execution-owned Text copyを作る。contextはsendからterminal null、fail-closed connection close、mode-selection cleanup、timeout、early Dropまで全pointer/length/format arrayとcopy payloadを保持する。constructor return後にoriginal text/blob source ownerをmove/mutate/dropできる。directは1 copied Query IDをownし、constructor failureはallocated済みならfree、published terminal rowsはrows Dropまでretainしてfreeする。preparedはstatement-owned Query IDを常にborrowしsuccess/failureのどちらもfreeしない。per-row container/hidden whole-result bufferなし。steady live setはbinder context/current PGresult/libpq transport/direct ID/explicit batchまでで、cancellation中だけ1 temporary `PGcancelConn`を追加する。`one_native`は上記public `clone_in(out)` allocationだけを追加し、zero/invalid-first-rowはzero clone、valid first rowはexactly one clone、remaining result比例のpackage allocationなし。successとpost-clone failure/Cardinalityはfirst Rowについて同じ`out` allocation count/byte/alignment patternを持つ。 |
| validation/error precedence | directはsettled §13.4順序を維持する。complete descriptor header/plan-shape agreementをidentity読取前にvalidateし、common/native source order、driver restriction、exact live exec/connection state、one context allocation、generated static-option/field-shape validation、lease、bind、deadline、execution、explicit selectorの順。static failureはlease/libpqなしでcontextをfreeし、overlapもcontextをfreeする。preparedは別にcomplete stmt v3/identity/plan/resolver、common/native、driver/live stateをvalidateし、その後lease→context→bind→deadline→send→selector。native option validatorはcompiler-private `pkg.db.internal.descriptor.prepared_parameter_ordinal(statement_ref, name) -> i32`だけを呼ぶ。このbridgeはexact concrete `resource_ref<db.stmt<P,R>>`と`str`を受け、synthesized `pkg.db.internal.resource.stmt_header_valid(wrapper)` control-dependent guard下だけでoffset 80をloadしretained `fn(name: str) -> i32`を呼び、unknownなら0を返す。application source、wrong resource/type、unguarded load/call、malformed HIRはrejectする。nonpositive resultはlease/allocation前に既存unknown-name errorとなる。Delivery位置ではcommandはpayloadが一切applicableでないためtag rejectionが先。Queryは§13.4どおりcurrent payloadをduplicate detection/registration前にvalidateし、duplicateでも全PortalBatch sizeをvalidateする。valid value間duplicateはexact duplicate error、invalid PortalBatchはexact size error。したがって`[SingleRow,PortalBatch(0)]`とreverseはいずれもsize、`[SingleRow,PortalBatch(1)]`はduplicate。command `execute_native`は`PortalBatch(0)`もQuery-only payload検証なしでrequires-Query error。既存earlier Text/common precedenceは不変。 |
| post-send invariant failure | send failureは既存connection error。immediate mode selection rejectはcleanup ruleでcancel/drainしquery-specific `InvalidQuery` item `postgres.rows.delivery`、message `PostgreSQL client rejected the selected row delivery mode`。invalid result sequenceは同category/item、message `PostgreSQL streamed result sequence is invalid`。COPY、pipeline、unknown numeric statusはearlier primaryがない時だけ同errorを使い常に上記immediate-close branchへ入り、connectionはreuse不可。ordinary known statusだけがdrain、transaction synchronization、blocking-mode restore全成功後reuse可。 |
| deadline/cancellation/error drain/Drop | explicit modeのcommon Timeoutはoverflow-check済みabsolute deadlineとなる。nonblocking enable直後かつsend直前にmonotonic clockを再読し、expiryならsend/selector/cancel zeroのままblocking restore+Timeout、restore failureならpoison/closeする。time remainingならdeadlineとoriginal positive durationをclean completionまで保持する。caller think time、one pending-cardinality drain、validation/decode/storage error drainもcountする。later expiryはearlier errorなしならcancel+drain後Timeout、別server cancelはCancelled。pending protocol中のvalidation/decode/batch-storage errorはunpublished package-owned valueをdestroyし、cloned `one_native` Rowはmonotonic-`out` ruleでunreachable化し、first errorをprimaryに保持してoriginal deadline下でdecodeせずnormal drainする。deadlineなしまたはtime remainingではnormal drainを続ける。clean completionならConn DML RETURNINGはcommit、Txはeffects retainedのINTRANS。later server fatalはfirst row/storage errorよりsecondaryだがConn statementをrollbackしてIDLE、またはexplicit TxをINERROR/rollback requiredにする。drain中expiryはD9 `max(original_duration,1ms)` budgetでcancelするがearlier errorを返す。raceでConn effectsはIDLE時にcommitted/rolled backのどちらもあり、Txはcompletion勝ちINTRANS/effects retainedまたはcancel勝ちINERROR/rollback required。early rows Drop/mode failureは即cancel+exact 1s recovery、cardinalityはcancelなし。helperはnonnull PGcancelConnをcreate/start/pollからall-path exactly one finishまでownする。first poll前に`PQcancelSocket`を再取得して`PQsocketPoll`でwritableを待ち、その後も各READING/WRITING結果ごとに変化し得るsocketを再取得し、remaining monotonic budget内でexact readinessを待ってから再pollする。socket/readiness failureまたはundocumented polling tagはfail closed。error bytesはfinish前にcopyし、main result classification前にfinishする。その後ordinary main results drain、transaction sync、blocking restore-or-poison、context/unpublished direct ID、wrapper、lease順。normal completion、first-error drain、deadline/mode recovery、early DropのどこでもCOPY、pipeline、unknown numeric statusを観測したら、current resultだけclearし、追加drain/pipeline-exit/cancel/probe/restoreなしで即close、earlier errorまたはsilent Dropを保持し、owner cleanup後lease releaseする。Conn targetはordinary known-status drain後`PQTRANS_IDLE`だけreuse可。explicit Txはcompletion勝ちなら`PQTRANS_INTRANS`、cancelまたはserver errorなら`PQTRANS_INERROR`でcallerのrollback/Dropまたはserver-rejected operationだけを許す。全outcomeでhidden rollbackなし。Timeout/mode failure/early Dropのprimary resultはrace stateに依存せず、statement effectはsubsequent DB/Tx stateだけでobservable。他stateはpoison/closeしearlier errorをoverwriteしない。clean terminal cleanup failureはexact db.rows.cleanup。timeout/native/sequence/decode/storageはunpublished Row/pending Cardinalityより先。Dropはreportせずpoison/close。 |
| overlap/global state | prerequisite PRが全shared-connection libpq consumerをinventoryする。typed command/prepare/rowsは既にlease取得、common/native oneはrows経由、transaction boundaryはactive lease reject、prepared Dropはactive中libpqなし、prerequisiteが全PostgreSQL catalog/common-native EXPLAINをlease化する。その後streamed rowsはbind/send前からcleanup/Dropまでleaseをownし、second command/Query/prepare/transaction/catalog/EXPLAIN/streamはsettled pre-native overlap errorでlibpq前にfailする。catalog/EXPLAINは上記exact query-less/query-specific `postgres.connection.active_execution`を使う。nonblocking flagはlease中だけsetしrestore failureはpoison後release。 |
| artifact/ABI/cache | runtime delivery選択はQuery/command descriptor bytes、batch-plan bytes、checked metadata、static Query artifact semantics、semantic fingerprintを変えず、mode別descriptor/cache identityは作らない。一方、public `Delivery` enumのlandはPostgreSQL module interface hashを変え、rows/statement ABI implementationのlandは関連producer implementation hashを変える。per-unit importerはnew dependency-interface hashを記録し、影響するcodegen/object cache entryは各implementation boundaryで1回invalidateされる。changed interfaceをimport/reachしないmoduleだけがprevious cache keyを保持できる。direct PRでshared Rowsは120-byte v3: 0--95 v2、delivery@96 (`0=PG BufferedFull`,`1=SingleRow`,`2=PortalBatch`,`3=SQLite Step`)、pending@97、zero@98--99、portal i32@100、deadline i64@104 (`-1` absent)、`timeout_duration_ns` i64@112 (`-1` absent、otherwise positive)。SQLite/PG BufferedFull/terminalは両deadline field -1。active explicit streamだけ`(-1,-1)`またはnonnegative absolute deadline+original positive durationのexact pairを持つ。他product/pairはinvalid。prepared parityはshared stmtを88-byte v3へし0--79 v2、@80にQuery descriptor既存nonnull producer-owned `fn(name:str)->i32` resolver。両driver prepareがstatic resolverをretainする。HIR validationはoffset80 resolverとretained binder/row validator/stream decoder/batch planが同じconcrete Query producer generation由来のstatement formationだけを受理し、cross-Query splice/raw replacementをrejectする。`stmt_header_valid`はversion 3、driver 1/2、live、reserved zero、全v2 pointer/layout invariant、valid batch plan、nonnull resolverをaccessor/indirect call前に要求する。Dropはclosed化しnative/connection/binder/identity/deallocate/row validator/stream decoder/batch plan/resolverをnull化してからfreeする。rows/stmt semantic-byte goldenがvalid/malformed全productをpinする。 |
| producer/runtime inspection | static producer thunkだけがbind/row validate/decode/batch authority。PostgreSQL packageはoption normalizationとstream state machine、libpqはprotocol deliveryをownする。reflection/field-name lookup/source/artifact/cache I/O/Query-body instantiationなし。static Query metadataはstatic factsだけを記録し、runtime delivery observationはartifact identityを変えない別label。 |
| acceptance/measurement | merged lease+result-status safety prerequisites、D8/D9/common A1、libpq>=17前提。direct PRはconn/tx x BufferedFull/SingleRow/PortalBatch x rows/one、prepared PRはconn/tx-prepared stmt x同3 mode x rows_stmt、next/batchは各rows axis。zero/one/many、official zero terminal、known libpq17全status+unknown sentinel、terminal後のordinary error/invalid/deferred/unknown全statusについてprotocol-state-before-status error precedenceとstatus別drain-or-close、one/two-row-then-fatal/two-row-then-clean、Conn/Tx DML RETURNING effect、pre-send/pending-cardinality timeout、partial-final restore、validation/decode/storage error後のnormal completion/deadline expiry、early Drop/cancel race、malformed rows/stmt v3とSQLite sibling、whole/per-unitをcover。status prerequisiteはshipped synchronous/timeout-completion/recovery/direct/prepared/command/rows/one/prepare/transaction/catalog/EXPLAIN/silent-cleanupの全PGresult consumerへCOPY全種、pipeline 2種、unknownをinjectし、one clear+physical close、later result/COPY/pipeline-exit/cancel/tx/restore zero、first error/silent Drop、balanced owner/lease、no reuseをassert。同prerequisiteはcompiler-side PGresult consumerもinventoryする。complete user SQLを`PQexec`するmigrationだけはnumeric-file/statement順でtop-level COPYをURL read、target open、lock、history publication、libpq call前にrejectする。`alignc db prepare`は`PQprepare`/`PQdescribePrepared`だけを使いCOPYをexecuteせず、他tool SQLはfixed producer-owned textである。migration ownerはRequired/Forbidden、lowercase/comment-leading COPY、ordinary statement後COPY、quoted/comment/dollar-body内COPY text、transaction-control後later COPY、COPY後later transaction-control、later Forbidden count、SQLite unchangedをcrossし、両prohibited-order caseがsingle statement-ordered classification passを証明する。全reject caseはenvironment read、target/connection open、lock、history publication、libpq zeroを証明する。stream ownerもfirst/data後と全cleanup causeで同matrixを反復する。direct/prepared explicit ownerはnonblocking enable中にclockを進め、expired-before-sendでsend/selector/cancel zero、exact Timeout、restore-or-closeをassert。phase-order、Delivery precedence、`one_native` allocation、effect-aware drain、Tx race、BufferedFull両subpath、live overlap、required server16.4/client>=17を保持する。cache ownerはpublic Delivery/rows/statement ABI land時のdeterministic interface/implementation invalidation 1回とruntime mode間で追加descriptor/cache identity splitなしをpinする。measurementはnon-gating。 |

status prerequisiteのRust tool acceptance ownerは
`postgres_tool_results_fail_closed_before_followup_native_work`である。migration command/queryと
preparation command/query/prepare/describe consumerを、null result、COPY 3種、
`PGRES_SINGLE_TUPLE`、`PGRES_TUPLES_CHUNK`、pipeline 2種、unknown numeric sentinelでcrossする。
1 private `crates/align_driver/src/db_postgres_status.rs` authorityがrow access/follow-up SQL前にcomplete
libpq 17 numeric setをexhaustive classifyする。new symbolをloadせずprerequisiteのcurrent client floorを
上げない。各adapterはexisting diagnosticを先にcopyし、present current resultを
exactly once clearし、mutable connection ownerを即finish+null化してfirst errorを返す。closed ownerにより
rollback、deallocation、result retrieval、row access、全later libpqはzero、Dropはsecond finishしない。
expected success/known complete error controlはexisting mapping/cleanupを保持する。このtool matrixは上記public
recordのmigration screening axisとcumulativeであり、fixed tool SQLや`PQprepare`がCOPYをexecuteしない事実で
弱めない。

Implementation closure matrix:

| Cell | Closure | Owner evidence |
|---|---|---|
| prerequisite libpq-consumer lease | rows ABI/mode変更前にQ5b2 PostgreSQL catalog cursorとcommon/native EXPLAINがfirst libpq前にconnection leaseを取得し全exitのresult/context clear後にreleaseする。typed execution/transaction/prepared Drop/common-native oneをunchanged siblingとしてinventoryし、live overlapはexact query-less/query-specific errorかつzero libpq call。 | independently mergeable prerequisite PR、`postgres_catalog_and_explain_share_the_execution_lease`、catalog/EXPLAIN no-call matrix、既存typed/transaction/Drop owner |
| prerequisite package result-status safety | stream ABI/mode変更前に全shipped package-owned PGresult consumerを、`postgres.align`/internal execution/resource cleanupがimport cycleなしで共有するsealed `apps/db/pkg/db/internal/postgres_status.align` authorityへrouteする。clear前に各consumerのexisting non-success mappingが選ぶexact errorをcopyし、earlier errorをprimary、silent cleanupをsilentのまま保持する。COPY、`PGRES_PIPELINE_SYNC`/`PGRES_PIPELINE_ABORTED`、unknownはcurrentをonce clearして即closeし、以後result/COPY/pipeline-exit/cancel/transaction-state/blocking-restoreなし。new public error category/item/message/query identityを作らずowner/lease cleanup後no reuse。隣接Rust tool closureとmigration screeningは同prerequisite PRのmandatory cellで、package ruleを弱めない。 | independently mergeable prerequisite PR、exact package consumer/error inventory、sealed-authority surface negative、COPY/pipeline/unknown x sync/timeout/recovery/Drop no-call/clear-close/error identity matrix、retained Q2/Q4a/Q4b/Q5b2 |
| prerequisite Rust tool result-status safety | migration `execute_raw`/`query`とpreparation `execute_command`/`query_rows`/`PQprepare`/`PQdescribePrepared`を、result field/row access/follow-up SQL前に1 private exhaustive Rust status authorityへrouteする。null result、COPY、partial single/chunk row、pipeline、unknown numeric statusはavailable existing diagnosticをcopyし、present resultをonce clear、mutable connection ownerをfinish+null化し、later libpqを禁止する。known complete resultはcurrent mappingを保持する。canonical PostgreSQL migration screeningはさらに1 source-ordered statement-classification passでfirst-token COPYを§17.6 exact diagnosticによりURL/native work前にrejectする。 | `postgres_tool_results_fail_closed_before_followup_native_work`、module x consumer x null/COPY/single/chunk/pipeline/unknown clear-close/error/no-row-access/no-rollback/no-deallocate matrix、`postgres_migration_copy_is_rejected_before_native_work`のtransaction-control-before-COPYとCOPY-before-transaction-control両case、prepare/migration SQL-origin inventory |
| staged public/option | direct PRはexact Delivery+1 variantを追加しrows/oneを1 validatorへroute、command reject/common name不可。Queryは全occurrenceで§13.4 payload-before-duplicate、commandはDelivery payload非適用なのでtag-first reject。prepared PRはexact rows_stmt_nativeを追加しstmt-specific validation後same validator。Text/Binary不変。Deliveryはpost-release D13 inventoryだけに置きinitial D1--D12 surfaceへ入れない。 | per-PR surface+initial/post-release inventory golden、common/native source-order、reversed-invalid/valid-duplicate、command/direct/prepared disposition |
| BufferedFull preservation | normalized Delivery absenceでrow-mode setup前にbranchしshipped BufferedFullへdelegateしてcomplete resultだけpublishする。Timeoutなしは1 synchronous `PQexec*`+nonblocking/send/selector zero、Timeoutありは既存nonblocking `PQsend*` deadline/cancel/drain+selector zeroでblocking restore後publication。rows v3 wrapper以外のallocation/error timing/SQL effect/parameter/result/lease lifecycleを維持。 | direct/prepared conn/tx x Timeout absent/present、exact PQexec/PQsend/nonblocking/wait/cancel/drain/selector count、pre-publication server error/DML RETURNING/allocation parity |
| explicit direct construction | §13.4をexactに維持する。complete header/identity/common/native/restriction→live exec/connection state→one context allocation→generated static validation→lease→bind→deadline→nonblocking→clock recheck→time remaining時だけsend→immediate explicit mode→conn/tx-dependent rows。expiryはsend/selector/cancel zeroでrestore+Timeout、restore failureはclose。static failureはcontextをfree、overlapもcontextをfreeしてbind/libpq zero。later failpointはParams/context/unpublished copied IDをonce free、published IDはDropまでown。 | complete pairwise phase-order x context allocation/free/lease/binder/libpq counter、direct conn/tx pre-send expiry/restore+send/mode failpoint、ID counter、Params Move/Drop/mutation |
| prepared state/construction | prepared PRでboth-driver stmtを88-byte v3へbumpしresolver@80をretain。existing prepared binder bridge隣にexact sealed `prepared_parameter_ordinal(resource_ref<db.stmt<P,R>>, str) -> i32`と`stmt_header_valid` control dependencyを追加する。両bridgeはconcrete stmt referenceだけを受けapplication/raw callを拒否し、whole/per-unitでresolver thunkをretainする。statement formationはresolver/binder/row validator/stream decoder/batch planを同じconcrete Query producer generationへbindする。complete stmt/plan/resolver/native optionをlease/context前にvalidateし、deadline→nonblocking→clock recheck→restore-or-poisonまたはprepared send+shared stream constructorを使う。statement-owned ID/resolverはborrowしfreeしない。 | stmt byte、sema/MIR wrong-resource/application/unguarded/cross-Query splice/malformed resolver no-call、SQLite/PG+whole/per-unit retention、prepared pre-send expiry/restore+send/mode failpoint、unknown/duplicate name、reuse/Drop |
| result advancement | common helperでordinary resultをeach once clearしknown libpq17 status/mode/bound超fetchなし。zero-row terminal後の全nonnull resultはstatus-to-error mapping前にinvalid sequenceを記録する。その後ordinary statusはclear/drainし、COPY、pipeline、unknown numeric statusはcurrent resultだけclearして追加protocol/pipeline-exit/cancel/probe/restoreなしでclose後owner cleanup+lease releaseする。oneはfirst Rowだけvalidate/decode/cloneしmultiplicityをpendingにしてlater rowをdecodeせずclean completionまでdrain、late native/timeout/cleanupが勝ちclean時だけCardinality publish。clone後はarenaをrewindしないため全non-successでcaller-region bytesが残る。全stream consumer/cleanup callerとmerged status prerequisiteをauditする。 | every-status/count/sequence（post-terminal ordinary-error/invalid/COPY/pipeline/unknownのerror precedence x cleanup actionを含む）、COPY/pipeline/unknown first/after-data/every-cleanup-cause no-call/clear-close、zero/one/many、one/two-fatal、two-clean、DML effect、singleton/全post-clone failure/Cardinalityの`out` byte/alignment/clone-count parity |
| view/batch | backing result clear前にrows generation invalidate。unread rowはcurrent resultへ保持し、batch childはclear前にcopy。late errorでunpublished partial batch destroy。 | delayed-view compile rejection、live text/blob/cross-result batch、allocation/Drop counter |
| failure/timeout/cleanup | mode/deadline/fatal/validation-decode-storage/Drop/clean-terminal restoreを1 drain/cancel/sync/restore helperへ集約する。cardinalityとfirst row/storage errorはnormal no-decode drain、deadline expiry/Drop/mode failureだけcancelし、expiry budget用original durationをpersistする。PGcancelConn exactly-one finishをmain result classification前に完了し、first error、ordinary result、restore-or-poison後release。cleanup中COPY/pipeline/unknownならcurrentだけclear、追加drain/pipeline-exit/cancel/probe/restoreなしで即close、earlier error/silent Dropを保持しowner cleanup+lease release。Conn cancelはIDLEだけ、Txはcompleted+effects retainedのINTRANSかcanceled/server-aborted+rollback requiredのINERRORだけを受理しhidden rollbackなし、他stateはpoison。clean restoreはexact cleanup、partial batch destroy。BufferedFull、merged status helper、common A1もaudit。 | full cancel failpoint、COPY/pipeline/unknown x mode/deadline/row/storage/Drop x Timeout absent/present x Conn/Tx first-error/no-call/clear-close、error x clean/later-fatal/expiry x Conn/Tx effects、original-duration budget、zero/partial-final restore、fatal、pre-send/post/pending timeout、Drop/timeout Conn/Tx race、reuse/rollback-or-poison |
| rows/stmt ABI malformed | shared rows 120-byte v3を1 authorityでconstruct/validateしSQLite Step+PG3 mode active/terminalを固定。terminalは両deadline sentinel -1、live explicit streamだけexact deadline/duration pair可。prepared PRはstmt v3 nonnull resolverと全sibling accessor/Dropを更新。 | rows/stmt byte golden、deadline/duration含むfield mutation no-native、v2 reject、driver/direct-prepared/active-terminal、both-driver stmt、SQLite retention |
| artifact/cache invalidation | runtime Delivery valueをstatic Query/command descriptor、batch plan、checked metadata、semantic fingerprint bytesへ入れない。direct public enum/rows ABIとprepared stmt ABIがlandする時はowning interface/implementation hashおよび影響する全per-unit dependency/codegen/object keyをexactly once invalidateし、mode別artifactを作らずimporterへstale dependency-interface keyを残さない。 | runtime mode間descriptor/plan/metadata byte parity、public interface/implementation hash before/after control、whole/per-unit affected-importer miss+unrelated-module retention、mode別artifact/cache splitなし |
| FFI/version | exact libpq17 send/row-mode/result/nonblocking cancel/cancel-socket readiness/transaction/cleanup signaturesとconstantをdeclare。native `int` chunk size、`pg_usec_time_t`、polling status signednessをC probeでpin。本railはCOPY data/terminationまたはpipeline-exit FFIを追加せず、COPY/pipeline/unknown処理はcurrent-result clear+ordinary connection closeだけ。whole/per-unit Linux/macOSのordered `pq`/`ssl`/`crypto`/`zstd`/`z` closureを保持し、client <17はbuild/evidence setupでreject、newer unknown statusはfail closed。 | C signature/status/unknown sentinel、cancel-socket readiness counter、forbidden COPY/pipeline-exit symbol/call inventory、client version、link inventory、x86_64/ARM64/macOS build |
| operation/allocation parity | direct PRはconn/tx x3 mode x rows/one、prepared PRはconn/tx-prepared x3 mode x rows_stmt、各rows axisでnext/batch。mode-specific sync/send/no hidden SQL/one lease/bounded native/exact parameter+ID+resolver/balanced allocationをassertし各shared bump後SQLite matrix retain。 | per-PR matrices、dual-driver rows/stmt、provisioned suite、measurement |

five-times-reopened boundaryは4 independently mergeable vertical PRである。latest consumer sweepにより
COPY/pipeline/unknown fail-closedはfuture stream限定でなく、shipped synchronous/timeout executorが既にPGresultを
受けてundrainable protocol中のconnectionをreleaseし得ることが判明した。このexisting correctness repairを
lease後のapplication-package-surface/ABI-neutral prerequisiteへ分割する。compiler-side sweepで追加の
user-SQL入口であるPostgreSQL migration `PQexec`も見つかり、同prerequisiteのcanonical screeningでtop-level
COPYをnative work前にrejectする。続くtool-consumer sweepにより、同PRはprepare/migrationのnull、COPY、
partial row-mode、pipeline、unknown resultでrollback/deallocation/row access/later libpq前にconnectionを
close+null化する。stream-only error drainとvalidation orderはdirect delivery、
prepared formationはmerged direct engine後の独立PRに保持する。

| PR | Exact scope | Merge gate |
|---|---|---|
| PostgreSQL libpq-consumer lease prerequisite | Q5b2の全PostgreSQL catalog/common-native EXPLAINをcorrectにlease化する。public symbol/ABI/delivery/libpq17/rows state変更なし。 | focused fake overlap/no-libpq、retained Q5b2 PostgreSQL、local DB、normal code preflight/review/CI |
| PostgreSQL result-status safety prerequisite | shipped synchronous/timeout direct/prepared rows/one/command/prepare/transaction/catalog/EXPLAIN/recovery/silent-cleanupの全package-owned PGresult consumerをauditする。clear前にconsumer existing non-success errorをretainしearlier error/silent Dropを保持する。COPY/pipeline/unknownはcurrentをonce clearし即close、later result/COPY/pipeline-exit/cancel/tx/restore zero、balanced owner、no reuse。全Rust prepare/migration PGresult consumerをprivate exhaustive tool classifierへrouteし、null、COPY、partial single/chunk、pipeline、unknown resultはavailable errorをcopy、present resultをclear、ownerをfinish+null化してrow access/rollback/deallocation/later libpqを禁止する。1 canonical source-ordered statement passでtop-level PostgreSQL migration COPYをURL/native work前にrejectする。`pkg.db` public error/ABI/delivery/libpq17/rows state変更なし。§17.6がexact new migration diagnosticをownする。 | every-package-consumer COPY/pipeline/unknown injection、no-call/clear-close/error identity、`postgres_tool_results_fail_closed_before_followup_native_work`、両prohibited statement orderを含む`postgres_migration_copy_is_rejected_before_native_work`、prepare/migration SQL-origin inventory、retained Q2/Q4a/Q4b/Q5a/Q5b2、local DB、normal preflight/review/CI |
| PostgreSQL direct streamed delivery | direct rows/oneへDelivery、libpq17、rows v3、advancement、normal-drain cardinality、cancel、batch、pre-send deadline recheck、direct matrix。absenceはshipped BufferedFullの両Timeout subpathを維持。 | direct+pre-send expiry/restore matrix、dual-driver rows/batch、required local/CI、normal preflight/review/CI |
| PostgreSQL prepared streamed parity | rows_stmt_native、stmt v3+producer resolver、prepared option/formationを追加しmerged stream engine再利用。 | stmt ABI/resolver、prepared matrix、SQLite/PG stmt/rows、required local/CI、normal preflight/review/CI |

first independent design reviewの7 findingは1 ownership/ABI/validation passでledger-firstに
閉じた。prepared failureはstatement-owned Query IDをfreeせず、terminal deadlineは`-1`、
`PGcancelConn`はcreate/start/poll/error-message/finish ownerを持つ。shared v3へSQLite Stepを追加し、
当初はduplicateをinvalid second payloadより先としたが後続§13.4 consistency reviewがpayload-firstへ
supersedeした。callable matrixからprepared `one_native`を除外し、全libpq17
statusをdata/terminal/native error/invalid sequenceへ分類した。ownerはdirect/prepared ID counter、
両driver terminal golden、cancel failpoint/allocation、SQLite ABI/rows、reversed multi-invalid、exact
callable operation、every-status synthetic sequenceである。

second independent reviewのnew P1 + 3 P2によりlocal patch loopを止めmatrixを再openした。
P1のunleased catalog/EXPLAINはcomplete D12 lease repairをfirst PRへ分割しno-libpq ownerを追加。
Cardinalityはunpublished pendingとしてclean completionまでdrainしlate failureを優先、partial-final
cleanup前publication禁止とexact error、absent-Delivery BufferedFull axisで閉じる。

next reviewは2 valid P1 + 1 P2と1 rejected P1 claimを返した。row-bearing terminal claimはofficial
libpq17 §32.6が全rowをTUPLES_CHUNK、terminalをzero-row TUPLES_OKと固定するためrejectした。有効な
cardinality cancel findingはnormal no-decode drain+DML RETURNING Conn/Tx effect owner、default findingは
shipped BufferedFullのTimeout absent/present両subpath+zero row selector owner、prepared name authorityは
direct/prepared分割+stmt v3 resolverで閉じる。

そのredesignのfresh reviewは1 new P1 + 2 P2を発見した。pending effectful protocol中の
validation/decode/storage error cleanup不足によりmatrixをthird reopenし、first errorを保持した
original-deadline normal drain、expiry時だけcancel、Conn/Tx effect ownerを追加した。timeout recovery用
original durationはrows-v3 offset112へpersistする。F31のbounded total transport表現はbounded per-result
delivery/peak bufferingへ修正し、total transportはQuery-definedのままとした。error drainはdirect streamなしに
independent consumerがなく、cleanupなしでstreamをpublishできないためfourth PRへ分割しない。

次のfresh reviewは1 new P1 + 1 P2を発見した。COPY statusはgeneric `PQgetResult` drainではterminalへ
到達できないためmatrixをfourth reopenした。COPY supportはdeferしたままcurrent resultをclearし、追加の
drain/COPY/cancel/transaction probe/blocking restoreなしで即closeする。earlier errorを保持しclose後にownerを
cleanupする。direct static validation順序もsettled §13.4/Q2のheader/options/restriction→live state→context→
generated static→lease→bind/nativeへ戻す。COPY/phase correctionはいずれもexplicit direct constructor以前に
consumerを持たないためfourth PRは追加せず、既存3 vertical boundaryを維持する。

fourth redesignのfresh reviewはnew P1なし、3 P2 consistency gapを発見した。streamed `one_native`は
valid first Rowを`out`へcloneした後のCardinality/later failureでもarenaをrewindできないため、singletonと
exact同じclone bytesがscope終了まで残ることとzero/invalid-first-rowのzero allocationを明記しowner化する。
Delivery precedenceはcanonical §13.4 payload-before-duplicateへ統一し、first reviewのlocal dispositionを
supersedeする。initial D1--D12 option inventoryからDeliveryを分離してpost-release D13 additionとして記録する。
いずれもmatrix reopen/PR re-splitを要しない。

base refresh後のfresh reviewは2 P1 + 1 P2を発見しmatrixをfifth reopenした。shipped BufferedFull、command、
prepared、one、timeout recovery、catalog/EXPLAIN、transaction、silent cleanupもCOPYを観測し得るため、
complete PGresult consumer classを閉じるresult-status safety prerequisiteをlease後へ分割する。explicit
direct/preparedはnonblocking enable直後にclockを再読し、expiredならsend/selector/cancel zeroでrestore+Timeout、
restore failureならcloseする。libpq >=17をsupportするためunknown numeric statusはordinary drainせずCOPYと
同じimmediate clear-closeにする。known-status exhaustive tableと全consumer/cleanup cause ownerで固定する。

fifth redesignのfresh reviewはnew P1なし、2 P2 consistency gapを発見した。`PGRES_PIPELINE_SYNC`と
`PGRES_PIPELINE_ABORTED`は`PQexitPipelineMode`までconnection-global pipeline modeを保持するが、本railは
pipeline-exit FFIをownしない。よって全current/streamed PGresult consumerでCOPY/unknownと同じcurrent-result
clear+immediate closeへ移し、later result/pipeline-exit/cancel/probe/restore zero、first-error/silent-Drop、
no-reuse ownerを追加する。introductory stagingも2 prerequisiteとlease -> status -> direct -> preparedの
exact 4 PR sequenceへ英日同期した。

次のfresh reviewはnew P1なし、1 P2を発見した。package PGresult matrix自体はcompleteだが、同じdeferred
subprotocol classがshipped migration toolingへ残っていた。`screen_postgres_catalog`はtop-level COPYを
acceptし、synchronous `PQexec`がCOPY modeへ入ったerror pathでrollbackを試みる。existing dollar-quote-aware
statement screenでURL/native work前にCOPYをrejectし、Required/Forbidden、lexical near-miss、multi-invalid
order、zero env/open/libpq ownerを追加する。同prerequisiteでprepare/migrationと全fixed tool SQLのoriginも
inventoryする。matrix reopen/PR re-splitは不要である。

続くfresh reviewはnew P1なし、さらに1 P2を発見した。SQL-origin inventoryだけではunexpected Rust tool
resultをsafeにしない。prepare/migration result siteを1 private exhaustive Rust classifierの後ろへ置き、
null、COPY、partial single/chunk row、pipeline、unknown statusではavailable errorをcopyし、present resultを
once clear、ownerをfinish+null化してからrow access、rollback、deallocation、later libpqを禁止する。
`postgres_tool_results_fail_closed_before_followup_native_work`がmodule x consumer x synthetic resultの
clear/close/error/no-follow-up counterとknown-complete controlをownする。これはexisting status
prerequisite内のP2 closureで、matrix reopen/4 PR boundary変更は不要である。

final base-bound reviewはnew P1なし、3 P2を発見した。zero-row terminal後のfatal/error statusは
native errorとinvalid-sequenceの両dispositionを持っていたため、protocol stateをstatus-to-error mapping
より先にvalidateし、全post-terminal resultをinvalid sequenceとしたうえでstatusがordinary drainまたは
COPY/pipeline/unknown immediate-closeだけを選ぶ。migration ownerは`COPY; BEGIN`を欠いていたため、1つの
source-ordered classification passと`BEGIN; COPY`/`COPY; BEGIN`両caseでForbidden count前のfirst
statement winnerを固定する。runtime-mode identity表現はdependency cache invalidationを隠していたため、
static descriptor semanticsはmode-independentのまま、public enumとrows/stmt ABIのlandで影響する
interface/implementation/importer keyをexactly once invalidateする。これらはexisting closure cellを
補強するP2で、matrix reopen/4 PR boundary変更は不要である。

implementation前にrevised five-times-redesigned ledgerと4 PR boundaryをfresh independent
adversarial reviewし、findingは
ledger-firstで閉じる。code review前にこのsectionの全normative `must`/`exact`/`every`/`before`/
`reject`/`required`をimplementation path+ownerへ対応させる。result advancement、cancellation、
v3 validation findingはline patchでなくcomplete sibling-consumer auditを要求する。

#### A1 PostgreSQL binary-format public-contract ledger

これは次のindependently useful D13 railのsource of truthである。すでにpublish済みの
`Text|Binary` parameter/result choiceをfirst-release PostgreSQL type matrixで完成させる。
public type/function/option/error variant/driver/Query shape/dynamic dispatchは追加しない。
binary encoderとstatic type proofを分離するとill-typed valueを送信でき、producer proofだけを
分離するとdormantになるため、generated descriptor semantics、package binder context、prepared
validation、libpq argument、row decodeを1 capability boundaryで変更する。strict producer-to-consumer
chainを分割すると同じwire/ABI/malformed-input/whole-per-unit proofが重複するため、implementationが
1,000 hand-written lineを超える場合もこの1 PR boundaryを維持する。

application-callable inventoryはexactly次のまま:

```text
postgres.Format { Text, Binary }
postgres.ExecuteOption.ParameterFormat(name: str, format: postgres.Format)
postgres.ExecuteOption.ResultFormat(format: postgres.Format)
```

```align
pub fn execute_native<P>(
  target: pkg.db.exec,
  statement: pkg.db.command<P>,
  params: P,
  options: slice<pkg.db.ExecuteOption>,
  native: slice<ExecuteOption>,
) -> Result<pkg.db.exec_result, pkg.db.Error>

pub fn rows_native<P, R>(
  target: pkg.db.exec,
  statement: pkg.db.query<P, R>,
  params: P,
  options: slice<pkg.db.ExecuteOption>,
  native: slice<ExecuteOption>,
) -> Result<pkg.db.rows<R>, pkg.db.Error>

pub fn one_native<P, R: RegionPlain>(
  target: pkg.db.exec,
  statement: pkg.db.query<P, R>,
  params: P,
  out: region,
  options: slice<pkg.db.ExecuteOption>,
  native: slice<ExecuteOption>,
) -> Result<R, pkg.db.Error>

pub fn rows_stmt_native<P, R>(
  borrow mut statement: pkg.db.stmt<P, R>,
  params: P,
  options: slice<pkg.db.ExecuteOption>,
  native: slice<ExecuteOption>,
) -> Result<pkg.db.rows<R>, pkg.db.Error>
```

`ParameterFormat(name, ...)` absentはそのparameterのText、`ResultFormat(...)` absentはcomplete
resultのTextである。1 named parameterにformat optionは最大1つ、1 executionにresult formatは
最大1つ。result formatは全result columnに対する1 libpq-wide choiceでper-column surfaceはない。
valid commandはresult columnを持たないが`execute_native`もselected result formatをlibpqへ渡す。
`rows_native`/`one_native`/`rows_stmt_native`は両formatを`BufferedFull`/`SingleRow`/
`PortalBatch`の全callable modeとcomposeする。common operationはPostgreSQL native optionを
書けないためabsenceによるText-onlyのまま。environment/server setting/row value/prepared history/
heuristicはformatを変えない。

first-release binary mappingはexact:

| Align value shape | required PostgreSQL canonical type / OID | binary parameter/result payload |
|---|---|---|
| `bool` / `Option<bool>` | `bool` / 16 | 1 byte、exactly `00`または`01` |
| `i16` / `Option<i16>` | `int2` / 21 | two's-complement 2-byte big-endian |
| `i32` / `Option<i32>` | `int4` / 23 | two's-complement 4-byte big-endian |
| `i64` / `Option<i64>` | `int8` / 20 | two's-complement 8-byte big-endian |
| `f32` / `Option<f32>` | `float4` / 700 | exact IEEE-754 bits、4-byte big-endian |
| `f64` / `Option<f64>` | `float8` / 701 | exact IEEE-754 bits、8-byte big-endian |
| `str`/`string`とnullable form | `text` / 25、`varchar` / 1043、または`name` / 19 | exact client-encoding bytes、recorded lengthにterminatorなし |
| `slice<u8>`/`array<u8>`とnullable form | `bytea` / 17 | embedded zero/high byteを含むexact bytes |

owned `string`/`array<u8>`はparameter shapeだけ。Row fieldは`Option`を含むborrowed `str`/
`slice<u8>` viewのままで、`one_native`はexisting explicit-region cloneを維持する。

connectionはclient encoding UTF-8にpinされたまま。binary text inputはexact UTF-8 bytesを
execution-owned storageへcopyし、package storage safety用sentinelだけをrecorded length外へappend
する。binary transportがlength-awareでもPostgreSQL text valueはU+0000を含めないため、`text`/
`varchar`/`name`はsend前にrejectする。binary `bytea`はpayload byteを追加/削除しない。present empty
Binary `bytea`はrecorded zero length外に1 zero sentinel byteをallocateし、そのnon-null pointerを
libpqへ渡す。`None`だけがnull pointerを使うためemptyとSQL NULLはdistinctである。NULL parameterは
otherwise ignored lengthでselected format codeを保持する。NULL resultはpayloadなしだが
RowDescriptionのexpected OID/result formatを必ず検証する。Text formatはexisting exact
lowercase `\\x` bytea encode/decodeとscalar parserを維持する。この表にないOID/domain/enum/array/
range/numeric/date-time/JSON/extension typeをcoerceしない。

semantic-to-wireとwire-to-semanticのindependent goldenは`false = 00`、`true = 01`、
`int2(-2) = ff fe`、`int4(0x01020304) = 01 02 03 04`、
`int8(-2) = ff ff ff ff ff ff ff fe`、`f32(1.5) = 3f c0 00 00`、
`f64(-0.0) = 80 00 00 00 00 00 00 00`、UTF-8 `é = c3 a9`、
`bytea([0, 255]) = 00 ff`。floatはsigned zero/infinity/NaN payloadを含むevery bitを保持しdecimal
textを経由しない。layoutはlibpq network-byte-order requirementとPostgreSQL built-in
`boolsend`、integer send/receive、`pq_sendfloat4`/`pq_sendfloat8`、`textsend`/`namesend`、
`byteasend` contractに従う。

compiler-private normalized-call ABIはexactである。`one_native`は
`rows_native_prevalidated`を使い、4つ目のinternal entry pointを持たない:

```align
pub fn execute_native_prevalidated<P>(
  target: pkg.db.exec,
  params: P,
  statement: pkg.db.command<P>,
  query_id: str,
  timeout: Option<i64>,
  format_plan: raw,
  format_count: u32,
  result_format: u8,
) -> Result<pkg.db.exec_result, pkg.db.Error>

pub fn rows_native_prevalidated<P, R>(
  target: pkg.db.exec,
  params: P,
  statement: pkg.db.query<P, R>,
  query_id: str,
  timeout: Option<i64>,
  delivery: u8,
  portal_max_rows: i32,
  format_plan: raw,
  format_count: u32,
  result_format: u8,
) -> Result<pkg.db.rows<R>, pkg.db.Error>

pub fn rows_stmt_native_prevalidated<P, R>(
  params: P,
  borrow mut statement: pkg.db.stmt<P, R>,
  timeout: Option<i64>,
  delivery: u8,
  portal_max_rows: i32,
  format_plan: raw,
  format_count: u32,
  result_format: u8,
) -> Result<pkg.db.rows<R>, pkg.db.Error>
```

`format_count`は8-byte/4-aligned entryのexact countで、operationのtrusted parameter count
（directはvalidated descriptor count/thunk、preparedはvalidated stmt count thunk/copy）と
`65_535`の両方以下。`format_plan`は`format_count == 0`のときexactly null、それ以外は
complete synchronous call中callerがownするchecked `i64(format_count) * 8`以上のreadable byteを
指す。entryは`i32 ordinal`、`i32 format`の順で、ordinalはone-based、`0=Text`、`1=Binary`。
`result_format`も`0=Text`、`1=Binary`。internal calleeはlive-state access/allocation/bind/send前に
result tag、parameter-count bound、pointer/count product、checked byte size、各entryのordinal/
format tag/duplicate ordinalをsource orderでvalidateする。codeをexecution-owned storageへcopyし、
caller planをretain/freeしない。malformed productは
`InvalidQuery(ContractError { query_id: Some(query_id), item: "postgres.execute.format_plan",
message: "invalid normalized PostgreSQL format plan" })`。compiler-private call validatorは
corresponding public wrapperが作ったlive scratch owner由来plan pointerだけを許し、application call、
wrong operation/type、pointer substitution、malformed HIRをcodegen前にrejectする。

Binary関連のpublic contract errorはexact。以下の全recordはtrusted Query/command IDを
`Some(query_id)`として使う:

| failure | exact public error |
|---|---|
| selected Binary parameterがdirect static proofまたはprepared effective-OID proofを欠く | `Unsupported`、item `postgres.execute.parameter_format`、message `binary PostgreSQL parameter requires an exact supported ParameterType` |
| malformed internal normalized plan | `InvalidQuery`、item `postgres.execute.format_plan`、message `invalid normalized PostgreSQL format plan` |
| malformed text/byte parameter pointer/length representation | `Encode`、item `db.parameter`、message `database parameter has an invalid memory representation` |
| PostgreSQL text-type parameterのU+0000 | `Encode`、item `db.parameter`、message `database text contains U+0000` |
| checked parameter encodingまたはcomplete Bind-message budget overflow | `Encode`、item `db.parameter`、message `PostgreSQL parameters exceed the Bind message length limit` |
| direct/preparation Parse-message budget overflow | `InvalidQuery`、item `postgres.execute.wire`、message `PostgreSQL Parse message exceeds the protocol length limit` |
| result name/OID/format/NULL/pointer/length/payload mismatch | `Decode`、item `db.row`、message `PostgreSQL row does not match the static Row contract` |

parameter formationはincreasing protocol ordinalをvisitする。Measure modeの1 text/byte field内では
memory representation、該当するtext U+0000、exact encoding arithmetic、remaining Bind budgetの順。
complete Measure pass後だけEncode modeがallocation/copyする。ordinary allocation failureはlanguage
hard abortで、別の`pkg.db.Error`ではない。各tuple-producing native resultは2 ordered
validation phaseを持つ。row count、zero-row/cardinality/terminal handling、result publication前にMetadata
modeがexact column countとincreasing declared Row ordinalのname/OID/formatをcheckする。complete pass
success後だけValue modeがdelivered rowをinspectし、increasing ordinalのNULL disposition、negative
length、present valueのnon-null pointer、fixed width後のbool valueまたはUTF-8 payload validityをcheckする。
したがってany metadata errorはevery value/cardinality outcomeよりwinし、phase内ではfirst declared
ordinalがwin。このorderと上表がvalid/malformed zero-row resultを含むevery multi-invalid direct/
prepared/buffered/streamed inputを支配する。

context matrixのretained v2 rangeはexactかつdriver-specific。byte 0--39はcommon v2 fieldを保つ。
SQLiteはoffset 40をretained text/blob pointer vector、48をその`u32` vector length、52をzero
`u32`に使い、every byte 56--79をzeroとrequireする。offset-40 vectorとentryだけをown/freeする。
PostgreSQLはoffset 40/48/56/64をvalue/OID/length/format vector、72をその`u32` length、76を
zero `u32`に使い、existing ownership/cleanupを保つ。両v4 constructorは全112 byteをexplicitに
initializeする。complete validatorはload/free前にこのdriver-specific productをcheckし、malformed
cleanupはSQLite zero-only slotをownerとしてinterpretしない。

actual aggregate boundはPostgreSQL protocol v3が固定する。trusted parameter count `N`はlibpqのwide C
`int` argumentに関係なくParse/Bind `Int16` count fieldのunsigned range `65_535`以下。ここで使うexact
libpq 17 call shapeはparameter-format count=`N`、result-format countはoneでvalidated `0=Text`または
`1=Binary` codeを持ち、portal name=empty、statement name=directではempty/preparedではretained
generated ASCII name。`S`をterminator込みstatement-name C-string byte count、`payload_i`をNULLなら
zero、それ以外はexact selected encoded lengthとする。one-byte message tagを除きown four byteを含む
Bind length fieldはexactly `13 + S + 6*N + sum(payload_i)`で`2_147_483_647`以下。
Parse lengthはPostgreSQL wire-SQLのterminator
込みC-string byte countを`Q`としてexactly `6 + S + Q + 4*N`でsame limit。direct executionは両formula、
preparationはParse、prepared executionはBindをcheckする。全arithmeticはnarrow前にchecked `u64`。
Measure前、context offset 96にsigned `i64`でstoreするexact fixed-budget resultは
`2_147_483_647 - (13 + S + 6*N)`。negativeまたはnon-`i64` resultはstore前にfailする。

descriptor/artifact formationはartifact/cache publication前に`N > 65_535`をdiagnostic
`PostgreSQL static query supports at most 65535 parameters`でrejectする。そのcountを持つmalformed
runtime descriptor/statementはoption access/allocation前にexisting query-less `InvalidQuery`、item
`db.descriptor.header`、message `invalid static database descriptor`を返す。payload allocation前に
generated binder-v2 Measure modeがincreasing protocol ordinalをvisitし、memory representation/text
U+0000 ruleをvalidate、exact selected encoding lengthをcomputeしてpreinstalled remaining Bind budgetから
debitする。Text scalar measurementはsame canonical formatterをcount-only sinkで使い、Text byteaはchecked
`2 + 2*source_len`、Binaryはfixed tableまたはsource byte length。encoding arithmetic/debitに失敗するfirst
ordinalがexact Bind `Encode` errorを返しlater fieldはinspectしない。complete successful Measure passだけが
binder-v2 Encode modeをpermitする。parameterized budget ownerはno-allocation measurement stubを使い、
multi-gigabyte fixtureなしでdirect/prepared、single/multiple/NULL value、every encoding class、both
result format、both statement-name length、`65_535`/`65_536`のaccepted-limit/rejected-limit-plus-one pairをderiveする。

binder callback familyはexact。every callbackは
`fn(context: raw, protocol_ordinal: u32, value: T) -> i32`。non-null fieldは`Some`へliftし、nullable
fieldは`Option`をunchangedで渡す。Measure modeはそのrowの`measure_*_v1` symbolだけ、
Encode modeはexisting `bind_*_v2` symbolだけをcallできる。

| `T` | Measure symbol | Encode symbol |
|---|---|---|
| `Option<bool>` | `pkg.db.internal.measure_bool_v1` | `pkg.db.internal.bind_bool_v2` |
| `Option<i16>` | `pkg.db.internal.measure_i16_v1` | `pkg.db.internal.bind_i16_v2` |
| `Option<i32>` | `pkg.db.internal.measure_i32_v1` | `pkg.db.internal.bind_i32_v2` |
| `Option<i64>` | `pkg.db.internal.measure_i64_v1` | `pkg.db.internal.bind_i64_v2` |
| `Option<f32>` | `pkg.db.internal.measure_f32_v1` | `pkg.db.internal.bind_f32_v2` |
| `Option<f64>` | `pkg.db.internal.measure_f64_v1` | `pkg.db.internal.bind_f64_v2` |
| `Option<str>` | `pkg.db.internal.measure_text_v1` | `pkg.db.internal.bind_text_v2` |
| `Option<slice<u8>>` | `pkg.db.internal.measure_bytes_v1` | `pkg.db.internal.bind_bytes_v2` |

eachはsettled callback status modelを使う。successはexact zero、first context-owned failureをrecordした後は
exact one。valid callbackはother statusを返さず、MeasureはEncodeへ、EncodeはMeasureへdelegateしない。

これらのcheckはsettled operation phaseをreorderせずextendする。direct executionはcomplete
descriptor/count/identity、option、driver/live state、context、generated static contractをvalidateし、続いて
leaseをacquire、exact full format vectorをallocate/install、Parse budgetとBind fixed budgetをvalidateした後に
binder-v2 Measure、Encode、deadline、sendの順。prepared executionはcomplete statement/count/identityと
option、driver/live stateをvalidate、leaseをacquire、context/full format vectorをcreate、Bind fixed budgetを
validateした後にMeasure、Encode、deadline、sendの順。preparationはcomplete descriptor/options/live
stateをvalidateしleaseをacquireした後にname formation、Parse budget、`PQprepare`の順。したがって
header/count errorはevery optionよりwinし、overlapはevery direct/prepared parameter measurementよりwin、
successful lease acquisition後はdirect Parse-budget errorがevery parameter-value errorよりwinし、Measure errorは
increasing protocol ordinal順。budget failureはbinder-v2 Encode callback/libpqをcallしない。

call-local sparse normalized planはsuccessful native-option validationの末尾でformし、execution contextには
installしない。下記closure matrixでnormalized planをinstallするとは、generated static validationと
successful lease acquisitionの後にexecution-owned exact full format vectorをallocate/installすることを指す。
package-context rowのcomplete fixed Bind formulaはexact
`2_147_483_647 - (13 + S + 6*N)`で、initializerはdirect/preparedの両executionでそのsuccessful
leaseの後だけrunする。

`one_native`でもMetadata modeはevery newly acquired tuple-producing resultでrunする。malformed second
generationはCardinality前にexact row-contract Decode errorを返す。metadata-valid second generationは
Value mode/decoder/second Row allocationなしにCardinalityを返す。このD13 ruleはolder D8
any-second-row winnerをsupersedeする。

implementation closure matrix:

| closure cell | required implementation closure | exact owner evidence |
|---|---|---|
| public surface/operation matrix | 上記inventoryをexactに維持する。direct Query/commandとprepared Query executionでindependently selected Text/Binary parameter formatを、同じoperationでindependently selected Text/Binary result formatをacceptする。prepared `one_native`、common native-option escape、per-column selector、reflective codec、compatibility aliasを追加しない。 | exported-surface golden、command/direct rows/direct one/prepared rows callable matrix、parameter-format plan/result-format axisはBufferedFull/SingleRow/PortalBatchとseparate |
| static type proof/binder/resolver ABI | 8-aligned execution descriptorを136-byte v5からexact 144-byte v6へbumpする。64/104/new 136以外のoffsetはv5 meaningを保持する。offset 64はnon-null generated binder-v2 `fn(context: raw, borrow params: P, mode: u8) -> i32`: `0=Measure`,`1=Encode`でother modeはpackage callback前にfail。両modeはsame increasing protocol ordinal/field typeをvisitし、Measureはtype-matched count/debit callbackだけ、Encodeはmatching allocation/bind callbackだけをcallする。SQLiteはexact non-measuring contextへEncodeをdirect invokeし、PostgreSQLはsame context/params/format vectorのone complete Measure success後だけEncodeを許す。offset 104はnon-null generated parameter-resolver v2: zero=unknown、positive ordinal=knownだがText-only、corresponding negative ordinal=上表のcompatible PostgreSQL `ParameterType`をexactに1つ持ちBinary可。`i32::MIN`またはabsolute ordinalが`1..=parameter_count`外はmalformed。offset 136はnon-null generated `fn() -> u32` parameter-count v1 thunkで、bodyはoffset 112のexisting distinct-parameter countとequalかつ`65_535`以下のsingle constant return exactly。complete descriptor validatorはbinder/resolver accept前に両count compare/protocol maximumをenforceする。existing sealed `parameter_known`は両signをacceptし、`parameter_ordinal`はabsolute ordinalを返す。sealed direct binary-ordinal operationはnegative formだけをabsolute ordinalへし、それ以外はzero。prepared siblingは下記retained effective-OID eligibility bitもrequireするため、異なる`ParameterOid` overrideはstale static proofをreuseできない。application call、wrong descriptor/resource、unguarded prepared load、complete header guard外のcount-thunk/binder invocation、Measure/Encode mismatch、cross-descriptor binder/resolver/count-thunk splice、malformed HIRはfail closed。Query/command formation、interface serialization、monomorphization、whole/per-unitはone producer generationのmatching binder v2/resolver v2/count thunkをretainする。 | exact v6 header/binder/resolver/count-thunk body/signature/relocation golden、`65_535`/`65_536`、offset count/thunk equality、zero/mismatch/malformed thunk product、mode/callback/ordinal parityとMeasure-before-Encode、every type/missing-`ParameterType` code、matching/differing/absent `ParameterOid` proof、sema/MIR negative、whole/per-unit runtime-selected twin |
| option validation/precedence | §13.4 source-order phaseを維持する。direct command/Queryはcomplete descriptor/identity、common/native option、driver/live stateの順。preparedはevery name/proof lookupがguarded stateに依存するため、common/native option前にcomplete statement v4 header、batch plan、resolver、count thunk/count equality、eligibility shape、trusted identityをvalidateする。malformed/closed statementとany invalid Binary optionの組合せはquery-less `InvalidQuery`、item `db.descriptor.header`、message `invalid static database descriptor`。そのprepared guard後、direct/preparedとも各`ParameterFormat`でname U+0000、unknown、direct negative proofまたはprepared negative+eligibility proofを欠くBinary、duplicateの順にrejectし、current payload validityをduplicate registrationより先にする。missing proofは`Unsupported`、trusted Query/command ID、item `postgres.execute.parameter_format`、exact message `binary PostgreSQL parameter requires an exact supported ParameterType`。`ResultFormat`はclosed enum以外のpayloadがなくduplicate detectionはexact。Deliveryはadjacent ledger orderを維持する。operation-specific header guard後の全option errorはdriver restriction、physical connection/live-state、lease/context/bind/SQL sendより先。valid SQLite stmt+invalid BinaryはPostgreSQL driver mismatch前にoption error。 | command/direct/prepared source-order/reversed-invalid/valid-duplicate matrix、malformed/closed stmt+every invalid Binaryのheader winner、valid SQLite stmt+invalid Binaryのoption winner、unknown/NUL/missing-proof zero execution-context/plan/libpq、retained Delivery owner |
| package context/prepared ABI | shared opaque binder/decode contextは両driverでone exact 112-byte v4。offset 0--95は上記exact v3 meaningを保持する。offset 96はsigned remaining Bind-payload budget、104はmeasurement state (`0=unmeasured`,`1=measuring`,`2=measured`)、105--111はzero。SQLiteはstate 0/budget `-1`をrequireする。new PostgreSQL contextも`(0,-1)`で開始し、sealed budget initializerだけがcomplete header/options/count/Parse validation後かつparameter payload allocation前にfixed Bind formulaのnonnegative resultとstate 1をstoreできる。Measure callbackはstate 1をrequireしexact lengthをnegativeにせずatomically debitする。complete successful binder-v2 Measure returnがstate 2へchangeし、PostgreSQL Encodeはstate 2をrequire、failure/second Measure/Encode passはpublishできない。offset 80はresult format `u8`（SQLiteは`0=Text`、PostgreSQLは`0=Text`,`1=Binary`）、81はmetadata state (`0=unvalidated`,`1=validated`)、82--87はzero、88はmetadata native pointerのまま。metadata state 0はoffset 88 nullをrequireする。sealed native-generation setterはevery row-producing SQLite Query executionとzero-row terminalを含むevery newly acquired tuple-producing PGresult前にoffset 8をinstallし81/88をresetする。successful metadata validationはstate 1とexact non-null offset-8 pointerをstoreし、value validationはそのequalityをrequireする。native state clear/replaceは両metadata fieldをresetし、pointer-address reuseはvalidationをinheritしない。one context validatorはevery driver/version/format/metadata-state/measurement-state/budget/reserved/pointer productをdereference/debit/bind/free前にrejectする。shared prepared stmt stateはexact 112-byte/8-aligned v4: offset 0--79はv3 meaning、80はresolver v2、88はpackage-owned byte-per-parameter Binary eligibility vector、96はdescriptor offset 136由来のnon-null producer-owned parameter-count v1 thunk、104はcopied exact `u32` parameter count、108--111はzero。one complete stmt-header validatorはfixed field/non-null thunkを先にestablishし、guarded thunk resultが`65_535`以下かつoffset 104とexactly equalと要求した後だけeligibility validationをboundする。SQLiteはevery countでnull eligibility pointer。PostgreSQLはzero parameterでnull、それ以外でtrusted count exactlyのreadable byteを持つnon-null vectorかつ全byte 0/1。PostgreSQL preparation時、byte `ordinal-1`はstatic validationがsupported nonzero canonical OIDをinstallし、任意の`ParameterOid` override後のeffective OIDがequalな場合だけone。arbitrary nonzero overrideはTextではvalidなままだが、異なるoverrideはそのordinalをBinary-ineligibleにする。sealed prepared binary-ordinal bridgeはcomplete guarded v4 header、trusted count内negative resolver result、eligibility byte oneをrequireする。HIR stmt formationはbinder v2/resolver/count thunk/row validator/decoder/batch planをone Query producerから要求する。stmt Dropはvector/thunk/countをnull/zero化してからvectorをonce freeする。context/stmt validator、全accessor/bridge/constructor/Drop、semantic/byte goldenを一緒に更新する。 | driver x context-v4 result-format/metadata-state/measurement-state/budget/reserved/native-pointer matrixとnew-result pointer-reuse reset/Measure replay reject、both-driver stmt v4 active/closed/malformed byte、`65_535`/`65_536`、thunk null/body/count mismatch、eligibility pointer/byte productのno out-of-bound scan/callback、cross-Query binder/thunk/resolver splice、matching/differing/absent override、constructor/accessor/Drop no-call/free counter、cumulative SQLite suite |
| normalized format formation/ownership | complete native-option validation後、explicit parameter optionごとにcall-local normalized 8-byte entry (`i32 ordinal`,`i32 format`)を最大1つ作る。public native wrapperが上記exact synchronous internal ABI中4-aligned scratchをownしsuccess/error後freeする。published rowsはborrowしない。complete plan/count validation後、internalはText initializeしたexact one `N`-entry execution-owned format vectorをallocateし、各explicit codeをown ordinalへcopyする。これがlibpqに`N` parameter format codeをemitさせ、両binder-v2 modeが読むnon-null vectorで、zero parameterはnull。zero explicit optionはcanonical `(null, 0)` scratch plan/no scratch allocationだが`N > 0`ならfull vectorはallocateする。ResultFormatはseparate allocationなし。 | zero/one/many optionとzero/positive-`N` scratch/vector allocation/free、every malformed result-tag/count/pointer/size/ordinal/tag/duplicate productのexact `InvalidQuery`+zero context/bind/send、full-vector default/explicit code byteとlibpq format-count parity、rows publication後scratch destruction/use-after-free probe、direct/prepared parity、sema/MIR call-formation negative |
| parameter measurement/encode/retention | directはgenerated static validation後にnormalized plan/exact full format vectorをinstallし、preparedはretained negative resolver proof/effective-OID eligibility bitの両方をrequireする。payload allocation前にapplicable Parse formulaをvalidateし、exact Bind budgetをinitializeしてbinder-v2 Measureをonce runする。Measure callbackはown ordinalのselected codeだけを読み上記どおりvalidate/debitし、allocation/bind/send/siblingへのcode broadcast/result format mutationをできない。その後binder-v2 Encodeがsame per-ordinal codeを読み上表どおりemitする。各non-null scalar/text/byteaはexactly one execution-owned payload allocation、NULLはnone。nonempty Binary byteaはexactly `len` bytes、empty Binary byteaは1 zero sentinel byteをallocateしてnon-null pointer+recorded length zeroを渡し、`None`とdistinct。BufferedFullはsync completion、SingleRow/PortalBatchは`PQgetResult` nullまたはfail-close context destructionまでretainする。every Measure/partial Encode/option-plan failureはinstalled context/vector/payload ownerをonce freeしsendしない。 | exact Parse/Bind formulaとevery encoding-class budget boundary、`65_535`/`65_536`、per-type/nullable measure-vs-wire byte/length/format/OID、every ordered two-parameter format pair+three-parameter heterogeneous、direct/prepared `Some(empty bytea)` vs `None`、U+0000、accepted-limit/rejected-next multi-parameter/statement-name twinのzero payload allocation/send、count-only scalar formatter parity、mode replay/wrong-mode、source mutation/Drop、partial allocation/free、sync/stream lifetime |
| libpq call parity | every `PQexecParams`/`PQexecPrepared`/`PQsendQueryParams`/`PQsendQueryPrepared`へindependently formed exact parameter vector/result codeを渡す。explicit Binaryを`PQexec`/text fallback/second SQLへrouteしない。timeout/cancel/drain/status fail-close/Tx effect/delivery selectionはadjacent D8/D9/A1 ledgerのまま。 | every ordered two-parameter format pair x every result-format state x sync/async direct Query/prepared Query stub capture、separate direct-command capture、required live PostgreSQL mixed-format control、retained deadline/delivery/status/cancel matrix |
| result metadata/decode | exact `PQfformat` FFIを追加し、descriptor offset 80/stmt offset 56/rows offset 40をsame generated row-validator v2 pointerとする。exact ABIは`fn(context: raw, mode: u8) -> i32`、`0=Metadata`,`1=Value`でother modeはpackage callback前にfailure。changed rows pointer signatureによりshared 120-byte/8-aligned rows recordをv3からv4へbumpし、other byte offsetは不変。Metadata modeは`validate_row_count_v3`をonce、続いて`validate_field_metadata_v3`をdeclared orderでcallする。SQLiteはcount/name、PostgreSQLはcount/name/OID/requested formatをvalidateし、successで上記context metadata stateをpublishする。Value modeはそのstateをrequireし、`validate_field_value_v3`をdeclared orderでNULL/representation/payloadにcallし、SQLiteはrow-dynamic native typeもvalidateする。Textはexisting parser。Binaryはwrong fixed width、boolの0/1以外、negative length、present valueのnull pointer、invalid UTF-8をsafe view/decoder前にrejectする。integer/floatはendian-explicit bit operation。every row-producing SQLite Query execution/every tuple-producing PGresult generationはrow count/zero-row terminal disposal/cardinality/`has_next`/decode/batch append前にMetadataをexactly once invokeする。Valueはdecoded/appended rowごとにonce、cardinality-only drainではnever。Binary text/bytea viewはcurrent PGresult generationをdirect viewし、Text byteaだけowned decode bufferを維持する。`one_native`は従来どおりvalidated first Rowを`out`へexactly once cloneする。descriptor producer、stmt/rows constructor、one complete rows-v4 validator、every rows consumer/Drop、sealed direct/prepared/current-metadata/current-value bridge、HIR validator、MIR、LLVM preflight、whole/per-unit retentionはv2 signatureを一緒にrecognizeし、v1/rows v3/cross-Query/invalid-mode/value-before-metadata/malformed-thunkをrejectする。 | BufferedFull/SingleRow/PortalBatch x direct/preparedのzero-row valid/wrong count/name/OID/format exact Decode-before-exhaustion/cardinality、terminalを含むevery new PGresult metadata call count、metadata-before-value multi-invalid、rows-v4/validator-v2 body/signature/relocation semantic-byte golden+sema/MIR malformed-call、per-type result-format/NULL/malformed width/bool/UTF-8/pointer、zero-copy identity、generation/one-clone、cumulative SQLite metadata/value suite |
| cleanup/error/allocation | 上記exact error record/precedence tableを返す。missing binary proofとmalformed normalized planはnative state前。parameter representation/NUL/lengthはpre-send。malformed binary metadata/payloadはshipped delivery cleanup中もfirst exact `Decode` row-contract errorを保持しlater decoderを呼ばない。row-cache Text bytea、parameter、PGresult、context、unpublished batch/Row、statement eligibility vector/dependency、leaseをsuccess/error/timeout/cancel/early Drop/malformed stateでexactly once releaseする。Binary scalar/text/bytea result decodeはexisting row cache以外のper-field heap allocationなし。 | exact error identity/intra-field precedence x direct/prepared/all delivery、first-error/no-more-decode x Conn/Tx、fault allocation/cleanup、early Drop/reuse、sanitizer-compatible stub |
| artifact/cache/docs identity | Text/Binary surfaceはexistingなのでapplication-facing module interface/static Query semantic fingerprint不変。`pkg.db.internal`/`pkg.db.internal.postgres`のadded/changed exported compiler-private measurement/prevalidated signatureは両internal module interface hashとevery importing module dependency-interface keyをonce changeする。descriptor v6の144-byte image、binder-v2/row-validator-v2/resolver/count-thunk body、package callback semantics、rows v4の120-byte implementation、stmt v4の112-byte implementation、shared context v4の112-byte implementationもcompiler/package implementation hashとaffected codegen/object/dependency cacheをexactly once invalidateする。checked artifact/metadata/SQL identity/driver restrictionは不変。English/Japanese designとlive handoffのnext/shipped boundaryを一致させる。 | both internal-module interface-hash/importer dependency-key before/after twin、descriptor semantic-byte parity+binder/row-validator/resolver/count-thunk relocation/body+cache twin、rows 120-byte/stmt 112-byte/context 112-byte golden、application-interface/artifact/metadata identity control、mirror consistency、implementation milestoneだけでhandoff update |
| acceptance/measurement | merged D8/D9、A1 batch/direct+prepared delivery、libpq client >=17、required PostgreSQL jobをprerequisiteとする。parameter/result formatはindependent axis: 2 named Query parameterで`{default Text, explicit Text, explicit Binary}`のevery ordered pair（9 plan）を`{default Text, explicit Text, explicit Binary}` result formatとcross（27 product）しdirect/prepared stub両方でown、3-parameter heterogeneous controlとapplicable direct-command parameter/result-code caseも含む。separately every mapped type/nullable formをText/Binary measure/bind/decodeとBufferedFull/both explicit deliveryでcrossする。exact Parse/Bind formula、direct/prepared statement-name length、single/multiple/NULL payload、all encoding class、accepted-limit/rejected-next、parameter count `65_535`/`65_536`をno-allocation stubでcrossし、その後timeout absent/present、Conn/Tx、callableなrows/one/command、zero/one/many、valid/malformed zero-row/nonempty metadata/payload、whole/per-unitを2 format axisをcollapseせずcoverする。push前に`scripts/db-verify-local.sh`。correctness後にpayload/copied byte、allocation、time-to-first-row、full-scan throughputを含むText-vs-Binary local measurementをrecordするがsemantic gateではない。 | focused 27-product Query format-plan/result owner+command/mixed live control、protocol-budget/count boundaryとMeasure/Encode parity、metadata-only zero-row/new-generation、per-type package/compiler owner、retained `pkg_db_q4b`/`pkg_db_a1`/`pkg_db_q5b2`、required live PostgreSQL、local parity、non-gating measurement |

first fresh reviewは2 P1 omissionと1 P2 error-contract gapを発見した。empty Binary byteaはexact
non-null zero-length representationとdirect/prepared ownerを持つ。normalized-plan rowはcomplete
compiler-private signature/bound/ownership/malformed result/call-formation guardをpinする。error tableは
every observable fieldとmulti-invalid orderを固定する。author-side sibling auditはさらに、異なるprepared
`ParameterOid`がdescriptor static Binary proofをreuseできる問題を発見したため、statement v4はText
preparationをnarrowせずeffective-OID eligibilityをretainする。これらはinternal ABI/ownership cellを
reopenするが、strict producer-to-consumer implementation boundaryは1つのまま。

next fresh reviewはさらに2 P1 closure failureを発見した。eligibility lengthはself-reported stmt count
だけだったため、descriptor v6/statement v4はone generated constant count thunkをretainし、vector
scan前にcomplete header guard下でvalidateする。prepared option validationはearlier precedence文が
option後へ置いたstateに依存していたため、single orderをcomplete stmt header+identity、common/native
option、driver+physical live stateとする。exact malformed-statement/invalid-optionとvalid-wrong-driver/
invalid-option winnerがmulti-invalid productを閉じる。これはABI/validation-order rowをreopenするが、
one implementation boundaryは不変。

redesigned reviewはさらに2 P1を発見したため、local patchでなくclosure matrixを
`result-generation-metadata x independent-format-product` axisでreopenする。result schemaは
generation-level obligationとなり、exact row-validator-v2 Metadata modeがzero-row/cardinality/
terminal handling前、guarded Value modeがdelivered rowでrunする。同じreopenでparameter-plan stateと
result-format stateをindependentにし、coupled `default/Text/Binary` shorthandでなくone parameterized
ownerがcomplete two-parameter 27-productを閉じる。changed producer thunk/shared context ABI/every result
consumer/format propagationはdormant proofまたはunchecked empty resultを避けるためtogetherにlandする
必要があり、implementation boundaryは1つのまま。

next redesigned reviewは2 P1/2 P2 omissionを発見したため、matrixを
`cross-driver-context-bytes x internal-interface-cache x metadata-cardinality x encoded-length`
axisでreopenする。SQLite context offset 56--79はcanonical zero/no ownership、internal prevalidated
signature changeはinterface/dependency keyをexplicitにinvalidate、every result generation metadataは
second-row Cardinality前にwin、each selected wire encodingはaccepted maximum/rejected next lengthを持つ。
constructor/validator/consumer/cleanup/cache identity/observable error precedenceがsame ABI generationで
一致する必要があり、分割するとunreadable context imageまたはstale callerを残すためone closure axis。

protocol-boundary reviewはさらに2 P2 omissionを発見した。libpq C `int`はcomplete Bind envelopeではなく、
Parse/Bind parameter countは16-bit protocol field。ledgerはexact Parse/Bind length formulaと`65_535`
maximumをownする。binder v2/context v4がEncode前にone no-payload-allocation Measure passを提供し、
impossible per-value maximumやlate libpq failureへ依存しない。wire-validation strategy changeなのでmatrixを
`protocol-count x aggregate-message-budget x measure-before-encode` axisでreopenするが、one
producer-to-consumer implementation boundaryは維持する。

implementation referenceは[PostgreSQL protocol message formats](https://www.postgresql.org/docs/17/protocol-message-formats.html)、
[libpq parameter/result formats](https://www.postgresql.org/docs/17/libpq-exec.html)、
[libpq single-row mode](https://www.postgresql.org/docs/17/libpq-single-row-mode.html)、上記built-in
send/receive functionである。implementation前にledgerとone-PR boundaryをfresh independent
adversarial reviewし、findingはledger-firstで閉じる。code review前にsubsectionの全normative
`must`/`exact`/`every`/`before`/`reject`/`required`を1 implementation path+discriminating ownerへ
対応させる。resolver sign、format propagation、endian conversion、result metadata order、cleanupの
findingはcomplete sibling type/operation/delivery auditを要求する。

#### A1 explicit-pool public-contract ledger

これは4つ目のconsumer-visible D13 railのsource of truthである。すでにshippedしたSQLite/
PostgreSQL constructor上に、明示的なfixed-capacity poolを1つ追加する。driver registry、background
worker、connection reset SQL、health check、retry、wait queue、dynamic resource collection、
compiler special caseは追加しない。one `Pool` resourceが下記explicit-size private pointer arrayを
opaque native representationとしてownするが、language-level Move resource collectionではない。
exact public surface:

```align
module pkg.db.pool

import pkg.db
import pkg.db.sqlite
import pkg.db.postgres
import pkg.db.pool.internal.resource

pub MAX_CAPACITY: i64 := 1024

pub resource Pool = pkg.db.pool.internal.resource.drop_pool

pub Info {
  driver: pkg.db.Driver,
  capacity: i64,
  idle: i64,
  checked_out: i64,
}

pub fn open_sqlite(
  path: str,
  capacity: i64,
  options: slice<pkg.db.sqlite.ConnectOption>,
) -> Result<Pool, pkg.db.Error>

pub fn open_postgres(
  url: str,
  capacity: i64,
  options: slice<pkg.db.postgres.ConnectOption>,
) -> Result<Pool, pkg.db.Error>

pub fn try_acquire(borrow owner: Pool) -> Result<pkg.db.conn, pkg.db.Error>

pub fn info(borrow owner: Pool) -> Result<Info, pkg.db.Error>
```

`db.Error` は§15のpayload-free `PoolExhausted` variantを追加する。`acquire` alias、manual
release/close、pool option sum、acquisition timeoutはない。`try_acquire` はwait、clock read、
connection open、database/native I/Oを行わない。valid poolで `idle == 0` ならexact
`db.Error.PoolExhausted` を即時に返す。malformed private pool imageはslot load/counter mutation前に
exact `Unsupported(ContractError { query_id: None, item: "db.pool.state",
message: "database pool state is invalid" })` を返す。`info` も同じcomplete state validationを行い、
1つのCopy snapshotを返す。fieldsは `0 <= idle`、`0 <= checked_out`、
`idle + checked_out <= capacity` を満たし、差分はunusable checked-out connectionのDrop後に
retireしたslot数である。

capacity validationが最初で、exact `1..=MAX_CAPACITY` だけをacceptする。それ以外はbookkeeping
allocation、input/option inspection、native work前に
`Unsupported(ContractError { query_id: None, item: "db.pool.capacity",
message: "database pool capacity must be between 1 and 1024" })` を返す。このboundはone constructor
callを1,024 external connection attempt、pointer storageをexact 8,192 byteへcapするresource-safety
boundであり、default/throughput recommendationではない。valid capacityならconstructorは
text/option sliceをcall中だけborrowし、fixed pool record 1つとchecked pointer array 1つをallocateし、
対応するshipped `sqlite.connect` / `postgres.connect` をphysical ordinal昇順に呼ぶ。したがって
UTF-8/U+0000、option default、duplicate/conflict、duration、client encoding、native connection
errorとsource orderはexactに各constructor contractのままである。poolはpath、URL、secret、option
valueを保持しない。ordinal `n` のfailureは、先に成功した `n-1` connectionをordinal逆順に
close/freeしpool bookkeepingを全てfreeしてから、そのexact driver errorを返し `Pool` をpublish
しない。constructorは全 `capacity` connection成功後だけpoolをpublishするため、後続acquisitionに
hidden network/filesystem/SQL/PRAGMA/authentication workはない。同じSQLite pathを繰り返しても
database identityをrewrite/inferしない。SQLite `:memory:`、empty、URI、cache、mutex semanticsは
callerの明示connect inputが持つexact meaningのままである。formation cleanupはownership-atomicだが
external-effect rollbackではない。earlier successful `sqlite.connect` がcreateしたSQLite fileやapplyした
PRAGMAはlater ordinal failureでもdelete/compensateしない。`ConnectTimeoutNs` その他のconnect optionは
各physical connectionへindependently applyし、aggregate pool-open deadlineはない。

open poolはidle physical connectionをownする。`try_acquire` はmost-recently-returned idle
connection、初期状態ではhighest open ordinalをordinary standalone `db.conn` 1つへtransferして
`checked_out` をincrementし、checked-out stateはidle arrayから外れる。connectionはprivate origin
pointerを1つ保持するが `Pool` へのcompiler borrow dependencyは持たない。settled
`db.begin`/`commit`/`rollback` APIは同じraw stateをnominal `db.conn`/`db.tx`間でtransferするため、
parent-dependent `db.conn`をfabricateして `resource.into_raw` を使うとparent provenanceが消える。
このためruntime ownershipがlast checked-out conn/tx Dropまでpool bookkeepingを生存させる。
acquired ownerはpool-specific execution pathなしに、既存の全common/native Query、command、
metadata、EXPLAIN、prepared statement、row stream、batch、transaction operationを使える。

live acquired `db.conn` をopen originating poolへDropする時は、package wrapperにexecution lease/
transaction ownerがないことを先にrequireし、その後native transaction idleをproveする。SQLiteは
`sqlite3_get_autocommit(connection) != 0`、PostgreSQLは
`PQtransactionStatus(connection) == PQTRANS_IDLE` をrequireする。proof missing/failureは
poison/closeしてslotをretireする。successはreset SQL、rollback、health probe、reconnect、allocation
なしにexact physical stateをLIFO returnする。これはtransaction-safety boundaryでありsession
sanitizationではない。successful session-global change（supported PRAGMA、PostgreSQL `SET`、temporary
object、advisory stateなど）はphysical connectionのpropertyとして残りnext acquirerからobservableである。
fresh session stateが必要なcallerはnew poolをopenするかDrop前にexplicit inverse native operationを
使い、poolはそれらをinfer/compensateしない。

`commit`/`rollback` は同じattached stateを `db.conn` として返す。`db.tx` Dropはsettled fail-safe
rollbackを先に行い、stronger exact rollback-and-idle proofを満たす場合だけ返す。SQLiteは
`sqlite3_exec("ROLLBACK") == SQLITE_OK` と
`sqlite3_get_autocommit(connection) != 0`、PostgreSQLはnon-null `PGRES_COMMAND_OK` result、exact `ROLLBACK` command status、clear後
`PQTRANS_IDLE`を全てrequireする。proofがmissing/failならpoison/closeしてslotをretireする。
既存dependent stmt/rows resourceがconn/tx parentのDropを防ぐため、active
result/prepared handleがidle arrayへ入ることはない。native connectionをpoison/closeするpathは
conn/tx Dropまでchecked-out accountingを保持し、その後stateをreturnせずfreeする。capacityはfixedで
そのslotはretireする。poolはcredentialを保持せずtransparent reconnectしない。後続 `info` は低下した
`idle + checked_out` を公開し、残るidle setがemptyなら `try_acquire` は `PoolExhausted`。
replenishmentはpoolを明示的にDropしてopenし直す。

`Pool` はsettled resource ruleによりMove、non-Copy、non-Sendである。borrowed operationがmutate
できるのはprivate native bookkeepingだけで、task capture/concurrent accessは許さない。したがって
v1のblocking acquireには意味がない。non-Send callerが待つ間にconnectionを返せるconcurrent ownerが
存在しない。future Send/thread-safe resource formにはsynchronization、wait fairness、timeout、
cancellation、Drop coordinationを持つnew pool contractが必要で、このcontractを黙って変更しない。

Pool Dropはclosing stateを先にpublishし、全idle connectionをreverse LIFOでclose/freeする。
checked-outがなければpointer array/pool recordを即freeする。残る場合、record/arrayはprivateのまま
checked-out connection originからだけ到達可能であり、そのconn/txはPool Drop後もusableである。
各later Dropはphysical stateをreturnせずclose/freeし、outstanding countをexactly once decrementし、
最後がarray/pool recordをfreeする。Pool Dropはblock、borrower wait、SQL発行、checked-out native
connection invalidationをしない。public manual close/detach/adopt/cross-pool returnはない。

private connection recordはexact 40-byte、8-aligned v2となる。offset `0..31` はsettled
driver/closed/native/busy-timeout/execution/transaction/statement-ordinal meaningを保持し、offset 32は
origin-pool pointerで、direct driver connectionはnull、attached pool slotはnon-null。全
connection-state validatorはnative access前にv1/unknown version/driver、invalid reserved、malformed
live/closed native-pointer productをrejectする。originはprivileged package producer/Drop pathだけが
install/consumeする。opaque resource内の全native pointerと同様、arbitrary pointer corruptionはprobe
すべきpublic inputでなくsafe source contract外である。private pool recordはexact 48-byte、8-aligned v1:
0に`u32 version=1`、4に`u8 driver`、5に`u8 lifecycle` (`0=open`,`1=closing`)、6にzero `u16`、
8/16/24にsigned `capacity`/`idle`/`checked_out`、32にnon-null slot-array pointer、40にzero `u64`。
arrayはexact `capacity` pointer cellで、`[0,idle)` だけがnon-null connection stateをownする。
constructor publication、acquire、return、retirement、Pool Drop、last-checkout Dropだけがtransitionである。
state pointer/slot array/counterはpublic ABI、persisted artifact、cache key、FFI signatureへ入らない。
`pkg.db.internal.resource` がshared conn-origin/pool-record validator/transitionをownし、
`drop_conn`/`drop_tx` はpublic pool moduleをupward importしない。
`pkg.db.pool.internal.resource.drop_pool` はrequired descendant hookとしてraw ownerをそのcommon
authorityへdownward delegateし、`pkg.db.pool` constructor/observerもsame sealed helperを使う。
これによりmodule graphをacyclicに保ちlayout ownerを1つにする。

implementationは1 capability PRとする。pool producerとconn/tx returnを分割するとdormant poolか、
returnせずcloseするconnection Drop pathを残す。package sourceとowner testを変更し、compiler ownership
semantics/IR shapeは変えない。throughput/latency数値を約束しないためbenchmarkはcorrectness gateで
なく、allocation/native-call counterがexplicit-cost ruleを証明する。push前に
`scripts/db-verify-local.sh` を実行しrequired live PostgreSQL jobをnon-skippableのままにする。

Implementation closure matrix:

| Closure cell | Required implementation closure | Exact owner evidence |
|---|---|---|
| public surface/error | constant/resource/Info/2 constructor/`try_acquire`/`info`だけexportし、payload-free `db.Error.PoolExhausted`だけ追加。interface/cache identityをonce更新。 | source/interface golden、whole/per-unit imported call、exhaustive Error match、acquire/close/release/adopt alias不存在 |
| capacity/constructor validation | capacityは全multi-invalid productに勝ち、0/negative/1025/`i64` extremaをallocation/native前にreject。valid 1/1024はexact driver validationへdelegateしordinal順にopen。 | allocation/connect counter、capacity x NUL/invalid-option winner、both-driver boundary owner |
| partial formation cleanup | 各physical ordinalのfailure/nullはprior unpublished connectionを逆順exactly once closeし、全raw ownerをfree、exact driver errorを返しpoolをpublishしない。 | SQLite/PostgreSQL parameterized ordinal failpoint、allocation/native-close ledger |
| acquisition/observation/order | complete state validationがslot read前。initial/returned stateをLIFO popし、successはallocation/clock/native callなしにidle/checked-outをexact 1変更。empty valid stateはmutationなしのexact `PoolExhausted`。session-global stateはphysical LIFO slotに意図的に追従する。 | byte/counter snapshot、session-identity/session-state LIFO probe、exhaustion replay、checkout前中後info |
| conn/tx ownership transfer | checked-out connはbegin/commit/rollback、failed commit/rollback cleanup、move、return、replacement、branch、loop、`?`、Dropでoriginを保持。Tx Dropは上記exact SQLite/PostgreSQL rollback-and-idle proof後だけreturn。raw provenanceでdependencyをlaunderしない。 | existing Q4a transaction matrix + pool counter、rollback status/tag/transaction-state failpoint、whole/per-unit |
| dependent execution resource | stmt/rows/batch view/catalog cursor/EXPLAINは既存parent generation/leaseを保持。child live中conn/txはreturn不能で、child Dropがpool return前に完了。 | compile-time parent-overlap negative + both-driver Q4a/Q4b/Q5b2/A1 pooled execution owner |
| poison/close/retirement | every existing poison/close causeとconn-return native-idle/tx rollback-and-idle proof failureはreuseをfailさせ、conn/tx Dropはidleにせずfree、checked-outをonce decrementしreconnect/resetなしのvisible capacity gapを残す。同期/native idleを保持するordinary DB errorはreturn可能。 | raw transaction-control command leakage attemptを含むdriver status/timeout/restore/conn-idle/rollback failure matrix、retirement後info/exhaustion、zero reconnect/reset call |
| outstanding owner付きPool Drop | closingをidle teardown前にpublishしidleをonce close。checkout中bookkeepingを保持しconn/tx operationを許し、各Dropでclose、最後だけbookkeeping free。 | zero/one/many idle x zero/one/many checked-out allocation/close/order、tx/dependent stmt/rows含む |
| private ABI/malformed state | 全v2 conn/v1 pool offset/tag/reserved/count/native-pointer product/slot prefixがone common internal authority、constructor/accessor、thin descendant Drop hook、conn/tx Dropで一致。malformed authenticated fieldはslot/native access前にfailしunknown driver dispatchなし。raw origin/slot pointer validityはprivileged unsafe producerのobligation。 | exact byte golden、one-field scalar/tag/null corruption no-call、v1 conn/unknown pool lifecycle rejection、constructor-origin transition/no-upward-import assertion |
| driver/build parity | direct/pooled connectionはsame execution code/result/error identity。pool codeはwhole/per-unitでboth driver producer Drop thunkをretainしambient configなし。 | complete fake-driver matrix、live SQLite/PostgreSQL acquire/use/tx/reuse/Drop、`scripts/db-verify-local.sh` |

`3da5488c` のfresh independent adversarial design reviewはP1 1件/P2 2件を発見した。この
ledger-first revisionがcomplete finding setを閉じる:

| Finding | Ledger-first closure | Required owner |
|---|---|---|
| wrapper-idle connがnative `BEGIN`/`START TRANSACTION`を隠しtransactionを次acquisitionへleakできた | every conn returnでexact driver-native idle proofをrequireしfailureはretire。non-transaction session stateのretentionを別に明記。 | raw transaction-control command x SQLite/PostgreSQL native-idle/retirement/no-next-acquirer matrix |
| pooled tx Dropがsettled close-only ruleと矛盾 | earlier transaction ruleを全てqualifyし、directはclose、pool-originはstronger exact rollback-and-idle proof後だけreturn。 | explicit-end failure/implicit Drop x direct/pool x rollback proof success/failure |
| exact pool contractにSettled recordがなかった | fixed-capacity non-waiting poolと、openのgeneral Send/thread-safe mutable-state questionを分離してrecord。 | source-of-truth consistency check |

code review前にauthor-side matrix-to-diff passを1回行い、全applicable cellをimplementation pathと
discriminating ownerへ対応させる。

bounded batch generation、segmented child/validity bitmap、eligible direct SoAはfirst A1でshipped。
PostgreSQL single-row/portal-batchとbinary formatは上記ledgerでspecified。
COPY/pipeline/LISTEN-NOTIFY、SQLite backup/blob/FTS、
explicit poolは上記ledgerでspecified、common contract実証後の追加driver。

### D14 — dynamic SQLとnative callback

minimal owned `db.value` とindexed `db.row`、visible dynamic SQL/value slice/exact
`db.Driver` restriction/execute option、pre-send mismatch/U+0000 rejection、
typed Queryと別artifact path、reflectionなし。SQLite function/collationやPostgreSQL
notice/COPY callbackはcapture、abort、reentrancy、thread、lifetimeを証明してから追加する。

### 初期release gate

D labelはacceptance ownershipを表す。publicationは上記delivery waveに従う。

```text
prerequisite gate -> Q1 -> Q2
                           +-> Q4a reusable -> Q4b streaming -> Q6 compound --+
                           +-> Q3 checked/offline -+-> Q5a migrations -------+-> initial release
                                                   +-> Q5b metadata/EXPLAIN -+

P0は最初のnative product wave前に並行実行する。
```

Q2はD2/D4を一緒に閉じ、common APIのdriver driftを防ぐ。Q3も同じ理由でD3/D5を一緒に
閉じる。Q3とQ4aはQ2後に並行開始する。Q4aはD6/D7のprepared/transaction reuse、Q4bは
D8/D9のstreaming/cancellation resilienceを閉じ、それぞれuseful capabilityを1回だけ
publish/reviewする。Q5a/Q5bはQ3後に
並行でき、Q5aはD3がownerのshared migration identityも使う。Q6はQ4b後に進む。

L1a〜L7と、driver-relevantなD1〜D12をSQLite/PostgreSQL両方で満たす。D11のSQL
migration lifecycleとD12のcategory metadata/明示Query planも、それらを約束する初期
release gateに含む。D13〜D14はbatch/SoA/native breadth、dynamic SQL、proved callbackの
committed additive roadmapである。
完了報告では、初期 `pkg.db` releaseをL1a〜L7 + D1〜D12、約束済み `pkg.db` roadmapの
全完了をさらにD13/D14まで含む状態とする。D13はtyped streaming/cancellation/compound
pathの後に進む。D14は両driverとproved cancellation/callback ruleの後に進み、D13には
依存しない。
single-table CRUDだけでは不完全。many-to-oneとone-to-many compound Outputをそれぞれ
execution-count付きで実証する。

## 24. Acceptance criteria

1. SQLが可視でrelational behaviorのsourceである。
2. primary public unitが名前付きQuery moduleである。
3. same-basename `.align`/`.sql` がpath文字列なしで動く。
4. explicit relative SQL linkageもある。
5. typed Queryが明示Paramsとexact flat Rowを持つ。
6. 通常Align codeで作るlogical Outputを公開できる。
7. 1 Queryが1 SQL statementを1回だけ実行する。
8. shapingはSQLを実行できず、既定1-pass。
9. field accessはI/Oを起こさない。
10. SQLite/PostgreSQLが初期common surfaceを持つ。
11. common-only codeは両driverで同じAlign interface。
12. connection/Query/prepare/execute/tx/metadata/explainにnative controlがある。
13. unsupported optionはignoreせずerror。
14. normal buildはoffline。
15. checked metadataはexplicit、hashed、stale-safe。
16. SQL NULLはOption、曖昧implicit conversionなし。
17. typed Row mappingはruntime reflectionなし。
18. streaming/materialization/allocation modeが可視。
19. borrowed viewはrow/batch/result bufferより長生きしない。
20. migrationはSQL。
21. metadataはcategory-specificでkeys/indexesを含む。
22. plan取得は明示、ANALYZEは実行を明示。
23. compound exampleがtransaction+master、User+Groupsを含む。
24. testがSQL実行回数とhidden follow-up 0を固定。
25. package overheadをdirect native loopに対してlocalに測定可能とする。この測定は
    PR、release、milestone gateではない。
26. handleはgeneral resource/borrowを使い、compilerにpkg.db名のruleがない。
27. caller materializationはnamed arena/region、ambient allocatorなし。
28. SQL-only editはproducer/artifactをinvalidateし、unchanged consumerを不要にrecompileしない。
29. compound shapingはPureな `borrow mut` state/独立builder stepとvisible rows loop。
30. region builder allocationと1 compact passをlocalに測定可能かつ可視に保つ。
31. structured Move error/Outputはordinary recursive tagged Drop、Ok path error allocationなし。
32. 3 moduleは1つの `pkg/db` subtreeでacyclic。
33. contextual `borrow`/`out`/`resource` parsingでcanonical signature/intrinsicがparseできる。
34. Copy stateのmutable borrowとindirect borrow-mode callがcaller mutation/ABIを保存する。
35. imported resource Dropがproducer thunkへlinkしexactly once。
36. 1 descriptorはwhole-body constructor 1個とunique artifact/thunk slotを持つ。
37. arena-built compound arrayはinlineでOutputへ入り、通常by-value callを越えない。
38. indirect borrow-returning callは全possible target owner/regionをconservativeに保持する。
39. inline SQLはitem-based tagged identityを持ち、diagnosticを `.align` literalへ戻す。
40. SQLite migration-backed prepareは実行前に1つのnumeric orderを検証/fingerprintする。
41. unresolved higher-order callはcompatibleなby-value Move input内部のprovenanceを保持する。
42. resource Drop hookは通常の完全修飾module pathを使い、resource専用name lookupを持たない。
43. PostgreSQL `BufferedFull` は、`one`/`maybe_one` の2-row decode limitとは別にfull
    transport/bufferingを報告する。
44. L1aは `Option<string>` field leafだけを許可し、`Option<MoveStruct>` はL1bだけが許可する。
45. dependent resource constructionとchecked owner-tied native viewはloweringを通して明示的な
    typed MIR operationである。
46. `borrow mut` はrecursiveにprovenanceを持つby-value Copy/Moveを含む全overlapping peer
    argumentを拒否する。
47. execute/result/prepare primitiveはoption-bearing signatureを正確に1つだけ持ち、`[]` が
    no optionを表し、optionless overloadはない。
48. canonical exampleはcallee parameterが `borrow mut` の `rows`/`stmt` を `mut` bindingにし、
    call siteへparameter modeを書かない。
49. English/Japaneseのprepared-statement exampleは同じsignatureに対してtype-checkする。
50. 最初のpublic database releaseはdriver-relevantなD1〜D12を完了し、約束済みroadmapの
    全完了はさらにD13/D14を完了する。
51. `rows`/`rows_stmt` はreturn時にParams source provenanceを全て解放し、SQLite v1は
    measured transient text/blob bind copyによって最初の `next` 前のsource invalidationを
    許可する。
52. dynamic SQLはexact `db.Driver` をsourceに書き、SQL送信前に照合する。
53. verified core signature tableはshipped formだけを含み、L4/L6 formは担当PRまで
    required-but-unimplementedと明記する。
54. 全category metadata primitiveは1つの `MetaOption` sliceを持ち、native formは別の
    native option sliceを1つ追加する。
55. metadata/EXPLAIN結果は§18のexact flat `RegionPlain` shapeと明示regionを使い、
    native bufferをborrowせずhidden heapも使わない。
56. checked stateは許可driverごとで、AnySupportedDriverのCheckedRequiredはSQLite/
    PostgreSQL両artifactを要求する。
57. StaticCommandArtifact/CommandStaticはQueryのsource/wire/binder/retention/checked/cache
    ruleを共有し、Row/result/decodeだけを省く。
58. §§11〜13のcommon/SQLite/PostgreSQL option sum/default/conflictは実装例でなく必須finite set。
59. D1/D2/D4/D6/D7が必要option APIを所有し、D9はinterim surfaceなしにdeadline/cancellationと
    cross-scope dispositionを完成する。
60. migration transaction policyは先頭lineのrequired/forbiddenだけで、requiredは既定atomic、
    forbiddenは1 statementとdirty state/checksum-bound repairを使う。
61. LEFT JOIN childなしは全child fieldがNULLのときだけで、どちら向きのpartial NULLもerror。
62. next_batchはD1〜D12 common surfaceになくD13だけで追加する。
63. many-parent canonical shapingはparent/child/offsetの並行arrayを作り、array-bearingな
    per-parent Outputをregion builderへpushしない。
64. PostgreSQL skipはoptional local runだけで、D4 merge/releaseはprovisioned non-skippable
    db-postgres CIを必須とする。
65. 通常package codeで `rows_stmt<P,R>` と `all<P,R: RegionPlain>` を定義でき、nested
    generic typeはMIR前にconcreteとなり、DB builtin helperを必要としない。
66. recursively Moveなreturnはdirect/indirect/imported ABIでdynamic cleanup bitを返し、
    arena-owned `Ok` とindividually owned `Err` を正確にDropする。
67. `borrow mut` は全peer modeのdirect/recursive overlapを、異なるaggregate holderと
    `Out` を含めて拒否する。
68. captured view/resource_refを返すfunction valueはdirect/indirect call、join、move後も
    selected environment/captured ownerに拘束される。
69. `borrow mut` によるowned replacementはold pointeeを正確に1回Dropしてcaller bitを更新し、
    unchanged pointeeをcallee exitでDropしない。
70. `resource.into_raw` はstandalone owned resource rootだけを受け、
    field/element/projection/borrowed/out/temporaryを拒否する。
71. static manifest/action keyはdriver別checked metadataのexact pathと
    `Missing | Present(hash, format_version)` を含み、create/change/deleteでinvalidateし、
    Anyはdirectory scanなしで両driverを追跡する。
72. synthetic field-selector/function-value factはnamed functionと同じcapture-root summaryと
    Move-return cleanup ABIを使う。
73. static/dynamic/migration SQL、PostgreSQL Text Params、libpq connection/control stringは
    native call前にU+0000を拒否し、Binary-format byteaはlength-awareのままにする。
74. PostgreSQL初期mappingはinteger/float/bool/text/bytea/Optionだけで、temporal/numeric/
    UUID/JSON/array/range/domainは後続の明示contractを必要とする。
75. D9は全deadlineをenforceまたはpre-send rejectし、hidden SQLを発行せず、general
    Send/thread-safe-resource前提がscheduleされるまでexternal cancel resourceを公開しない。
76. SQLite `BusyTimeoutNs` はstreamed `next` 中も有効で、exhaustion/error/Drop時に
    connection reuse前のpackage-tracked prior valueへrestoreする。
77. synthetic field selectorはtop-level viewだけでなくnested view-bearing return typeの
    receiver provenanceもrecursiveに保持する。
78. PostgreSQL Text-format byteaはlowercase `\x` hexでraw byteを渡さず、Binary formatだけが
    raw byteとexplicit lengthを使う。
79. timeout/cancel後のPostgreSQL connectionはprotocol/transaction state同期を証明できる
    場合だけ返し、それ以外はpoison/closeする。
80. `ContractError` はQuery IDを捏造せずQueryなしoperation/input validationを表現できる。
81. 初期releaseの例とmappingはinteger/float/bool/text/bytea/Optionだけを使い、deferred
    logical typeは後続の明示contract前にpublic exampleへ出さない。
82. `ContractError.query_id` はmetadata/EXPLAINを含めQuery/command subjectがあれば
    `Some(id)`、subject自体がないoperationだけ `None` にする。
83. `db.exec_result` はallocation-freeなCopy record
    `{ rows_affected: Option<i64> }` で、native result-buffer viewを残さない。
84. resolve後にfiniteなstruct fieldはCopyまたはordinary Drop planを持つrecursive Moveでよく、
    L1aの `Option<string>` fieldはaggregate field ruleと矛盾しない。
85. `resource.borrow` はresource typeがvisibleならpublic safe ownership operationであり、
    raw construction/extraction/transfer/owner-tied raw viewだけdeclaring subtree privilegeにする。
86. repairを含むD11 live commandはentry/catalog/driver/matching targetを全て明示しambient
    defaultを使わない。
87. metadata filterはexact Copy `SchemaRef`/`TableRef` を使い、search path、`main`、SQL
    interpolationで推測しない。
88. `KeyMeta` はavailableなforeign-key match/update/delete、deferrability、initial deferral、
    validation evidenceを保持し、duplicate nameを `key_ordinal` でdeterministicにgroup化する。
89. `IndexMeta` はkey/include、unique/primary backing、sort/null order、predicate/expression、
    native method/opclass、valid/ready evidenceを保持する。
90. `ColumnMeta`/`QueryMeta` は§16/§18が約束するnative identity、origin、checked artifact、
    rewrite、prepare/schema/server、descriptive fieldをexactに持つ。
91. D0がengine/version nullability/origin evidenceを記録し、D3/D5がmerge前にfail-closed
    support matrixを所有する。`Unknown` はnon-nullを証明せずruntime NULL guardを常に残す。
92. 全metadata category/detailと `MetaQueryEntry` discriminatorは§18.2.1のexact row、
    field presence、Unknown state、group ordering、ordinal、artifact schema/digest contractに従う。
93. metadata schema/table inputのU+0000は両driverでnative/catalog request前にexact
    Query-less `db.Error.Encode` としてrejectし、multi-invalid inputはdeclaration-order
    precedenceに従う。
94. `pkg.db.pool` はexactに `MAX_CAPACITY`、`Pool`、`Info`、`open_sqlite`、
    `open_postgres`、`try_acquire`、`info` だけをexportする。acquisitionはimmediateでwait、
    timeout、hidden open、aliasがない。
95. pool capacityはexact `1..=1024`。全physical connectionをone explicit driver input setから
    eagerly openし、partial formationはexact driver errorを返す前に全unpublished ownerをcloseする。
96. checked-out connはconn/tx conversionと全dependent execution resourceでone pool originを保持する。
    live Dropはexact native-idle proof後だけLIFO returnし、poison/proof failureはretireする。どのpathも
    reset/reconnectせず、他のsession-global stateはphysical slotに意図的に残る。
97. Pool Dropはidle connectionを先にcloseするがchecked-out conn/tx ownerをinvalidateしない。
    last later owner Dropがconnectionをcloseしretained bookkeepingをexactly once freeする。
98. `db.Error.PoolExhausted` はpayload-free/allocation-free。`pkg.db.pool.Info` がexact capacity、
    idle、checked-out countを公開するためretired capacityもvisibleである。

## 25. 実装前にconsumerで確定するtype/native detail

load-bearing shapeは確定済み。残るもの:

1. decimal precision/scale表現。
2. UUID、temporal、JSON/JSONB、PostgreSQL array/range/domain、SQLite custom type mapping。
3. minimal safe dynamic `db.row`/`db.value` variant。
4. SQLite function/collation callback safety。
5. measured consumerを持つCOPY/pipeline/backup/blob operation。PostgreSQL COPY consumer/workload
   measurementはcurrent recordに存在しないため、そのpublic surface/implementationはdeferredである。

engine/versionごとのnullability/origin support matrixは§16.3.1で確定しD0/D3/D5が所有する。
残りはD12〜D14に担当を持つ。Query identity、one-execution、ownership、artifact、
runtime validation、option rejectionを弱める理由にはならない。

## 26. 実装agentへの指示

1. この文書と
   [`../../17-library-boundary-prerequisites.md`](../../17-library-boundary-prerequisites.md)
   を完全に読む。
2. L1a〜L7の依存DAGに従い、独立なL3/L4/L5を直列化しない。全前提gateの前にsafe
   driver APIを始めない。
3. Rust compiler PRごとにAlign self-reviewを実行する。
4. owner matrixの整合性を保つ最少数のindependently correct capability PRを使う。
   roadmap/acceptance labelをPR境界にせず、閉じるcapabilityの指定negative/cleanup owner
   testを全て入れる。
5. database keyword、ORM、Query DSL、reflection、public trait hierarchy、ambient allocator、
   public manual close、package-name ownership special caseを導入しない。
6. 足りない前提をraw、destroy function、hidden heap vector、lint-only lifetime、
   whole-program-only shortcutで代替しない。
7. D1以降、separate compilation/cache contractを守る。
8. SQLite/PostgreSQL metadata behaviorを推測せず実測して記録する。
9. SQL execution countとallocation/copy countをcorrectness testとして扱う。
10. single-table CRUDだけでQuery model完成としない。
