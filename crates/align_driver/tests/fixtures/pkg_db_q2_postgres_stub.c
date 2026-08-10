#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct {
  char *keyword;
  char *envvar;
  char *compiled;
  char *val;
  char *label;
  char *dispchar;
  int dispsize;
} PQconninfoOption;

typedef struct {
  int status;
  int encoding;
  int transaction_status;
  const char *message;
} FakeConn;

typedef struct {
  int status;
  const char *command_status;
  int rows;
  int fields;
  const char *names[16];
  uint32_t oids[16];
  const char *values[64][16];
  int nulls[64][16];
  int delivered[64];
  const char *affected;
  const char *sqlstate;
  const char *message;
  const char *detail;
  const char *constraint_name;
  const char *table_name;
  const char *column_name;
  int row_fault;
} FakeResult;

typedef struct {
  FakeConn *connection;
} FakeCancel;

static int connect_calls;
static int finish_calls;
static int encoding_calls;
static int execute_calls;
static int clear_calls;
static int protocol_ok;
static int protocol_error;
static int last_timeout;
static int prepare_calls;
static int execute_prepared_calls;
static int control_calls;
static int deallocate_calls;
static int fail_next_control;
static int rollback_next_commit;
static char prepared_name[64];
static FakeResult *async_result;
static int async_busy;
static int async_cancelled;
static int async_cancel_fail;
static int async_drain_fail;
static int async_flush_fail;
static int async_consume_fail;
static int async_cancel_resource_fail;
static int async_transaction_unknown;
static int fail_next_nonblocking_enable;
static int fail_next_nonblocking_restore;
static int delay_next_nonblocking_enable;
static int prepared_timeout_wait;
static int nonblocking_calls;
static int cancel_calls;
static int consume_calls;
static int q6_delivered_rows;
static int q6_fail_next_execute;

static char *copy_text(const char *text) {
  size_t n = strlen(text) + 1;
  char *copy = (char *)malloc(n);
  if (copy != NULL) memcpy(copy, text, n);
  return copy;
}

static int has(const char *text, const char *needle) {
  return text != NULL && strstr(text, needle) != NULL;
}

void align_pg_reset(void) {
  connect_calls = 0;
  finish_calls = 0;
  encoding_calls = 0;
  execute_calls = 0;
  clear_calls = 0;
  protocol_ok = 1;
  protocol_error = 0;
  last_timeout = -1;
  prepare_calls = 0;
  execute_prepared_calls = 0;
  control_calls = 0;
  deallocate_calls = 0;
  fail_next_control = 0;
  rollback_next_commit = 0;
  prepared_name[0] = '\0';
  async_result = NULL;
  async_busy = 0;
  async_cancelled = 0;
  async_cancel_fail = 0;
  async_drain_fail = 0;
  async_flush_fail = 0;
  async_consume_fail = 0;
  async_cancel_resource_fail = 0;
  async_transaction_unknown = 0;
  fail_next_nonblocking_enable = 0;
  fail_next_nonblocking_restore = 0;
  delay_next_nonblocking_enable = 0;
  prepared_timeout_wait = 0;
  nonblocking_calls = 0;
  cancel_calls = 0;
  consume_calls = 0;
  q6_delivered_rows = 0;
  q6_fail_next_execute = 0;
}

