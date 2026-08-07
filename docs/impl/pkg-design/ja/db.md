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
optionで選び、unsupportedならerrorで、黙ってdowngradeしない。

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
- transparent retry、transparent pooling;
- 「最適」なrelational fetch strategyの自動選択。

poolは将来、明示的な `pkg.db.pool` として追加できる。Query意味論を変えず、
connection acquisition/wait costを隠さない。

## 4. Package layout

### 4.1 初期public module

```text
pkg.db
pkg.db.sqlite
pkg.db.postgres
```

これは独立versionの3 packageではない。Alignのpackage規則に従う、1つのvendorable
`pkg/db` subtree内の3つのpublic module境界である。root/common moduleはpublic driver
moduleをimportせず、driver moduleがcommon/internalへ下向きに呼ぶ。この向きでmodule
graphをacyclicに保つ。

将来候補:

```text
pkg.db.pool
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
db.exec_result
db.Driver
db.row
db.value
```

- `db.conn`: SQLite/PostgreSQL connectionを1つ所有するopaque Move resource。
- `db.tx`: transactionへmoveされたconnectionを所有するopaque Move resource。
  activeなままDropするとfail-safe rollback後にcloseし、commitはしない。
- `db.exec`: conn/txのどちらからも生成する短命なborrowed execution view。
- `db.query<P,R>`: SQL identity、Params/Row contract、driver restriction、hash、
  static optionを持つCopy static descriptor。
- `db.command<P>`: Rowを返さないdescriptor。`RETURNING` はQueryである。
- `db.stmt<P,R>`: prepareしたconnectionへのdependencyを持つMove statement。
- `db.rows<R>`: 1実行だけのone-pass typed stream。native bufferが必要な間、
  statement/connectionへdependencyを持つ。
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
- private sqlite3/libpq FFI宣言を包むsafe wrapper;
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
libsqlite3/libpqへ直接行う。

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
  generated QueryMeta materialization thunk

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
producer-owned QueryMeta table/thunkはD12 rowをcaller regionへmaterializeし、runtimeで
decoder codeをinspectしたり `.align-db` を開いたりしない。

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
する。将来のsingle-row/portal/pipeline/async pathはnative protocolが不要になるまで自身の
parameter bytesを保持する。これはper-execution bind copyでありper-row copyではない。
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
```

`one`/`maybe_one` はcardinality判定に最大2 delivered rowsをdecodeするが、§2.4の
driver-specific transport/buffering costは別である。`all` はstructural
`RegionPlain<R>` を要求し、region builderのchunk growthと1回のcompact passを使う。
exact package definitionは `P, R: RegionPlain` のgeneric functionである。`RegionPlain` は
L7のclosed builtin structural boundであり、public/user-defined trait hierarchyではない。
v1 static Rowはすべて満たす。
bounded `next_batch` はD13の追加APIであり、D1〜D12の初期common operationには含めない。
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

後続D13でbounded batch generation、validity bitmap、segmented child buffer、eligibleな
`soa<Row>` 直接decodeを追加できる。初期Query契約を変更せず、intermediate AoSを
必須にしない。

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

### 12.2 parameter/result format

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

`name`と`canonical_type_name`は`str` literalである。

Text formatのbyteaは§5.6.1のexact hex encodingを使い、Binary formatだけがraw byteと
explicit libpq lengthを使う。未実装のbinary mapping要求はSQL送信前に `Unsupported` とする。

初期実装がtext中心でもbinaryを閉ざさない。unknown/conflicting OIDをignored hintにせず
errorにする。ParameterTypeはstatic artifact/public contractへ入りPostgreSQLへpinする。
field別controlのunknown/duplicateもerror。binary mapping未実装ならSQL送信前にUnsupported。

connection sumは `ApplicationName`、`ConnectTimeoutNs`、`SslMode`、
`TargetSessionAttrs`、`Parameter(name,value)` で固定する。URLとoptionのsemantic key重複は
conflict、secretはartifactへ入れない。transactionの `[]` はReadCommitted/ReadWrite/
non-deferrableで、deferrableはserializable read-only以外ではBEGIN前にrejectする。
SearchPathOnlyとIncludeSystemCatalogsはconflict。Buffers/Timing/Walはnative Analyzeが
なければ実行前にconflictする。

### 12.3 native feature

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
consumeし、成功時にconnを返す。明示end失敗またはDropはbest-effort rollback後に
owned connectionをcloseする。状態不明connを返さず、Dropでcommitしない。

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
connection/transaction/metadataなどQueryなしのoperation/input validationなら `None` とし、
どちらも `item` がexact operation/inputを示す。success hot pathはerror stringをallocateしない。

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
`--memory [--migrations]` は§16.6からidentityをderiveして `--schema-id` を禁止する。

### 16.3 metadata location

```text
.align-db/
  sqlite/<descriptor-id-hash>.json
  postgres/<descriptor-id-hash>.json
