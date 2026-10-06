#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct { const unsigned char *ptr; int64_t len; } View;
typedef View (*Encode)(const unsigned char *, int64_t);
extern View align_rt_percent_encode(const unsigned char *, int64_t);
extern View align_rt_percent_encode_path(const unsigned char *, int64_t);
extern View align_rt_html_escape(const unsigned char *, int64_t);
extern View align_rt_form_encode(const unsigned char *, int64_t);
extern void align_rt_free(const unsigned char *);

static void require(int condition, const char *message) {
    if (!condition) { fprintf(stderr, "%s\n", message); exit(2); }
}
static uint64_t now(void) {
    struct timespec value;
    require(clock_gettime(CLOCK_MONOTONIC, &value) == 0, "clock");
    return (uint64_t)value.tv_sec * UINT64_C(1000000000) + (uint64_t)value.tv_nsec;
}
static int unreserved(unsigned char c) {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
        (c >= '0' && c <= '9') || c == '-' || c == '.' || c == '_' || c == '~';
}
static size_t expected(const unsigned char *input, size_t length, int kind, unsigned char *output) {
    size_t written = 0;
    const char *hex = "0123456789ABCDEF";
    for (size_t i = 0; i < length; ++i) {
        unsigned char c = input[i];
        if (kind == 2) {
            const char *entity = NULL;
            switch (c) {
                case '&': entity = "&amp;"; break;
                case '<': entity = "&lt;"; break;
                case '>': entity = "&gt;"; break;
                case '"': entity = "&quot;"; break;
                case '\'': entity = "&#39;"; break;
                default: break;
            }
            if (entity) { size_t n = strlen(entity); memcpy(output + written, entity, n); written += n; }
            else { output[written++] = c; }
        } else if (unreserved(c) || (kind == 1 && c == '/')) {
            output[written++] = c;
        } else if (kind == 3 && c == ' ') {
            output[written++] = '+';
        } else {
            output[written++] = '%'; output[written++] = hex[c >> 4]; output[written++] = hex[c & 15];
        }
    }
    return written;
}
int main(void) {
    const char *seeds[] = {"Alpha09-file_name/path~segment", "<item key=\"日本語\">two words & a/b?</item>'"};
    const char *names[] = {"plain", "mixed"};
    size_t sizes[] = {256, 4096, 65536};
    Encode encoders[] = {align_rt_percent_encode, align_rt_percent_encode_path, align_rt_html_escape, align_rt_form_encode};
    puts("input,input_bytes,kind,trial,calls,output_bytes,ns_per_call");
    for (int seed = 0; seed < 2; ++seed) {
        size_t unit = strlen(seeds[seed]);
        for (int size = 0; size < 3; ++size) {
            size_t length = ((sizes[size] + unit - 1) / unit) * unit;
            unsigned char *input = malloc(length), *oracle = malloc(length * 6);
            require(input && oracle, "fixture allocation");
            for (size_t i = 0; i < length; i += unit) memcpy(input + i, seeds[seed], unit);
            size_t repeats = 4 * 1024 * 1024 / length;
            if (repeats < 64) repeats = 64;
            for (int kind = 0; kind < 4; ++kind) {
                size_t output_length = expected(input, length, kind, oracle);
                View check = encoders[kind](input, (int64_t)length);
                require(check.len == (int64_t)output_length && check.ptr != NULL, "producer extent");
                require(memcmp(check.ptr, oracle, output_length) == 0, "oracle mismatch");
                align_rt_free(check.ptr);
                for (int trial = 0; trial < 9; ++trial) {
                    uint64_t bytes = 0, calls = 0, start = now();
                    for (size_t call = 0; call < repeats; ++call) {
                        View output = encoders[kind](input, (int64_t)length);
                        require(output.len == (int64_t)output_length && output.ptr != NULL, "timed producer extent");
                        bytes += (uint64_t)output.len; ++calls;
                        align_rt_free(output.ptr);
                    }
                    uint64_t elapsed = now() - start;
                    require(calls == repeats && bytes == repeats * output_length, "work accounting");
                    printf("%s,%zu,%d,%d,%llu,%llu,%.1f\n", names[seed], length, kind, trial,
                        (unsigned long long)calls, (unsigned long long)bytes, (double)elapsed / calls);
                }
            }
            free(input); free(oracle);
        }
    }
    return 0;
}
