#include <stdint.h>
#include <sqlite3.h>
#include <time.h>

static int64_t monotonic_ns(void) {
  struct timespec now;
  if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return -1;
  return (int64_t)now.tv_sec * 1000000000LL + (int64_t)now.tv_nsec;
}

int64_t align_sqlite_q4a_direct_prepared_bench(int32_t iterations) {
  sqlite3 *database = NULL;
  sqlite3_stmt *statement = NULL;
  static const unsigned char payload[] = {1, 2, 3};
  const char *sql = "SELECT :id AS id WHERE :label = :label AND :payload = :payload";
  if (iterations <= 0 || sqlite3_open_v2(
          ":memory:", &database, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE, NULL) != SQLITE_OK) {
    if (database != NULL) sqlite3_close_v2(database);
    return -1;
  }
  if (sqlite3_prepare_v2(database, sql, -1, &statement, NULL) != SQLITE_OK) {
    sqlite3_close_v2(database);
    return -1;
  }
  int64_t start = monotonic_ns();
  for (int32_t index = 0; index < iterations; index++) {
    if (sqlite3_bind_int64(statement, 1, index) != SQLITE_OK ||
        sqlite3_bind_text(statement, 2, "bench", 5, SQLITE_TRANSIENT) != SQLITE_OK ||
        sqlite3_bind_blob(statement, 3, payload, 3, SQLITE_TRANSIENT) != SQLITE_OK ||
        sqlite3_reset(statement) != SQLITE_OK ||
        sqlite3_clear_bindings(statement) != SQLITE_OK) {
      sqlite3_finalize(statement);
      sqlite3_close_v2(database);
      return -1;
    }
  }
  int64_t elapsed = monotonic_ns() - start;
  sqlite3_finalize(statement);
  sqlite3_close_v2(database);
  return elapsed;
}
