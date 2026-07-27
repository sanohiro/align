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
- `db.exec_result`: affected rowsと利用可能なcommand status。
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

L1a〜L6後に通常のファーストパーティーAlign package codeで実装するもの:

- public handle/descriptor/option/error/metadata形状;
- common/SQLite/PostgreSQL module API;
- private sqlite3/libpq FFI宣言を包むsafe wrapper;
- connection/transaction/statement/rows lifecycle;
- bind/step/result/metadata driver operation;
- Query-local `run`、Pure shaping step、builder、Output;
- SQL migrationと明示的tool orchestration。

compiler/frontendが所有するもの:

- L1a〜L6のlanguage/ownership/region/interface/MIR;
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
  source identity: File(logical path) | Inline(query_id)
  exact source SQL bytes/hash
  driver別wire SQL bytes/hashとsource map
  Params/Row identities
  driver restriction
  canonical static options
  named-parameter occurrence/source maps
  driver別binding plan/parameter retention class
  driver別checked metadata policy/state/reference/digest
  generated binder/decoder bodies

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

`execute` は `db.exec_result` を返す。`RETURNING` を含むstatementはQueryとして定義する。

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
これはpublic traitではないcompile-time structural checkで、v1 static Rowはすべて満たす。
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
genericな `db.fold` は提供しない。

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
text系/byteaはParamsでviewまたはowned、Rowで `str` / `slice<u8>` viewにmappingする。

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

初期releaseのconnection sumも次で固定する。

```text
OpenReadOnly | OpenReadWrite | Create | Uri
PrivateCache | SharedCache | NoMutex | FullMutex
BusyTimeoutNs(ns) | Pragma(name, value)
```

`[]` はcreate/URI/cache/mutex/busy/PRAGMA指定なしのread-write openと、transactionの
Deferred、EXPLAINのQueryPlanを意味する。read-onlyとread-write/create、privateとshared、
no-mutexとfull-mutex、重複busy timeout/PRAGMA名はconflict。durationは正でなければならない。
`RequireVersionAtLeast` はstatic artifact/public contractへ入りSQLiteへpinする。
Persistent/Normalizeはprepareだけ、execution BusyTimeoutはそのexecution中だけ適用して
restoreする。capability不足はUnsupportedでありfallback/ignoreしない。extension loadingは
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
secretはruntime valueでありstatic Query metadataへ入れない。

### 12.2 parameter/result format

```text
postgres.QueryOption.ParameterType(name, native_type)
postgres.CommandOption.ParameterType(name, native_type)
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
connection/execute、D6 prepare、D7 transaction、D9 common deadline/cancellationと全scope
matrix、D12 metadata/EXPLAINである。D9までpreliminary APIを待たせず、別表現も作らない。

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
  query_id: string,
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
  InvalidQuery(ContractError),
  Unsupported(ContractError),
  Native(NativeError),
}
```

SQLite primary/extended code、PostgreSQL SQLSTATE/detailを所有して保持する。native buffer
viewをerrorへ残さない。success hot pathはerror stringをallocateしない。

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
alignc db prepare app.align --driver sqlite --database dev.sqlite
alignc db prepare app.align --driver sqlite --memory --migrations db/migrations
alignc db prepare app.align --driver postgres --url-env APP_DATABASE_URL
alignc db prepare app.align --driver postgres --url-env APP_DATABASE_URL --query app.user
alignc db prepare app.align --driver postgres --url-env APP_DATABASE_URL --check
```

entry moduleとdriverは必須。Query discoveryはreachable static Query/command graphだけを
対象にし、directoryをscanしない。SQLiteで明示した `--migrations <dir>` だけは§16.6の
catalogをtool action内で列挙する。`--query` は対象をさらに絞る。prepare regeneration
modeだけがmissing/stale artifactを許し、`--check` は何も書かない。normal buildは
environmentを読まずDBへ接続しない。

### 16.3 metadata location

```text
.align-db/
  sqlite/
  postgres/
```

artifactはversioned canonical formatで、Query id、File(logical path)|Inline(query_id) source
identity、source SQL hash、driver wire SQL hash、rewrite-format version、option hash、
Params/Row contract、driver/engine identity、
schema/search-path/extension evidence、parameter/result metadata、
nullability/origin confidence、生成時刻以外のreproducible identityを持つ。secret/URLを
保存しない。

### 16.4 stale判定

SQL、static option、Params/Row、driver restriction、metadata policy、relevant schema
fingerprintの変更でstaleになる。metadata pathの存在/不存在もaction keyへ
`Missing` / `Present(hash)` として入れ、file作成/削除がdirectory scanなしでinvalidateする。

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
- 各file全体をその順でmigration scriptとして適用（Queryの1-statement rule対象外）;
- `(version, exact UTF-8 filename, content hash)` のordered tuple列をschema fingerprintへ入れる。

name/version/gap/symlink/UTF-8 errorは1件目を適用する前に報告する。D11のmigrateも同じ
catalog ruleを再利用する。PRAGMA、attached DB、extension policyをartifactへ記録し、
undeclared ambient stateを使わない。

### 16.7 PostgreSQL prepare environment

URLは指定environment variableからprepare toolだけが読む。search_path、server version、
extension/type OID evidence、schema fingerprintを記録する。equivalentに再作成したschemaで
canonical outputが一致するようにする。

## 17. Migration

### 17.1 SQL file

```text
migrations/
  0001_create_users.sql
  0002_add_groups.sql
