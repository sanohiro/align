#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>

typedef struct { unsigned char *ptr; int64_t len; } View;
extern void *align_rt_buffer_filled(int64_t, unsigned char, int64_t);
extern int32_t align_rt_buffer_try_filled(int64_t, unsigned char, int64_t, void **);
extern void align_rt_buffer_bytes(void *, View *);
extern void align_rt_buffer_free(void *);
typedef struct { struct timespec wall; struct rusage usage; } Stamp;
static void require(int ok) { if (!ok) { fputs("buffer probe failed\n", stderr); exit(2); } }
static Stamp stamp(void) {
    Stamp s;
    require(clock_gettime(CLOCK_MONOTONIC, &s.wall) == 0);
    require(getrusage(RUSAGE_SELF, &s.usage) == 0);
    return s;
}
static int64_t cpu(struct timeval t) { return (int64_t)t.tv_sec * 1000000000 + (int64_t)t.tv_usec * 1000; }
static void report(int fallible, size_t n, unsigned char value, int alignment,
                   int trial, const char *phase, Stamp before) {
    Stamp after = stamp();
    int64_t wall = (int64_t)(after.wall.tv_sec - before.wall.tv_sec) * 1000000000
                 + after.wall.tv_nsec - before.wall.tv_nsec;
    long rss = after.usage.ru_maxrss;
#ifdef __APPLE__
    rss /= 1024;
#endif
    printf("%d,%zu,%u,%d,%d,%s,%lld,%lld,%lld,%ld,%ld\n", fallible, n, value, alignment,
           trial, phase, (long long)wall,
           (long long)(cpu(after.usage.ru_utime) - cpu(before.usage.ru_utime)),
           (long long)(cpu(after.usage.ru_stime) - cpu(before.usage.ru_stime)),
           after.usage.ru_minflt - before.usage.ru_minflt, rss);
}
static void row(int fallible, size_t n, unsigned char value, int alignment, int trial) {
    Stamp start = stamp();
    void *buffer = NULL;
    if (fallible) require(align_rt_buffer_try_filled((int64_t)n, value, alignment, &buffer) == 0);
    else buffer = align_rt_buffer_filled((int64_t)n, value, alignment);
    require(buffer != NULL);
    report(fallible, n, value, alignment, trial, "acquire", start);
    View view = {0}; align_rt_buffer_bytes(buffer, &view);
    require(view.len == (int64_t)n && (!n || view.ptr != NULL));
    require((uintptr_t)view.ptr % (unsigned)alignment == 0);
    start = stamp();
    uint64_t sum = 0;
    for (size_t i = 0; i < n; ++i) sum += view.ptr[i];
    require(sum == (uint64_t)n * value);
    report(fallible, n, value, alignment, trial, "first_read", start);
    start = stamp(); memset(view.ptr, 0x5a, n);
    report(fallible, n, value, alignment, trial, "first_write", start);
    start = stamp(); memset(view.ptr, 0xa5, n);
    report(fallible, n, value, alignment, trial, "refill", start);
    sum = 0;
    for (size_t i = 0; i < n; ++i) sum += view.ptr[i];
    require(sum == (uint64_t)n * 0xa5);
    start = stamp(); align_rt_buffer_free(buffer);
    report(fallible, n, value, alignment, trial, "drop", start);
}
int main(void) {
    const size_t sizes[] = {0, 64, 1048576, 551157760, 1653473280, 2204631040};
    puts("fallible,bytes,value,alignment,trial,phase,wall_ns,user_ns,sys_ns,minor_faults,peak_rss_kib");
    fflush(stdout);
    for (int trial = 0; trial < 3; ++trial) {
        for (size_t i = 0; i < sizeof(sizes)/sizeof(sizes[0]); ++i) {
            for (int variant = 0; variant < (i == 2 ? 4 : 1); ++variant) {
                pid_t pid = fork(); require(pid >= 0);
                if (pid == 0) {
                    row(variant == 3, sizes[i], variant == 1 ? 0xa5 : 0,
                        variant == 2 ? 4096 : 1, trial);
                    exit(0);
                }
                int status = 0;
                require(waitpid(pid, &status, 0) == pid && WIFEXITED(status) && WEXITSTATUS(status) == 0);
            }
        }
    }
    return 0;
}