int align_pg_connect_calls(void) { return connect_calls; }
int align_pg_finish_calls(void) { return finish_calls; }
int align_pg_encoding_calls(void) { return encoding_calls; }
int align_pg_execute_calls(void) { return execute_calls; }
int align_pg_clear_calls(void) { return clear_calls; }
int align_pg_protocol_ok(void) { return protocol_ok; }
int align_pg_protocol_error(void) { return protocol_error; }
int align_pg_nonblocking_calls(void) { return nonblocking_calls; }
int align_pg_cancel_calls(void) { return cancel_calls; }
int align_pg_consume_calls(void) { return consume_calls; }
int align_pg_q6_delivered_rows(void) { return q6_delivered_rows; }
void align_pg_q6_fail_next_execute(void) { q6_fail_next_execute = 1; }
int align_pg_last_timeout(void) { return last_timeout; }
int align_pg_prepare_calls(void) { return prepare_calls; }
int align_pg_execute_prepared_calls(void) { return execute_prepared_calls; }
int align_pg_control_calls(void) { return control_calls; }
int align_pg_deallocate_calls(void) { return deallocate_calls; }
void align_pg_fail_next_control(void) { fail_next_control = 1; }
void align_pg_rollback_next_commit(void) { rollback_next_commit = 1; }
void align_pg_fail_next_nonblocking_enable(void) { fail_next_nonblocking_enable = 1; }
void align_pg_fail_next_nonblocking_restore(void) { fail_next_nonblocking_restore = 1; }
void align_pg_delay_next_nonblocking_enable(void) { delay_next_nonblocking_enable = 1; }

PQconninfoOption *PQconninfoParse(const char *connection_info, char **error_out) {
  if (error_out != NULL) *error_out = NULL;
  if (connection_info == NULL || has(connection_info, "invalid-url")) {
    if (error_out != NULL) *error_out = copy_text("invalid URL");
    return NULL;
  }
  PQconninfoOption *items = (PQconninfoOption *)calloc(4, sizeof(PQconninfoOption));
  if (items == NULL) return NULL;
  int index = 0;
  if (has(connection_info, "client_encoding=")) {
    items[index].keyword = copy_text("client_encoding");
    items[index].val = copy_text("LATIN1");
    index++;
  }
  if (has(connection_info, "application_name=")) {
    items[index].keyword = copy_text("application_name");
    items[index].val = copy_text("url-app");
    index++;
  }
  const char *options = strstr(connection_info, "stub_options=");
  if (options != NULL) {
    items[index].keyword = copy_text("options");
    items[index].val = copy_text(options + strlen("stub_options="));
    index++;
  }
  return items;
}

void PQconninfoFree(PQconninfoOption *items) {
  if (items == NULL) return;
  for (int i = 0; items[i].keyword != NULL; i++) {
    free(items[i].keyword);
    free(items[i].envvar);
    free(items[i].compiled);
    free(items[i].val);
    free(items[i].label);
    free(items[i].dispchar);
  }
  free(items);
}

void PQfreemem(void *pointer) { free(pointer); }

FakeConn *PQconnectdbParams(const char *const *keywords, const char *const *values, int expand_dbname) {
  connect_calls++;
  if (expand_dbname != 1 || keywords == NULL || values == NULL) {
    protocol_ok = 0;
    if (protocol_error == 0) protocol_error = 82;
  }
  const char *dbname = NULL;
  int client_count = 0;
  int pair_count = 0;
  for (; keywords != NULL && keywords[pair_count] != NULL; pair_count++) {
    if (values[pair_count] == NULL) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 83;
    }
    if (strcmp(keywords[pair_count], "dbname") == 0) dbname = values[pair_count];
    if (strcmp(keywords[pair_count], "client_encoding") == 0) {
      client_count++;
      if (strcmp(values[pair_count], "UTF8") != 0) {
        protocol_ok = 0;
        if (protocol_error == 0) protocol_error = 84;
      }
    }
    if (strcmp(keywords[pair_count], "connect_timeout") == 0) {
      last_timeout = atoi(values[pair_count]);
    }
  }
  if (pair_count == 0 || strcmp(keywords[pair_count - 1], "client_encoding") != 0 || client_count != 1) {
    protocol_ok = 0;
    if (protocol_error == 0) protocol_error = 85;
  }
  if (has(dbname, "null-connection")) return NULL;
  FakeConn *connection = (FakeConn *)calloc(1, sizeof(FakeConn));
  if (connection == NULL) return NULL;
  connection->status = has(dbname, "bad-connection") ? 1 : 0;
  connection->encoding = has(dbname, "bad-encoding") ? -1 : 6;
  connection->transaction_status = 0;
  connection->message = "stub connection failure";
  return connection;
}

