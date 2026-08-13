#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#if !defined(_WIN32)
#include <pthread.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>
#endif

typedef struct {
  int autocommit;
  int ordinal;
} PoolDatabase;

static int prepare_calls;
static int bind_i64_calls;
static int bind_text_calls;
static int bind_blob_calls;
static int reset_calls;
static int clear_calls;
static int finalize_calls;
static int busy_timeout_calls;
static int last_busy_timeout;
static int protocol_ok;
static int fail_next_text;
static int fail_next_reset;
static int fail_next_busy_timeout;
static int fail_next_finalize;
static int failed_bind_pending;
static const char *bound_text;
static int bound_text_bytes;
static const unsigned char *bound_blob;
static int64_t bound_i64;
static int step_phase;
static int view_mode;
static int row_fault;
static int dynamic_mode;
static int pool_connect_calls;
static int pool_close_calls;
static int pool_fail_connect_at;
static int pool_rollback_fault;
static int pool_control_fault;
static int pool_close_ordinals[4];
static int callback_fixture_active;
static int callback_fixture_errcode(void *database);
static int callback_registration_mode;
static int callback_registration_calls;
static int callback_registration_protocol_ok;
static int callback_sqlite_version = 3030000;

void align_sqlite_q4a_reset(void) {
  prepare_calls = 0;
  bind_i64_calls = 0;
  bind_text_calls = 0;
  bind_blob_calls = 0;
  reset_calls = 0;
  clear_calls = 0;
  finalize_calls = 0;
  busy_timeout_calls = 0;
  last_busy_timeout = -1;
  protocol_ok = 1;
  fail_next_text = 0;
  fail_next_reset = 0;
  fail_next_busy_timeout = 0;
  fail_next_finalize = 0;
  failed_bind_pending = 0;
  bound_text = NULL;
  bound_text_bytes = 0;
  bound_blob = NULL;
  bound_i64 = 0;
  step_phase = 0;
  view_mode = 0;
  row_fault = 0;
  dynamic_mode = 0;
  pool_connect_calls = 0;
  pool_close_calls = 0;
  pool_fail_connect_at = 0;
  pool_rollback_fault = 0;
  pool_control_fault = 0;
  memset(pool_close_ordinals, 0, sizeof(pool_close_ordinals));
}

void align_sqlite_pool_fail_connect_at(int ordinal) { pool_fail_connect_at = ordinal; }
int align_sqlite_pool_connect_calls(void) { return pool_connect_calls; }
int align_sqlite_pool_close_calls(void) { return pool_close_calls; }
int align_sqlite_pool_close_ordinal(int index) {
  return index < 0 || index >= 4 ? -1 : pool_close_ordinals[index];
}
void align_sqlite_pool_rollback_fault(int fault) { pool_rollback_fault = fault; }
void align_sqlite_pool_control_fault(int fault) { pool_control_fault = fault; }
int align_sqlite_pool_connection_ordinal(void *database) {
  return database == NULL ? -1 : ((PoolDatabase *)database)->ordinal;
}

int align_sqlite_q4a_prepare_calls(void) { return prepare_calls; }
int align_sqlite_q4a_bind_i64_calls(void) { return bind_i64_calls; }
int align_sqlite_q4a_bind_text_calls(void) { return bind_text_calls; }
int align_sqlite_q4a_bind_blob_calls(void) { return bind_blob_calls; }
int align_sqlite_q4a_reset_calls(void) { return reset_calls; }
int align_sqlite_q4a_clear_calls(void) { return clear_calls; }
int align_sqlite_q4a_finalize_calls(void) { return finalize_calls; }
int align_sqlite_q4a_busy_timeout_calls(void) { return busy_timeout_calls; }
int align_sqlite_q4a_last_busy_timeout(void) { return last_busy_timeout; }
int align_sqlite_q4a_protocol_ok(void) { return protocol_ok; }
void align_sqlite_q4a_fail_next_text(void) { fail_next_text = 1; }
void align_sqlite_q4a_fail_next_reset(void) { fail_next_reset = 1; }
void align_sqlite_q4a_fail_next_busy_timeout(void) { fail_next_busy_timeout = 1; }
void align_sqlite_q4a_fail_next_finalize(void) { fail_next_finalize = 1; }
void align_sqlite_q4a_set_row_fault(int fault) { row_fault = fault; }

