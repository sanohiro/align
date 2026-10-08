#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct { const unsigned char *ptr; int64_t len; } View;
typedef int32_t (*Decode)(const unsigned char *, int64_t, void **);
extern int32_t align_rt_percent_decode(const unsigned char *, int64_t, void **);
extern int32_t align_rt_form_decode(const unsigned char *, int64_t, void **);
extern void align_rt_buffer_bytes(void *, View *);
extern int64_t align_rt_buffer_capacity(void *);
extern void align_rt_buffer_free(void *);

static void require(int condition, const char *message) {
    if (!condition) { fprintf(stderr, "%s\n", message); exit(2); }
}
static uint64_t now(void) {
    struct timespec value;
    require(clock_gettime(CLOCK_MONOTONIC, &value) == 0, "clock");
    return (uint64_t)value.tv_sec * UINT64_C(1000000000) + (uint64_t)value.tv_nsec;
}
static unsigned char digit(unsigned char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    require(c >= 'A' && c <= 'F', "invalid fixture hex");
    return c - 'A' + 10;
}
static size_t expected(const unsigned char *input, size_t length, int form, unsigned char *output) {
    size_t written = 0;
    for (size_t i = 0; i < length; ++i) {
        unsigned char c = input[i];
        if (c == '%') {
            require(length - i >= 3, "incomplete fixture escape");
            c = digit(input[i + 1]) * 16 + digit(input[i + 2]);
            i += 2;
        } else if (form && c == '+') c = ' ';
        output[written++] = c;
    }
    return written;
}
int main(void) {
    const char *seeds[] = {"Alpha09-file_name/path~segment", "ordinary_query_value_with_long_runs%20next_value+tail", "%00%2B%ff%7F", "++++++++", "a%20b+c%2B"};
    const char *names[] = {"plain", "sparse", "dense", "plus", "mixed"};
    size_t sizes[] = {0, 8, 64, 4096, 65536};
    Decode decoders[] = {align_rt_percent_decode, align_rt_form_decode};
    puts("input,input_bytes,form,trial,calls,output_bytes,ns_per_call");
    for (int seed = 0; seed < 5; ++seed) {
        size_t unit = strlen(seeds[seed]);
        for (int size = 0; size < 5; ++size) {
            if (size == 0 && seed != 0) continue;
            size_t length = ((sizes[size] + unit - 1) / unit) * unit;
            unsigned char *input = malloc(length + 1), *oracle = malloc(length + 1);
            require(input && oracle, "fixture allocation");
            for (size_t i = 0; i < length; i += unit) memcpy(input + i, seeds[seed], unit);
            size_t repeats = 2 * 1024 * 1024 / (length + 64);
            if (repeats < 64) repeats = 64;
            for (int form = 0; form < 2; ++form) {
                size_t output_length = expected(input, length, form, oracle);
                uint64_t warm_start = now();
                do {
                    void *output = NULL;
                    require(decoders[form](input, (int64_t)length, &output) == 0 && output, "warm status");
                    align_rt_buffer_free(output);
                } while (now() - warm_start < UINT64_C(100000000));
                for (int trial = 0; trial < 7; ++trial) {
                    void *check = NULL;
                    require(decoders[form](input, (int64_t)length, &check) == 0 && check, "decode status");
                    View view = {0};
                    align_rt_buffer_bytes(check, &view);
                    require(view.len == (int64_t)output_length && (view.ptr || !view.len), "producer extent");
                    require(!output_length || memcmp(view.ptr, oracle, output_length) == 0, "oracle mismatch");
                    require(align_rt_buffer_capacity(check) == (int64_t)output_length, "read capacity");
                    align_rt_buffer_free(check);
                    uint64_t bytes = 0, calls = 0, start = now();
                    for (size_t call = 0; call < repeats; ++call) {
                        void *output = NULL;
                        require(decoders[form](input, (int64_t)length, &output) == 0 && output, "timed status");
                        View value = {0};
                        align_rt_buffer_bytes(output, &value);
                        require(value.len == (int64_t)output_length, "timed extent");
                        bytes += (uint64_t)value.len; ++calls;
                        align_rt_buffer_free(output);
                    }
                    uint64_t elapsed = now() - start;
                    require(calls == repeats && bytes == repeats * output_length, "work accounting");
                    printf("%s,%zu,%d,%d,%llu,%llu,%.1f\n", names[seed], length, form, trial,
                        (unsigned long long)calls, (unsigned long long)bytes, (double)elapsed / calls);
                }
            }
            free(input); free(oracle);
        }
    }
    return 0;
}