```

順序、name、exact content hashがidentityである。structから生成しない。
prepareとD11 migrateは§16.6の同じfilename/version/order/symlink/hash catalog ruleを使う。

### 17.2 command

```text
alignc db migrate ...
alignc db status ...
alignc db check ...
```

実行対象、driver、connection inputを明示する。通常buildには組み込まない。

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

recoveryはexact checksumを要求する
`alignc db repair --version N --accept-applied --expect-checksum HASH` または
`--clear-dirty` だけである。前者はoperatorがnative stateを確認してAppliedにし、後者は
safe retryを確認してdirty rowだけを削除する。DB effectをundoせず、Applied rowは対象外。

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
MetaTableKind       Table | View | MaterializedView | Native
MetaNullability     Yes | No | Unknown
MetaKeyKind         Primary | Unique | Foreign | Check | Exclusion | Native
MetaQueryState      Declared | DatabaseChecked
MetaQueryEntry      Summary | Parameter | Column
MetaStatementClass  Select | Dml | Ddl | Native | Unknown
PlanFormat          Text | Json | Native
DatabaseMeta  driver, name, engine_version, optional schema/encoding/collation/read_only/transactional_ddl
SchemaMeta    name, optional owner, visible, system
TableMeta     schema, name, kind, optional native_kind/owner/comment/estimated_rows
ColumnMeta    schema/table/name, ordinal, optional logical/native type, nullable, optional default/generated/origin
KeyMeta       schema/table/name/kind, term_ordinal, optional local/ref columns/expression
IndexMeta     schema/table/name/optional unique, term_ordinal, optional column/expression/predicate/method/opclass
QueryMeta     query_id/driver/restriction/statement class/artifact digest/state/metadata fingerprint/entry,
              optional ordinal/name/alias/logical/native/origin, nullable
QueryPlan     driver, format, analyzed, body
```

multi-term key/indexは同じnameを持つordered flat rowとして返しnested allocationを作らない。
QueryMetaはSummary 1行の後にparameter/column行をordinal順で返す。detail/engineにないfieldは
Option.Noneだがbase identityは常に存在する。

common callは全て明示destination `out: region` をoption sliceの直前に持つ。

```align
database: db.DatabaseMeta = db.meta_database(exec, detail, out, [])?
schemas: array<db.SchemaMeta> = db.meta_schemas(exec, detail, out, [])?
tables: array<db.TableMeta> = db.meta_tables(exec, schema_filter, detail, out, [])?
table: db.TableMeta = db.meta_table(exec, table_ref, detail, out, [])?
columns: array<db.ColumnMeta> = db.meta_columns(exec, table_ref, detail, out, [])?
keys: array<db.KeyMeta> = db.meta_keys(exec, table_ref, detail, out, [])?
indexes: array<db.IndexMeta> = db.meta_indexes(exec, table_ref, detail, out, [])?
query_meta: array<db.QueryMeta> = db.meta_query(exec, query(), detail, out, [])?
plan: db.QueryPlan = db.explain(exec, query(), params, out, options)?
```

対応する `sqlite.meta_*_native` / `postgres.meta_*_native` はcommon sliceとdriver-native
option sliceを別々に受け、同じoutを先に受ける。optionless/hidden-heap overloadはない。
全string/array/bodyをnative result解放前にoutへcopyし、connection/row bufferをborrowしない。
arrayはL6 builderの1 compact passを使う。meta_tableの欠落はNotFoundでありpartial recordや
Optionを返さない。

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
native detailを分ける。

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

cancellation/timeoutはD9のoperation-scoped optionと明示cancel resourceで扱う。driverが
supportしないoptionはunsupported error。timeout後のconnection再利用可否をdriverが判定し、
protocol/transaction state不明のconnectionをpoolやcallerへ返さない。transparent retryはない。

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

### 21.3 benchmark anchor

