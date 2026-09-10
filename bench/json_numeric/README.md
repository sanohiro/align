# JSON numeric native probe

Local R63 timing probe; not a CI timing gate. Build outside the checkout:

```text
cc -O3 -std=c11 bench/json_numeric/main.c -ldl -o /tmp/align-json-numeric
/tmp/align-json-numeric /absolute/path/to/libalign_runtime.so
```

The Linux probe loads one runtime per process. It measures direct f32/f64 decode
and an exact u64 control, then native descriptor encoding of finite f32/f64,
owned text, an optional value and a float array. Each decode runs 1,800,000 calls;
each encode runs 200,000. Values include 0.3, integral values, signed zero, normal,
small and large exponents. Output includes encoded bytes and result checksums.

The baseline branch uses the existing descriptor leaf writers directly: its
compiler did not admit this owned float schema. `encode_raw` isolates native
writer cost; `encode_owned_copy` additionally copies the baseline result to model
the explicit copy needed by an escaping public encoder view. R63 transfers its
buffer, so its raw result already owns the bytes. This probe does not claim
baseline public schema admission. Compare exact bytes before interpreting timing.

Prepare immutable baseline/candidate runtimes separately, keep the same compiler
flags and host, alternate their order for nine pairs and retain raw outputs.
Use the existing `json_decode` and `json_soa` harnesses for ordinary and projection
controls. Plan 47 owns the acceptance and allocation/capacity correctness owners.

Resource observation is separate from timing. Build a private `alloc-count`
runtime and run `python3 bench/json_numeric/resources.py /absolute/path/to/libalign_runtime.so`.
The probe records retained and peak requested live bytes, including the baseline
explicit owned copy, and asserts receiving-owner cleanup returns to zero.
See [R63 observations](r63-results.md) for artifact identities and results.
