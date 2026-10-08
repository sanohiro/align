#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct { const unsigned char *ptr; int64_t len; } View;
typedef struct {
    const unsigned char *name;
    int64_t length;
    uint64_t hash;
    int32_t tag, reserved;
} Field;
extern uint64_t align_rt_hash64(const unsigned char *, int64_t);
extern void *align_rt_arena_begin(void);
extern void align_rt_arena_end(void *);
extern int32_t align_rt_csv_decode_soa_v1(const unsigned char *, int64_t,
    const Field *, int64_t, void *, int32_t, int32_t, int64_t, View *);

#ifndef CSV_SELECTED_TEXT
#define CSV_SELECTED_TEXT 0
#endif
static Field fields[CSV_SELECTED_TEXT ? 3 : 2];
static unsigned char *expected_text;
static size_t expected_length;
static void require(int condition, const char *message) {
    if (!condition) { fprintf(stderr, "%s\n", message); exit(2); }
}
static uint64_t now(void) {
    struct timespec value;
    require(clock_gettime(CLOCK_MONOTONIC, &value) == 0, "clock");
    return (uint64_t)value.tv_sec * UINT64_C(1000000000) + (uint64_t)value.tv_nsec;
}
static void check_row(View output, size_t row, int full_check) {
    size_t score_start = ((size_t)output.len + 3) & ~(size_t)3;
    int32_t score;
    memcpy(&score, output.ptr + score_start + row * sizeof(score), sizeof(score));
    require(output.ptr[row] == 1 && score == 7, "decoded column values");
    if (CSV_SELECTED_TEXT) {
        size_t text_start = (score_start + (size_t)output.len * sizeof(score) + 15) & ~(size_t)15;
        View text;
        memcpy(&text, output.ptr + text_start + row * sizeof(text), sizeof(text));
        require(text.len == (int64_t)expected_length, "decoded text length");
        if (expected_length) {
            require(text.ptr && text.ptr[0] == expected_text[0] &&
                    text.ptr[expected_length - 1] == expected_text[expected_length - 1], "text edges");
            if (full_check) require(memcmp(text.ptr, expected_text, expected_length) == 0, "text bytes");
        }
    }
}
static void one(const unsigned char *input, size_t length, size_t rows, int status,
                int full_check, uint64_t *calls, uint64_t *observed_rows) {
    void *arena = align_rt_arena_begin();
    require(arena != NULL, "arena");
    View output = {NULL, -1};
    int32_t result = align_rt_csv_decode_soa_v1(input, (int64_t)length,
        fields, CSV_SELECTED_TEXT ? 3 : 2, arena, 0, 1, (int64_t)rows, &output);
    require(result == status, "decode status");
    if (result == 0) {
        require(output.ptr && output.len == (int64_t)rows, "output extent");
        if (full_check) {
            for (size_t row = 0; row < rows; ++row) check_row(output, row, 1);
        } else {
            check_row(output, 0, 0);
            check_row(output, rows - 1, 0);
        }
    } else {
        require(output.ptr == NULL && output.len == 0, "refused output");
    }
    ++*calls;
    *observed_rows += (uint64_t)output.len;
    align_rt_arena_end(arena);
}
static void measure(size_t kind, size_t body, size_t rows) {
    static const char *names[] = {"plain", "quoted", "escaped", "dense_quotes", "unterminated"};
    const char *header = "active,score,note\n", *prefix = "true,7,";
    size_t capacity = strlen(header) + rows * (strlen(prefix) + 2 * body + 4);
    unsigned char *input = malloc(capacity);
    require(input != NULL, "fixture allocation");
    if (CSV_SELECTED_TEXT) {
        expected_length = body;
        expected_text = malloc(body ? body : 1);
        require(expected_text != NULL, "expected text allocation");
        for (size_t index = 0; index < body; ++index)
            expected_text[index] = kind == 3 || (kind == 2 && index % 32 == 31) ? '"' : 'x';
    }
    size_t at = strlen(header);
    memcpy(input, header, at);
    for (size_t row = 0; row < rows; ++row) {
        memcpy(input + at, prefix, strlen(prefix)); at += strlen(prefix);
        if (kind != 0) input[at++] = '"';
        for (size_t index = 0; index < body; ++index) {
            if (kind == 3 || (kind == 2 && index % 32 == 31)) {
                input[at++] = '"'; input[at++] = '"';
            } else {
                input[at++] = 'x';
            }
        }
        if (kind != 0 && kind != 4) input[at++] = '"';
        input[at++] = '\n';
    }
    require(at <= capacity, "fixture extent");
    int status = kind == 4 ? 1 : 0;
    uint64_t calls = 0, observed_rows = 0;
    one(input, at, rows, status, 1, &calls, &observed_rows);
    calls = observed_rows = 0;
    uint64_t warm = now();
    do { one(input, at, rows, status, 0, &calls, &observed_rows); }
    while (now() - warm < UINT64_C(50000000));
    require(calls <= SIZE_MAX, "warm call count");
    size_t repeats = (size_t)calls;
    if (repeats < 16) repeats = 16;
    for (size_t trial = 0; trial < 7; ++trial) {
        calls = observed_rows = 0;
        uint64_t start = now();
        for (size_t call = 0; call < repeats; ++call)
            one(input, at, rows, status, 0, &calls, &observed_rows);
        uint64_t elapsed = now() - start;
        require(calls == repeats && observed_rows == calls * (status ? 0 : rows), "observed work");
        printf("%s,%zu,%zu,%zu,%zu,%llu,%llu,%.2f\n", names[kind], body, rows, at, trial,
            (unsigned long long)calls, (unsigned long long)observed_rows, (double)elapsed / calls);
    }
    free(expected_text);
    free(input);
}
int main(void) {
    const char *names[] = {"active", "score", "note"};
    int32_t tags[] = {0x0101, 0x10004, 0x0310};
    for (size_t i = 0; i < (CSV_SELECTED_TEXT ? 3 : 2); ++i) {
        fields[i] = (Field){(const unsigned char *)names[i], (int64_t)strlen(names[i]), 0, tags[i], 0};
        fields[i].hash = align_rt_hash64(fields[i].name, fields[i].length);
    }
    puts("pattern,body_bytes,rows,input_bytes,trial,calls,output_rows,ns_per_call");
    size_t sizes[] = {0, 8, 16, 64, 4096, 65536};
    for (size_t kind = 0; kind < 5; ++kind)
        for (size_t size = 0; size < 6; ++size) measure(kind, sizes[size], 1);
    measure(1, 8, 128);
    return 0;
}