int PQstatus(const FakeConn *connection) { return connection == NULL ? 1 : connection->status; }

int PQclientEncoding(const FakeConn *connection) {
  encoding_calls++;
  return connection == NULL ? -1 : connection->encoding;
}

void PQfinish(FakeConn *connection) {
  finish_calls++;
  if (async_result != NULL) {
    clear_calls++;
    free(async_result);
    async_result = NULL;
  }
  free(connection);
}

const char *PQerrorMessage(const FakeConn *connection) {
  return connection == NULL ? "stub null connection" : connection->message;
}

static FakeResult *new_result(void) {
  FakeResult *result = (FakeResult *)calloc(1, sizeof(FakeResult));
  if (result == NULL) return NULL;
  result->status = 2;
  result->command_status = "";
  result->rows = 1;
  result->fields = 1;
  result->names[0] = "value";
  result->oids[0] = 20;
  result->values[0][0] = "0";
  result->affected = "";
  result->sqlstate = "XX000";
  result->message = "stub execution failure";
  result->detail = "stub detail";
  result->constraint_name = "stub_constraint";
  result->table_name = "stub_table";
  result->column_name = "stub_column";
  return result;
}

FakeResult *PQprepare(
    FakeConn *connection,
    const char *name,
    const char *command,
    int parameter_count,
    const uint32_t *parameter_types) {
  (void)connection;
  prepare_calls++;
  int common_types = prepare_calls == 1 && parameter_types != NULL &&
                     parameter_types[0] == 20 && parameter_types[1] == 0 &&
                     parameter_types[2] == 0;
  int overridden_types = prepare_calls == 2 && parameter_types != NULL &&
                         parameter_types[0] == 20 && parameter_types[1] == 25 &&
                         parameter_types[2] == 17;
  if (name == NULL || strncmp(name, "__align_pkg_db_", 15) != 0 || command == NULL ||
      parameter_count != 3 || (!common_types && !overridden_types)) {
    protocol_ok = 0;
    if (protocol_error == 0) protocol_error = 88;
  }
  if (name != NULL) {
    size_t length = strlen(name);
    if (length >= sizeof(prepared_name)) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 89;
    } else {
      memcpy(prepared_name, name, length + 1);
    }
  }
  prepared_timeout_wait = has(command, "TIMEOUT_WAIT");
  FakeResult *result = new_result();
  if (result != NULL) {
    result->status = 1;
    result->rows = 0;
    result->fields = 0;
  }
  return result;
}

FakeResult *PQexecPrepared(
    FakeConn *connection,
    const char *name,
    int parameter_count,
    const char *const *parameter_values,
    const int *parameter_lengths,
    const int *parameter_formats,
    int result_format) {
  (void)connection;
  execute_prepared_calls++;
  if (name == NULL || strcmp(name, prepared_name) != 0 || parameter_count != 3 ||
      parameter_values == NULL || parameter_lengths == NULL || parameter_formats == NULL ||
      result_format != 0 || parameter_values[0] == NULL || parameter_values[1] == NULL ||
      parameter_values[2] == NULL ||
      !(strcmp(parameter_values[0], "7") == 0 || strcmp(parameter_values[0], "8") == 0) ||
      !((strcmp(parameter_values[0], "7") == 0 && strcmp(parameter_values[1], "first") == 0) ||
        (strcmp(parameter_values[0], "8") == 0 && strcmp(parameter_values[1], "second") == 0)) ||
      strcmp(parameter_values[2], "\\x010203") != 0) {
    protocol_ok = 0;
    if (protocol_error == 0) protocol_error = 90;
  }
  for (int i = 0; i < parameter_count; i++) {
    if (parameter_values == NULL || parameter_lengths == NULL || parameter_formats == NULL ||
        parameter_values[i] == NULL || parameter_formats[i] != 0 ||
        parameter_lengths[i] != (int)strlen(parameter_values[i])) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 91;
    }
  }
  FakeResult *result = new_result();
  if (result != NULL) result->names[0] = "id";
  return result;
}

