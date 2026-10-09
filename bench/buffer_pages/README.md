# Explicit buffer page preference

Run bash bench/buffer_pages/run.sh on Linux.
The shared supervisor bounds build/link/probe groups, refreshes the ordinary
release runtime and removes scratch. Each sample runs in a fresh fork, verifies
byte contents and 64-byte alignment, and records acquisition, optional first
read, first write, same-region rewrite and exact release separately.

Modes 0/1 are the real buffer.filled ABI with Default/PreferHuge. Modes 2/3 are
private anonymous mapping controls with identical sizes, page alignment and
initial zero state; only mode 3 requests MADV_HUGEPAGE. They separate page advice
from changing allocator placement. They are controls, not replacement runtime
implementations. Each trial alternates the mode order. All paths check every
byte after both writes; read-first paths also check every initial zero byte.
Write-first paths check only the two boundary zero bytes before the timed write,
so that validation does not pre-fault the whole working set.

linux.csv retains five trials per mode and access order at 551,157,760 bytes,
one actual align-llm cache slot. Measurements: 2026-10-09, Linux x86_64 WSL2
6.18.40.1, Ryzen 9 5950X, 4096-byte base pages, THP and defrag set to madvise.
Runtime archive SHA-256: 57f29bdbdff93b3df4fdcb533ae505bd8f2406aff45f27d163460e611c4b618c.
Probe source SHA-256: b575711cc22f02a0fd2c1e8056a152d5cf54a1785a6a114b676e44ca67633cf1.

| Access order | Allocation | Through first write, ms | Rewrite, ms | First-write minor faults | AnonHugePages, KiB |
| --- | --- | ---: | ---: | ---: | ---: |
| write first | Default | 222.891 | 33.708 | 0 | 0 |
| write first | PreferHuge | 192.665 | 29.815 | 678 | 536576 |
| write first | Private mapping, no hint | 175.905 | 34.894 | 134560 | 0 |
| write first | Private mapping, PreferHuge | 91.518 | 29.337 | 678 | 536576 |
| read then write | Default | 272.843 | 33.146 | 0 | 0 |
| read then write | PreferHuge | 128.504 | 28.903 | 678 | 536576 |
| read then write | Private mapping, no hint | 319.692 | 33.932 | 134560 | 0 |
| read then write | Private mapping, PreferHuge | 104.922 | 29.381 | 678 | 536576 |

Values are five-trial medians; each through-first-write value sums the phases
within a trial before taking the median. This host's selected workloads benefit
overall, but the costs move between acquisition and first touch. There is no
portable latency, RSS or huge-page guarantee. Do not infer model/HTTP/GPU gains
or the complete four-slot cache's behavior from this one-slot CPU measurement.

RSS is process peak RSS in KiB for each fresh child. AnonHugePages is the sum for
VMAs intersecting the payload; allocator metadata/padding or adjacent extents
may share those VMAs, so it is not exclusive per-buffer accounting. Reading
smaps happens outside the timed phase but can perturb later phases. A runtime
row's advice_result=-1 means not observed through the production ABI; control
mode 3 records the actual syscall result. Backing observations, rather than a
successful hint alone, establish huge pages on this host.

Native owner tests separately force advice/acquisition/header refusal and
release failure, verify exact mapping extent retirement and ordinary reuse
without the old huge-page flag, and exercise small/unsupported fallback.
