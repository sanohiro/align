> 🌐 **English** · [Japanese](./ja/25-vector-search.md)

# 25 — Vector search through `pkg.db`

You can use vector search through `pkg.db` when the database accepts a vector as text and returns
fields that `pkg.db` supports. The application generates the embedding and defines `Params` and
`Row`. Each query uses the database's SQL to specify distance, filtering, index use, and tuning;
there is no common vector API.

This path has been tested with PostgreSQL 16 and pgvector 0.8.6. Enable the extension with
`CREATE EXTENSION vector`; neither `pkg.db` nor `alignc` installs or upgrades it.

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

The application supplies the vector as text, which the server parses. This allows use of the
existing string binding; it does not provide zero-copy vector transport. The query definition works
with `pkg.db.rows`, prepared statements through `pkg.db.prepare` and `pkg.db.rows_stmt`, checked
metadata, `pkg.db.meta_query`, and `pkg.db.explain`. Checked PostgreSQL metadata records the
extension's schema, name, and version. Preparation fails if pgvector is absent, and changing its
version changes the recorded schema identity.

## Differences between databases

The table compares the databases' vector features. It does not mean that `pkg.db` supports every
database listed. Follow the linked vendor documentation when choosing a version or tuning a
production index.

| Database | Storage and availability | Exact search and distance | Approximate index/search | Tuning boundary |
|---|---|---|---|---|
| PostgreSQL + [pgvector 0.8.6](https://github.com/pgvector/pgvector/tree/v0.8.6) | Separately installed `vector(N)` extension type. The integration test pins the official `pgvector/pgvector:0.8.6-pg16-bookworm` image. | Order by `<->` (L2), `<#>` (negative inner product), `<=>` (cosine), or `<+>` (L1) without an index for an exact scan. | HNSW and IVFFlat operator-class indexes; the query operator must match the index. | HNSW construction/search and IVFFlat lists/probes remain extension SQL/settings. |
| SQLite + [vec1 0.7](https://sqlite.org/vec1/doc/version-0.7/doc/vec1.md) | Separately built virtual-table extension. Native vectors are BLOBs of packed finite IEEE-754 `f32` values in machine byte order. It is not a built-in SQLite type. | `none` and `flat` models scan full vectors. `vec1_l2_distance` is squared L2; `vec1_cos_distance` uses vec1's documented cosine-distance formula. JSON conversion is explicit SQL. | A trained IVFADC model with PQ/OPQ/BQ compression; nearest-neighbor queries use the virtual table as a table-valued function. | Model training, buckets, quantizer, code size, residuals, `nprobe`, and threads belong to vec1 configuration. |
| MySQL Community [9.7](https://dev.mysql.com/doc/refman/9.7/en/vector.html) versus HeatWave | Community has built-in `VECTOR(N)` storage of `f32` elements, but a `VECTOR` column cannot be any kind of key. | Community does not include `DISTANCE`; the [function reference](https://dev.mysql.com/doc/refman/9.7/en/vector-functions.html) limits cosine, dot, and Euclidean `DISTANCE` to HeatWave on OCI and MySQL AI. | Community supplies no vector index/search contract. [HeatWave](https://dev.mysql.com/doc/heatwave/en/mys-hw-genai-vector-index-creation.html) separately provides automatic HNSW indexes for eligible workloads. | HeatWave distance execution, index creation, memory quota, build timeout, and HNSW search controls are service settings, not Community-portable SQL options. |
| [MariaDB 11.7.1+](https://mariadb.com/docs/server/reference/sql-structure/vectors/vector-overview) | Built-in `VECTOR(N)` with text conversion functions. | `VEC_DISTANCE` expresses Euclidean or cosine distance; index-free evaluation is exact. | `VECTOR INDEX` uses modified HNSW and accelerates matching `ORDER BY ... LIMIT` queries. | `M`, default metric, `mhnsw_ef_search`, and cache limits remain MariaDB index/session/server controls. |
| [SQL Server 2025 (17.x)](https://learn.microsoft.com/en-us/sql/sql-server/ai/vectors?view=sql-server-ver17) | Built-in binary `VECTOR(N)` of `f32`, exposed as JSON arrays. Availability also varies across Azure SQL products. | `VECTOR_DISTANCE` is always exact and supports cosine, Euclidean, and negative dot product. | `CREATE VECTOR INDEX` and `VECTOR_SEARCH` use DiskANN; the index/search surface is preview where the vendor documentation says so. | Metric, index build parallelism, preview enablement, and search options remain T-SQL/service concerns. |
| [Oracle AI Database 26ai](https://docs.oracle.com/en/database/oracle/oracle-database/26/vecse/vector_distance.html) | Built-in `VECTOR` with element format and dimension declared by Oracle SQL. | `VECTOR_DISTANCE` and its shorthand functions support cosine, dot, Euclidean, squared Euclidean, Manhattan, Hamming, and Jaccard as applicable. `FETCH EXACT` forces a flat search. | HNSW and IVF vector indexes. A query metric that differs from the index metric falls back to exact search. | Target accuracy, vector pool, neighbor partitions, graph/index settings, and metric choice remain Oracle DDL/session concerns. |

The portable part is intentionally small: an application may reuse the idea of `Params` containing
an embedding and `Row` containing an identifier, payload, and scalar distance. It must still define
a different driver-restricted `Query` for each database. `pkg.db` does not generate embeddings,
load native extensions, rewrite vector SQL, choose an index, or normalize distance values across
vendors.

## Why SQLite vec1 is not available through `pkg.db`

A separate x86-64 Linux experiment used sqlite.org's `version-0.7` source. It built `vec1.c` as a
loadable extension and loaded it into the SQLite CLI, with these results:

```text
vec1_info()                                      version 0.7 (Scalar, multi-threaded)
hex(vec1_from_json('[1,2,3]'))                   0000803F0000004000004040
vec1_l2_distance([1,2,3], [2,2,3])              1.0
flat L2 top-2 with category = 'keep'             row 1: 0.0; row 2: 1.0
```

The bytes are little-endian `f32` on the test host. vec1 uses machine byte order, so this is not a
portable storage format. The experiment loaded the extension outside `pkg.db`: Align's SQLite
connections disable extension loading, so this experiment does not establish SQLite vector support
in Align. Bundling or statically linking vec1 would require a separate design covering the
dependency's provenance, platform builds, initialization, migrations, and public API.

Binding native vector values directly is also future work. Each driver needs rules for type
identity, dimensions, finite values, encoding, ownership, allocation, error precedence, and row
lifetimes. Similar SQL names do not imply that the databases share a vector representation.
