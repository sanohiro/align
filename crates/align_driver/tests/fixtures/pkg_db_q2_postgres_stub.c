#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
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
  int ordinal;
  uint32_t expected_second_prepare_id_oid;
  const char *message;
} FakeConn;

typedef struct {
  int status;
  const char *command_status;
  int rows;
  int fields;
  const char *names[16];
  uint32_t oids[16];
  int formats[16];
  const char *values[64][16];
  int lengths[64][16];
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
  int started;
} FakeCancel;

#define ASYNC_QUEUE_CAPACITY 80

static int connect_calls;
static int fail_connect_at;
static int finish_calls;
static int finish_ordinals[4];
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
static int pool_rollback_fault;
static char prepared_name[64];
static FakeResult *async_result;
static int q6_compound_owner;
static FakeResult *async_queue[ASYNC_QUEUE_CAPACITY];
static int async_queue_count;
static int async_queue_index;
static int async_pause_after_results;
static FakeConn *async_connection;
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
static int cancel_socket_wait_calls;
static int consume_calls;
static int single_row_mode_calls;
static int chunked_row_mode_calls;
static int last_chunk_size;
static int fail_next_row_mode;
static int stream_fatal_after_rows;
static int stream_post_terminal_status;
static int stream_missing_terminal;
static int stream_initial_status;
static int q6_delivered_rows;
static int q6_fail_next_execute;
static int forced_result_status;
static int unsafe_status_seen;
static int forbidden_after_status_calls;
static int prepared_format_matrix;
static int format_matrix_direct_calls;
static int format_matrix_prepared_calls;
static int binary_fault;
static int binary_empty_calls;
static unsigned char format_matrix_left_binary[27][4];
static unsigned char format_matrix_right_binary[27][1];
static char format_matrix_left_text[27][12];
static char format_matrix_right_text[27][5];
static char dynamic_echo_value[64];
static unsigned char dynamic_echo_bytes[64];
static int dynamic_echo_bytes_len;

static void clear_async_results(void) {
  if (async_result != NULL) {
    free(async_result);
    async_result = NULL;
  }
  for (int i = async_queue_index; i < async_queue_count; i++) free(async_queue[i]);
  async_queue_count = 0;
  async_queue_index = 0;
  async_pause_after_results = 0;
  async_connection = NULL;
}

static int enqueue_async(FakeResult *result) {
  if (result == NULL || async_queue_count >= ASYNC_QUEUE_CAPACITY) return 0;
  async_queue[async_queue_count++] = result;
  return 1;
}

static int status_requires_close(int status) {
  return status == 3 || status == 4 || status == 8 || status == 10 || status == 11 ||
         status < 0 || status > 12;
}

static void note_forbidden_after_status(void) {
  if (unsafe_status_seen) forbidden_after_status_calls++;
}

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
  clear_async_results();
  connect_calls = 0;
  fail_connect_at = 0;
  finish_calls = 0;
  memset(finish_ordinals, 0, sizeof(finish_ordinals));
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
  pool_rollback_fault = 0;
  prepared_name[0] = '\0';
  async_busy = 0;
  q6_compound_owner = 0;
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
  cancel_socket_wait_calls = 0;
  consume_calls = 0;
  single_row_mode_calls = 0;
  chunked_row_mode_calls = 0;
  last_chunk_size = 0;
  fail_next_row_mode = 0;
  stream_fatal_after_rows = -1;
  stream_post_terminal_status = -1;
  stream_missing_terminal = 0;
  stream_initial_status = -1;
  q6_delivered_rows = 0;
  q6_fail_next_execute = 0;
  forced_result_status = -1;
  unsafe_status_seen = 0;
  forbidden_after_status_calls = 0;
  prepared_format_matrix = 0;
  format_matrix_direct_calls = 0;
  format_matrix_prepared_calls = 0;
  binary_fault = 0;
  binary_empty_calls = 0;
  dynamic_echo_value[0] = '\0';
  dynamic_echo_bytes_len = 0;
}

