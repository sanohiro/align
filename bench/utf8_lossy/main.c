#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct { const unsigned char *ptr; int64_t len; } View;
extern View align_rt_utf8_decode_lossy(const unsigned char *, int64_t);
extern void align_rt_free(const unsigned char *);

static void require(int condition, const char *message) {
    if (!condition) { fprintf(stderr, "%s\n", message); exit(2); }
}
static uint64_t now(void) {
    struct timespec value;
    require(clock_gettime(CLOCK_MONOTONIC, &value) == 0, "clock");
    return (uint64_t)value.tv_sec * UINT64_C(1000000000) + (uint64_t)value.tv_nsec;
}
static void one(const unsigned char *input, size_t length, size_t expected,
                uint64_t *calls, uint64_t *bytes) {
    View output = align_rt_utf8_decode_lossy(input, (int64_t)length);
    require(output.len == (int64_t)expected && (!expected || output.ptr), "output extent");
    ++*calls; *bytes += (uint64_t)output.len;
    align_rt_free(output.ptr);
}
int main(void) {
    static const unsigned char ascii[] = "status=ok\0line\n";
    static const unsigned char unicode[] = "日本語\0🦀�";
    static const unsigned char mixed[] = "a\xff" "日本\xe1\x80";
    static const unsigned char mixed_out[] = "a�日本�";
    static const unsigned char dense[] = {0xff, 0x80, 0xc0, 0xaf};
    static const unsigned char dense_out[] = "����";
    const unsigned char *seeds[] = {ascii, unicode, ascii, ascii, mixed, dense};
    size_t seed_lengths[] = {sizeof(ascii)-1, sizeof(unicode)-1, sizeof(ascii)-1,
                            sizeof(ascii)-1, sizeof(mixed)-1, sizeof(dense)};
    const unsigned char *outputs[] = {ascii, unicode, ascii, ascii, mixed_out, dense_out};
    size_t output_lengths[] = {sizeof(ascii)-1, sizeof(unicode)-1, sizeof(ascii)-1,
                              sizeof(ascii)-1, sizeof(mixed_out)-1, sizeof(dense_out)-1};
    const char *names[] = {"ascii", "unicode", "early_invalid", "late_invalid", "mixed", "dense_invalid"};
    size_t sizes[] = {0, 8, 64, 4096, 65536};
    puts("pattern,input_bytes,trial,calls,output_bytes,ns_per_call");
    for (size_t kind = 0; kind < 6; ++kind) {
        for (size_t size = 0; size < 5; ++size) {
            if (size == 0 && kind != 0) continue;
            size_t units = (sizes[size] + seed_lengths[kind] - 1) / seed_lengths[kind];
            int extra = kind == 2 || kind == 3;
            size_t length = units * seed_lengths[kind] + (extra ? 2 : 0);
            size_t expected = units * output_lengths[kind] + (extra ? 3 : 0);
            unsigned char *input = malloc(length + 1), *oracle = malloc(expected + 1);
            require(input && oracle, "fixture allocation");
            size_t in_at = kind == 2 ? 2 : 0, out_at = kind == 2 ? 3 : 0;
            for (size_t unit = 0; unit < units; ++unit) {
                memcpy(input + in_at, seeds[kind], seed_lengths[kind]);
                memcpy(oracle + out_at, outputs[kind], output_lengths[kind]);
                in_at += seed_lengths[kind]; out_at += output_lengths[kind];
            }
            if (extra) {
                memcpy(input + (kind == 2 ? 0 : in_at), "\xe1\x80", 2);
                memcpy(oracle + (kind == 2 ? 0 : out_at), "�", 3);
            }
            View checked = align_rt_utf8_decode_lossy(input, (int64_t)length);
            require(checked.len == (int64_t)expected && (!expected || checked.ptr), "oracle extent");
            require(!expected || memcmp(checked.ptr, oracle, expected) == 0, "oracle bytes");
            align_rt_free(checked.ptr);
            size_t repeats = 2 * 1024 * 1024 / (length + 64);
            if (repeats < 64) repeats = 64;
            uint64_t calls = 0, bytes = 0, warm = now();
            do { one(input, length, expected, &calls, &bytes); } while (now() - warm < UINT64_C(100000000));
            for (size_t trial = 0; trial < 7; ++trial) {
                calls = bytes = 0;
                uint64_t start = now();
                for (size_t call = 0; call < repeats; ++call) one(input, length, expected, &calls, &bytes);
                uint64_t elapsed = now() - start;
                require(calls == repeats && bytes == calls * expected, "observed work");
                printf("%s,%zu,%zu,%llu,%llu,%.2f\n", names[kind], length, trial,
                       (unsigned long long)calls, (unsigned long long)bytes, (double)elapsed / calls);
            }
            free(input); free(oracle);
        }
    }
    return 0;
}