int sqlite3_open_v2(const char *filename, void **database_out, int flags, const char *vfs) {
  (void)flags;
  (void)vfs;
  pool_connect_calls++;
  if (filename == NULL || database_out == NULL) return 1;
  *database_out = NULL;
  if (pool_fail_connect_at > 0 && pool_connect_calls == pool_fail_connect_at) return 1;
  PoolDatabase *database = (PoolDatabase *)calloc(1, sizeof(PoolDatabase));
  if (database == NULL) return 7;
  database->autocommit = 1;
  database->ordinal = pool_connect_calls;
  *database_out = database;
  return 0;
}

int sqlite3_close_v2(void *database) {
  if (pool_close_calls < 4 && database != NULL) {
    pool_close_ordinals[pool_close_calls] = ((PoolDatabase *)database)->ordinal;
  }
  pool_close_calls++;
  free(database);
  return 0;
}

int sqlite3_extended_result_codes(void *database, int enabled) {
  return database != NULL && enabled == 1 ? 0 : 1;
}

int sqlite3_errcode(void *database) {
  if (callback_fixture_active) return callback_fixture_errcode(database);
  if (callback_registration_mode == 1) return 257;
  return database == NULL ? 1 : (dynamic_mode == 1 && step_phase == 1 ? 100 : 0);
}
int sqlite3_extended_errcode(void *database) { return sqlite3_errcode(database); }
const char *sqlite3_errmsg(void *database) {
  (void)database;
  if (callback_registration_mode == 1) return "x";
  return "SQLite pool stub failure";
}

int sqlite3_get_autocommit(void *database) {
  return database == NULL ? 0 : ((PoolDatabase *)database)->autocommit;
}

int sqlite3_libversion_number(void) { return callback_sqlite_version; }

int sqlite3_create_function_v2(
    void *database,
    const char *name,
    int arity,
    int flags,
    void *application,
    void *scalar,
    void *step,
    void *finalizer,
    void *destroy) {
  callback_registration_calls++;
  if (database == NULL || name == NULL || name[0] == 0 || strlen(name) > 255
      || arity < 0 || arity > 127 || (flags & 1) == 0 || (flags & 524288) == 0
      || application != NULL || step != NULL || finalizer != NULL || destroy != NULL) {
    callback_registration_protocol_ok = 0;
  }
  if (callback_registration_mode == 0 && scalar == NULL) {
    /* Removal is the only successful null-scalar operation. */
  }
  if (callback_registration_mode == 2) {
    ((PoolDatabase *)database)->autocommit = 0;
    return 0;
  }
  return callback_registration_mode == 1 ? 1 : 0;
}

void test_sqlite_callback_registration_reset(int mode, int version) {
  callback_registration_mode = mode;
  callback_registration_calls = 0;
  callback_registration_protocol_ok = 1;
  callback_sqlite_version = version;
}

int test_sqlite_callback_registration_calls(void) { return callback_registration_calls; }
int test_sqlite_callback_registration_protocol_ok(void) {
  return callback_registration_protocol_ok;
}

int sqlite3_exec(
    void *database,
    const char *sql,
    void *callback,
    void *argument,
    char **error_out) {
  (void)callback;
  (void)argument;
  (void)error_out;
  if (database == NULL || sql == NULL) return 1;
  PoolDatabase *pool = (PoolDatabase *)database;
  if (strncmp(sql, "BEGIN", 5) == 0) pool->autocommit = 0;
  if (strcmp(sql, "ROLLBACK") == 0) {
    if (pool_control_fault == 2) {
      pool_control_fault = 0;
      return 1;
    }
    int fault = pool_rollback_fault;
    pool_rollback_fault = 0;
    if (fault == 1) return 1;
    if (fault == 2) return 0;
    pool->autocommit = 1;
  }
  if (strcmp(sql, "COMMIT") == 0) {
    if (pool_control_fault == 1) {
      pool_control_fault = 0;
      return 1;
    }
    pool->autocommit = 1;
  }
  return 0;
}

int sqlite3_busy_timeout(void *database, int milliseconds) {
  busy_timeout_calls++;
  last_busy_timeout = milliseconds;
  if (database == NULL || milliseconds < 0) protocol_ok = 0;
  if (fail_next_busy_timeout) {
    fail_next_busy_timeout = 0;
    return 1;
  }
  return 0;
}

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
  step_phase = 0;
  view_mode = strstr(sql, "label AS label") != NULL;
  *statement_out = calloc(1, 8);
  *tail_out = sql + strlen(sql);
  return *statement_out == NULL ? 7 : 0;
}