- generated binder/decoder対hand-written native loop;
- SQLite package path対direct libsqlite3;
- SQLite streamed text/blob transient bindのcopy bytes/allocation;
- PostgreSQL package path対direct libpq;
- file/inline Query/command artifact generationとcold/warm rebuild;
- SQLite canonical migration catalog/replay（10/100/1000 files）;
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

### L1a〜L6 — 必須Align前提

詳細scope、file、acceptanceは
[`../../17-library-boundary-prerequisites.md`](../../17-library-boundary-prerequisites.md)。

```text
L1a recursive DropPlan framework + Option<string> field
L1b Move sum/Option/Result payload completion
L2  contextual borrow mode + Copy mutation + Fn mode/joined provenance + interface summary
L3  opaque/dependent resource + linkable Drop thunk + resource_ref/native view
L4  named arena region + clone_in
L5  deterministic tagged file/inline input + one-item Query/command identity/artifact
L6  RegionPlain region array_builder
```

この順で実装し、完了前にsafe DB driver APIを始めない。

### D0 — native feasibility probe

production APIを作らず、SQLite pointer validity、libpq full/single-row result、extended
protocolの1 statement性、parameter/result metadata、nullability evidence、cancel/cleanupを
実測して記録する。

### D1 — fake driver上のgenerated Query/command

inline/sibling SQLからQueryとcommandのdescriptor/artifactを作り、exact source identityと
SQLite source/PostgreSQL `$n` wire entry・reverse span map、named occurrence table、
両kindのbinder/Queryだけのdecoder thunk、driver別 `BindValue` / `BindCopy`、flat scalar Row、
Query/command別interface/implementation/cache invalidationをDBなしで証明する。commandは
Row/decodeを持たず、それ以外のidentity/hash/checked/binding schemaを共有する。static
common/native Query/Command option sumもここで実装する。reflectionとper-row name lookupが
ないことをbenchmark/IRで確認する。

### D2 — 最小SQLite vertical

- in-memory SQLite connection;
- i64をinsertする1つの `db.command<Params>`;
- i64を1つselectするsibling-file `db.query<Params,Row>`;
- Params/Rowはself-contained scalarだけ;
- `execute` と `one`;
- 0/1/2+ cardinality;
- structured SQLite primary/extended error;
- §11のexact SQLite connection/baseline execute option sumと全conflict/unsupported branch;
- execution-count hook;
- 全pathでclose/finalize exactly once。

text view、all、stream、transaction、migration、dynamic row、metadata catalog、追加native
breadthは含めない。direct libsqlite3 loopと比較する。

### D3 — checked metadata core + SQLite

`.align-db/sqlite` canonical artifact、`alignc db prepare`/`--check`、explicit temp/in-memory
schema setupとcanonical migration catalog/order/fingerprint test、
Declared/CheckedOptional/CheckedRequired、stale/missing診断、offline normal build、runtime
storage-class/NULL validation。

### D4 — 最小PostgreSQL vertical

D2と同じcommon Query module形状、libpq connection、dialect-aware named scanと `$n` rewrite、
同名ordinal reuse、scalar bind/decode、SQLSTATE/owned detail、send前driver mismatch、
初期 `BufferedFull` delivery（`one` decodeは最大2行でもtransportは全result）、
両driverでportable `CAST(:value AS BIGINT)`、execution count/cleanup。明示設定された
local/ephemeral PostgreSQLでintegration testする。§12のexact PostgreSQL
connection/baseline execute option sumと全conflict/unsupported branchも含む。
local開発では未設定時に理由付きskipできるが、D4 merge/DB releaseでは
`ALIGN_DB_POSTGRES_REQUIRED=1` のrequired `db-postgres` CIがpinned ephemeral serverを
provisionし、skip/接続不能をfailureにする。同じjobでportable Queryを両driverに実行する。
direct libpq benchmarkは通常PRではenvironment-gatedだがD4初回/release evidenceでは必須。

### D5 — PostgreSQL checked metadata

`.align-db/postgres`、engine/search path/extension/schema fingerprint、type name/OID evidence、
conservative nullability/origin、equivalent recreated schemaでreproducible `--check`、
runtime describe comparison。

### D6 — prepared statement lifecycle

dependent `db.stmt<P,R>`、connection/driver check、§11〜§13のexact common/両driver prepare
option sumとdisposition test、
`rows_stmt` の `borrow mut` statement parameter、rows Drop後のsequential reuse、
text/blob rebind時の旧transient copy解放、partial-bind failureの全binding/Params cleanup、
全path finalize/close、
implicit global cacheなし。prepared/common/reprepare costを別々に測る。

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
PostgreSQL初期pathは `BufferedFull` 上のone-pass decodeで、single-row/portal deliveryはD13。

### D9 — scoped native option、timeout、cancellation

