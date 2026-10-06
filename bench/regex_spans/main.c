#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
typedef struct { int64_t start, end; } Span;
typedef struct { Span *ptr; int64_t len; } Output;
typedef int32_t (*Search)(void *, const unsigned char *, int64_t, Output *);
extern int32_t align_rt_regex_compile(const unsigned char *, int64_t, void **);
extern int32_t align_rt_regex_find_all(void *, const unsigned char *, int64_t, Output *);
extern int32_t align_rt_regex_split(void *, const unsigned char *, int64_t, Output *);
extern void align_rt_regex_free(void *);
extern void align_rt_free(void *);
static void require(int ok, const char *message) {
    if (!ok) { fprintf(stderr, "%s\n", message); exit(2); }
}
static uint64_t now(void) {
    struct timespec t;
    require(clock_gettime(CLOCK_MONOTONIC, &t) == 0, "clock");
    return (uint64_t)t.tv_sec * UINT64_C(1000000000) + (uint64_t)t.tv_nsec;
}
static int same(Span a, Span b) { return a.start == b.start && a.end == b.end; }
int main(int argc, char **argv) {
    long selected = -1;
    require(argc <= 2, "usage: probe [case-index 0..49]");
    if (argc == 2) {
        char *end = NULL;
        selected = strtol(argv[1], &end, 10);
        require(argv[1][0] && end && !*end && selected >= 0 && selected < 50, "case index must be 0..49");
    }
    const size_t sizes[] = {0, 1, 16, 4096, 65536};
    const char *patterns[] = {"z", "x", "x", "a+", ""};
    const int64_t lengths[] = {1, 1, 1, 2, 0};
    Search searches[] = {align_rt_regex_find_all, align_rt_regex_split};
    puts("input_bytes,kind,split,trial,calls,output_bytes,ns_per_call");
    for (size_t s = 0; s < sizeof(sizes)/sizeof(sizes[0]); ++s) {
        for (size_t kind = 0; kind < 5; ++kind) {
            if (selected >= 0 && (size_t)selected / 2 != s * 5 + kind) continue;
            size_t units = sizes[s], n = units * (kind == 4 ? 3 : 1), count = 0;
            unsigned char *text = malloc(n + 1);
            Span *matches = malloc((n + 2) * sizeof(Span)), *fields = malloc((n + 3) * sizeof(Span));
            require(text && matches && fields, "fixture allocation");
            /* Golden spans are built independently from this explicit corpus, never by regex. */
            for (size_t i = 0; i < n; ++i) {
                text[i] = kind == 2 || (kind == 1 && i % 127 == 0) ? 'x' : 'a';
                if (kind == 3) text[i] = i % 4 == 3 ? 'b' : 'a';
                if (kind == 4) { const unsigned char unit[] = {0xcf, 0x80, 'a'}; text[i] = unit[i % 3]; }
                if (text[i] == 'x') matches[count++] = (Span){(int64_t)i, (int64_t)i + 1};
            }
            if (kind == 3) for (size_t i = 0; i < n; i += 4)
                matches[count++] = (Span){(int64_t)i, (int64_t)(i + 3 < n ? i + 3 : n)};
            if (kind == 4) {
                for (size_t i = 0; i < n; i += 3) {
                    matches[count++] = (Span){(int64_t)i, (int64_t)i};
                    matches[count++] = (Span){(int64_t)i + 2, (int64_t)i + 2};
                }
                matches[count++] = (Span){(int64_t)n, (int64_t)n};
            }
            int64_t previous = 0;
            for (size_t i = 0; i < count; ++i) {
                fields[i] = (Span){previous, matches[i].start}; previous = matches[i].end;
            }
            fields[count] = (Span){previous, (int64_t)n};
            void *re = NULL;
            require(align_rt_regex_compile((const unsigned char *)patterns[kind], lengths[kind], &re) == 0 && re, "compile");
            for (int split = 0; split < 2; ++split) {
                if (selected >= 0 && selected % 2 != split) continue;
                Span *gold = split ? fields : matches; size_t expected = count + (size_t)split;
                /* Warm each independently runnable case for 200ms to settle short-case clocks. */
                uint64_t warm_start = now();
                do {
                    Output output = {0};
                    require(searches[split](re, text, (int64_t)n, &output) == 0, "oracle status");
                    require(output.len == (int64_t)expected && (!expected || output.ptr), "oracle header");
                    if (!expected) require(!output.ptr, "empty pointer");
                    for (size_t i = 0; i < expected; ++i) require(same(output.ptr[i], gold[i]), "oracle span");
                    align_rt_free(output.ptr);
                } while (now() - warm_start < UINT64_C(200000000));
                size_t repeats = 2097152 / (n + 1);
                if (repeats > 10000) repeats = 10000;
                if (repeats < 32) repeats = 32;
                if (expected <= 2 && repeats < 2000) repeats = 2000;
                for (int trial = 0; trial < 7; ++trial) {
                    uint64_t calls = 0, bytes = 0, start = now();
                    for (size_t i = 0; i < repeats; ++i) {
                        Output output = {0};
                        require(searches[split](re, text, (int64_t)n, &output) == 0, "timed status");
                        require(output.len == (int64_t)expected && (!expected || output.ptr), "timed header");
                        if (expected) require(same(output.ptr[0], gold[0]) && same(output.ptr[expected-1], gold[expected-1]), "timed endpoints");
                        else require(!output.ptr, "timed empty pointer");
                        bytes += (uint64_t)output.len * sizeof(Span); ++calls;
                        align_rt_free(output.ptr);
                    }
                    uint64_t elapsed = now() - start;
                    require(calls == repeats && bytes == repeats * expected * sizeof(Span), "work accounting");
                    printf("%zu,%zu,%d,%d,%llu,%llu,%.2f\n", n, kind, split, trial,
                        (unsigned long long)calls, (unsigned long long)bytes, (double)elapsed / calls);
                }
            }
            align_rt_regex_free(re); free(fields); free(matches); free(text);
        }
    }
    return 0;
}