FakeResult *PQexec(FakeConn *connection, const char *command) {
  if (command == NULL) {
    protocol_ok = 0;
    if (protocol_error == 0) protocol_error = 92;
  }
  if (command != NULL && strncmp(command, "DEALLOCATE __align_pkg_db_", 26) == 0) {
    deallocate_calls++;
    if (prepared_name[0] == '\0' || strcmp(command + 11, prepared_name) != 0) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 93;
    }
  } else {
    control_calls++;
    if (command == NULL ||
        (strcmp(command, "BEGIN ISOLATION LEVEL READ COMMITTED READ WRITE NOT DEFERRABLE") != 0 &&
         strcmp(command, "BEGIN ISOLATION LEVEL SERIALIZABLE READ ONLY DEFERRABLE") != 0 &&
         strcmp(command, "COMMIT") != 0 && strcmp(command, "ROLLBACK") != 0)) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 86;
    }
  }
  FakeResult *result = new_result();
  if (result != NULL) {
    result->status = fail_next_control ? 7 : 1;
    if (command != NULL && strncmp(command, "BEGIN ", 6) == 0) {
      result->command_status = "BEGIN";
      if (connection != NULL) connection->transaction_status = 2;
    } else if (command != NULL && strcmp(command, "COMMIT") == 0) {
      result->command_status = rollback_next_commit ? "ROLLBACK" : "COMMIT";
      if (connection != NULL) connection->transaction_status = 0;
    } else if (command != NULL && strcmp(command, "ROLLBACK") == 0) {
      result->command_status = "ROLLBACK";
      if (connection != NULL) connection->transaction_status = 0;
    } else if (command != NULL && strncmp(command, "DEALLOCATE ", 11) == 0) {
      result->command_status = "DEALLOCATE";
    }
    result->rows = 0;
    result->fields = 0;
  }
  fail_next_control = 0;
  rollback_next_commit = 0;
  return result;
}

static void q6_user_result(FakeResult *result, int parameter, int last_parameter) {
  static const char *names[4] = {"user_id", "user_name", "group_id", "group_name"};
  static const uint32_t oids[4] = {20, 25, 20, 25};
  result->fields = 4;
  for (int column = 0; column < 4; column++) {
    result->names[column] = names[column];
    result->oids[column] = oids[column];
  }
  if (parameter == 0) {
    result->rows = 0;
    return;
  }
  if (parameter == 1 && last_parameter == 3) {
    static const char *values[4][4] = {
        {"1", "Alice", "10", "Admin"},
        {"1", "Alice", "20", "Dev"},
        {"2", "Bob", "", ""},
        {"3", "Cara", "40", "Ops"},
    };
    result->rows = 4;
    for (int row = 0; row < 4; row++) {
      for (int column = 0; column < 4; column++) result->values[row][column] = values[row][column];
    }
    result->nulls[2][2] = 1;
    result->nulls[2][3] = 1;
    return;
  }
  if (parameter == 1) {
    result->rows = 2;
    result->values[0][0] = "1";
    result->values[0][1] = "Alice";
    result->values[0][2] = "10";
    result->values[0][3] = "Admin";
    result->values[1][0] = "1";
    result->values[1][1] = "Alice";
    result->values[1][2] = "20";
    result->values[1][3] = "Dev";
    return;
  }
  if (parameter == 2) {
    result->rows = 1;
    result->values[0][0] = "2";
    result->values[0][1] = "Bob";
    result->values[0][2] = "";
    result->values[0][3] = "";
    result->nulls[0][2] = 1;
    result->nulls[0][3] = 1;
    return;
  }
  if (parameter == 7) {
    static const char *values[3][4] = {
        {"1", "Alice", "10", "Admin"},
        {"2", "Bob", "20", "Dev"},
        {"1", "Alice", "30", "Ops"},
    };
    result->rows = 3;
    for (int row = 0; row < 3; row++) {
      for (int column = 0; column < 4; column++) result->values[row][column] = values[row][column];
    }
    return;
  }
  result->rows = 1;
  result->values[0][0] = "3";
  result->values[0][1] = "Cara";
  result->values[0][2] = parameter == 4 ? "" : "30";
  result->values[0][3] = parameter == 3 ? "" : "Ops";
  result->nulls[0][2] = parameter == 4;
  result->nulls[0][3] = parameter == 3;
}

