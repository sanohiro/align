# A bounded text, audio and media pipeline

[`examples/multimodal/`](../../examples/multimodal/) is a runnable application
written in ordinary Align. It combines HTTP, owned JSON, structured tasks,
retained directories and `std.process.child_scope`. Its exact contract is
[plan91](../impl/91-multimodal-reference-composition.md).

The reference generates fixed mock text, one second of mono WAV silence and a
16×16 PPM image, then runs real FFmpeg to assemble audio/video into fragmented
MP4. It demonstrates resource ownership and transport; model output quality,
phoneme alignment, GPU fit and GPU release require real engine adapters.

## Run on Linux or WSL2

Install LLVM22 and FFmpeg/ffprobe. WSL2 must pass the existing child_scope kernel
feature probe. The full managed-process reference refuses unsupported kernels
and macOS before starting its workers. Common source/model tests run on macOS.

Build from the repository root. The three ports are distinct explicit inputs:
one control port and one port per observer. Each observer serves one ongoing
stream; select the other port for a second stream.

```bash
scripts/cargo.sh build --workspace
./target/debug/alignc build examples/multimodal/main.align
./main selftest
root=$(mktemp -d)
"$(pwd)/main" serve "$root" 8080 8081 8082 /usr/bin/ffmpeg
```

ROOT must be an existing empty absolute directory with mode0700. Use an absolute
executable path. The process remains running until a shutdown request.

In another terminal, submit a pipeline job:

```bash
curl -sS http://127.0.0.1:8080/v1/jobs/pipeline \
  -H 'Content-Type: application/json' \
  -d '{"input":"A short scene","delay_ns":0}'
```

The 202 response contains `epoch`, `id` and `status:"queued"`. Substitute those
values into the following routes:

| Action | Method and route |
| --- | --- |
| Status, artifacts and audio metadata | Control GET `/v1/jobs/EPOCH/ID` |
| Progress SSE | Observer GET `/v1/jobs/EPOCH/ID/events` |
| Download completed MP4 | Observer GET `/v1/jobs/EPOCH/ID/artifacts/mux` |
| Cancel | Control POST `/v1/jobs/EPOCH/ID/cancel`, empty body |
| Health | Control GET `/health` |
| Clean shutdown | Control POST `/shutdown`, empty body |

Use `curl -N` for events and `curl -o scene.mp4` for an artifact. Audio/text/batch
jobs use the corresponding submit suffix. Completed artifacts use phase suffixes
`text`, `audio`, `batch`, `mux`. Observer `/v1/live/text` demonstrates direct SSE;
`/v1/live/pcm` sends raw s16le silence with explicit rate/channels/frame headers.
Raw PCM is distinct from a completed WAV file.

## Bounds and ownership

One controller owns eight scalar SoA job slots, at most four queued jobs and
one managed phase. The tasks exchange bounded authenticated HTTP snapshots;
no mutable job table is shared. Every response has an explicit write budget.

Output uses fixed4KiB scratch and at most1MiB reserved per job. Completed
artifacts stay file-backed. Three terminal jobs remain publicly retained;
older jobs retire. An existing reader lease keeps retired files and their
reservation alive until exact-id release. Disconnecting events does not cancel
a job. None-valued JSON fields are omitted, including unavailable percentages
and unsupported alignment. The mock reports only its known silence interval
in sample frames; it invents no word or phoneme times.

Each phase drains captured pipes, observes native process state and explicitly
releases its child_scope before publishing or launching the next phase. A
failed release retains Faulted ownership and blocks the next launch. This
process absence witness does not prove a real engine released device memory.

Clean shutdown stops admission, releases managed work, joins observers, removes
only recorded owned artifacts and removes `controller.lock` last. After an
unclean exit the fence remains: reconcile remaining native/device ownership
before manually removing it. Root exit and elapsed time are insufficient.

The provider regression is `scripts/cargo.sh test -p align_driver --test
multimodal_reference`. Run its Linux composition owner with FFmpeg and ffprobe
installed; it checks real muxing, cancellation, retained readers, stale releases,
stalled control replies and abnormal producers. No GPU inference is qualified.
