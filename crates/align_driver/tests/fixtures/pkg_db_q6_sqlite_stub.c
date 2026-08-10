#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
  int kind;
  int row;
  int rows;
  int64_t parameter;
  int64_t last_parameter;
} Q6Statement;

static int q6_prepare_calls;
static int q6_step_calls;
static int q6_delivered_rows;
static int q6_finalize_calls;
static int q6_protocol_ok;
static int q6_fail_next_bind;

void align_sqlite_q6_reset(void) {
  q6_prepare_calls = 0;
  q6_step_calls = 0;
  q6_delivered_rows = 0;
  q6_finalize_calls = 0;
  q6_protocol_ok = 1;
  q6_fail_next_bind = 0;
}

void align_sqlite_q6_fail_next_bind(void) { q6_fail_next_bind = 1; }

int align_sqlite_q6_prepare_calls(void) { return q6_prepare_calls; }
int align_sqlite_q6_step_calls(void) { return q6_step_calls; }
int align_sqlite_q6_delivered_rows(void) { return q6_delivered_rows; }
int align_sqlite_q6_finalize_calls(void) { return q6_finalize_calls; }
int align_sqlite_q6_protocol_ok(void) { return q6_protocol_ok; }

static int q6_rows(int kind, int64_t parameter, int64_t last_parameter) {
  if (kind == 2) return parameter == 0 ? 0 : 2;
  if (parameter == 1 && last_parameter == 3) return 4;
  switch (parameter) {
    case 0: return 0;
    case 1: return 2;
    case 2: return 1;
    case 3: return 1;
    case 4: return 1;
    case 5: return 2;
    case 6: return 2;
    case 7: return 3;
    case 8: return 10000;
    case 9: return 2;
    default: return 1;
  }
}

int sqlite3_prepare_v3(
    void *database,
    const char *sql,
    int bytes,
    unsigned int flags,
    void **statement_out,
    const char **tail_out) {
  q6_prepare_calls++;
  if (database == NULL || sql == NULL || bytes != -1 || flags != 3 ||
      statement_out == NULL || tail_out == NULL) {
    q6_protocol_ok = 0;
    return 1;
  }
  Q6Statement *statement = (Q6Statement *)calloc(1, sizeof(Q6Statement));
  if (statement == NULL) return 7;
  if (strstr(sql, "Q6_USER_GROUPS") != NULL) statement->kind = 1;
  else if (strstr(sql, "Q6_TRANSACTION_MASTER") != NULL) statement->kind = 2;
  else q6_protocol_ok = 0;
  *statement_out = statement;
  *tail_out = sql + strlen(sql);
  return statement->kind == 0 ? 1 : 0;
}

int sqlite3_prepare_v2(
    void *database,
    const char *sql,
    int bytes,
    void **statement_out,
    const char **tail_out) {
  return sqlite3_prepare_v3(database, sql, bytes, 3, statement_out, tail_out);
}

int sqlite3_bind_int64(void *raw_statement, int index, int64_t value) {
  Q6Statement *statement = (Q6Statement *)raw_statement;
  if (statement == NULL || index < 1 || index > (statement->kind == 1 ? 2 : 1)) {
    q6_protocol_ok = 0;
    return 1;
  }
  if (q6_fail_next_bind) {
    q6_fail_next_bind = 0;
    return 7;
  }
  if (index == 1) statement->parameter = value;
  else statement->last_parameter = value;
  statement->rows = q6_rows(
      statement->kind,
      statement->parameter,
      statement->kind == 1 ? statement->last_parameter : statement->parameter);
  return 0;
}

int sqlite3_step(void *raw_statement) {
  Q6Statement *statement = (Q6Statement *)raw_statement;
  q6_step_calls++;
  if (statement == NULL) {
    q6_protocol_ok = 0;
    return 1;
  }
  if (statement->row < statement->rows) {
    statement->row++;
    q6_delivered_rows++;
    return 100;
  }
  return 101;
}

int sqlite3_column_count(void *raw_statement) {
  Q6Statement *statement = (Q6Statement *)raw_statement;
  if (statement == NULL) return -1;
  return statement->kind == 1 ? 4 : 8;
}