static void q6_transaction_result(FakeResult *result) {
  static const char *names[8] = {
      "transaction_id", "posted_at", "amount_cents", "status_id",
      "status_code", "status_name", "customer_id", "customer_name",
  };
  static const uint32_t oids[8] = {20, 25, 20, 20, 25, 25, 20, 25};
  static const char *values[2][8] = {
      {"100", "2026-08-10", "5000", "7", "posted", "Posted", "900", "Ada"},
      {"101", "2026-08-09", "5001", "7", "posted", "Posted", "901", "Linus"},
  };
  result->rows = 2;
  result->fields = 8;
  for (int column = 0; column < 8; column++) {
    result->names[column] = names[column];
    result->oids[column] = oids[column];
    result->values[0][column] = values[0][column];
    result->values[1][column] = values[1][column];
  }
}

FakeResult *PQexecParams(
    FakeConn *connection,
    const char *command,
    int parameter_count,
    const uint32_t *parameter_types,
    const char *const *parameter_values,
    const int *parameter_lengths,
    const int *parameter_formats,
    int result_format) {
  (void)connection;
  execute_calls++;
  if (q6_fail_next_execute) {
    q6_fail_next_execute = 0;
    return NULL;
  }
  if (command == NULL || result_format != 0) {
    protocol_ok = 0;
    if (protocol_error == 0) protocol_error = 87;
  }
  int full_matrix = has(command, "FULL_MATRIX");
  int view_fault = has(command, "VIEW_FAULT");
  int q6_user = has(command, "Q6_USER_GROUPS");
  int q6_transaction = has(command, "Q6_TRANSACTION_MASTER");
  if (full_matrix) {
    static const uint32_t expected_types[16] = {
        16, 16, 21, 21, 23, 23, 20, 20, 700, 700, 701, 701, 25, 25, 17, 17,
    };
    if (parameter_count != 16 || parameter_types == NULL || parameter_values == NULL ||
        parameter_lengths == NULL || parameter_formats == NULL) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 1;
    } else {
      for (int i = 0; i < 16; i++) {
        if (parameter_types[i] != expected_types[i]) {
          protocol_ok = 0;
          if (protocol_error == 0) protocol_error = 10 + i;
        }
        if (parameter_formats[i] != 0) {
          protocol_ok = 0;
          if (protocol_error == 0) protocol_error = 30 + i;
        }
        if (parameter_values[i] == NULL) {
          if ((i & 1) == 0 || parameter_lengths[i] != 0) {
            protocol_ok = 0;
            if (protocol_error == 0) protocol_error = 50 + i;
          }
        } else if (parameter_lengths[i] != (int)strlen(parameter_values[i])) {
          protocol_ok = 0;
          if (protocol_error == 0) protocol_error = 70 + i;
        }
      }
    }
  } else if (view_fault) {
    static const uint32_t expected_types[3] = {25, 17, 20};
    if (parameter_count != 3 || parameter_types == NULL || parameter_values == NULL ||
        parameter_lengths == NULL || parameter_formats == NULL) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 94;
    } else {
      for (int i = 0; i < 3; i++) {
        if (parameter_types[i] != expected_types[i] || parameter_values[i] == NULL ||
            parameter_formats[i] != 0 ||
            parameter_lengths[i] != (int)strlen(parameter_values[i])) {
          protocol_ok = 0;
          if (protocol_error == 0) protocol_error = 95;
        }
      }
    }
  } else if (q6_user || q6_transaction) {
    int expected_parameters = q6_user ? 2 : 1;
    if (parameter_count != expected_parameters || parameter_types == NULL || parameter_values == NULL ||
        parameter_lengths == NULL || parameter_formats == NULL ||
        parameter_values[0] == NULL || parameter_formats[0] != 0 ||
        parameter_lengths[0] != (int)strlen(parameter_values[0])) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 81;
    }
    for (int index = 0; index < parameter_count; index++) {
      if ((parameter_types[index] != 0 && parameter_types[index] != 20) ||
          parameter_values[index] == NULL || parameter_formats[index] != 0 ||
          parameter_lengths[index] != (int)strlen(parameter_values[index])) {
        protocol_ok = 0;
        if (protocol_error == 0) protocol_error = 81;
      }
    }
  } else if (parameter_count != 1 || parameter_types == NULL || parameter_values == NULL ||
             parameter_lengths == NULL || parameter_formats == NULL || parameter_types[0] != 20 ||
             parameter_values[0] == NULL || parameter_formats[0] != 0 ||
             parameter_lengths[0] != (int)strlen(parameter_values[0])) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 96;
  }
  if (has(command, "NULL_RESULT")) return NULL;
  FakeResult *result = new_result();
  if (result == NULL) return NULL;
  if (q6_user) {
    q6_user_result(
        result,
        parameter_values == NULL ? 0 : atoi(parameter_values[0]),
        parameter_values == NULL || parameter_count < 2 ? 0 : atoi(parameter_values[1]));
  } else if (q6_transaction) {
    q6_transaction_result(result);
  } else if (full_matrix && parameter_values != NULL && parameter_count == 16) {
    static const char *names[16] = {
        "b", "nb", "i16v", "ni16", "i32v", "ni32", "i64v", "ni64",
        "f32v", "nf32", "f64v", "nf64", "textv", "ntext", "bytesv", "nbytes",
    };
    static const uint32_t oids[16] = {
        16, 16, 21, 21, 23, 23, 20, 20, 700, 700, 701, 701, 25, 25, 17, 17,
    };
    result->fields = 16;
    for (int i = 0; i < 16; i++) {
      result->names[i] = names[i];
      result->oids[i] = oids[i];
      result->values[0][i] = parameter_values[i] == NULL ? "" : parameter_values[i];
      result->nulls[0][i] = parameter_values[i] == NULL;
    }
  } else {
    result->values[0][0] = parameter_values != NULL && parameter_count > 0 ? parameter_values[0] : "0";
  }
  if (view_fault) {
    static const char invalid_utf8[] = {(char)0xff, 0};
    result->fields = 2;
    result->names[0] = "label";
    result->names[1] = "payload";
    result->oids[0] = 25;
    result->oids[1] = 17;
    result->values[0][0] = has(command, "TEXT_UTF8") ? invalid_utf8 : "view";
    result->values[0][1] = has(command, "BYTES_HEX") ? "\\x0g" : "\\x010203";
    if (has(command, "TEXT_NULL")) result->row_fault = 1;
    if (has(command, "TEXT_LENGTH")) result->row_fault = 2;
    if (has(command, "BYTES_NULL")) result->row_fault = 4;
    if (has(command, "BYTES_LENGTH")) result->row_fault = 5;
  }
  if (has(command, "COMMAND_OK")) {
    result->status = 1;
    result->rows = 0;
    result->fields = 0;
    result->affected = has(command, "AFFECTED_EMPTY") ? "" : "2";
  }
  if (has(command, "ROW_COMMAND")) result->status = 2;
  if (has(command, "AFFECTED_MALFORMED")) result->affected = "2x";
  if (has(command, "AFFECTED_NEGATIVE")) result->affected = "-1";
  if (has(command, "AFFECTED_OVERFLOW")) result->affected = "9223372036854775808";
  if (has(command, "ZERO_ROWS")) result->rows = 0;
  if (has(command, "BAD_FIRST")) {
    result->rows = 2;
    result->values[0][0] = "bad";
    result->values[1][0] = "9223372036854775808";
  }
  if (has(command, "VALID_FIRST")) {
    result->rows = 2;
    result->values[1][0] = "bad";
  }
  if (has(command, "WRONG_NAME")) result->names[0] = "other";
  if (has(command, "WRONG_OID")) result->oids[0] = 23;
  if (has(command, "NULL_VALUE")) result->nulls[0][0] = 1;
  if (has(command, "OUT_OF_RANGE")) result->values[0][0] = "9223372036854775808";
  if (has(command, "NATIVE_CONSTRAINT")) {
    result->status = 7;
    result->sqlstate = "23505";
  }
  if (has(command, "NATIVE_SERIALIZATION")) {
    result->status = 7;
    result->sqlstate = "40001";
  }
  if (has(command, "NATIVE_DEADLOCK")) {
    result->status = 7;
    result->sqlstate = "40P01";
  }
  if (has(command, "NATIVE_CANCELLED")) {
    result->status = 7;
    result->sqlstate = "57014";
  }
  return result;
}

