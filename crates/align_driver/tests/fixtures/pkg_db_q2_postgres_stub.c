#include <stdint.h>
#include <stdlib.h>
#include <string.h>

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
  const char *message;
} FakeConn;

typedef struct {
  int status;
  int rows;
  int fields;
  const char *names[2];
  uint32_t oids[2];
  const char *values[2][2];
  int nulls[2][2];
  const char *affected;
  const char *sqlstate;
  const char *message;
  const char *detail;
  const char *constraint_name;
  const char *table_name;
  const char *column_name;
} FakeResult;

static int connect_calls;
static int finish_calls;
static int encoding_calls;
static int execute_calls;
static int clear_calls;
static int protocol_ok;
static int last_timeout;

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
  last_timeout = -1;
}

int align_pg_connect_calls(void) { return connect_calls; }
int align_pg_finish_calls(void) { return finish_calls; }
int align_pg_encoding_calls(void) { return encoding_calls; }
int align_pg_execute_calls(void) { return execute_calls; }
int align_pg_clear_calls(void) { return clear_calls; }
int align_pg_protocol_ok(void) { return protocol_ok; }
int align_pg_last_timeout(void) { return last_timeout; }

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
  if (expand_dbname != 1 || keywords == NULL || values == NULL) protocol_ok = 0;
  const char *dbname = NULL;
  int client_count = 0;
  int pair_count = 0;
  for (; keywords != NULL && keywords[pair_count] != NULL; pair_count++) {
    if (values[pair_count] == NULL) protocol_ok = 0;
    if (strcmp(keywords[pair_count], "dbname") == 0) dbname = values[pair_count];
    if (strcmp(keywords[pair_count], "client_encoding") == 0) {
      client_count++;
      if (strcmp(values[pair_count], "UTF8") != 0) protocol_ok = 0;
    }
    if (strcmp(keywords[pair_count], "connect_timeout") == 0) {
      last_timeout = atoi(values[pair_count]);
    }
  }
  if (pair_count == 0 || strcmp(keywords[pair_count - 1], "client_encoding") != 0 || client_count != 1) {
    protocol_ok = 0;
  }
  if (has(dbname, "null-connection")) return NULL;
  FakeConn *connection = (FakeConn *)calloc(1, sizeof(FakeConn));
  if (connection == NULL) return NULL;
  connection->status = has(dbname, "bad-connection") ? 1 : 0;
  connection->encoding = has(dbname, "bad-encoding") ? -1 : 6;
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
  free(connection);
}

const char *PQerrorMessage(const FakeConn *connection) {
  return connection == NULL ? "stub null connection" : connection->message;
}

static FakeResult *new_result(void) {
  FakeResult *result = (FakeResult *)calloc(1, sizeof(FakeResult));
  if (result == NULL) return NULL;
  result->status = 2;
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
  if (command == NULL || result_format != 0) protocol_ok = 0;
  if (parameter_count != 1 || parameter_types == NULL || parameter_values == NULL ||
      parameter_lengths == NULL || parameter_formats == NULL || parameter_types[0] != 20 ||
      parameter_values[0] == NULL || parameter_formats[0] != 0 ||
      parameter_lengths[0] != (int)strlen(parameter_values[0])) {
    protocol_ok = 0;
  }
  if (has(command, "NULL_RESULT")) return NULL;
  FakeResult *result = new_result();
  if (result == NULL) return NULL;
  result->values[0][0] = parameter_values != NULL && parameter_count > 0 ? parameter_values[0] : "0";
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

int PQresultStatus(const FakeResult *result) { return result == NULL ? 7 : result->status; }
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
  return (char *)(result->values[row][column] == NULL ? "" : result->values[row][column]);
}
int PQgetlength(const FakeResult *result, int row, int column) {
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