int sqlite3_prepare_v2(
    void *database,
    const char *sql,
    int bytes,
    void **statement_out,
    const char **tail_out) {
  prepare_calls++;
  int dynamic = sql != NULL && strstr(sql, "DYNAMIC_SQLITE") != NULL;
  if (database == NULL || sql == NULL ||
      (dynamic ? bytes != (int)strlen(sql) + 1 : bytes != -1) || statement_out == NULL ||
      tail_out == NULL) {
    protocol_ok = 0;
    return 1;
  }
  step_phase = 0;
  dynamic_mode = dynamic
      ? (strstr(sql, "DYNAMIC_SQLITE_COMMAND") != NULL ? 2
         : (strstr(sql, "DYNAMIC_SQLITE_MIXED") != NULL ? 3
            : (strstr(sql, "DYNAMIC_SQLITE_ZERO_ROWS") != NULL ? 4 : 1)))
      : 0;
  view_mode = !dynamic && strstr(sql, "label AS label") != NULL;
  if (strstr(sql, "DYNAMIC_SQLITE_EMPTY") != NULL) {
    dynamic_mode = 0;
    *statement_out = NULL;
    *tail_out = sql + strlen(sql);
    return 0;
  }
  *statement_out = calloc(1, 8);
  *tail_out = strstr(sql, "DYNAMIC_SQLITE_MULTI") != NULL
      ? strstr(sql, "DYNAMIC_SQLITE_MULTI") + strlen("DYNAMIC_SQLITE_MULTI")
      : sql + strlen(sql);
  return *statement_out == NULL ? 7 : 0;
}

int sqlite3_bind_int64(void *statement, int index, int64_t value) {
  bind_i64_calls++;
  if (dynamic_mode == 1) {
    int valid = (index == 2 && value == 1) || (index == 3 && value == -2) ||
                (index == 4 && value == 16909060) || (index == 5 && value == -2);
    if (statement == NULL || !valid) protocol_ok = 0;
    return 0;
  }
  int expected_index = view_mode ? 3 : 1;
  if (statement == NULL || index != expected_index || (value != 7 && value != 8)) protocol_ok = 0;
  bound_i64 = value;
  return 0;
}

int sqlite3_step(void *statement) {
  if (statement == NULL) {
    protocol_ok = 0;
    return 1;
  }
  if (dynamic_mode == 2) return 101;
  if (dynamic_mode == 3) {
    if (step_phase < 2) {
      step_phase++;
      return 100;
    }
    return 101;
  }
  if (dynamic_mode == 4) return 101;
  if (dynamic_mode == 1 && row_fault == 20) return 1;
  if (step_phase == 0) {
    step_phase = 1;
    return 100;
  }
  return 101;
}

int sqlite3_column_count(void *statement) {
  if (statement == NULL) return -1;
  if (dynamic_mode == 1) return 9;
  if (dynamic_mode == 3 || dynamic_mode == 4) return 1;
  if (dynamic_mode == 2) return 0;
  return view_mode ? 2 : 1;
}

int sqlite3_bind_parameter_count(void *statement) {
  if (statement == NULL) return -1;
  return dynamic_mode == 1 ? 9
      : ((dynamic_mode == 2 || dynamic_mode == 3 || dynamic_mode == 4)
          ? 0 : (view_mode ? 3 : 2));
}

const char *sqlite3_column_name(void *statement, int column) {
  if (statement == NULL) return NULL;
  if (!view_mode) return column == 0 ? "id" : NULL;
  if (column == 0) return "label";
  return column == 1 ? "payload" : NULL;
}

int sqlite3_column_type(void *statement, int column) {
  if (statement == NULL) return 5;
  if (dynamic_mode == 3) return column != 0 ? 5 : (step_phase == 1 ? 1 : 3);
  if (dynamic_mode == 4) return 5;
  if (dynamic_mode == 1) {
    if (row_fault == 10 && column == 0) return 99;
    static const int types[9] = {5, 1, 1, 1, 1, 2, 2, 3, 4};
    return column < 0 || column >= 9 ? 5 : types[column];
  }
  if (!view_mode) return column == 0 ? 1 : 5;
  if (column == 0) return 3;
  return column == 1 ? 4 : 5;
}