int PQsendQueryParams(
    FakeConn *connection,
    const char *command,
    int parameter_count,
    const uint32_t *parameter_types,
    const char *const *parameter_values,
    const int *parameter_lengths,
    const int *parameter_formats,
    int result_format) {
  if (has(command, "SEND_FAIL")) return 0;
  async_result = PQexecParams(
      connection,
      command,
      parameter_count,
      parameter_types,
      parameter_values,
      parameter_lengths,
      parameter_formats,
      result_format);
  async_busy = has(command, "TIMEOUT_WAIT");
  async_cancel_fail = has(command, "CANCEL_FAIL");
  async_drain_fail = has(command, "DRAIN_FAIL");
  async_flush_fail = has(command, "FLUSH_FAIL");
  async_consume_fail = has(command, "CONSUME_FAIL");
  async_cancel_resource_fail = has(command, "CANCEL_RESOURCE_FAIL");
  async_transaction_unknown = has(command, "TX_UNKNOWN");
  async_cancelled = 0;
  return async_result == NULL ? 0 : 1;
}

int PQsendQueryPrepared(
    FakeConn *connection,
    const char *name,
    int parameter_count,
    const char *const *parameter_values,
    const int *parameter_lengths,
    const int *parameter_formats,
    int result_format) {
  async_result = PQexecPrepared(
      connection,
      name,
      parameter_count,
      parameter_values,
      parameter_lengths,
      parameter_formats,
      result_format);
  async_busy = prepared_timeout_wait;
  async_cancel_fail = 0;
  async_drain_fail = 0;
  async_flush_fail = 0;
  async_consume_fail = 0;
  async_cancel_resource_fail = 0;
  async_transaction_unknown = 0;
  async_cancelled = 0;
  return async_result == NULL ? 0 : 1;
}

