//! Independently rounded sample goldens for expm1/log1p; no global ULP promise.
// Decimal exp/ln at 600 and 900 digits agreed before IEEE conversion (Plan234).
// All widths include signed zero, subnormals, tiny signed inputs and IEEE domain edges.
#[derive(Clone, Copy)]
pub struct Sample {
    pub operation: &'static str,
    pub bits: u8,
    pub input: u64,
    pub expected: Option<u64>,
}
#[rustfmt::skip]
pub const SAMPLES: &[Sample] = &[
    Sample { operation: "expm1", bits: 32, input: 0x0, expected: Some(0x0) },
    Sample { operation: "expm1", bits: 32, input: 0x80000000, expected: Some(0x80000000) },
    Sample { operation: "expm1", bits: 32, input: 0x33800000, expected: Some(0x33800000) },
    Sample { operation: "expm1", bits: 32, input: 0xb3800000, expected: Some(0xb3800000) },
    Sample { operation: "expm1", bits: 32, input: 0x34000000, expected: Some(0x34000001) },
    Sample { operation: "expm1", bits: 32, input: 0xb4000000, expected: Some(0xb3ffffff) },
    Sample { operation: "expm1", bits: 32, input: 0x322bcc77, expected: Some(0x322bcc77) },
    Sample { operation: "expm1", bits: 32, input: 0xb22bcc77, expected: Some(0xb22bcc77) },
    Sample { operation: "expm1", bits: 32, input: 0x3e800000, expected: Some(0x3e916bc8) },
    Sample { operation: "expm1", bits: 32, input: 0xbe800000, expected: Some(0xbe62820c) },
    Sample { operation: "expm1", bits: 32, input: 0x3f000000, expected: Some(0x3f261299) },
    Sample { operation: "expm1", bits: 32, input: 0xbf000000, expected: Some(0xbec974d0) },
    Sample { operation: "expm1", bits: 32, input: 0x3f7d70a4, expected: Some(0x3fd87a5f) },
    Sample { operation: "expm1", bits: 32, input: 0xbf7d70a4, expected: Some(0xbf20e05a) },
    Sample { operation: "expm1", bits: 32, input: 0x3f800000, expected: Some(0x3fdbf0a9) },
    Sample { operation: "expm1", bits: 32, input: 0x40000000, expected: Some(0x40cc7326) },
    Sample { operation: "expm1", bits: 32, input: 0x41a00000, expected: Some(0x4de75844) },
    Sample { operation: "expm1", bits: 32, input: 0xc1a00000, expected: Some(0xbf800000) },
    Sample { operation: "expm1", bits: 32, input: 0x7f800000, expected: Some(0x7f800000) },
    Sample { operation: "expm1", bits: 32, input: 0xff800000, expected: Some(0xbf800000) },
    Sample { operation: "expm1", bits: 32, input: 0x7fc00000, expected: None },
    Sample { operation: "expm1", bits: 32, input: 0x1, expected: Some(0x1) },
    Sample { operation: "expm1", bits: 32, input: 0x80000001, expected: Some(0x80000001) },
    Sample { operation: "expm1", bits: 32, input: 0x42a00000, expected: Some(0x792abbce) },
    Sample { operation: "expm1", bits: 32, input: 0x42b40000, expected: Some(0x7f800000) },
    Sample { operation: "expm1", bits: 32, input: 0xc2c80000, expected: Some(0xbf800000) },
    Sample { operation: "log1p", bits: 32, input: 0x0, expected: Some(0x0) },
    Sample { operation: "log1p", bits: 32, input: 0x80000000, expected: Some(0x80000000) },
    Sample { operation: "log1p", bits: 32, input: 0x33800000, expected: Some(0x33800000) },
    Sample { operation: "log1p", bits: 32, input: 0xb3800000, expected: Some(0xb3800000) },
    Sample { operation: "log1p", bits: 32, input: 0x34000000, expected: Some(0x33ffffff) },
    Sample { operation: "log1p", bits: 32, input: 0xb4000000, expected: Some(0xb4000001) },
    Sample { operation: "log1p", bits: 32, input: 0x322bcc77, expected: Some(0x322bcc77) },
    Sample { operation: "log1p", bits: 32, input: 0xb22bcc77, expected: Some(0xb22bcc77) },
    Sample { operation: "log1p", bits: 32, input: 0x3e800000, expected: Some(0x3e647fbe) },
    Sample { operation: "log1p", bits: 32, input: 0xbe800000, expected: Some(0xbe934b11) },
    Sample { operation: "log1p", bits: 32, input: 0x3f000000, expected: Some(0x3ecf991f) },
    Sample { operation: "log1p", bits: 32, input: 0xbf000000, expected: Some(0xbf317218) },
    Sample { operation: "log1p", bits: 32, input: 0x3f7d70a4, expected: Some(0x3f302998) },
    Sample { operation: "log1p", bits: 32, input: 0xbf7d70a4, expected: Some(0xc0935d90) },
    Sample { operation: "log1p", bits: 32, input: 0x3f800000, expected: Some(0x3f317218) },
    Sample { operation: "log1p", bits: 32, input: 0x40000000, expected: Some(0x3f8c9f54) },
    Sample { operation: "log1p", bits: 32, input: 0x41a00000, expected: Some(0x4042d975) },
    Sample { operation: "log1p", bits: 32, input: 0xc1a00000, expected: None },
    Sample { operation: "log1p", bits: 32, input: 0x7f800000, expected: Some(0x7f800000) },
    Sample { operation: "log1p", bits: 32, input: 0xff800000, expected: None },
    Sample { operation: "log1p", bits: 32, input: 0x7fc00000, expected: None },
    Sample { operation: "log1p", bits: 32, input: 0x1, expected: Some(0x1) },
    Sample { operation: "log1p", bits: 32, input: 0x80000001, expected: Some(0x80000001) },
    Sample { operation: "log1p", bits: 32, input: 0xbf800000, expected: Some(0xff800000) },
    Sample { operation: "log1p", bits: 32, input: 0xbf7fffff, expected: Some(0xc1851592) },
    Sample { operation: "log1p", bits: 32, input: 0xbf800001, expected: None },
    Sample { operation: "log1p", bits: 32, input: 0x7f7fffff, expected: Some(0x42b17218) },
    Sample { operation: "expm1", bits: 64, input: 0x0, expected: Some(0x0) },
    Sample { operation: "expm1", bits: 64, input: 0x8000000000000000, expected: Some(0x8000000000000000) },
    Sample { operation: "expm1", bits: 64, input: 0x3ca0000000000000, expected: Some(0x3ca0000000000000) },
    Sample { operation: "expm1", bits: 64, input: 0xbca0000000000000, expected: Some(0xbca0000000000000) },
    Sample { operation: "expm1", bits: 64, input: 0x3cb0000000000000, expected: Some(0x3cb0000000000001) },
    Sample { operation: "expm1", bits: 64, input: 0xbcb0000000000000, expected: Some(0xbcafffffffffffff) },
    Sample { operation: "expm1", bits: 64, input: 0x3e45798ee2308c3a, expected: Some(0x3e45798ee3fdb764) },
    Sample { operation: "expm1", bits: 64, input: 0xbe45798ee2308c3a, expected: Some(0xbe45798ee0636111) },
    Sample { operation: "expm1", bits: 64, input: 0x3fd0000000000000, expected: Some(0x3fd22d78f0fa061a) },
    Sample { operation: "expm1", bits: 64, input: 0xbfd0000000000000, expected: Some(0xbfcc5041854df7d4) },
    Sample { operation: "expm1", bits: 64, input: 0x3fe0000000000000, expected: Some(0x3fe4c2531c3c0d38) },
    Sample { operation: "expm1", bits: 64, input: 0xbfe0000000000000, expected: Some(0xbfd92e9a0720d3ec) },
    Sample { operation: "expm1", bits: 64, input: 0x3fefae147ae147ae, expected: Some(0x3ffb0f4be0c9b9f3) },
    Sample { operation: "expm1", bits: 64, input: 0xbfefae147ae147ae, expected: Some(0xbfe41c0b33035762) },
    Sample { operation: "expm1", bits: 64, input: 0x3ff0000000000000, expected: Some(0x3ffb7e151628aed3) },
    Sample { operation: "expm1", bits: 64, input: 0x4000000000000000, expected: Some(0x40198e64b8d4ddae) },
    Sample { operation: "expm1", bits: 64, input: 0x4034000000000000, expected: Some(0x41bceb088a68e804) },
    Sample { operation: "expm1", bits: 64, input: 0xc034000000000000, expected: Some(0xbfeffffffee4b79b) },
    Sample { operation: "expm1", bits: 64, input: 0x7ff0000000000000, expected: Some(0x7ff0000000000000) },
    Sample { operation: "expm1", bits: 64, input: 0xfff0000000000000, expected: Some(0xbff0000000000000) },
    Sample { operation: "expm1", bits: 64, input: 0x7ff8000000000000, expected: None },
    Sample { operation: "expm1", bits: 64, input: 0x1, expected: Some(0x1) },
    Sample { operation: "expm1", bits: 64, input: 0x8000000000000001, expected: Some(0x8000000000000001) },
    Sample { operation: "expm1", bits: 64, input: 0x4085e00000000000, expected: Some(0x7f0d945df4f8ec8e) },
    Sample { operation: "expm1", bits: 64, input: 0x4086300000000000, expected: Some(0x7ff0000000000000) },
    Sample { operation: "expm1", bits: 64, input: 0xc087700000000000, expected: Some(0xbff0000000000000) },
    Sample { operation: "log1p", bits: 64, input: 0x0, expected: Some(0x0) },
    Sample { operation: "log1p", bits: 64, input: 0x8000000000000000, expected: Some(0x8000000000000000) },
    Sample { operation: "log1p", bits: 64, input: 0x3ca0000000000000, expected: Some(0x3ca0000000000000) },
    Sample { operation: "log1p", bits: 64, input: 0xbca0000000000000, expected: Some(0xbca0000000000000) },
    Sample { operation: "log1p", bits: 64, input: 0x3cb0000000000000, expected: Some(0x3cafffffffffffff) },
    Sample { operation: "log1p", bits: 64, input: 0xbcb0000000000000, expected: Some(0xbcb0000000000001) },
    Sample { operation: "log1p", bits: 64, input: 0x3e45798ee2308c3a, expected: Some(0x3e45798ee0636111) },
    Sample { operation: "log1p", bits: 64, input: 0xbe45798ee2308c3a, expected: Some(0xbe45798ee3fdb764) },
    Sample { operation: "log1p", bits: 64, input: 0x3fd0000000000000, expected: Some(0x3fcc8ff7c79a9a22) },
    Sample { operation: "log1p", bits: 64, input: 0xbfd0000000000000, expected: Some(0xbfd269621134db92) },
    Sample { operation: "log1p", bits: 64, input: 0x3fe0000000000000, expected: Some(0x3fd9f323ecbf984c) },
    Sample { operation: "log1p", bits: 64, input: 0xbfe0000000000000, expected: Some(0xbfe62e42fefa39ef) },
    Sample { operation: "log1p", bits: 64, input: 0x3fefae147ae147ae, expected: Some(0x3fe60532ef13c385) },
    Sample { operation: "log1p", bits: 64, input: 0xbfefae147ae147ae, expected: Some(0xc0126bb1bbb55515) },
    Sample { operation: "log1p", bits: 64, input: 0x3ff0000000000000, expected: Some(0x3fe62e42fefa39ef) },
    Sample { operation: "log1p", bits: 64, input: 0x4000000000000000, expected: Some(0x3ff193ea7aad030b) },
    Sample { operation: "log1p", bits: 64, input: 0x4034000000000000, expected: Some(0x40085b2e946faeb1) },
    Sample { operation: "log1p", bits: 64, input: 0xc034000000000000, expected: None },
    Sample { operation: "log1p", bits: 64, input: 0x7ff0000000000000, expected: Some(0x7ff0000000000000) },
    Sample { operation: "log1p", bits: 64, input: 0xfff0000000000000, expected: None },
    Sample { operation: "log1p", bits: 64, input: 0x7ff8000000000000, expected: None },
    Sample { operation: "log1p", bits: 64, input: 0x1, expected: Some(0x1) },
    Sample { operation: "log1p", bits: 64, input: 0x8000000000000001, expected: Some(0x8000000000000001) },
    Sample { operation: "log1p", bits: 64, input: 0xbff0000000000000, expected: Some(0xfff0000000000000) },
    Sample { operation: "log1p", bits: 64, input: 0xbfefffffffffffff, expected: Some(0xc0425e4f7b2737fa) },
    Sample { operation: "log1p", bits: 64, input: 0xbff0000000000001, expected: None },
    Sample { operation: "log1p", bits: 64, input: 0x7fefffffffffffff, expected: Some(0x40862e42fefa39ef) },
];