int64_t sqlite3_column_int64(void *statement, int column) {
  if (dynamic_mode == 3) {
    if (statement == NULL || column != 0 || step_phase != 1) protocol_ok = 0;
    return 7;
  }
  if (dynamic_mode == 1) {
    static const int64_t values[9] = {0, 1, -2, 16909060, -2, 0, 0, 0, 0};
    if (statement == NULL || column < 1 || column > 4) protocol_ok = 0;
    return column < 0 || column >= 9 ? 0 : values[column];
  }
  if (statement == NULL || column != 0) protocol_ok = 0;
  return bound_i64;
}

double sqlite3_column_double(void *statement, int column) {
  if (statement == NULL || dynamic_mode != 1 || (column != 5 && column != 6)) protocol_ok = 0;
  if (row_fault == 16 && column == 5) return NAN;
  return column == 5 ? 1.5 : 2.5;
}

const unsigned char *sqlite3_column_text(void *statement, int column) {
  static const unsigned char valid[] = "view";
  static const unsigned char invalid_utf8[] = {0xff, 0};
  static const unsigned char dynamic_text[] = {'a', 0, 'b', 0};
  static const unsigned char mixed_text[] = "two";
  if (dynamic_mode == 3) {
    return statement != NULL && column == 0 && step_phase == 2 ? mixed_text : NULL;
  }
  if (dynamic_mode == 1) {
    if (column == 7 && row_fault == 11) return NULL;
    if (column == 7 && row_fault == 13) return invalid_utf8;
    return statement != NULL && column == 7 ? dynamic_text : NULL;
  }
  if (statement == NULL || !view_mode || column != 0) return NULL;
  if (row_fault == 1) return NULL;
  return row_fault == 3 ? invalid_utf8 : valid;
}

const void *sqlite3_column_blob(void *statement, int column) {
  static const unsigned char valid[] = {1, 2, 3};
  static const unsigned char dynamic_blob[] = {0, 255};
  if (dynamic_mode == 1) {
    if (column == 8 && (row_fault == 14 || row_fault == 17)) return NULL;
    return statement != NULL && column == 8 ? dynamic_blob : NULL;
  }
  if (statement == NULL || !view_mode || column != 1 || row_fault == 4) return NULL;
  return valid;
}

int sqlite3_column_bytes(void *statement, int column) {
  if (dynamic_mode == 1) {
    if (statement == NULL) return -1;
    if (column == 7) {
      if (row_fault == 12) return -1;
      if (row_fault == 11 || row_fault == 13) return 1;
      return 3;
    }
    if (column == 8) {
      if (row_fault == 15) return -1;
      if (row_fault == 14) return 1;
      if (row_fault == 17) return 0;
      return 2;
    }
    return 0;
  }
  if (dynamic_mode == 3) return statement != NULL && column == 0 && step_phase == 2 ? 3 : 0;
  if (statement == NULL || !view_mode) return -1;
  if (column == 0) {
    if (row_fault == 2) return -1;
    if (row_fault == 1 || row_fault == 3) return 1;
    return 4;
  }
  if (column == 1) {
    if (row_fault == 5) return -1;
    if (row_fault == 4) return 1;
    return 3;
  }
  return -1;
}