const char *sqlite3_column_name(void *raw_statement, int column) {
  static const char *user_names[] = {
      "user_id", "user_name", "group_id", "group_name",
  };
  static const char *transaction_names[] = {
      "transaction_id", "posted_at", "amount_cents", "status_id",
      "status_code", "status_name", "customer_id", "customer_name",
  };
  Q6Statement *statement = (Q6Statement *)raw_statement;
  if (statement == NULL || column < 0) return NULL;
  if (statement->kind == 1) return column < 4 ? user_names[column] : NULL;
  return column < 8 ? transaction_names[column] : NULL;
}

static int q6_user_id(const Q6Statement *statement) {
  int row = statement->row - 1;
  if (statement->parameter == 1 && statement->last_parameter == 3)
    return row < 2 ? 1 : row == 2 ? 2 : 3;
  if (statement->parameter == 6) return row == 0 ? 6 : 7;
  if (statement->parameter == 7) return row == 0 || row == 2 ? 1 : 2;
  return statement->parameter == 8 ? 1 : (int)statement->parameter;
}

static int q6_child_present(const Q6Statement *statement, int column) {
  int row = statement->row - 1;
  if (statement->parameter == 2) return 0;
  if (statement->parameter == 3) return column == 2;
  if (statement->parameter == 4) return column == 3;
  if (statement->parameter == 9) return row == 0 ? column == 2 : column == 3;
  if (statement->parameter == 1 && statement->last_parameter == 3 && row == 2) return 0;
  return 1;
}

int sqlite3_column_type(void *raw_statement, int column) {
  Q6Statement *statement = (Q6Statement *)raw_statement;
  if (statement == NULL) return 5;
  if (statement->kind == 1) {
    if ((column == 2 || column == 3) && !q6_child_present(statement, column)) return 5;
    return column == 0 || column == 2 ? 1 : 3;
  }
  return column == 0 || column == 2 || column == 3 || column == 6 ? 1 : 3;
}

int64_t sqlite3_column_int64(void *raw_statement, int column) {
  Q6Statement *statement = (Q6Statement *)raw_statement;
  if (statement == NULL) return 0;
  int row = statement->row - 1;
  if (statement->kind == 1) {
    if (column == 0) return q6_user_id(statement);
    if (column == 2) return statement->parameter == 8 ? 1000 + row : 10 * (row + 1);
  } else {
    if (column == 0) return 100 + row;
    if (column == 2) return 5000 + row;
    if (column == 3) return 7;
    if (column == 6) return 900 + row;
  }
  q6_protocol_ok = 0;
  return 0;
}

const unsigned char *sqlite3_column_text(void *raw_statement, int column) {
  Q6Statement *statement = (Q6Statement *)raw_statement;
  if (statement == NULL) return NULL;
  int row = statement->row - 1;
  if (statement->kind == 1) {
    if (column == 1) {
      if (statement->parameter == 5 && row == 1) return (const unsigned char *)"Changed";
      int user = q6_user_id(statement);
      return (const unsigned char *)(user == 1 ? "Alice" : user == 2 ? "Bob" : "Cara");
    }
    if (column == 3 && q6_child_present(statement, column)) {
      if (statement->parameter == 8) return (const unsigned char *)"Fan";
      return (const unsigned char *)(row == 0 ? "Admin" : row == 1 ? "Dev" : "Ops");
    }
  } else {
    if (column == 1) return (const unsigned char *)(row == 0 ? "2026-08-10" : "2026-08-09");
    if (column == 4) return (const unsigned char *)"posted";
    if (column == 5) return (const unsigned char *)"Posted";
    if (column == 7) return (const unsigned char *)(row == 0 ? "Ada" : "Linus");
  }
  return NULL;
}

int sqlite3_column_bytes(void *raw_statement, int column) {
  const unsigned char *value = sqlite3_column_text(raw_statement, column);
  return value == NULL ? 0 : (int)strlen((const char *)value);
}

const void *sqlite3_column_blob(void *statement, int column) {
  (void)statement;
  (void)column;
  return NULL;
}

int sqlite3_reset(void *raw_statement) {
  Q6Statement *statement = (Q6Statement *)raw_statement;
  if (statement == NULL) return 1;
  statement->row = 0;
  return 0;
}

int sqlite3_clear_bindings(void *raw_statement) {
  return raw_statement == NULL ? 1 : 0;
}

int sqlite3_finalize(void *raw_statement) {
  q6_finalize_calls++;
  if (raw_statement == NULL) {
    q6_protocol_ok = 0;
    return 1;
  }
  free(raw_statement);
  return 0;
}
