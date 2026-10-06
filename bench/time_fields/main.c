#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
typedef struct { const unsigned char *ptr; int64_t len; } View;
extern int32_t align_rt_time_format(View *, int64_t, int32_t);
extern void align_rt_free(void *);
typedef struct { int64_t ns; const char *text[5]; } Golden;
/* Independent UTC calendar/decimal goldens; NULL is an unrepresentable floor. */
static const Golden goldens[] = {
    {INT64_MIN, {"1677-09-21T00:12:43.145224192Z", NULL, NULL, NULL, NULL}},
    {INT64_C(-9223285636854775808), {"1677-09-22T00:12:43.145224192Z", "1677-09-22T00:12:43.145Z", "Wed, 22 Sep 1677 00:12:43 GMT", "16770922T001243Z", "16770922"}},
    {INT64_C(-951782400123456789), {"1939-11-03T23:59:59.876543211Z", "1939-11-03T23:59:59.876Z", "Fri, 03 Nov 1939 23:59:59 GMT", "19391103T235959Z", "19391103"}},
    {INT64_C(-1), {"1969-12-31T23:59:59.999999999Z", "1969-12-31T23:59:59.999Z", "Wed, 31 Dec 1969 23:59:59 GMT", "19691231T235959Z", "19691231"}},
    {INT64_C(0), {"1970-01-01T00:00:00Z", "1970-01-01T00:00:00.000Z", "Thu, 01 Jan 1970 00:00:00 GMT", "19700101T000000Z", "19700101"}},
    {INT64_C(1), {"1970-01-01T00:00:00.000000001Z", "1970-01-01T00:00:00.000Z", "Thu, 01 Jan 1970 00:00:00 GMT", "19700101T000000Z", "19700101"}},
    {INT64_C(951782400123456789), {"2000-02-29T00:00:00.123456789Z", "2000-02-29T00:00:00.123Z", "Tue, 29 Feb 2000 00:00:00 GMT", "20000229T000000Z", "20000229"}},
    {INT64_C(1791000000123456789), {"2026-10-03T04:00:00.123456789Z", "2026-10-03T04:00:00.123Z", "Sat, 03 Oct 2026 04:00:00 GMT", "20261003T040000Z", "20261003"}},
    {INT64_MAX, {"2262-04-11T23:47:16.854775807Z", "2262-04-11T23:47:16.854Z", "Fri, 11 Apr 2262 23:47:16 GMT", "22620411T234716Z", "22620411"}},
    {INT64_C(951782400000000001), {"2000-02-29T00:00:00.000000001Z", "2000-02-29T00:00:00.000Z", "Tue, 29 Feb 2000 00:00:00 GMT", "20000229T000000Z", "20000229"}},
    {INT64_C(951782400000000010), {"2000-02-29T00:00:00.00000001Z", "2000-02-29T00:00:00.000Z", "Tue, 29 Feb 2000 00:00:00 GMT", "20000229T000000Z", "20000229"}},
    {INT64_C(951782400000000100), {"2000-02-29T00:00:00.0000001Z", "2000-02-29T00:00:00.000Z", "Tue, 29 Feb 2000 00:00:00 GMT", "20000229T000000Z", "20000229"}},
    {INT64_C(951782400000001000), {"2000-02-29T00:00:00.000001Z", "2000-02-29T00:00:00.000Z", "Tue, 29 Feb 2000 00:00:00 GMT", "20000229T000000Z", "20000229"}},
    {INT64_C(951782400000010000), {"2000-02-29T00:00:00.00001Z", "2000-02-29T00:00:00.000Z", "Tue, 29 Feb 2000 00:00:00 GMT", "20000229T000000Z", "20000229"}},
    {INT64_C(951782400000100000), {"2000-02-29T00:00:00.0001Z", "2000-02-29T00:00:00.000Z", "Tue, 29 Feb 2000 00:00:00 GMT", "20000229T000000Z", "20000229"}},
    {INT64_C(951782400001000000), {"2000-02-29T00:00:00.001Z", "2000-02-29T00:00:00.001Z", "Tue, 29 Feb 2000 00:00:00 GMT", "20000229T000000Z", "20000229"}},
    {INT64_C(951782400010000000), {"2000-02-29T00:00:00.01Z", "2000-02-29T00:00:00.010Z", "Tue, 29 Feb 2000 00:00:00 GMT", "20000229T000000Z", "20000229"}},
    {INT64_C(951782400100000000), {"2000-02-29T00:00:00.1Z", "2000-02-29T00:00:00.100Z", "Tue, 29 Feb 2000 00:00:00 GMT", "20000229T000000Z", "20000229"}},
};
static void require(int ok, const char *message) {
    if (!ok) { fprintf(stderr, "%s\n", message); exit(2); }
}
static uint64_t now(void) {
    struct timespec t;
    require(clock_gettime(CLOCK_MONOTONIC, &t) == 0, "clock");
    return (uint64_t)t.tv_sec * UINT64_C(1000000000) + (uint64_t)t.tv_nsec;
}
int main(int argc, char **argv) {
    int selected = -1;
    require(argc <= 2, "usage: probe [format-kind 0..4]");
    if (argc == 2) {
        require(strlen(argv[1]) == 1 && argv[1][0] >= '0' && argv[1][0] <= '4', "format-kind 0..4");
        selected = argv[1][0] - '0';
    }
    puts("kind,trial,calls,output_bytes,invalid_count,ns_per_call");
    const size_t count = sizeof(goldens) / sizeof(goldens[0]);
    for (int kind = 0; kind < 5; ++kind) {
        if (selected >= 0 && selected != kind) continue;
        size_t lengths[sizeof(goldens) / sizeof(goldens[0])];
        uint64_t expected_bytes = 0, expected_invalid = 0;
        for (size_t i = 0; i < count; ++i) {
            lengths[i] = goldens[i].text[kind] ? strlen(goldens[i].text[kind]) : 0;
            expected_bytes += lengths[i];
            expected_invalid += goldens[i].text[kind] == NULL;
        }
        uint64_t warm = now();
        do {
            for (size_t i = 0; i < count; ++i) {
                View out = {NULL, 0};
                int status = align_rt_time_format(&out, goldens[i].ns, kind);
                require(status == (goldens[i].text[kind] ? 0 : 2), "golden status");
                require(out.len == (int64_t)lengths[i], "golden length");
                if (status) require(out.ptr == NULL, "invalid output");
                else require(out.ptr && memcmp(out.ptr, goldens[i].text[kind], lengths[i]) == 0, "golden bytes");
                align_rt_free((void *)out.ptr);
            }
        } while (now() - warm < UINT64_C(200000000));
        const uint64_t repeats = 20000;
        for (int trial = 0; trial < 7; ++trial) {
            uint64_t calls = 0, bytes = 0, invalid = 0, start = now();
            for (uint64_t repeat = 0; repeat < repeats; ++repeat) {
                for (size_t i = 0; i < count; ++i) {
                    View out = {NULL, 0};
                    int status = align_rt_time_format(&out, goldens[i].ns, kind);
                    require(status == (goldens[i].text[kind] ? 0 : 2), "timed status");
                    require(out.len == (int64_t)lengths[i], "timed length");
                    require(status ? out.ptr == NULL : out.ptr != NULL, "timed output");
                    calls++; bytes += (uint64_t)out.len; invalid += status != 0;
                    align_rt_free((void *)out.ptr);
                }
            }
            uint64_t elapsed = now() - start;
            require(calls == repeats * count && bytes == repeats * expected_bytes &&
                    invalid == repeats * expected_invalid, "observed work accounting");
            printf("%d,%d,%llu,%llu,%llu,%.2f\n", kind, trial,
                   (unsigned long long)calls, (unsigned long long)bytes,
                   (unsigned long long)invalid, (double)elapsed / calls);
        }
    }
    return 0;
}