int PQsetnonblocking(FakeConn *connection, int enabled) {
  (void)connection;
  nonblocking_calls++;
  if (enabled == 1 && fail_next_nonblocking_enable) {
    fail_next_nonblocking_enable = 0;
    return -1;
  }
  if (enabled == 1 && delay_next_nonblocking_enable) {
    struct timespec delay = {0, 2000000};
    delay_next_nonblocking_enable = 0;
    (void)nanosleep(&delay, NULL);
  }
  if (enabled == 0 && fail_next_nonblocking_restore) {
    fail_next_nonblocking_restore = 0;
    return -1;
  }
  return enabled == 0 || enabled == 1 ? 0 : -1;
}

int PQflush(FakeConn *connection) {
  (void)connection;
  return async_flush_fail ? -1 : 0;
}

int PQconsumeInput(FakeConn *connection) {
  (void)connection;
  consume_calls++;
  if (async_consume_fail || (async_cancelled && async_drain_fail)) return 0;
  return 1;
}

int PQisBusy(FakeConn *connection) {
  (void)connection;
  return async_busy;
}

FakeResult *PQgetResult(FakeConn *connection) {
  (void)connection;
  if (async_busy) return NULL;
  FakeResult *result = async_result;
  async_result = NULL;
  return result;
}

int PQtransactionStatus(FakeConn *connection) {
  if (async_transaction_unknown) return 4;
  return connection == NULL ? 4 : connection->transaction_status;
}