int align_pg_connect_calls(void) { return connect_calls; }
void align_pg_fail_connect_at(int ordinal) { fail_connect_at = ordinal; }
int align_pg_finish_calls(void) { return finish_calls; }
int align_pg_finish_ordinal(int index) {
  return index < 0 || index >= 4 ? -1 : finish_ordinals[index];
}
int align_pg_encoding_calls(void) { return encoding_calls; }
int align_pg_execute_calls(void) { return execute_calls; }
int align_pg_clear_calls(void) { return clear_calls; }
int align_pg_protocol_ok(void) { return protocol_ok; }
int align_pg_protocol_error(void) { return protocol_error; }
int align_pg_nonblocking_calls(void) { return nonblocking_calls; }
int align_pg_cancel_calls(void) { return cancel_calls; }
int align_pg_cancel_socket_wait_calls(void) { return cancel_socket_wait_calls; }
int align_pg_consume_calls(void) { return consume_calls; }
int align_pg_single_row_mode_calls(void) { return single_row_mode_calls; }
int align_pg_chunked_row_mode_calls(void) { return chunked_row_mode_calls; }
int align_pg_last_chunk_size(void) { return last_chunk_size; }
int align_pg_q6_delivered_rows(void) { return q6_delivered_rows; }
void align_pg_q6_fail_next_execute(void) { q6_fail_next_execute = 1; }
void align_pg_q6_compound_owner(void) { q6_compound_owner = 1; }
void align_pg_force_result_status(int status) { forced_result_status = status; }
int align_pg_forbidden_after_status_calls(void) { return forbidden_after_status_calls; }
int align_pg_last_timeout(void) { return last_timeout; }
int align_pg_prepare_calls(void) { return prepare_calls; }
int align_pg_execute_prepared_calls(void) { return execute_prepared_calls; }
int align_pg_control_calls(void) { return control_calls; }
int align_pg_deallocate_calls(void) { return deallocate_calls; }
void align_pg_fail_next_control(void) { fail_next_control = 1; }
void align_pg_rollback_next_commit(void) { rollback_next_commit = 1; }
void align_pg_pool_rollback_fault(int fault) { pool_rollback_fault = fault; }
void align_pg_fail_next_nonblocking_enable(void) { fail_next_nonblocking_enable = 1; }
void align_pg_fail_next_nonblocking_restore(void) { fail_next_nonblocking_restore = 1; }
void align_pg_delay_next_nonblocking_enable(void) { delay_next_nonblocking_enable = 1; }
void align_pg_fail_next_row_mode(void) { fail_next_row_mode = 1; }
void align_pg_set_binary_fault(int fault) { binary_fault = fault; }

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
  if (fail_connect_at > 0 && connect_calls == fail_connect_at) return NULL;
  FakeConn *connection = (FakeConn *)calloc(1, sizeof(FakeConn));
  if (connection == NULL) return NULL;
  connection->status = has(dbname, "bad-connection") ? 1 : 0;
  connection->encoding = has(dbname, "bad-encoding") ? -1 : 6;
  connection->transaction_status = 0;
  connection->ordinal = connect_calls;
  connection->expected_second_prepare_id_oid = has(dbname, "/q4a") ? 20 : 23;
  connection->message = "stub connection failure";
  return connection;
}

int PQstatus(const FakeConn *connection) { return connection == NULL ? 1 : connection->status; }

int PQclientEncoding(const FakeConn *connection) {
  encoding_calls++;
  return connection == NULL ? -1 : connection->encoding;
}

void PQfinish(FakeConn *connection) {
  if (finish_calls < 4 && connection != NULL) finish_ordinals[finish_calls] = connection->ordinal;
  finish_calls++;
  if (async_result != NULL) clear_calls++;
  clear_calls += async_queue_count - async_queue_index;
  clear_async_results();
  free(connection);
}

const char *PQerrorMessage(const FakeConn *connection) {
  return connection == NULL ? "stub null connection" : connection->message;
}

