> 🌐 [English](../25-vector-search.md) · **日本語**

# 25 — `pkg.db` を通じたベクトル検索

データベースが SQL 境界でテキスト表現のベクトルを受け取り、結果を既存の対応済み
フィールドへ整形できるなら、ベクトル検索はすでに `pkg.db` から利用できます。Align は
データベース固有の契約を共通ベクトル API の背後へ隠しません。embedding の生成と
`Params`/`Row` の形はアプリケーションが所有し、driver 固定の SQL、距離の意味、filter、
index 利用、tuning は各 `Query` が所有します。

VC1 は PostgreSQL 16 と pgvector 0.8.6 でこの経路を実証します。test database は明示的に
`CREATE EXTENSION vector` を実行し、`pkg.db` も `alignc` も extension を install または
upgrade しません。

```align
module app.search
import pkg.db
import pkg.db.postgres

pub Params {
  embedding: str,
  category: str,
}

pub Row {
  id: i64,
  label: str,
  distance: f64,
}

pub fn nearest() -> pkg.db.query<Params, Row> = pkg.db.postgres.query(
  "SELECT id, label, embedding <-> CAST(:embedding AS text)::vector(3) AS distance FROM items WHERE category = CAST(:category AS text) ORDER BY distance, id LIMIT 10",
  [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
  [],
)
```

ベクトルは text として可視に組み立てられ、server が parse します。これは互換経路であり、
zero-copy や最適 transport の約束ではありません。直接の `pkg.db.rows`、`pkg.db.prepare` と
`pkg.db.rows_stmt`、checked/offline metadata、`pkg.db.meta_query`、`pkg.db.explain` はすべて
同じ descriptor を使います。checked PostgreSQL metadata は canonical な extension schema、
name、version を含むため、pgvector のない database では preparation が失敗し、version が
変われば fallback を選ばず schema identity が変わります。

## Vendor ごとの境界

この表は product contract を記録するものであり、交換可能な Align capability を示すものでは
ありません。本番 index の version や tuning を決めるときは、link 先の vendor 文書を確認して
ください。

