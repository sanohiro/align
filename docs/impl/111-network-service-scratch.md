# Fixed scratch for numeric network services

TCP connect/listen and UDP bind/send currently form the resolver's numeric
service with `CString::new(port.to_string())`. Each admitted port needs only
one to five ASCII digits plus NUL, yet the measured expression allocates and
reallocates once. An actual native loopback TCP connection currently performs
three Rust allocations and one reallocation, including its unchanged host
CString and returned connection shell. HTTP clients reuse this TCP path; UDP
send repeats service preparation per datagram.

The native owner on both Linux ARM64 and macOS ARM64 measures Rust allocation
operations (including reallocations) as follows. Host CString and successful
connection-shell allocations remain; native resolver storage is outside the count.

| Actual call | Before | Fixed service scratch |
| --- | ---: | ---: |
| Successful TCP connect | 4 | 2 |
| TCP listen on a still-occupied port | 3 | 1 |
| UDP bind on a still-occupied port | 3 | 1 |
| UDP send to an owned loopback receiver | 3 | 1 |

Use one private checked numeric-service value containing six initialized bytes
and a suffix offset. Construction accepts exactly `1..=65535`, renders ordinary
decimal without leading zeros, and retains one terminating NUL. It owns bytes,
not a pointer into itself; a pointer borrowed after construction remains live
only through the existing synchronous resolver call. No heap allocation or
cleanup is needed for the service. Preserve output-slot clearing and port-before-
host validation, including on multi-invalid input. Host conversion, wildcard
selection, resolver hints/order/errors, timeout behavior, socket ownership and
all public signatures and runtime ABI records remain unchanged.

| Closure axis | Implementation and acceptance |
| --- | --- |
| Numeric formation and boundaries | One checked helper serves all four existing call sites. `numeric_service_decimal_and_admission` compares all 65,535 admitted ports against independent decimal text plus NUL; rejects zero, negative, above-u16 and i64 extrema; checks storage after moving the owning value. No pointer is retained across a move. |
| Native consumers and allocation | `numeric_service_native_call_allocations` measures complete native TCP connect/listen and UDP bind/send calls with the existing thread-local Rust allocator and an allocating positive witness. Successful TCP connects to an exclusively bound nonblocking peer; failed native TCP/UDP binds target still-held ports and must preserve exact EADDRINUSE/null output. UDP sends exact binary bytes to an exclusively bound receiver. Restoring allocating service preparation must fail. |
| Validation and resolver failure | Existing `tcp_connect_resolver_status_and_order_matrix`, TCP/listen/UDP bad-port/null-output and UDP bad-argument owners retain error and cleanup coverage. The new admission owner crosses invalid host bytes and invalid ports through all four exports, proving port rejection before host access/allocation and ordinary invalid-host rejection after valid ports. |
| Ownership and control paths | Service bytes are private call-local initialized storage. Native input is borrowed only during getaddrinfo. No resource move, Drop, replacement, returned borrow, branch/loop join, compiler IR, generic, interface/cache or whole/per-unit representation changes. Existing `m11_net` source/execution owners cover the unchanged generated path. |
| Fixture lifetime | Rust-owned loopback sockets and an immediately armed connection guard own every descriptor; nonblocking capture has a short deadline. No child, detached thread, temporary path, process-global mutation or free-port reservation gap is introduced. |
| Resource claim | Compare the actual native owner on Linux and macOS before/after. Count Rust allocations/reallocations only; native resolver allocations, host storage and returned shells remain. Claim removal of temporary service allocations, without a connection-latency, throughput, RSS or DNS improvement promise. |

This implements the existing native-string boundary strategy from allocation
audit section 6.1, with bounded scalar scratch replacing transient service
ownership. It introduces no public contract or safety strategy. Author-side
matrix extraction plus the one independent preflight diff review closes this
private refinement; no separate broad design review or specification mirror
change is needed.