static FakeResult *new_result(void) {
  FakeResult *result = (FakeResult *)calloc(1, sizeof(FakeResult));
  if (result == NULL) return NULL;
  for (int row = 0; row < 64; row++) {
    for (int column = 0; column < 16; column++) result->lengths[row][column] = -1;
  }
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

static FakeResult *copy_result_rows(
    const FakeResult *source, int first_row, int row_count, int status) {
  FakeResult *copy = new_result();
  if (copy == NULL || source == NULL) return copy;
  *copy = *source;
  copy->status = status;
  copy->rows = row_count;
  memset(copy->delivered, 0, sizeof(copy->delivered));
  for (int row = 0; row < row_count; row++) {
    for (int column = 0; column < source->fields; column++) {
      copy->values[row][column] = source->values[first_row + row][column];
      copy->lengths[row][column] = source->lengths[first_row + row][column];
      copy->nulls[row][column] = source->nulls[first_row + row][column];
    }
  }
  return copy;
}

static FakeResult *stream_error_result(int status, const char *sqlstate, const char *message) {
  FakeResult *result = new_result();
  if (result != NULL) {
    result->status = status;
    result->rows = 0;
    result->sqlstate = sqlstate;
    result->message = message;
  }
  return result;
}

FakeResult *align_pg_make_result(void) { return new_result(); }

static int format_matrix_index(
    int expected_call,
    int parameter_count,
    const char *const *parameter_values,
    const int *parameter_lengths,
    const int *parameter_formats,
    int result_format) {
  if (expected_call < 0 || expected_call >= 27 || parameter_count != 2 ||
      parameter_values == NULL || parameter_lengths == NULL || parameter_formats == NULL ||
      parameter_values[0] == NULL || parameter_values[1] == NULL) return -1;
  int index = -1;
  if (parameter_formats[0] == 0) {
    if (parameter_lengths[0] != (int)strlen(parameter_values[0])) return -1;
    index = atoi(parameter_values[0]);
  } else if (parameter_formats[0] == 1 && parameter_lengths[0] == 4) {
    const unsigned char *bytes = (const unsigned char *)parameter_values[0];
    index = (int)(((uint32_t)bytes[0] << 24) | ((uint32_t)bytes[1] << 16) |
                  ((uint32_t)bytes[2] << 8) | (uint32_t)bytes[3]);
  } else {
    return -1;
  }
  int left_mode = index / 9;
  int right_mode = (index / 3) % 3;
  int result_mode = index % 3;
  if (index != expected_call || parameter_formats[0] != (left_mode == 2) ||
      parameter_formats[1] != (right_mode == 2) || result_format != (result_mode == 2)) return -1;
  unsigned char expected_byte = (unsigned char)index;
  if (parameter_formats[1] == 1) {
    if (parameter_lengths[1] != 1 ||
        (unsigned char)parameter_values[1][0] != expected_byte) return -1;
  } else {
    char expected_text[5];
    snprintf(expected_text, sizeof(expected_text), "\\x%02x", expected_byte);
    if (parameter_lengths[1] != 4 || memcmp(parameter_values[1], expected_text, 4) != 0) return -1;
  }
  return index;
}

static FakeResult *format_matrix_result(int index, int result_format) {
  FakeResult *result = new_result();
  if (result == NULL || index < 0 || index >= 27) return result;
  result->fields = 2;
  result->names[0] = "left";
  result->names[1] = "right";
  result->oids[0] = 23;
  result->oids[1] = 17;
  result->formats[0] = result_format;
  result->formats[1] = result_format;
  if (result_format == 1) {
    format_matrix_left_binary[index][0] = 0;
    format_matrix_left_binary[index][1] = 0;
    format_matrix_left_binary[index][2] = 0;
    format_matrix_left_binary[index][3] = (unsigned char)index;
    format_matrix_right_binary[index][0] = (unsigned char)index;
    result->values[0][0] = (const char *)format_matrix_left_binary[index];
    result->values[0][1] = (const char *)format_matrix_right_binary[index];
    result->lengths[0][0] = 4;
    result->lengths[0][1] = 1;
  } else {
    snprintf(format_matrix_left_text[index], sizeof(format_matrix_left_text[index]), "%d", index);
    snprintf(format_matrix_right_text[index], sizeof(format_matrix_right_text[index]), "\\x%02x", index);
    result->values[0][0] = format_matrix_left_text[index];
    result->values[0][1] = format_matrix_right_text[index];
    result->lengths[0][0] = (int)strlen(format_matrix_left_text[index]);
    result->lengths[0][1] = 4;
  }
  return result;
}

FakeResult *PQprepare(
    FakeConn *connection,
    const char *name,
    const char *command,
    int parameter_count,
    const uint32_t *parameter_types) {
  (void)connection;
  note_forbidden_after_status();
  prepare_calls++;
  int format_matrix = has(command, "FORMAT_MATRIX");
  int common_types = prepare_calls == 1 && parameter_types != NULL &&
                     parameter_types[0] == 20 && parameter_types[1] == 0 &&
                     parameter_types[2] == 0;
  int overridden_types = prepare_calls == 2 && connection != NULL && parameter_types != NULL &&
                         parameter_types[0] == connection->expected_second_prepare_id_oid &&
                         parameter_types[1] == 25 && parameter_types[2] == 17;
  int format_types = format_matrix && parameter_types != NULL &&
                     parameter_count == 2 && parameter_types[0] == 23 && parameter_types[1] == 17;
  if (name == NULL || strncmp(name, "__align_pkg_db_", 15) != 0 || command == NULL ||
      (!format_types && (parameter_count != 3 || (!common_types && !overridden_types)))) {
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
  prepared_format_matrix = format_matrix;
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
  note_forbidden_after_status();
  execute_prepared_calls++;
  if (prepared_format_matrix) {
    int index = format_matrix_index(
        format_matrix_prepared_calls++, parameter_count, parameter_values,
        parameter_lengths, parameter_formats, result_format);
    if (index < 0) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 98;
      index = 0;
    }
    return format_matrix_result(index, result_format);
  }
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
  note_forbidden_after_status();
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
    result->status = fail_next_control ||
            (command != NULL && strcmp(command, "ROLLBACK") == 0 && pool_rollback_fault == 1)
        ? 7
        : 1;
    if (command != NULL && strncmp(command, "BEGIN ", 6) == 0) {
      result->command_status = "BEGIN";
      if (connection != NULL) connection->transaction_status = 2;
    } else if (command != NULL && strcmp(command, "COMMIT") == 0) {
      result->command_status = rollback_next_commit ? "ROLLBACK" : "COMMIT";
      if (connection != NULL) connection->transaction_status = 0;
    } else if (command != NULL && strcmp(command, "ROLLBACK") == 0) {
      result->command_status = pool_rollback_fault == 2 ? "COMMIT" : "ROLLBACK";
      if (connection != NULL) connection->transaction_status = pool_rollback_fault == 3 ? 2 : 0;
    } else if (command != NULL && strncmp(command, "DEALLOCATE ", 11) == 0) {
      result->command_status = "DEALLOCATE";
    }
    result->rows = 0;
    result->fields = 0;
  }
  fail_next_control = 0;
  rollback_next_commit = 0;
  pool_rollback_fault = 0;
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
  if (!q6_compound_owner && parameter == 1 && last_parameter == 1) {
    result->rows = 1;
    result->values[0][0] = "1";
    result->values[0][1] = "Alice";
    result->values[0][2] = "10";
    result->values[0][3] = "Admin";
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
  result->rows = 0;
  protocol_ok = 0;
  if (protocol_error == 0) protocol_error = 97;
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

static int full_binary_value_ok(int index, const char *value, int length) {
  static const unsigned char expected[][8] = {
      {0x01}, {0x01},
      {0xff, 0xf4}, {0xfb, 0x2e},
      {0x00, 0x00, 0x0d, 0x80}, {0xff, 0xfe, 0x1d, 0xc0},
      {0x00, 0x00, 0x00, 0x00, 0x00, 0x78, 0x64, 0xcb},
      {0xff, 0xff, 0xff, 0xff, 0xf8, 0xa4, 0x32, 0xeb},
      {0x3f, 0xc0, 0x00, 0x00}, {0x40, 0x20, 0x00, 0x00},
      {0xc0, 0x23, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00},
      {0xc0, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00},
  };
  static const int widths[] = {1, 1, 2, 2, 4, 4, 8, 8, 4, 4, 8, 8};
  if (value == NULL || index < 0 || index >= 16) return 0;
  if (index < 12) {
    return length == widths[index] && memcmp(value, expected[index], (size_t)length) == 0;
  }
  if (index == 12) return length == 5 && memcmp(value, "hello", 5) == 0;
  if (index == 13) return length == 8 && memcmp(value, "nullable", 8) == 0;
  static const unsigned char bytes[] = {0x00, 0x7f, 0xff};
  return length == 3 && memcmp(value, bytes, 3) == 0;
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
  note_forbidden_after_status();
  execute_calls++;
  if (q6_fail_next_execute) {
    q6_fail_next_execute = 0;
    return NULL;
  }
  if (command == NULL || result_format < 0 || result_format > 1) {
    protocol_ok = 0;
    if (protocol_error == 0) protocol_error = 87;
  }
  int full_matrix = has(command, "FULL_MATRIX");
  int dynamic_matrix = has(command, "DYNAMIC_MATRIX");
  int dynamic_command = has(command, "DYNAMIC_COMMAND");
  int dynamic_zero = has(command, "DYNAMIC_ZERO_ROWS");
  int dynamic_simple = has(command, "DYNAMIC_SIMPLE");
  int dynamic_empty = has(command, "DYNAMIC_EMPTY_VALUES");
  int dynamic_echo = has(command, "DYNAMIC_ECHO");
  int format_matrix = has(command, "FORMAT_MATRIX");
  int format_command = has(command, "FORMAT_COMMAND");
  int binary_fault_query = has(command, "BINARY_FAULT");
  int binary_empty = has(command, "BINARY_EMPTY");
  int view_fault = has(command, "VIEW_FAULT");
  int q6_user = has(command, "Q6_USER_GROUPS");
  int q6_transaction = has(command, "Q6_TRANSACTION_MASTER");
  if (!full_matrix && !dynamic_matrix && !dynamic_command && !dynamic_zero && !dynamic_simple &&
      !dynamic_empty && !dynamic_echo &&
      !format_matrix && !format_command && !binary_fault_query &&
      !binary_empty && result_format != 0) {
    protocol_ok = 0;
    if (protocol_error == 0) protocol_error = 87;
  }
  if (dynamic_matrix || dynamic_zero) {
    static const uint32_t expected_types[9] = {0, 16, 21, 23, 20, 700, 701, 25, 17};
    static const int expected_lengths[9] = {0, 1, 2, 4, 8, 4, 8, 2, 2};
    static const unsigned char expected_bool[] = {1};
    static const unsigned char expected_i16[] = {0xff, 0xfe};
    static const unsigned char expected_i32[] = {0x01, 0x02, 0x03, 0x04};
    static const unsigned char expected_i64[] = {0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe};
    static const unsigned char expected_f32[] = {0x3f, 0xc0, 0x00, 0x00};
    static const unsigned char expected_f64[] = {0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};
    static const unsigned char expected_text[] = {0xc3, 0xa9};
    static const unsigned char expected_bytes[] = {0x00, 0xff};
    static const unsigned char *expected_values[9] = {
        NULL, expected_bool, expected_i16, expected_i32, expected_i64,
        expected_f32, expected_f64, expected_text, expected_bytes,
    };
    if (parameter_count != 9 || parameter_types == NULL || parameter_values == NULL ||
        parameter_lengths == NULL || parameter_formats == NULL || result_format != 1) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 110;
    } else {
      for (int i = 0; i < 9; i++) {
        int expected_format = i == 0 ? 0 : 1;
        if (parameter_types[i] != expected_types[i] ||
            parameter_formats[i] != expected_format ||
            parameter_lengths[i] != expected_lengths[i] ||
            (i == 0 ? parameter_values[i] != NULL
                    : parameter_values[i] == NULL ||
                      memcmp(parameter_values[i], expected_values[i], expected_lengths[i]) != 0)) {
          protocol_ok = 0;
          if (protocol_error == 0) protocol_error = 111 + i;
        }
      }
    }
  } else if (dynamic_echo) {
    if (parameter_count != 2 || parameter_types == NULL || parameter_values == NULL ||
        parameter_lengths == NULL || parameter_formats == NULL || result_format != 1 ||
        parameter_types[0] != 25 || parameter_types[1] != 17 ||
        parameter_formats[0] != 1 || parameter_formats[1] != 1 ||
        parameter_values[0] == NULL || parameter_values[1] == NULL ||
        parameter_lengths[0] < 0 || parameter_lengths[0] >= (int)sizeof(dynamic_echo_value) ||
        parameter_lengths[1] < 0 || parameter_lengths[1] > (int)sizeof(dynamic_echo_bytes)) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 123;
    } else {
      memcpy(dynamic_echo_value, parameter_values[0], (size_t)parameter_lengths[0]);
      dynamic_echo_value[parameter_lengths[0]] = '\0';
      memcpy(dynamic_echo_bytes, parameter_values[1], (size_t)parameter_lengths[1]);
      dynamic_echo_bytes_len = parameter_lengths[1];
    }
  } else if (dynamic_empty) {
    if (parameter_count != 3 || parameter_types == NULL || parameter_values == NULL ||
        parameter_lengths == NULL || parameter_formats == NULL || result_format != 1 ||
        parameter_types[0] != 25 || parameter_types[1] != 17 || parameter_types[2] != 0 ||
        parameter_formats[0] != 1 || parameter_formats[1] != 1 || parameter_formats[2] != 0 ||
        parameter_lengths[0] != 0 || parameter_lengths[1] != 0 || parameter_lengths[2] != 0 ||
        parameter_values[0] == NULL || parameter_values[1] == NULL || parameter_values[2] != NULL) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 122;
    }
  } else if (dynamic_command || dynamic_simple) {
    if (parameter_count != 0 || parameter_types != NULL || parameter_values != NULL ||
        parameter_lengths != NULL || parameter_formats != NULL || result_format != 1) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 121;
    }
  } else if (format_matrix || format_command) {
    if (parameter_types == NULL || parameter_types[0] != 23 || parameter_types[1] != 17) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 98;
    }
    int expected = format_command ? 26 : format_matrix_direct_calls++;
    int index = format_matrix_index(
        expected, parameter_count, parameter_values, parameter_lengths,
        parameter_formats, result_format);
    if (index < 0) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 98;
    }
  } else if (binary_empty) {
    int expected_present = binary_empty_calls++ == 0;
    if (parameter_count != 1 || parameter_types == NULL || parameter_values == NULL ||
        parameter_lengths == NULL || parameter_formats == NULL || parameter_types[0] != 17 ||
        parameter_lengths[0] != 0 || parameter_formats[0] != 1 || result_format != 0 ||
        (expected_present ? parameter_values[0] == NULL : parameter_values[0] != NULL)) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 100;
    }
  } else if (binary_fault_query) {
    static const unsigned char expected_flag[] = {1};
    if (parameter_count != 2 || parameter_types == NULL || parameter_values == NULL ||
        parameter_lengths == NULL || parameter_formats == NULL || parameter_types[0] != 16 ||
        parameter_types[1] != 25 || parameter_formats[0] != 1 || parameter_formats[1] != 1 ||
        parameter_values[0] == NULL || parameter_lengths[0] != 1 ||
        memcmp(parameter_values[0], expected_flag, 1) != 0 || parameter_values[1] == NULL ||
        parameter_lengths[1] != 5 || memcmp(parameter_values[1], "valid", 5) != 0 ||
        result_format != 1) {
      protocol_ok = 0;
      if (protocol_error == 0) protocol_error = 99;
    }
  } else if (full_matrix) {
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
        if (parameter_formats[i] != 0 && parameter_formats[i] != 1) {
          protocol_ok = 0;
          if (protocol_error == 0) protocol_error = 30 + i;
        }
        if (parameter_values[i] == NULL) {
          if ((i & 1) == 0 || parameter_lengths[i] != 0) {
            protocol_ok = 0;
            if (protocol_error == 0) protocol_error = 50 + i;
          }
        } else {
          int value_ok = parameter_formats[i] == 0
              ? parameter_lengths[i] == (int)strlen(parameter_values[i])
              : full_binary_value_ok(i, parameter_values[i], parameter_lengths[i]);
          if (!value_ok) {
            protocol_ok = 0;
            if (protocol_error == 0) protocol_error = 70 + i;
          }
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
  if (dynamic_matrix || dynamic_zero) {
    static const unsigned char dynamic_bool[] = {1};
    static const unsigned char dynamic_i16[] = {0xff, 0xfe};
    static const unsigned char dynamic_i32[] = {0x01, 0x02, 0x03, 0x04};
    static const unsigned char dynamic_i64[] = {0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe};
    static const unsigned char dynamic_f32[] = {0x3f, 0xc0, 0x00, 0x00};
    static const unsigned char dynamic_f64[] = {0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};
    static const unsigned char dynamic_text[] = {0xc3, 0xa9};
    static const unsigned char dynamic_bytes[] = {0x00, 0xff};
    static const char *names[9] = {
        "nullv", "boolv", "i16v", "i32v", "i64v", "f32v", "f64v", "textv", "bytesv",
    };
    static const uint32_t oids[9] = {25, 16, 21, 23, 20, 700, 701, 25, 17};
    static const char *values[9] = {
        "", (const char *)dynamic_bool, (const char *)dynamic_i16,
        (const char *)dynamic_i32, (const char *)dynamic_i64,
        (const char *)dynamic_f32, (const char *)dynamic_f64,
        (const char *)dynamic_text, (const char *)dynamic_bytes,
    };
    static const int lengths[9] = {0, 1, 2, 4, 8, 4, 8, 2, 2};
    result->fields = 9;
    result->rows = dynamic_zero ? 0 : (has(command, "DYNAMIC_MANY_ROWS") ? 2 : 1);
    for (int i = 0; i < 9; i++) {
      result->names[i] = names[i];
      result->oids[i] = oids[i];
      result->formats[i] = 1;
      result->values[0][i] = values[i];
      result->lengths[0][i] = lengths[i];
      result->nulls[0][i] = i == 0;
      if (result->rows == 2) {
        result->values[1][i] = values[i];
        result->lengths[1][i] = lengths[i];
        result->nulls[1][i] = i == 0;
      }
    }
  } else if (dynamic_echo) {
    result->fields = 2;
    result->rows = 1;
    result->names[0] = "textv";
    result->names[1] = "bytesv";
    result->oids[0] = 25;
    result->oids[1] = 17;
    result->formats[0] = 1;
    result->formats[1] = 1;
    result->values[0][0] = dynamic_echo_value;
    result->values[0][1] = (const char *)dynamic_echo_bytes;
    result->lengths[0][0] = (int)strlen(dynamic_echo_value);
    result->lengths[0][1] = dynamic_echo_bytes_len;
  } else if (dynamic_empty) {
    static const char present_empty[] = "";
    result->fields = 3;
    result->rows = 1;
    result->names[0] = "textv";
    result->names[1] = "bytesv";
    result->names[2] = "nullv";
    result->oids[0] = 25;
    result->oids[1] = 17;
    result->oids[2] = 25;
    result->formats[0] = 1;
    result->formats[1] = 1;
    result->formats[2] = 1;
    result->values[0][0] = present_empty;
    result->values[0][1] = present_empty;
    result->values[0][2] = present_empty;
    result->lengths[0][0] = 0;
    result->lengths[0][1] = 0;
    result->lengths[0][2] = 0;
    result->nulls[0][2] = 1;
  } else if (dynamic_simple) {
    static const unsigned char dynamic_true[] = {1};
    result->fields = 1;
    result->rows = 1;
    result->names[0] = "value";
    result->oids[0] = 16;
    result->formats[0] = 1;
    result->values[0][0] = (const char *)dynamic_true;
    result->lengths[0][0] = 1;
    if (has(command, "DYNAMIC_ENCODING_BAD") && connection != NULL) connection->encoding = -1;
    if (has(command, "DYNAMIC_TX_DRIFT") && connection != NULL) connection->transaction_status = 2;
    if (has(command, "DYNAMIC_TX_IDLE") && connection != NULL) connection->transaction_status = 0;
    if (has(command, "DYNAMIC_NATIVE_ERROR")) {
      result->status = 7;
      result->sqlstate = "XX000";
      result->message = "dynamic native failure";
    }
    if (has(command, "DYNAMIC_UNSUPPORTED_OID")) result->oids[0] = 1700;
    if (has(command, "DYNAMIC_TEXT_FORMAT")) result->formats[0] = 0;
    if (has(command, "DYNAMIC_ZERO_COLUMNS")) result->fields = 0;
    if (has(command, "DYNAMIC_MANY_ROWS")) {
      result->rows = 2;
      result->values[1][0] = (const char *)dynamic_true;
      result->lengths[1][0] = 1;
    }
  } else if (format_matrix) {
    int index = format_matrix_direct_calls - 1;
    free(result);
    return format_matrix_result(index < 0 ? 0 : index, result_format);
  } else if (binary_fault_query) {
    static const unsigned char valid_flag[] = {1};
    static const unsigned char invalid_flag[] = {2};
    static const unsigned char invalid_utf8[] = {0xff};
    result->fields = 2;
    result->names[0] = "flag";
    result->names[1] = "text";
    result->oids[0] = 16;
    result->oids[1] = 25;
    result->formats[0] = 1;
    result->formats[1] = 1;
    result->values[0][0] = (const char *)valid_flag;
    result->values[0][1] = "valid";
    result->lengths[0][0] = 1;
    result->lengths[0][1] = 5;
    if (binary_fault == 1) result->formats[0] = 0;
    if (binary_fault == 2) result->lengths[0][0] = 2;
    if (binary_fault == 3) result->values[0][0] = (const char *)invalid_flag;
    if (binary_fault == 4) {
      result->values[0][1] = (const char *)invalid_utf8;
      result->lengths[0][1] = 1;
    }
    if (binary_fault == 5) result->row_fault = 4;
    if (binary_fault == 6) result->row_fault = 5;
    if (binary_fault == 7) {
      result->rows = 0;
      result->formats[1] = 0;
    }
    if (binary_fault == 8) result->oids[1] = 17;
    if (binary_fault == 9) result->names[1] = "other";
    binary_fault = 0;
  } else if (q6_user) {
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
      result->formats[i] = result_format;
      result->values[0][i] = parameter_values[i] == NULL ? "" : parameter_values[i];
      result->lengths[0][i] = parameter_values[i] == NULL ? 0 : parameter_lengths[i];
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
  if (has(command, "COMMAND_OK") || format_command || dynamic_command) {
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
  note_forbidden_after_status();
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
  async_connection = connection;
  stream_fatal_after_rows = has(command, "STREAM_FATAL_AFTER_ONE") ? 1 :
      (has(command, "STREAM_FATAL_AFTER_TWO") ? 2 : -1);
  stream_post_terminal_status = has(command, "POST_TERMINAL_FATAL") ? 7 :
      (has(command, "POST_TERMINAL_BAD") ? 5 : -1);
  stream_missing_terminal = has(command, "MISSING_TERMINAL");
  stream_initial_status = has(command, "STREAM_COPY") ? 3 :
      (has(command, "STREAM_COMMAND") ? 1 :
       (has(command, "STREAM_EMPTY") ? 0 : -1));
  async_pause_after_results = has(command, "TIMEOUT_AFTER_ONE") ? 1 : 0;
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
  note_forbidden_after_status();
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
  async_connection = connection;
  stream_fatal_after_rows = -1;
  stream_post_terminal_status = -1;
  stream_missing_terminal = 0;
  stream_initial_status = -1;
  async_pause_after_results = 0;
  return async_result == NULL ? 0 : 1;
}

static int select_stream_mode(int status, int chunk_size) {
  if (fail_next_row_mode) {
    fail_next_row_mode = 0;
    return 0;
  }
  FakeResult *source = async_result;
  if (source == NULL) return 0;
  async_result = NULL;
  async_queue_count = 0;
  async_queue_index = 0;
  if (stream_initial_status >= 0 || source->status != 2) {
    source->status = stream_initial_status >= 0 ? stream_initial_status : source->status;
    return enqueue_async(source);
  }

  int delivered = 0;
  int row_limit = source->rows;
  if (stream_fatal_after_rows >= 0 && stream_fatal_after_rows < row_limit) {
    row_limit = stream_fatal_after_rows;
  }
  while (delivered < row_limit) {
    int rows = status == 9 ? 1 : chunk_size;
    if (rows > row_limit - delivered) rows = row_limit - delivered;
    if (!enqueue_async(copy_result_rows(source, delivered, rows, status))) {
      free(source);
      clear_async_results();
      return 0;
    }
    delivered += rows;
  }
  if (stream_fatal_after_rows >= 0 && stream_fatal_after_rows <= source->rows) {
    if (!enqueue_async(stream_error_result(7, "40001", "streamed late failure"))) {
      free(source);
      clear_async_results();
      return 0;
    }
  } else if (!stream_missing_terminal) {
    if (!enqueue_async(copy_result_rows(source, 0, 0, 2))) {
      free(source);
      clear_async_results();
      return 0;
    }
  }
  if (stream_post_terminal_status >= 0) {
    if (!enqueue_async(stream_error_result(
            stream_post_terminal_status, "XX000", "post-terminal failure"))) {
      free(source);
      clear_async_results();
      return 0;
    }
  }
  free(source);
  return 1;
}

int PQsetSingleRowMode(FakeConn *connection) {
  (void)connection;
  note_forbidden_after_status();
  single_row_mode_calls++;
  return select_stream_mode(9, 1);
}

int PQsetChunkedRowsMode(FakeConn *connection, int chunk_size) {
  (void)connection;
  note_forbidden_after_status();
  chunked_row_mode_calls++;
  last_chunk_size = chunk_size;
  return chunk_size > 0 ? select_stream_mode(12, chunk_size) : 0;
}

int PQsetnonblocking(FakeConn *connection, int enabled) {
  (void)connection;
  note_forbidden_after_status();
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
  note_forbidden_after_status();
  return async_flush_fail ? -1 : 0;
}

int PQconsumeInput(FakeConn *connection) {
  (void)connection;
  note_forbidden_after_status();
  consume_calls++;
  if (async_consume_fail || (async_cancelled && async_drain_fail)) return 0;
  return 1;
}

int PQisBusy(FakeConn *connection) {
  (void)connection;
  note_forbidden_after_status();
  return async_busy;
}

FakeResult *PQgetResult(FakeConn *connection) {
  (void)connection;
  note_forbidden_after_status();
  if (async_busy) return NULL;
  if (async_queue_index < async_queue_count) {
    FakeResult *queued = async_queue[async_queue_index++];
    if (async_pause_after_results > 0 && async_queue_index == async_pause_after_results) {
      async_busy = 1;
    }
    if (queued != NULL && queued->status == 7 && async_connection != NULL &&
        async_connection->transaction_status == 2) {
      async_connection->transaction_status = 3;
    }
    return queued;
  }
  FakeResult *result = async_result;
  async_result = NULL;
  return result;
}

int PQtransactionStatus(FakeConn *connection) {
  note_forbidden_after_status();
  if (async_transaction_unknown) return 4;
  return connection == NULL ? 4 : connection->transaction_status;
}

static int perform_cancel(FakeConn *connection) {
  cancel_calls++;
  if (connection == NULL || async_cancel_fail) return 0;
  async_cancelled = 1;
  async_busy = async_drain_fail;
  if (connection->transaction_status == 2) connection->transaction_status = 3;
  if (async_result != NULL) {
    async_result->status = 7;
    async_result->sqlstate = "57014";
    async_result->message = "cancelled by deadline";
  } else if (async_queue_count > 0) {
    FakeResult *last = async_queue[async_queue_count - 1];
    if (last != NULL) {
      last->status = 7;
      last->sqlstate = "57014";
      last->message = "cancelled by deadline";
    }
  }
  return 1;
}

FakeCancel *PQcancelCreate(FakeConn *connection) {
  note_forbidden_after_status();
  if (async_cancel_resource_fail) return NULL;
  FakeCancel *cancel = (FakeCancel *)calloc(1, sizeof(FakeCancel));
  if (cancel != NULL) cancel->connection = connection;
  return cancel;
}

int PQcancelStart(FakeCancel *cancel) {
  note_forbidden_after_status();
  if (cancel == NULL || !perform_cancel(cancel->connection)) return 0;
  cancel->started = 1;
  return 1;
}

int PQcancelPoll(FakeCancel *cancel) {
  note_forbidden_after_status();
  return cancel != NULL && cancel->started ? 3 : 0;
}

int PQcancelSocket(const FakeCancel *cancel) {
  note_forbidden_after_status();
  return cancel == NULL ? -1 : 42;
}

int64_t PQgetCurrentTimeUSec(void) {
  struct timespec now;
  if (clock_gettime(CLOCK_REALTIME, &now) != 0) return -1;
  return (int64_t)now.tv_sec * 1000000 + now.tv_nsec / 1000;
}

int PQsocketPoll(int socket, int for_read, int for_write, int64_t end_time_us) {
  note_forbidden_after_status();
  cancel_socket_wait_calls++;
  return socket < 0 || (for_read == 0 && for_write == 0) || end_time_us < 0 ? -1 : 1;
}

char *PQcancelErrorMessage(const FakeCancel *cancel) {
  (void)cancel;
  return "stub cancel failure";
}

void PQcancelFinish(FakeCancel *cancel) { free(cancel); }

FakeCancel *PQgetCancel(FakeConn *connection) {
  note_forbidden_after_status();
  if (async_cancel_resource_fail) return NULL;
  FakeCancel *cancel = (FakeCancel *)malloc(sizeof(FakeCancel));
  if (cancel != NULL) cancel->connection = connection;
  return cancel;
}

int PQcancel(FakeCancel *cancel, char *error_buffer, int error_buffer_size) {
  (void)error_buffer;
  (void)error_buffer_size;
  note_forbidden_after_status();
  return cancel == NULL ? 0 : perform_cancel(cancel->connection);
}

void PQfreeCancel(FakeCancel *cancel) { free(cancel); }

int PQresultStatus(const FakeResult *result) {
  note_forbidden_after_status();
  int status = result == NULL ? 7 : (forced_result_status >= 0 ? forced_result_status : result->status);
  if (status_requires_close(status)) unsafe_status_seen = 1;
  return status;
}
char *PQcmdStatus(const FakeResult *result) {
  note_forbidden_after_status();
  return (char *)(result == NULL ? NULL : result->command_status);
}
int PQntuples(const FakeResult *result) {
  note_forbidden_after_status();
  return result == NULL ? -1 : result->rows;
}
int PQnfields(const FakeResult *result) {
  note_forbidden_after_status();
  return result == NULL ? -1 : result->fields;
}
char *PQfname(const FakeResult *result, int column) {
  note_forbidden_after_status();
  return (char *)(result == NULL || column < 0 || column >= result->fields ? NULL : result->names[column]);
}
uint32_t PQftype(const FakeResult *result, int column) {
  note_forbidden_after_status();
  return result == NULL || column < 0 || column >= result->fields ? 0 : result->oids[column];
}
int PQfformat(const FakeResult *result, int column) {
  note_forbidden_after_status();
  return result == NULL || column < 0 || column >= result->fields ? -1 : result->formats[column];
}
int PQgetisnull(const FakeResult *result, int row, int column) {
  note_forbidden_after_status();
  return result == NULL || row < 0 || row >= result->rows || column < 0 || column >= result->fields
      ? 1
      : result->nulls[row][column];
}
char *PQgetvalue(const FakeResult *result, int row, int column) {
  note_forbidden_after_status();
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
  note_forbidden_after_status();
  if (result != NULL && ((result->row_fault == 2 && column == 0) ||
                         (result->row_fault == 5 && column == 1))) {
    return -1;
  }
  char *value = PQgetvalue(result, row, column);
  if (value == NULL) return 0;
  return result->lengths[row][column] >= 0
      ? result->lengths[row][column]
      : (int)strlen(value);
}
char *PQcmdTuples(const FakeResult *result) {
  note_forbidden_after_status();
  return (char *)(result == NULL ? NULL : result->affected);
}

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