int sqlite3_bind_text(
    void *statement,
    int index,
    const char *value,
    int bytes,
    void (*destructor)(void *)) {
  bind_text_calls++;
  if (dynamic_mode == 1) {
    const unsigned char expected[] = {'a', 0, 'b'};
    if (statement == NULL || index != 8 || value == NULL || bytes != 3 ||
        destructor != (void (*)(void *))-1 || memcmp(value, expected, sizeof(expected)) != 0) {
      protocol_ok = 0;
    }
    if (fail_next_text) {
      fail_next_text = 0;
      return 1;
    }
    return 0;
  }
  int expected_index = view_mode ? 1 : 2;
  if (statement == NULL || index != expected_index || value == NULL || destructor != NULL ||
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
  if (dynamic_mode == 1) {
    static const unsigned char dynamic_expected[] = {0, 255};
    if (statement == NULL || index != 9 || value == NULL || bytes != 2 ||
        destructor != (void (*)(void *))-1 ||
        memcmp(value, dynamic_expected, sizeof(dynamic_expected)) != 0) {
      protocol_ok = 0;
    }
    return 0;
  }
  int expected_index = view_mode ? 2 : 3;
  if (statement == NULL || index != expected_index || value == NULL || bytes != 3 || destructor != NULL ||
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
  step_phase = 0;
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
  dynamic_mode = 0;
  if (fail_next_finalize) {
    fail_next_finalize = 0;
    return 1;
  }
  return 0;
}

int sqlite3_bind_null(void *statement, int index) {
  if (statement == NULL || dynamic_mode != 1 || index != 1) protocol_ok = 0;
  return 0;
}

int sqlite3_bind_double(void *statement, int index, double value) {
  if (statement == NULL || dynamic_mode != 1 ||
      !((index == 6 && value == 1.5) || (index == 7 && value == 2.5))) {
    protocol_ok = 0;
  }
  return 0;
}

/* Reverse-callback ABI and injected native-value trace owner. Ordinary Q4a programs never call
 * test_sqlite_callback_invoke, so these definitions remain link-only for that suite. */
static int callback_scenario;
static int callback_step;
static int callback_protocol_ok;
static int callback_db_calls;
static int callback_result_calls;
static int callback_result_kind;
static long long callback_result_i64;
static int callback_value_type_calls;
static char callback_error[96];
static int callback_error_len;
static const unsigned char callback_text[] = {0xc3, 0xa9};
static const unsigned char callback_blob[] = {0x00, 0xff};
#if !defined(_WIN32)
static pthread_t callback_invoking_thread;
#endif

void *sqlite3_context_db_handle(void *context) {
  callback_db_calls++;
#if !defined(_WIN32)
  if (!pthread_equal(callback_invoking_thread, pthread_self())) callback_protocol_ok = 0;
#endif
  if (callback_scenario == 11) return NULL;
  return context;
}

int sqlite3_value_type(void *value) {
  callback_value_type_calls++;
  if (callback_scenario == 12) {
    if (value == NULL) callback_protocol_ok = 0;
    return 5;
  }
  if (value == NULL || callback_step != 0) callback_protocol_ok = 0;
  callback_step = 1;
  switch (callback_scenario) {
    case 3: return 99;
    case 4:
    case 5:
    case 6:
    case 14:
    case 15: return 3;
    case 7:
    case 8: return 4;
    case 9: return 2;
    default: return 5;
  }
}

long long sqlite3_value_int64(void *value) {
  if (value == NULL || callback_step != 1) callback_protocol_ok = 0;
  callback_step = 2;
  return -2;
}

double sqlite3_value_double(void *value) {
  if (value == NULL || callback_step != 1) callback_protocol_ok = 0;
  callback_step = 2;
  return callback_scenario == 9 ? NAN : -0.0;
}

int sqlite3_value_bytes(void *value) {
  if (value == NULL || callback_step != 1) callback_protocol_ok = 0;
  callback_step = 2;
  switch (callback_scenario) {
    case 4:
    case 7: return 0;
    case 5: return 1;
    case 6:
    case 14:
    case 8: return 2;
    case 15: return 0;
    default: callback_protocol_ok = 0; return -1;
  }
}

const unsigned char *sqlite3_value_text(void *value) {
  if (value == NULL || callback_step != 2) callback_protocol_ok = 0;
  callback_step = 3;
  if (callback_scenario == 4 || callback_scenario == 5) return NULL;
  if (callback_scenario == 6 || callback_scenario == 14 || callback_scenario == 15) {
    return callback_text;
  }
  callback_protocol_ok = 0;
  return NULL;
}

const void *sqlite3_value_blob(void *value) {
  if (value == NULL || callback_step != 2) callback_protocol_ok = 0;
  callback_step = 3;
  if (callback_scenario == 7) return NULL;
  if (callback_scenario == 8) return callback_blob;
  callback_protocol_ok = 0;
  return NULL;
}

static int callback_fixture_errcode(void *database) {
  if (database == NULL || callback_step != 3
      || (callback_scenario != 4 && callback_scenario != 5)) {
    callback_protocol_ok = 0;
  }
  callback_step = 4;
  return callback_scenario == 5 ? 7 : 1;
}

static void callback_result(int kind) {
  callback_result_calls++;
  callback_result_kind = kind;
}

void sqlite3_result_null(void *context) { (void)context; callback_result(1); }
void sqlite3_result_int64(void *context, long long value) {
  (void)context;
  callback_result_i64 = value;
  callback_result(2);
}
void sqlite3_result_double(void *context, double value) {
  (void)context;
  (void)value;
  callback_result(3);
}
void sqlite3_result_text(void *context, const char *value, int bytes, void (*destroy)(void *)) {
  (void)context; (void)value; (void)bytes; (void)destroy; callback_result(4);
}
void sqlite3_result_blob(void *context, const void *value, int bytes, void (*destroy)(void *)) {
  (void)context; (void)value; (void)bytes; (void)destroy; callback_result(5);
}
void sqlite3_result_error(void *context, const char *message, int bytes) {
  (void)context;
  callback_error_len = bytes;
  if (bytes < 0 || bytes >= (int)sizeof(callback_error) || message == NULL) {
    callback_protocol_ok = 0;
  } else {
    memcpy(callback_error, message, (size_t)bytes);
    callback_error[bytes] = 0;
  }
  callback_result(6);
}
void sqlite3_result_error_nomem(void *context) { (void)context; callback_result(7); }

int test_sqlite_callback_invoke(void *raw_callback, int scenario) {
  typedef void (*Callback)(void *, int, void **);
  Callback callback = (Callback)raw_callback;
  void *context = &callback_scenario;
  void *value = &callback_scenario;
  void *argv[2] = {value, NULL};
  void *many_argv[127];
  int argc = 1;
  void **arguments = argv;
  callback_scenario = scenario;
  callback_step = 0;
  callback_protocol_ok = callback != NULL;
  callback_db_calls = 0;
  callback_result_calls = 0;
  callback_result_kind = 0;
  callback_result_i64 = 0;
  callback_value_type_calls = 0;
  callback_error[0] = 0;
  callback_error_len = 0;
  callback_fixture_active = 1;
#if !defined(_WIN32)
  callback_invoking_thread = pthread_self();
#endif
#if !defined(_WIN32)
  if (scenario == 10 || scenario == 11) {
    pid_t child = fork();
    int status = 0;
    if (child < 0) return 25;
    if (child == 0) {
      callback(scenario == 10 ? NULL : context, 0, NULL);
      _exit(0);
    }
    if (waitpid(child, &status, 0) != child) return 26;
    callback_fixture_active = 0;
    return WIFSIGNALED(status) || (WIFEXITED(status) && WEXITSTATUS(status) != 0) ? 42 : 27;
  }
#else
  if (scenario == 10 || scenario == 11) {
    callback_fixture_active = 0;
    return 42;
  }
#endif
  if (scenario == 0) argc = -1;
  if (scenario == 1) argc = 128;
  if (scenario == 2) arguments = NULL;
  if (scenario == 12) {
    int i;
    for (i = 0; i < 127; i++) many_argv[i] = value;
    argc = 127;
    arguments = many_argv;
  }
  if (scenario == 13) {
    argc = 0;
    arguments = NULL;
  }
  if (scenario == 14) {
    argv[1] = value;
    argc = 2;
  }
  callback(context, argc, arguments);
  callback_fixture_active = 0;
  if (!callback_protocol_ok || callback_db_calls != 1 || callback_result_calls != 1) return 10 + scenario;
  if (scenario == 5) {
    return callback_step == 4 && callback_result_kind == 7 ? 42 : 20;
  }
  if (scenario == 7 || scenario == 15) {
    return callback_step == 3 && callback_result_kind == 2 && callback_result_i64 == 0 ? 42 : 21;
  }
  if (scenario == 14) {
    return callback_step == 3 && callback_value_type_calls == 1
      && callback_result_kind == 2 && callback_result_i64 == 4 ? 42 : 29;
  }
  if (scenario == 6 || scenario == 8) {
    return callback_step == 3 && callback_result_kind == 2 && callback_result_i64 == 2 ? 42 : 22;
  }
  if (scenario == 0 || scenario == 1 || scenario == 2 || scenario == 3
      || scenario == 4 || scenario == 9) {
    const char *expected = "pkg.db SQLite function callback received an invalid native value";
    return callback_result_kind == 6
      && callback_error_len == (int)strlen(expected)
      && memcmp(callback_error, expected, (size_t)callback_error_len) == 0 ? 42 : 23;
  }
  if (scenario == 12 || scenario == 13) {
    const char *expected = "unexpected arity";
    int expected_types = scenario == 12 ? 1 : 0;
    return callback_result_kind == 6
      && callback_value_type_calls == expected_types
      && callback_error_len == (int)strlen(expected)
      && memcmp(callback_error, expected, (size_t)callback_error_len) == 0 ? 42 : 28;
  }
  return 24;
}
