#include <stdint.h>
#include <stdlib.h>
#include <string.h>

static int prepare_calls;
static int bind_i64_calls;
static int bind_text_calls;
static int bind_blob_calls;
static int reset_calls;
static int clear_calls;
static int finalize_calls;
static int protocol_ok;
static int fail_next_text;
static int fail_next_reset;
static int failed_bind_pending;
static const char *bound_text;
static int bound_text_bytes;
static const unsigned char *bound_blob;

void align_sqlite_q4a_reset(void) {
  prepare_calls = 0;
  bind_i64_calls = 0;
  bind_text_calls = 0;
  bind_blob_calls = 0;
  reset_calls = 0;
  clear_calls = 0;
  finalize_calls = 0;
  protocol_ok = 1;
  fail_next_text = 0;
  fail_next_reset = 0;
  failed_bind_pending = 0;
  bound_text = NULL;
  bound_text_bytes = 0;
  bound_blob = NULL;
}

int align_sqlite_q4a_prepare_calls(void) { return prepare_calls; }
int align_sqlite_q4a_bind_i64_calls(void) { return bind_i64_calls; }
int align_sqlite_q4a_bind_text_calls(void) { return bind_text_calls; }
int align_sqlite_q4a_bind_blob_calls(void) { return bind_blob_calls; }
int align_sqlite_q4a_reset_calls(void) { return reset_calls; }
int align_sqlite_q4a_clear_calls(void) { return clear_calls; }
int align_sqlite_q4a_finalize_calls(void) { return finalize_calls; }
int align_sqlite_q4a_protocol_ok(void) { return protocol_ok; }
void align_sqlite_q4a_fail_next_text(void) { fail_next_text = 1; }
void align_sqlite_q4a_fail_next_reset(void) { fail_next_reset = 1; }

int sqlite3_prepare_v3(
    void *database,
    const char *sql,
    int bytes,
    unsigned int flags,
    void **statement_out,
    const char **tail_out) {
  prepare_calls++;
  if (database == NULL || sql == NULL || bytes != -1 || flags != 3 || statement_out == NULL ||
      tail_out == NULL) {
    protocol_ok = 0;
    return 1;
  }
  *statement_out = calloc(1, 8);
  *tail_out = sql + strlen(sql);
  return *statement_out == NULL ? 7 : 0;
}

int sqlite3_bind_int64(void *statement, int index, int64_t value) {
  bind_i64_calls++;
  if (statement == NULL || index != 1 || (value != 7 && value != 8)) protocol_ok = 0;
  return 0;
}

int sqlite3_bind_text(
    void *statement,
    int index,
    const char *value,
    int bytes,
    void (*destructor)(void *)) {
  bind_text_calls++;
  if (statement == NULL || index != 2 || value == NULL || destructor != NULL ||
      !((bytes == 5 && memcmp(value, "first", 5) == 0) ||
        (bytes == 6 && memcmp(value, "second", 6) == 0))) {
    protocol_ok = 0;
  }
  if (fail_next_text) {
    fail_next_text = 0;
    failed_bind_pending = 1;
    return 1;
  }
  bound_text = value;
  bound_text_bytes = bytes;
  return 0;
}

int sqlite3_bind_blob(
    void *statement,
    int index,
    const void *value,
    int bytes,
    void (*destructor)(void *)) {
  static const unsigned char expected[] = {1, 2, 3};
  bind_blob_calls++;
  if (statement == NULL || index != 3 || value == NULL || bytes != 3 || destructor != NULL ||
      memcmp(value, expected, sizeof(expected)) != 0) {
    protocol_ok = 0;
  }
  bound_blob = (const unsigned char *)value;
  return 0;
}

int sqlite3_reset(void *statement) {
  static const unsigned char expected_blob[] = {1, 2, 3};
  const char *expected_text = reset_calls == 0 ? "first" : "second";
  int expected_text_bytes = reset_calls == 0 ? 5 : 6;
  reset_calls++;
  if (failed_bind_pending) {
    failed_bind_pending = 0;
    return 0;
  }
  if (statement == NULL || bound_text == NULL || bound_blob == NULL ||
      bound_text_bytes != expected_text_bytes ||
      memcmp(bound_text, expected_text, (size_t)expected_text_bytes) != 0 ||
      memcmp(bound_blob, expected_blob, sizeof(expected_blob)) != 0) {
    protocol_ok = 0;
  }
  if (fail_next_reset) {
    fail_next_reset = 0;
    return 1;
  }
  return 0;
}

int sqlite3_clear_bindings(void *statement) {
  clear_calls++;
  if (statement == NULL) protocol_ok = 0;
  return 0;
}

int sqlite3_finalize(void *statement) {
  finalize_calls++;
  if (statement == NULL) {
    protocol_ok = 0;
  } else {
    free(statement);
  }
  return 0;
}