| Database | Storage と availability | Exact search と distance | Approximate index/search | Tuning boundary |
|---|---|---|---|---|
| PostgreSQL + [pgvector 0.8.6](https://github.com/pgvector/pgvector/tree/v0.8.6) | 別途 install する `vector(N)` extension type。VC1 は公式 `pgvector/pgvector:0.8.6-pg16-bookworm` image を pin する。 | index なしで `<->` (L2)、`<#>` (negative inner product)、`<=>` (cosine)、`<+>` (L1) により並べると exact scan。 | HNSW と IVFFlat の operator-class index。query operator は index と一致させる。 | HNSW の構築/検索と IVFFlat の lists/probes は extension SQL/settings のまま。 |
| SQLite + [vec1 0.7](https://sqlite.org/vec1/doc/version-0.7/doc/vec1.md) | 別途 build する virtual-table extension。native vector は有限 IEEE-754 `f32` を machine byte order で詰めた BLOB。SQLite built-in type ではない。 | `none` と `flat` model は full vector を scan。`vec1_l2_distance` は squared L2、`vec1_cos_distance` は vec1 文書の cosine-distance 式を使う。JSON 変換は明示的 SQL。 | training 済み IVFADC model と PQ/OPQ/BQ 圧縮。nearest-neighbor query は virtual table を table-valued function として使う。 | model training、bucket、quantizer、code size、residual、`nprobe`、thread は vec1 configuration が所有する。 |
| MySQL Community [9.7](https://dev.mysql.com/doc/refman/9.7/en/vector.html) と HeatWave | Community は `f32` element の built-in `VECTOR(N)` storage を持つが、`VECTOR` column はどの key にもできない。 | Community は `DISTANCE` を含まない。[function reference](https://dev.mysql.com/doc/refman/9.7/en/vector-functions.html) は cosine、dot、Euclidean の `DISTANCE` を HeatWave on OCI と MySQL AI に限定する。 | Community は vector index/search contract を提供しない。[HeatWave](https://dev.mysql.com/doc/heatwave/en/mys-hw-genai-vector-index-creation.html) は対象 workload に automatic HNSW index を別途提供する。 | HeatWave の distance execution、index creation、memory quota、build timeout、HNSW search control は service setting であり、Community-portable SQL option ではない。 |
| [MariaDB 11.7.1+](https://mariadb.com/docs/server/reference/sql-structure/vectors/vector-overview) | text conversion function を伴う built-in `VECTOR(N)`。 | `VEC_DISTANCE` は Euclidean または cosine distance を表す。index-free evaluation は exact。 | `VECTOR INDEX` は modified HNSW を使い、対応する `ORDER BY ... LIMIT` query を高速化する。 | `M`、default metric、`mhnsw_ef_search`、cache limit は MariaDB の index/session/server control のまま。 |
| [SQL Server 2025 (17.x)](https://learn.microsoft.com/en-us/sql/sql-server/ai/vectors?view=sql-server-ver17) | JSON array として公開される `f32` の binary built-in `VECTOR(N)`。Azure SQL product ごとにも availability が異なる。 | `VECTOR_DISTANCE` は常に exact で、cosine、Euclidean、negative dot product を支援する。 | `CREATE VECTOR INDEX` と `VECTOR_SEARCH` は DiskANN を使う。vendor 文書が示す product では index/search surface は preview。 | metric、index build parallelism、preview enablement、search option は T-SQL/service の関心事のまま。 |
| [Oracle AI Database 26ai](https://docs.oracle.com/en/database/oracle/oracle-database/26/vecse/vector_distance.html) | element format と dimension を Oracle SQL で宣言する built-in `VECTOR`。 | `VECTOR_DISTANCE` と shorthand function は、適用可能な cosine、dot、Euclidean、squared Euclidean、Manhattan、Hamming、Jaccard を支援する。`FETCH EXACT` は flat search を強制する。 | HNSW と IVF vector index。query metric が index metric と異なると exact search に fallback する。 | target accuracy、vector pool、neighbor partition、graph/index setting、metric choice は Oracle DDL/session の関心事のまま。 |

portable な部分は意図的に小さく保ちます。embedding を含む `Params` と、identifier、payload、
scalar distance を含む `Row` という考え方は再利用できます。それでも database ごとに別の
driver-restricted `Query` を定義しなければなりません。`pkg.db` は embedding の生成、native
extension の load、vector SQL の rewrite、index の選択、vendor 間の distance 値の正規化を
行いません。

## SQLite vec1 の disposition

VC1 は sqlite.org の `version-0.7` source から隔離した x86-64 Linux probe も実行しました。
probe は `vec1.c` を loadable extension として build し、SQLite CLI に load して次を観測しました。

```text
vec1_info()                                      version 0.7 (Scalar, multi-threaded)
hex(vec1_from_json('[1,2,3]'))                   0000803F0000004000004040
vec1_l2_distance([1,2,3], [2,2,3])              1.0
flat L2 top-2 with category = 'keep'             row 1: 0.0; row 2: 1.0
```

この byte 列は probe host 上の little-endian `f32` です。vec1 は machine byte order を規定する
ため、portable な永続 encoding ではありません。extension は `pkg.db` の外で load する必要が
ありました。Align の SQLite connection は意図的に extension loading を無効のまま保つため、
VC1 は SQLite vector support を ship しません。将来 vec1 を bundle または static link する提案は、
dependency provenance、platform build、initialization、migration behavior、public contract を
別途所有しなければなりません。

直接の native-vector binding も将来の作業です。type identity、dimension、finite value、encoding、
ownership、allocation、error precedence、Row lifetime について driver-qualified design が必要です。
これらの database が似た用語を使うことは、1つの表現がすべてに安全だという根拠にはなりません。