D1/D2/D4/D6/D7で既に公開したoption API上へcommon deadline/cancellation machineryを完成
させ、全scopeのapplied/unsupported/conflicting/precedence matrix、timeout/cancel後connection
stateをauditする。D9でpreliminary APIや別表現を作らない。要求optionのsilent ignoreを
全driverでnegative testする。

### D10 — compound Output

many-to-one/master projectionと一対多Outputをend-to-endで実装する。Query-local visible loop、
Pure step（`borrow mut state`、0個以上の独立した `borrow mut` builder、row、out）、
1 execution、hidden SQL 0、copy/allocation countを固定する。builderはState fieldにしない。
初期DB releaseの必須項目。

### D11 — SQL migration

ordered SQL file、checksum/history、先頭lineのexact required/forbidden policy、
required default atomicity、forbidden 1-statement制限、Applying/Failed dirty state、
checksum-bound repair、明示 `alignc db migrate/status/check/repair`。

### D12 — category metadataとEXPLAIN

database/schema/table/column/key/constraint/index/Query/planの分離、各common categoryの
明示region、exact flat result shape、`MetaOption` slice、native formの追加native option
slice、§11〜§13のexact metadata/EXPLAIN option sum、PostgreSQL native detail、
SQLite native detail、1 categoryが無関係categoryをfetchしないtest。native result解放後も
out arenaまでstringが有効、hidden heapなし、multi-term flat ordering、NotFoundも固定する。
`EXPLAIN ANALYZE` は実行を明示する。

### D13 — batch、SoA、高価値native path

bounded batch generation、PostgreSQL binary format、segmented child/validity bitmap、
eligible direct SoA、COPY/pipeline/single-row/LISTEN-NOTIFY、SQLite backup/blob/FTS、
explicit pool、common contract実証後の追加driver。

### D14 — dynamic SQLとnative callback

minimal owned `db.value` とindexed `db.row`、visible dynamic SQL/value slice/exact
`db.Driver` restriction/execute option、pre-send mismatch rejection、
typed Queryと別artifact path、reflectionなし。SQLite function/collationやPostgreSQL
notice/COPY callbackはcapture、abort、reentrancy、thread、lifetimeを証明してから追加する。

### 初期release gate

L1a〜L6と、driver-relevantなD1〜D12をSQLite/PostgreSQL両方で満たす。D11のSQL
migration lifecycleとD12のcategory metadata/明示Query planも、それらを約束する初期
release gateに含む。D13〜D14はbatch/SoA/native breadth、dynamic SQL、proved callbackの
committed additive roadmapである。
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
25. benchmarkがpackage overheadをdirect native loopと比較。
26. handleはgeneral resource/borrowを使い、compilerにpkg.db名のruleがない。
27. caller materializationはnamed arena/region、ambient allocatorなし。
28. SQL-only editはproducer/artifactをinvalidateし、unchanged consumerを不要にrecompileしない。
29. compound shapingはPureな `borrow mut` state/独立builder stepとvisible rows loop。
30. region builder allocationと1 compact passを測る。
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
50. 最初のpublic database releaseはdriver-relevantなD1〜D12を完了し、D13/D14はcommitted
    additive workとして続く。
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

## 25. 実装前にconsumerで確定するtype/native detail

load-bearing shapeは確定済み。残るもの:

1. decimal precision/scale表現。
2. UUID、temporal、JSON/JSONB、PostgreSQL array/range/domain、SQLite custom type mapping。
3. engine/versionごとのconservative nullability/origin evidence。
4. minimal safe dynamic `db.row`/`db.value` variant。
5. SQLite function/collation callback safety。
6. measured consumerを持つCOPY/pipeline/backup/blob operation。

これらはD12〜D14に担当を持つ。Query identity、one-execution、ownership、artifact、
runtime validation、option rejectionを弱める理由にはならない。

## 26. 実装agentへの指示

1. この文書と
   [`../../17-library-boundary-prerequisites.md`](../../17-library-boundary-prerequisites.md)
   を完全に読む。
2. L1a〜L6を順番に実装し、gate前にsafe driver APIを始めない。
3. Rust compiler PRごとにAlign self-reviewを実行する。
4. 1 PR = 1 roadmap sliceとし、指定negative/cleanup testを入れる。
5. database keyword、ORM、Query DSL、reflection、public trait hierarchy、ambient allocator、
   public manual close、package-name ownership special caseを導入しない。
6. 足りない前提をraw、destroy function、hidden heap vector、lint-only lifetime、
   whole-program-only shortcutで代替しない。
7. D1以降、separate compilation/cache contractを守る。
8. SQLite/PostgreSQL metadata behaviorを推測せず実測して記録する。
9. SQL execution countとallocation/copy countをcorrectness testとして扱う。
10. single-table CRUDだけでQuery model完成としない。
