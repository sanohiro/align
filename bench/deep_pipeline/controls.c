// Equal-LLVM C controls for the deep-pipeline benchmark. Signed Align arithmetic wraps, so the
// C control operates on uint64_t bits and implements the signed arithmetic shift explicitly.
#include <stdbool.h>
#include <stdint.h>

typedef struct {
    const int64_t *ptr;
    int64_t len;
} Slice;

static inline uint64_t ashr17(uint64_t x) {
    // This control is compiled only with clang-22 on two's-complement targets, where signed right
    // shift is arithmetic. The cast therefore produces the same single LLVM `ashr` as Align.
    return (uint64_t)((int64_t)x >> 17);
}

static inline uint64_t mix(uint64_t x) {
    return (x ^ ashr17(x)) * UINT64_C(6364136223846793005) +
           UINT64_C(1442695040888963407);
}

static inline uint64_t capture_stage(uint64_t x, uint64_t k) {
    return (x ^ k) * UINT64_C(6364136223846793005) + UINT64_C(1442695040888963407);
}

static inline bool keep(uint64_t x) { return (x & 1) == 0; }

int64_t c_named_1(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        acc += mix(x);
    }
    return (int64_t)acc;
}

int64_t c_masked_1(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        uint64_t y = x;
        if (keep(y)) acc += y;
    }
    return (int64_t)acc;
}

int64_t c_guarded_1(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        if (keep(x)) acc += mix(x);
    }
    return (int64_t)acc;
}

int64_t c_capture_1(Slice s, int64_t k) {
    uint64_t acc = 0;
    uint64_t key = (uint64_t)k;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        acc += capture_stage(x, key);
    }
    return (int64_t)acc;
}

int64_t c_named_2(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        acc += mix(mix(x));
    }
    return (int64_t)acc;
}

int64_t c_masked_2(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        uint64_t y = mix(x);
        if (keep(y)) acc += y;
    }
    return (int64_t)acc;
}

int64_t c_guarded_2(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        if (keep(x)) acc += mix(mix(x));
    }
    return (int64_t)acc;
}

int64_t c_capture_2(Slice s, int64_t k) {
    uint64_t acc = 0;
    uint64_t key = (uint64_t)k;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        acc += capture_stage(capture_stage(x, key), key);
    }
    return (int64_t)acc;
}

int64_t c_named_4(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        acc += mix(mix(mix(mix(x))));
    }
    return (int64_t)acc;
}

int64_t c_masked_4(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        uint64_t y = mix(mix(mix(x)));
        if (keep(y)) acc += y;
    }
    return (int64_t)acc;
}

int64_t c_guarded_4(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        if (keep(x)) acc += mix(mix(mix(mix(x))));
    }
    return (int64_t)acc;
}

int64_t c_capture_4(Slice s, int64_t k) {
    uint64_t acc = 0;
    uint64_t key = (uint64_t)k;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        acc += capture_stage(capture_stage(capture_stage(capture_stage(x, key), key), key), key);
    }
    return (int64_t)acc;
}

int64_t c_named_8(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        acc += mix(mix(mix(mix(mix(mix(mix(mix(x))))))));
    }
    return (int64_t)acc;
}

int64_t c_masked_8(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        uint64_t y = mix(mix(mix(mix(mix(mix(mix(x)))))));
        if (keep(y)) acc += y;
    }
    return (int64_t)acc;
}

int64_t c_guarded_8(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        if (keep(x)) acc += mix(mix(mix(mix(mix(mix(mix(mix(x))))))));
    }
    return (int64_t)acc;
}

int64_t c_capture_8(Slice s, int64_t k) {
    uint64_t acc = 0;
    uint64_t key = (uint64_t)k;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        acc += capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(x, key), key), key), key), key), key), key), key);
    }
    return (int64_t)acc;
}

int64_t c_named_16(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        acc += mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(x))))))))))))))));
    }
    return (int64_t)acc;
}

int64_t c_masked_16(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        uint64_t y = mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(x)))))))))))))));
        if (keep(y)) acc += y;
    }
    return (int64_t)acc;
}

int64_t c_guarded_16(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        if (keep(x)) acc += mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(x))))))))))))))));
    }
    return (int64_t)acc;
}

int64_t c_capture_16(Slice s, int64_t k) {
    uint64_t acc = 0;
    uint64_t key = (uint64_t)k;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        acc += capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(x, key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key);
    }
    return (int64_t)acc;
}

int64_t c_named_32(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        acc += mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(x))))))))))))))))))))))))))))))));
    }
    return (int64_t)acc;
}

int64_t c_masked_32(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        uint64_t y = mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(x)))))))))))))))))))))))))))))));
        if (keep(y)) acc += y;
    }
    return (int64_t)acc;
}

int64_t c_guarded_32(Slice s) {
    uint64_t acc = 0;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        if (keep(x)) acc += mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(mix(x))))))))))))))))))))))))))))))));
    }
    return (int64_t)acc;
}

int64_t c_capture_32(Slice s, int64_t k) {
    uint64_t acc = 0;
    uint64_t key = (uint64_t)k;
    for (int64_t i = 0; i < s.len; ++i) {
        uint64_t x = (uint64_t)s.ptr[i];
        acc += capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(capture_stage(x, key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key), key);
    }
    return (int64_t)acc;
}
