#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
typedef struct { const unsigned char *ptr; int64_t len; } View;
extern int32_t align_rt_hex_decode(const unsigned char *, int64_t, void **);
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
int main(int argc, char **argv) {
    long selected = -1;
    require(argc <= 2, "usage: probe [case-index 0..107]");
    if (argc == 2) {
        char *end = NULL; selected = strtol(argv[1], &end, 10);
        require(argv[1][0] && end && !*end && selected >= 0 && selected < 108, "case index must be 0..107");
    }
    const size_t sizes[] = {0, 1, 2, 3, 4, 8, 32, 1024, 65536};
    puts("output_bytes,alphabet,case,trial,calls,observed_bytes,ns_per_call");
    for (size_t s = 0; s < sizeof(sizes)/sizeof(sizes[0]); ++s) {
        const size_t n = sizes[s];
        for (int alphabet = 0; alphabet < 3; ++alphabet) {
            for (int mode = 0; mode < 4; ++mode) {
                size_t ordinal = (s * 3 + (size_t)alphabet) * 4 + (size_t)mode;
                if (selected >= 0 && (size_t)selected != ordinal) continue;
                unsigned char *raw = malloc(n + 1), *input = malloc(n * 2 + 3);
                require(raw && input, "fixture allocation");
                /* Independent encoder; neither decoder implementation supplies its oracle. */
                for (size_t i = 0; i < n; ++i) {
                    raw[i] = (unsigned char)(i * 73 + 251);
                    const char *table = alphabet == 1 || (alphabet == 2 && i % 2)
                        ? "0123456789ABCDEF" : "0123456789abcdef";
                    input[2*i] = (unsigned char)table[raw[i] >> 4];
                    input[2*i+1] = (unsigned char)table[raw[i] & 15];
                }
                size_t length = n * 2;
                if (mode == 1 || mode == 2) {
                    if (!length) { input[0] = input[1] = '0'; length = 2; }
                    input[mode == 1 ? 0 : length - 1] = '!';
                } else if (mode == 3) input[length++] = '0';
                uint64_t warm_start = now();
                do {
                    void *output = NULL;
                    int status = align_rt_hex_decode(input, (int64_t)length, &output);
                    require(status == (mode ? 2 : 0), "oracle status");
                    if (mode) require(output == NULL, "invalid output");
                    else {
                        View view; require(output != NULL, "missing Buffer");
                        align_rt_buffer_bytes(output, &view);
                        require(view.len == (int64_t)n, "oracle length");
                        if (n) require(view.ptr && memcmp(view.ptr, raw, n) == 0, "oracle bytes");
                    }
                    align_rt_buffer_free(output);
                } while (now() - warm_start < UINT64_C(200000000));
                size_t repeats = 33554432 / (length + 1);
                if (repeats < 64) repeats = 64;
                if (repeats > 2000000 || mode == 1 || mode == 3) repeats = 2000000;
                for (int trial = 0; trial < 7; ++trial) {
                    uint64_t calls = 0, bytes = 0, start = now();
                    for (size_t i = 0; i < repeats; ++i) {
                        void *output = NULL;
                        int status = align_rt_hex_decode(input, (int64_t)length, &output);
                        require(status == (mode ? 2 : 0), "timed status");
                        if (mode) require(output == NULL, "timed invalid output");
                        else {
                            require(output != NULL, "timed missing Buffer");
                            int64_t actual = align_rt_buffer_len(output);
                            require(actual == (int64_t)n, "timed length"); bytes += (uint64_t)actual;
                        }
                        ++calls; align_rt_buffer_free(output);
                    }
                    uint64_t elapsed = now() - start;
                    require(calls == repeats && bytes == (mode ? 0 : repeats * n), "work accounting");
                    printf("%zu,%d,%d,%d,%llu,%llu,%.2f\n", n, alphabet, mode, trial,
                        (unsigned long long)calls, (unsigned long long)bytes, (double)elapsed/calls);
                }
                free(raw); free(input);
            }
        }
    }
    return 0;
}
