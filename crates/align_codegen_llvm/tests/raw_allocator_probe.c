/* Linux native qualification: refuse at the actual malloc call, never exhaust the host. */
#include <assert.h>
#include <stdint.h>
#include <stdlib.h>

extern int32_t align_rt_raw_try_alloc(int64_t, void **);
extern void align_rt_free(void *);
extern void *__real_malloc(size_t);
static int refusing;
static unsigned attempts;

void *__wrap_malloc(size_t size) {
    if (refusing) { ++attempts; return NULL; }
    return __real_malloc(size);
}

int main(void) {
    unsigned char dirty;
    void *pointer = &dirty;
    refusing = 1;
    assert(align_rt_raw_try_alloc(-1, NULL) == 2);
    assert(align_rt_raw_try_alloc(37, NULL) == 2);
    assert(align_rt_raw_try_alloc(-1, &pointer) == 2 && pointer == NULL);
    pointer = &dirty;
    assert(align_rt_raw_try_alloc(0, &pointer) == 0 && pointer == NULL);
    assert(attempts == 0);
    pointer = &dirty;
    assert(align_rt_raw_try_alloc(37, &pointer) == 17 && pointer == NULL);
    assert(attempts == 1);
    pointer = &dirty;
    assert(align_rt_raw_try_alloc(INT64_MAX, &pointer) == 17 && pointer == NULL);
    assert(attempts == 2);
    refusing = 0;
    const int64_t sizes[] = {1, 7, 8, 37, 104, 152, 5120};
    for (unsigned i = 0; i < sizeof sizes / sizeof sizes[0]; ++i) {
        assert(align_rt_raw_try_alloc(sizes[i], &pointer) == 0 && pointer != NULL);
        assert((uintptr_t)pointer % 8 == 0);
        ((unsigned char *)pointer)[0] = 0xa5;
        ((unsigned char *)pointer)[sizes[i] - 1] = 0x5a;
        align_rt_free(pointer);
    }
    align_rt_free(NULL);
    return 0;
}
