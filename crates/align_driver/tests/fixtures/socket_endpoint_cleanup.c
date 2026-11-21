#include <stdint.h>
#include <stdlib.h>

struct Endpoint { void *host; int64_t length; int64_t port; };
struct Datagram { int64_t n; struct Endpoint peer; };
extern int32_t __real_align_rt_udp_local_addr(void *, struct Endpoint *);
extern int32_t __real_align_rt_udp_recv_datagram(void *, void *, struct Datagram *);
extern void __real_align_rt_free(void *);

static void *live[16];
static unsigned acquired, released;
static void remember(struct Endpoint ep) {
    if (!ep.host || ep.length != 9 || ep.port <= 0) abort();
    for (unsigned i = 0; i < 16; ++i) {
        if (!live[i]) { live[i] = ep.host; ++acquired; return; }
    }
    abort();
}
int32_t __wrap_align_rt_udp_local_addr(void *socket, struct Endpoint *out) {
    int32_t status = __real_align_rt_udp_local_addr(socket, out);
    if (!status) remember(*out);
    return status;
}
int32_t __wrap_align_rt_udp_recv_datagram(void *socket, void *buffer, struct Datagram *out) {
    int32_t status = __real_align_rt_udp_recv_datagram(socket, buffer, out);
    if (!status) remember(out->peer);
    return status;
}
void __wrap_align_rt_free(void *pointer) {
    if (pointer) {
        for (unsigned i = 0; i < 16; ++i) {
            if (live[i] == pointer) { live[i] = NULL; ++released; break; }
        }
    }
    __real_align_rt_free(pointer);
}
int32_t endpoint_balance(void) { return acquired == 8 && acquired == released; }
