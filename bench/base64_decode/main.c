#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
typedef struct { const unsigned char *ptr; int64_t len; } View;
typedef int32_t (*Decode)(const unsigned char *, int64_t, void **);
extern int32_t align_rt_base64_decode(const unsigned char *, int64_t, void **);
extern int32_t align_rt_base64url_decode(const unsigned char *, int64_t, void **);
extern int64_t align_rt_buffer_len(void *);
extern void align_rt_buffer_bytes(void *, View *);
extern void align_rt_buffer_free(void *);
static void require(int ok, const char *message) {
    if (!ok) { fprintf(stderr, "%s\n", message); exit(2); }
}
static uint64_t now(void) {
    struct timespec t;
    require(clock_gettime(CLOCK_MONOTONIC, &t) == 0, "clock");
    return (uint64_t)t.tv_sec * UINT64_C(1000000000) + (uint64_t)t.tv_nsec;
}
/* Independent encoder: the decoder under measurement never constructs its own oracle. */
static size_t encode(const unsigned char *raw, size_t n, int url, unsigned char *text) {
    const char *table = url ? "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
                            : "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    size_t written = 0; uint32_t bits = 0; unsigned count = 0;
    for (size_t i = 0; i < n; ++i) {
        bits = (bits << 8) | raw[i]; count += 8;
        while (count >= 6) { count -= 6; text[written++] = (unsigned char)table[(bits >> count) & 63]; }
    }
    if (count) text[written++] = (unsigned char)table[(bits << (6 - count)) & 63];
    if (!url) while (written % 4) text[written++] = '=';
    return written;
}
int main(void) {
    const size_t sizes[] = {0, 1, 2, 3, 8, 31, 32, 1024, 65536};
    const Decode decoders[] = {align_rt_base64_decode, align_rt_base64url_decode};
    /* Warm the allocator and CPU before the first short-input measurement. */
    for (size_t i = 0; i < 2000000; ++i) {
        void *output = NULL;
        require(align_rt_base64_decode((const unsigned char *)"Zm9v", 4, &output) == 0, "warmup");
        require(output && align_rt_buffer_len(output) == 3, "warmup output");
        align_rt_buffer_free(output);
    }
    puts("input_bytes,kind,case,trial,calls,output_bytes,ns_per_call");
    for (size_t size = 0; size < sizeof(sizes)/sizeof(sizes[0]); ++size) {
        const size_t n = sizes[size];
        unsigned char *raw = malloc(n + 1), *text = malloc((n + 2)/3*4 + 1);
        require(raw && text, "fixture allocation");
        for (size_t i = 0; i < n; ++i) raw[i] = (unsigned char)(i * 73 + 251);
        for (int kind = 0; kind < 2; ++kind) {
            for (int mode = 0; mode < (n >= 1024 ? 3 : 1); ++mode) {
                size_t length = encode(raw, n, kind, text);
                if (mode) text[mode == 1 ? 0 : length - 4] = '!';
                void *check = NULL; int status = decoders[kind](text, (int64_t)length, &check);
                require(status == (mode ? 2 : 0), "oracle status");
                if (mode) require(check == NULL, "invalid published output");
                else {
                    View view; require(check != NULL, "missing buffer");
                    align_rt_buffer_bytes(check, &view);
                    require(view.len == (int64_t)n, "oracle length");
                    if (n) require(view.ptr && memcmp(view.ptr, raw, n) == 0, "oracle bytes");
                }
                align_rt_buffer_free(check);
                size_t repeats = 2 * 1024 * 1024 / (length ? length : 1);
                if (repeats > 30000) repeats = 30000;
                if (repeats < 32) repeats = 32;
                if (mode == 1) repeats = 30000;
                if (mode == 2) repeats = 10000; /* Tail admission can also reject before the loop. */
                for (int trial = 0; trial < 7; ++trial) {
                    uint64_t calls = 0, bytes = 0, start = now();
                    for (size_t call = 0; call < repeats; ++call) {
                        void *output = NULL;
                        status = decoders[kind](text, (int64_t)length, &output);
                        require(status == (mode ? 2 : 0), "timed status");
                        if (mode) require(output == NULL, "timed invalid output");
                        else {
                            require(output != NULL, "timed missing output");
                            int64_t actual = align_rt_buffer_len(output);
                            require(actual == (int64_t)n, "timed length"); bytes += (uint64_t)actual;
                        }
                        ++calls; align_rt_buffer_free(output);
                    }
                    uint64_t elapsed = now() - start;
                    require(calls == repeats && bytes == (mode ? 0 : repeats*n), "work accounting");
                    printf("%zu,%d,%d,%d,%llu,%llu,%.2f\n", n, kind, mode, trial,
                           (unsigned long long)calls, (unsigned long long)bytes, (double)elapsed/calls);
                }
            }
        }
        free(raw); free(text);
    }
    return 0;
}