FakeCancel *PQgetCancel(FakeConn *connection) {
  if (async_cancel_resource_fail) return NULL;
  FakeCancel *cancel = (FakeCancel *)malloc(sizeof(FakeCancel));
  if (cancel != NULL) cancel->connection = connection;
  return cancel;
}

int PQcancel(FakeCancel *cancel, char *error_buffer, int error_buffer_size) {
  (void)error_buffer;
  (void)error_buffer_size;
  cancel_calls++;
  if (cancel == NULL || async_cancel_fail) return 0;
  async_cancelled = 1;
  async_busy = async_drain_fail;
  if (async_result != NULL) {
    async_result->status = 7;
    async_result->sqlstate = "57014";
    async_result->message = "cancelled by deadline";
  }
  return 1;
}

void PQfreeCancel(FakeCancel *cancel) { free(cancel); }

int PQresultStatus(const FakeResult *result) { return result == NULL ? 7 : result->status; }
char *PQcmdStatus(const FakeResult *result) {
  return (char *)(result == NULL ? NULL : result->command_status);
}
int PQntuples(const FakeResult *result) { return result == NULL ? -1 : result->rows; }
int PQnfields(const FakeResult *result) { return result == NULL ? -1 : result->fields; }
char *PQfname(const FakeResult *result, int column) {
  return (char *)(result == NULL || column < 0 || column >= result->fields ? NULL : result->names[column]);
}
uint32_t PQftype(const FakeResult *result, int column) {
  return result == NULL || column < 0 || column >= result->fields ? 0 : result->oids[column];
}
int PQgetisnull(const FakeResult *result, int row, int column) {
  return result == NULL || row < 0 || row >= result->rows || column < 0 || column >= result->fields
      ? 1
      : result->nulls[row][column];
}
char *PQgetvalue(const FakeResult *result, int row, int column) {
  if (result == NULL || row < 0 || row >= result->rows || column < 0 || column >= result->fields) return NULL;
  if (column == 0 && result->names[0] != NULL &&
      (strcmp(result->names[0], "user_id") == 0 ||
       strcmp(result->names[0], "transaction_id") == 0)) {
    FakeResult *mutable_result = (FakeResult *)result;
    if (!mutable_result->delivered[row]) {
      mutable_result->delivered[row] = 1;
      q6_delivered_rows++;
    }
  }
  if ((result->row_fault == 1 && column == 0) || (result->row_fault == 4 && column == 1)) {
    return NULL;
  }
  return (char *)result->values[row][column];
}
int PQgetlength(const FakeResult *result, int row, int column) {
  if (result != NULL && ((result->row_fault == 2 && column == 0) ||
                         (result->row_fault == 5 && column == 1))) {
    return -1;
  }
  char *value = PQgetvalue(result, row, column);
  return value == NULL ? 0 : (int)strlen(value);
}
char *PQcmdTuples(const FakeResult *result) { return (char *)(result == NULL ? NULL : result->affected); }

char *PQresultErrorField(const FakeResult *result, int field_code) {
  if (result == NULL) return NULL;
  switch (field_code) {
    case 67: return (char *)result->sqlstate;
    case 77: return (char *)result->message;
    case 68: return (char *)result->detail;
    case 110: return (char *)result->constraint_name;
    case 116: return (char *)result->table_name;
    case 99: return (char *)result->column_name;
    default: return NULL;
  }
}

void PQclear(FakeResult *result) {
  clear_calls++;
  free(result);
}