```

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
- 各file全体をその順でmigration scriptとして適用（Queryの1-statement rule対象外）;
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
boundaryを所有する。driver-authoritativeなscript preparation/screeningで最初のmutation前に
rejectする。
requiredはmigration lock取得後、全statementとApplied history insertを1 transactionで
行い、どのerrorでも全体rollbackする。transaction内で拒否されたstatementを外へ出して
retryしない。

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
- structural contract/artifactとQueryMeta thunk（1/10/100 reachable definitions）;
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
L2d reusable Move ownerへのshared borrow
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

### D1 — fake driver上のgenerated Query/command

#### Q1/D1 implementation closure matrix

Q1は1つの実行可能capabilityである。public package declaration、compiler-produced
artifact、generated binder/decoder/metadata thunk、fake-driver consumerは1つのdescriptor
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
plan/materialization thunkも生成し、separate compiled Queryでruntime artifact/source I/Oなしを
testする。reflectionとper-row name lookupがないことをtest/IRで固定し、このpathが最初に
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
追加する。headerはexact ABI version、statement kind、driver mask、descriptor IDと各permitted
wire SQL entryのstatic `str` view、Q1 plan pointer、direct producer-owned binder pointer、
Queryだけのdecoder function pointerを持つ。pointer fieldはtarget pointer width、`str`は通常の
Align `{pointer, i64 length}` ABI、version/kind/maskは`u32`/`u8`/`u8`である。これはobject内の
relocation-bearing constantであり、persisted codecでもuntrusted runtime inputでもない。
Binder ABI v1は`fn(context: raw, borrow params: P) -> i32`で、0はsuccess、nonzeroはpackage
execution contextが既に所有したerrorを表す。Decoder ABI v1は`fn(context: raw) -> R`である。
packageがrow count、column count、NULL、native scalar representationをvalidateした後だけ、
infallibleなdirect field-offset constructionを呼ぶ。calling/layout contractを変える場合は既存
artifact ABI versionとexecution-header versionの両方をincrementする。

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
| native descriptor ABI | exact Q1 plan bytesを保ったままdescriptorごとに1つのrelocation-bearing `QueryStatic`/`CommandStatic`をemitする。object publication前にheader kind/mask/thunk presenceをvalidateし、public descriptorをone-pointer Copy valueのままにする。SQL-only producer editはunchanged public consumer interfaceを変えずproducer constant/object identityを置換する。 | exact MIR/LLVM header inventory、ABI-version golden、Query/command omission twin、同じ型の2 descriptorのruntime selection、whole/per-unit executable parity |
| generated binder/decoder | Query/commandのdirect ordinal `i64` binderとQueryだけのdirect ordinal `i64` decoderを生成する。binderはprotocol ordinalを使い最初のpackage-recorded failureで停止する。decoderはpackage validation後だけ実行し、name/map/boxing/reflectionなしで`R`をconstructする。Q2未対応field shapeはnative send前にfailし、後続mappingはD8が所有する。 | 両driverのportable `CAST(:value AS BIGINT)` bind/decode、repeated PostgreSQL placeholder ordinal、field-order twin、unsupported-shape no-send、generated thunk MIR/LLVM inspection |
| type and monomorph closure | generic instantiation、function-value signature、interface serialization、whole-program/per-unit compilation、cache/implementation identityを通じてconcrete `P`/`R`を保つ。unresolved type、wrong header kind、absent decoder、mismatched thunk signatureをMIR/codegenへ到達させない。 | whole/per-unit Query/command execution、generic mono-key/header-signature golden、malformed HIR/MIR/header rejection owner |
| connection formation and ownership | SQLite/PostgreSQL descendantがinput/optionsをvalidateし、1つのphysical native connectionと1つのtagged package stateを作りroot `db.conn`をconstructする。ownerのmove/return/replacementでstateは1つのまま、`db.exec`はgeneration-checked `resource_ref`だけを持つ。source nulling、branch/loop join、early `?`、Dropでclose/free exactly once。 | construction、move-in/out、return、replacement、malformed null、early return/`?`、branch/loop join、use-after-move、whole/per-unit producer Drop thunk linkageのresource owner matrix |
| common validation precedence | driver-global state取得やSQL sendより前にcommon option shape/duration/duplicate、descriptor kind、driver restriction、connection stateをvalidateする。deadline enforcementはD9所有なのでQ2のcommon deadline要求は`Unsupported`。driver mismatchは`Some(query_id)`を持ちexecution/native-call countを増やさない。 | common timeout、wrong kind、mismatch、poisoned/closed state、duplicate optionのordered multi-invalid tableとno-send/no-lease/no-allocation counter |
| SQLite connection options | §11.2の全`sqlite.ConnectOption`、exact `[]` default、conflict、positive duration、NUL-free path、PRAGMA name grammar/value quoting、duplicate PRAGMA、linked capability rejection、setup failureを実装する。指定されたvalidationはopen/setup前に行い、flag/PRAGMAをdegrade/ignoreしない。 | parameterized option disposition、multi-invalid precedence、open/setup counter、PRAGMA round trip、unsupported-capability injection、failed-open exactly-once cleanup |
| SQLite execution lease/options | bind/timeout/native workより前にconnection-wide leaseを取得する。`sqlite.ExecuteOption.BusyTimeoutNs`はpositive/uniqueでtracked valueを一時置換し、success、bind error、prepare/step error、0/1/2+ cardinality、Drop unwindの全synchronous return前にrestoreする。second operationはfirst leaseのread/restore前にfailし、restore failureはpoison/closeする。 | overlap table、busy-timeout apply/restore counter、failed-second-operation、success/error/cardinality/early-`?` cleanup、restore-failure poisoning、execution count |
| SQLite command/query lifecycle | exactly one statementをprepareしtailがwhitespace/commentだけであることを要求し、`i64`をbindし、commandをcompletionまでstepし、nonnegative affected rowsを読む。`one`は最大2回stepする。全pathでlease restore/returnより前にfinalize exactly onceし、primary/extended codeとowned messageはfinalize後も生存する。 | in-memory insert/select、0/1/2+ cardinality、second-statement rejection、bind/prepare/step/finalize fault injection、affected-row case、cleanup後error ownership、direct-libsqlite comparison |
| PostgreSQL connection options | §12.2の全`postgres.ConnectOption`、exact `[]` default、URL collisionを含むsemantic-key conflict、positive timeout conversion、SSL/target attribute、arbitrary parameter name/valueを実装する。URL/application/parameter stringのU+0000をlibpq前にrejectし、secretをstatic artifactへ入れない。 | option disposition/multi-invalid table、URL/option conflict、embedded-NUL no-call、unreachable/auth owned error、artifact/interface/cache bytesへのsecret非混入 |
| PostgreSQL execution options/binding | Text `i64` bindingとexact baseline `postgres.ExecuteOption` validationを実装する。unknown/duplicate parameter name、Binary `i64`、unavailable result formatはsend前に`Unsupported`。repeated source nameは1つの`$n`をreuseする。synchronous callがreturnまでparameter transportを所有する。shared bytea codecもここで閉じる。Textは`\\x`とbyteごとのlowercase hex 2桁を生成し、recorded length外にNUL sentinelを置く。Binaryだけがraw bytesとexplicit lengthを公開する。D8まではbyteaをexecutable descriptor shapeにしない。 | format disposition、no-send counter、`CAST($1 AS BIGINT)` execution、repeated-placeholder、embedded zero/high byteを含むindependent Text/Binary bytea golden、parameter buffer allocation/free counter |
| PostgreSQL result/cardinality | explicit `BufferedFull`を使う。transportは全rowを所有してよいが`one`がvalidate/decodeするのは最大2行。decoder前にresult status、exact column count、NULL、full-range decimal `i64` parseをcheckする。connection reuse/return前に各`PGresult`をclear exactly once。 | 0/1/2+ result、2超rowのdecode-count pin、NULL/type/range/column-count failure、result-clear counter、full-result pointer-lifetime probe、direct-libpq comparison |
| native error ownership | statement/result cleanup前にSQLite code/messageとPostgreSQL SQLSTATE/message/detail/constraint/table/columnをexact owned `db.NativeError`へcopyする。message parseなしでstable constraint/serialization/deadlock categoryへmapし、Query/command errorは`Some(query_id)`、connection-input errorは`None`。 | finalize/clear/connection Drop後のerror-field golden、SQLSTATE category table、SQLite primary/extended table、Query-less connection error、allocation/drop counter |
| FFI/ABI and malformed input | supported targetごとに使用するSQLite/libpq declaration、enum/status constant、pointer/length signedness、destructor order、linked libraryをpinする。negative/overflow length、null-with-positive-length、invalid UTF-8/native text、malformed execution-header/thunk stateをdereference/side effect前にrejectする。 | D0 probe record、compile-time C signature probe、Rust/Align declaration inventory、malformed boundary test、利用可能なASan/Valgrind相当owner、x86_64/ARM64/macOS CI |
| allocation parity | scalar connect/execute/one successはvisible connection/execution/native objectとPostgreSQL Text parameter storageだけをallocateする。per-row heap allocation、error allocation、runtime dictionary、artifact/source I/Oは禁止する。partial allocationごとにownerとcleanup edgeを1つ持つ。 | success/各injected partial failureのallocation/copy counter、DB runtime helperを含まないemitted-symbol inventory、package対direct driver measurement |
| required PostgreSQL gate | pinned provisioned `db-postgres` CI jobを追加する。`ALIGN_DB_POSTGRES_REQUIRED=1`はmissing/unreachable configをfailureにし、同じportable Queryを両driverで実行する。local absenceだけは理由付きskip可。native library/server versionをevidenceとして表示する。 | missing URLのrequired-mode self-test、provisioned PostgreSQL job、portable dual-driver integration target、unconditional/required-mode skip branch不在 |

author-side matrix-to-diff passでは、全acquired native pointer、active lease、timeout override、
statement/result、parameter buffer、owned error stringを、success、各native error、cardinality exit、
early `?`、Dropそれぞれの1つのcleanup ownerへ対応付ける。header/thunk strategyを変える指摘、
second SQLite operationがconnection-global stateへ触れる経路、complete validation前のPostgreSQL
send、driver semanticsのruntime移動が見つかった場合はこのmatrixを再度開き、high-risk review
pathを要求する。

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
commit/rollback consume、Drop rollback+close、success/error/panic相当exitのexact cleanup、
public traitなし。§11〜§14のexact common/両driver transaction option sum、SQLite begin
mode、PostgreSQL isolation/access/deferrableのBEGIN前conflict rejectionもここで実装する。

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

### D10 — compound Output

many-to-one/master projectionと一対多Outputをend-to-endで実装する。Query-local visible loop、
Pure step（`borrow mut state`、0個以上の独立した `borrow mut` builder、row、out）、
1 execution、hidden SQL 0、copy/allocation countを固定する。builderはState fieldにしない。
初期DB releaseの必須項目。

### D11 — SQL migration

ordered SQL file、D3と共有するexact `ALIGNMIG`/`ALIGNSID` byte/digest golden、
checksum/history、先頭lineのexact required/forbidden policy、
required default atomicity、forbidden 1-statement制限、Applying/Failed dirty state、
checksum-bound repair、全file適用前のU+0000拒否、
明示 `alignc db migrate/status/check/repair`。

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

### D13 — batch、SoA、高価値native path

bounded batch generation、PostgreSQL binary format、segmented child/validity bitmap、
eligible direct SoA、COPY/pipeline/single-row/LISTEN-NOTIFY、SQLite backup/blob/FTS、
explicit pool、common contract実証後の追加driver。

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

## 25. 実装前にconsumerで確定するtype/native detail

load-bearing shapeは確定済み。残るもの:

1. decimal precision/scale表現。
2. UUID、temporal、JSON/JSONB、PostgreSQL array/range/domain、SQLite custom type mapping。
3. minimal safe dynamic `db.row`/`db.value` variant。
4. SQLite function/collation callback safety。
5. measured consumerを持つCOPY/pipeline/backup/blob operation。

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