/// C supplies exact IEEE inputs and checks independent expected bits. It never computes math.
pub fn sources() -> (String, String, usize) {
    let mut c = String::from(
        "#include <stdint.h>\n#include <string.h>\n#include <math.h>\n#include <stdio.h>\n#include <stdlib.h>\nstatic int64_t observed;\nint64_t math_observed(void){return observed;}\n",
    );
    let mut source =
        String::from("extern \"C\" fn math_observed() -> i64\npub fn keep<T>(value:T)->T=value\n");
    let mut main = String::from("pub fn exercise() -> i32 { unsafe {\n");
    let mut count = 0;
    for bits in [32, 64] {
        let ty = if bits == 32 { "float" } else { "double" };
        let int = format!("uint{bits}_t");
        let sign = 1u64 << (bits - 1);
        let exponent = if bits == 32 {
            0x7f80_0000u64
        } else {
            0x7ff0_0000_0000_0000
        };
        let rows = SAMPLES
            .iter()
            .filter(|row| row.bits == bits)
            .collect::<Vec<_>>();
        c.push_str(&format!(
            "static const struct {{ {int} input,expected; int nan; }} samples{bits}[]={{\n"
        ));
        for row in &rows {
            c.push_str(&format!(
                "{{{}ULL,{}ULL,{}}},\n",
                row.input,
                row.expected.unwrap_or(0),
                i32::from(row.expected.is_none())
            ));
        }
        c.push_str(&format!("}};\n{ty} math_get{bits}(int64_t i){{if(i<0||i>={})abort(); {ty} x; memcpy(&x,&samples{bits}[i].input,sizeof(x));return x;}}\n",rows.len()));
        c.push_str(&format!("int32_t math_check{bits}({ty} x,int64_t i){{if(i<0||i>={})abort();observed++; if(samples{bits}[i].nan)return isnan(x)?0:1; {int} actual;memcpy(&actual,&x,sizeof(x)); {int} expected=samples{bits}[i].expected; {int} a=(actual&{sign}ULL)?({int})~actual:(actual|{sign}ULL); {int} e=(expected&{sign}ULL)?({int})~expected:(expected|{sign}ULL); {int} distance=a>e?a-e:e-a; int exact=(expected&~{sign}ULL)==0||(expected&{exponent}ULL)=={exponent}ULL; if(distance>(exact?0:4)){{fprintf(stderr,\"math{bits} sample %lld differs by %llu ULP\\n\",(long long)i,(unsigned long long)distance);return 2;}}return 0;}}\n",rows.len()));
        source.push_str(&format!("extern \"C\" fn math_get{bits}(index:i64)->f{bits}\nextern \"C\" fn math_check{bits}(value:f{bits},index:i64)->i32\n"));
        for operation in ["expm1", "log1p"] {
            let indices = rows
                .iter()
                .enumerate()
                .filter_map(|(i, row)| (row.operation == operation).then_some(i))
                .collect::<Vec<_>>();
            for lanes in [1, 2, 4, 8, 16] {
                let name = format!("case_{operation}_{bits}_{lanes}");
                let value_ty = if lanes == 1 {
                    format!("f{bits}")
                } else {
                    format!("vec{lanes}<f{bits}>")
                };
                source.push_str(&format!(
                    "pub fn {name}(value:{value_ty}) -> {value_ty} = keep(value.{operation}())\n"
                ));
                let runner = format!("run_{operation}_{bits}_{lanes}");
                let mut body = format!("fn {runner}()->i32 {{ unsafe {{\n");
                for (batch, chunk) in indices.chunks(lanes).enumerate() {
                    let selected = (0..lanes)
                        .map(|i| chunk[i % chunk.len()])
                        .collect::<Vec<_>>();
                    let inputs = selected
                        .iter()
                        .map(|i| format!("math_get{bits}({i})"))
                        .collect::<Vec<_>>();
                    let input = if lanes == 1 {
                        inputs[0].clone()
                    } else {
                        format!("[{}]", inputs.join(","))
                    };
                    body.push_str(&format!("value{batch}:={name}({input})\n"));
                    for (lane, index) in selected.iter().enumerate() {
                        let value = if lanes == 1 {
                            format!("value{batch}")
                        } else {
                            format!("value{batch}[{lane}]")
                        };
                        body.push_str(&format!(
                            "if math_check{bits}({value},{index})!=0 {{ return 1 }}\n"
                        ));
                        count += 1;
                    }
                }
                body.push_str("return 0\n} }\n");
                source.push_str(&body);
                main.push_str(&format!("if {runner}()!=0 {{ return 1 }}\n"));
            }
        }
    }
    main.push_str(&format!(
        "if math_observed()!={count} {{ return 2 }}\nreturn 0\n}} }}\n"
    ));
    source.push_str(&main);
    (c, source, count)
}
