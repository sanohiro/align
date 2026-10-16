#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
typedef struct { unsigned char *ptr; int64_t len; } AlignStr;
extern void *align_rt_builder_new(void *, int64_t);
extern void align_rt_builder_write_int(void *, int64_t);
extern void align_rt_builder_write_uint(void *, uint64_t);
extern void align_rt_builder_write_str_int_str(void *, const unsigned char *, int64_t, int64_t, const unsigned char *, int64_t);
extern AlignStr align_rt_builder_into_string(void *);
extern void align_rt_free(unsigned char *);
typedef struct { int group; int64_t signed_value; uint64_t unsigned_value; const char *expected; } Golden;
/* Fixed independent decimal goldens; neither runtime computes the expected bytes. */
static const Golden goldens[] = {
    {0, INT64_C(0), 0, "0"},
    {0, INT64_C(1), 0, "1"},
    {0, -INT64_C(1), 0, "-1"},
    {0, INT64_C(9), 0, "9"},
    {0, -INT64_C(9), 0, "-9"},
    {0, INT64_C(10), 0, "10"},
    {0, -INT64_C(10), 0, "-10"},
    {0, INT64_C(99), 0, "99"},
    {0, -INT64_C(99), 0, "-99"},
    {0, INT64_C(100), 0, "100"},
    {0, -INT64_C(100), 0, "-100"},
    {0, INT64_C(999), 0, "999"},
    {0, -INT64_C(999), 0, "-999"},
    {1, INT64_C(1000), 0, "1000"},
    {1, -INT64_C(1000), 0, "-1000"},
    {1, INT64_C(1234), 0, "1234"},
    {1, -INT64_C(1234), 0, "-1234"},
    {1, INT64_C(9999), 0, "9999"},
    {1, -INT64_C(9999), 0, "-9999"},
    {1, INT64_C(10000), 0, "10000"},
    {1, -INT64_C(10000), 0, "-10000"},
    {1, INT64_C(123456), 0, "123456"},
    {1, -INT64_C(123456), 0, "-123456"},
    {1, INT64_C(999999999), 0, "999999999"},
    {1, -INT64_C(999999999), 0, "-999999999"},
    {2, INT64_C(1000000000), 0, "1000000000"},
    {2, -INT64_C(1000000000), 0, "-1000000000"},
    {2, INT64_C(1000000000000), 0, "1000000000000"},
    {2, -INT64_C(1000000000000), 0, "-1000000000000"},
    {2, INT64_C(1000000000000000000), 0, "1000000000000000000"},
    {2, -INT64_C(1000000000000000000), 0, "-1000000000000000000"},
    {2, INT64_C(9223372036854775807), 0, "9223372036854775807"},
    {2, INT64_MIN, 0, "-9223372036854775808"},
    {2, INT64_C(9876543210123456), 0, "9876543210123456"},
    {2, -INT64_C(9876543210123456), 0, "-9876543210123456"},
    {3, 0, UINT64_C(0), "0"},
    {3, 0, UINT64_C(1), "1"},
    {3, 0, UINT64_C(9), "9"},
    {3, 0, UINT64_C(10), "10"},
    {3, 0, UINT64_C(99), "99"},
    {3, 0, UINT64_C(100), "100"},
    {3, 0, UINT64_C(999), "999"},
    {4, 0, UINT64_C(1000), "1000"},
    {4, 0, UINT64_C(123456), "123456"},
    {4, 0, UINT64_C(999999999), "999999999"},
    {4, 0, UINT64_C(1000000000000), "1000000000000"},
    {4, 0, UINT64_C(1000000000000000000), "1000000000000000000"},
    {4, 0, UINT64_C(9223372036854775807), "9223372036854775807"},
    {4, 0, UINT64_C(9223372036854775808), "9223372036854775808"},
    {4, 0, UINT64_C(18446744073709551615), "18446744073709551615"},
    {5, INT64_C(0), 0, "item-0-status "},
    {5, INT64_C(1), 0, "item-1-status "},
    {5, -INT64_C(1), 0, "item--1-status "},
    {5, INT64_C(9), 0, "item-9-status "},
    {5, -INT64_C(9), 0, "item--9-status "},
    {5, INT64_C(10), 0, "item-10-status "},
    {5, -INT64_C(10), 0, "item--10-status "},
    {5, INT64_C(99), 0, "item-99-status "},
    {5, -INT64_C(99), 0, "item--99-status "},
    {5, INT64_C(100), 0, "item-100-status "},
    {5, -INT64_C(100), 0, "item--100-status "},
    {5, INT64_C(999), 0, "item-999-status "},
    {5, -INT64_C(999), 0, "item--999-status "},
    {6, INT64_C(1000), 0, "item-1000-status "},
    {6, -INT64_C(1000), 0, "item--1000-status "},
    {6, INT64_C(123456), 0, "item-123456-status "},
    {6, -INT64_C(123456), 0, "item--123456-status "},
    {6, INT64_C(1000000000000), 0, "item-1000000000000-status "},
    {6, -INT64_C(1000000000000), 0, "item--1000000000000-status "},
    {6, INT64_C(9223372036854775807), 0, "item-9223372036854775807-status "},
    {6, INT64_MIN, 0, "item--9223372036854775808-status "},
};
static void require(int ok, const char *message) {
    if (!ok) { fprintf(stderr, "%s\n", message); exit(2); }
}
static uint64_t now(void) {
    struct timespec ts;
    require(clock_gettime(CLOCK_MONOTONIC, &ts) == 0, "clock");
    return (uint64_t)ts.tv_sec * UINT64_C(1000000000) + (uint64_t)ts.tv_nsec;
}
static uint64_t batch(int group, const Golden **cases, size_t count, const unsigned char *expected, size_t length) {
    void *builder = align_rt_builder_new(NULL, (int64_t)length);
    require(builder != NULL, "builder");
    uint64_t calls = 0;
    for (size_t repeat = 0; repeat < 128; ++repeat) {
        for (size_t i = 0; i < count; ++i) {
            const Golden *value = cases[i];
            if (group == 3 || group == 4) align_rt_builder_write_uint(builder, value->unsigned_value);
            else if (group >= 5) align_rt_builder_write_str_int_str(builder,
                (const unsigned char *)"item-", 5, value->signed_value,
                (const unsigned char *)"-status ", 8);
            else align_rt_builder_write_int(builder, value->signed_value);
            calls++;
        }
    }
    AlignStr out = align_rt_builder_into_string(builder);
    require(out.len == (int64_t)length && memcmp(out.ptr, expected, length) == 0, "complete output bytes");
    align_rt_free(out.ptr);
    require(calls == 128 * count, "observed calls");
    return calls;
}
int main(int argc, char **argv) {
    require(argc == 2 && strlen(argv[1]) == 1 && argv[1][0] >= '0' && argv[1][0] <= '6', "usage: probe group[0..6]");
    int group = argv[1][0] - '0';
    const Golden *cases[32]; size_t count = 0, length = 0;
    for (size_t i = 0; i < sizeof(goldens)/sizeof(goldens[0]); ++i) {
        if (goldens[i].group != group) continue;
        require(count < 32, "case capacity");
        cases[count++] = &goldens[i]; length += strlen(goldens[i].expected) * 128;
    }
    require(count > 0, "nonempty group");
    unsigned char *expected = malloc(length); require(expected != NULL, "expected allocation");
    size_t pos = 0;
    for (size_t repeat = 0; repeat < 128; ++repeat) {
        for (size_t i = 0; i < count; ++i) {
            size_t bytes = strlen(cases[i]->expected); memcpy(expected + pos, cases[i]->expected, bytes); pos += bytes;
        }
    }
    require(pos == length, "expected size");
    uint64_t warm = now();
    do { batch(group, cases, count, expected, length); } while (now() - warm < UINT64_C(50000000));
    puts("group,trial,calls,bytes,ns_per_call");
    const uint64_t repeats = 1024;
    for (int trial = 0; trial < 7; ++trial) {
        uint64_t calls = 0, bytes = 0, started = now();
        for (uint64_t repeat = 0; repeat < repeats; ++repeat) {
            calls += batch(group, cases, count, expected, length); bytes += length;
        }
        uint64_t elapsed = now() - started;
        require(calls == repeats * 128 * count && bytes == repeats * length, "observed work");
        printf("%d,%d,%llu,%llu,%.3f\n", group, trial, (unsigned long long)calls, (unsigned long long)bytes, (double)elapsed/calls);
    }
    free(expected);
}
