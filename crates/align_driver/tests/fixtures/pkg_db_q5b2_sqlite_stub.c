#include <stdint.h>
#include <string.h>

static int fake_database;
static int fake_statement;
static int prepare_calls;
static int step_calls;
static int finalize_calls;
static int sqlite_key_query;
static int fake_postgres;
static int fake_result;
static int pq_exec_calls;
static int pq_clear_calls;
static int postgres_key_query;

void *align_q5b2_fake_sqlite(void) { return &fake_database; }
int32_t align_q5b2_finalize_calls(void) { return finalize_calls; }
void *align_q5b2_fake_postgres(void) { return &fake_postgres; }
int32_t align_q5b2_clear_calls(void) { return pq_clear_calls; }

int32_t sqlite3_prepare_v2(
    void *database,
    const char *sql,
    int32_t bytes,
    void **statement_out,
    const char **tail_out) {
  (void)database;
  (void)sql;
  (void)bytes;
  (void)tail_out;
  prepare_calls += 1;
  step_calls = 0;
  sqlite_key_query = strstr(sql, "WITH pk_terms") != 0;
  *statement_out = &fake_statement;
  return 0;
}

int32_t sqlite3_step(void *statement) {
  (void)statement;
  if (prepare_calls == 1 && step_calls == 0) {
    step_calls += 1;
    return 100;
  }
  if (sqlite_key_query && step_calls < 2) {
    step_calls += 1;
    return 100;
  }
  return 101;
}

int32_t sqlite3_column_count(void *statement) {
  (void)statement;
  return sqlite_key_query ? 13 : 2;
}

int32_t sqlite3_bind_text(
    void *statement,
    int32_t index,
    const char *value,
    int32_t bytes,
    void *destructor) {
  (void)statement;
  (void)index;
  (void)value;
  (void)bytes;
  (void)destructor;
  return 0;
}

int32_t sqlite3_column_type(void *statement, int32_t column) {
  (void)statement;
  return column == 0 || column == 2 || column == 3 || column == 12 ? 1 : 5;
}

int64_t sqlite3_column_int64(void *statement, int32_t column) {
  (void)statement;
  if (column == 3) return step_calls == 1 ? 0 : 2;
  if (column == 12) return 1;
  return 0;
}

int32_t sqlite3_finalize(void *statement) {
  (void)statement;
  finalize_calls += 1;
  return 0;
}

int32_t sqlite3_close_v2(void *database) {
  (void)database;
  return 0;
}

int32_t sqlite3_get_autocommit(void *database) { return database == NULL ? 0 : 1; }

void *PQexecParams(
    void *connection,
    const char *command,
    int32_t parameter_count,
    const uint32_t *parameter_types,
    const char *const *parameter_values,
    const int32_t *parameter_lengths,
    const int32_t *parameter_formats,
    int32_t result_format) {
  (void)connection;
  (void)command;
  (void)parameter_count;
  (void)parameter_types;
  (void)parameter_values;
  (void)parameter_lengths;
  (void)parameter_formats;
  (void)result_format;
  pq_exec_calls += 1;
  postgres_key_query = strstr(command, "WITH constraints") != 0;
  return &fake_result;
}
int32_t PQsendQueryParams(
    void *connection,
    const char *command,
    int32_t parameter_count,
    const uint32_t *parameter_types,
    const char *const *parameter_values,
    const int32_t *parameter_lengths,
    const int32_t *parameter_formats,
    int32_t result_format) {
  (void)connection;
  (void)command;
  (void)parameter_count;
  (void)parameter_types;
  (void)parameter_values;
  (void)parameter_lengths;
  (void)parameter_formats;
  (void)result_format;
  return 0;
}
int32_t PQclientEncoding(void *connection) {
  (void)connection;
  return 6;
}
void *PQexec(void *connection, const char *command) {
  (void)connection;
  (void)command;
  return &fake_result;
}
int32_t PQresultStatus(void *result) { (void)result; return 2; }
char *PQcmdStatus(void *result) { (void)result; return "ROLLBACK"; }
int32_t PQntuples(void *result) {
  (void)result;
  if (pq_exec_calls == 1) return 1;
  if (postgres_key_query) return 2;
  return 0;
}
int32_t PQnfields(void *result) {
  (void)result;
  if (pq_exec_calls == 1) return 3;
  if (postgres_key_query) return 15;
  return 4;
}
char *PQfname(void *result, int32_t column) {
  (void)result; (void)column; return "";
}
uint32_t PQftype(void *result, int32_t column) {
  (void)result; (void)column; return 0;
}
int32_t PQfformat(void *result, int32_t column) {
  (void)result; (void)column; return 0;
}
int32_t PQgetisnull(void *result, int32_t row, int32_t column) {
  (void)result; (void)row;
  if (postgres_key_query && (column == 0 || column == 2 || column == 3)) return 0;
  return 1;
}
char *PQgetvalue(void *result, int32_t row, int32_t column) {
  (void)result;
  static char zero[] = "0";
  static char two[] = "2";
  if (postgres_key_query && column == 3 && row == 1) return two;
  return zero;
}
int32_t PQgetlength(void *result, int32_t row, int32_t column) {
  (void)result; (void)row; (void)column;
  return postgres_key_query ? 1 : 0;
}
char *PQcmdTuples(void *result) { (void)result; return 0; }
char *PQerrorMessage(void *connection) { (void)connection; return 0; }
char *PQresultErrorField(void *result, int32_t field) {
  (void)result; (void)field; return 0;
}
void PQclear(void *result) { (void)result; pq_clear_calls += 1; }
void PQfinish(void *connection) { (void)connection; }
int32_t PQsetnonblocking(void *connection, int32_t enabled) {
  (void)connection; (void)enabled; return 0;
}
int32_t PQflush(void *connection) { (void)connection; return 0; }
int32_t PQconsumeInput(void *connection) { (void)connection; return 1; }
int32_t PQisBusy(void *connection) { (void)connection; return 0; }
void *PQgetResult(void *connection) { (void)connection; return 0; }
int32_t PQtransactionStatus(void *connection) { (void)connection; return 0; }
void *PQcancelCreate(void *connection) { (void)connection; return &fake_postgres; }
int32_t PQcancelStart(void *cancel) { (void)cancel; return 1; }
int32_t PQcancelPoll(void *cancel) { (void)cancel; return 3; }
int32_t PQcancelSocket(const void *cancel) { (void)cancel; return 0; }
char *PQcancelErrorMessage(const void *cancel) { (void)cancel; return 0; }
void PQcancelFinish(void *cancel) { (void)cancel; }
int32_t PQsocketPoll(
    int32_t socket,
    int32_t for_read,
    int32_t for_write,
    int64_t end_time_us) {
  (void)socket; (void)for_read; (void)for_write; (void)end_time_us; return 1;
}
int64_t PQgetCurrentTimeUSec(void) { return 0; }
