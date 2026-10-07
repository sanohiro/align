//! Runs inside the C-layout owner's bounded process group and artifact stage.
use std::{fs, path::Path};
struct Record {
    name: String,
    fields: Vec<&'static str>,
    alignment: u32,
    fp: Option<usize>,
    gp: usize,
}
fn c_type(ty: &str) -> &str {
    match ty {
        "i8" => "int8_t",
        "u8" => "uint8_t",
        "i16" => "int16_t",
        "u16" => "uint16_t",
        "i32" => "int32_t",
        "u32" => "uint32_t",
        "i64" => "int64_t",
        "u64" => "uint64_t",
        "f32" => "float",
        "f64" => "double",
        "raw" => "void *",
        _ => panic!("unknown case"),
    }
}
fn value(ty: &str, index: usize, changed: bool, c: bool) -> String {
    if ty == "raw" {
        return if changed {
            if c { "NULL" } else { "raw.null()" }
        } else if c {
            "&pointee"
        } else {
            "probe_data()"
        }
        .into();
    }
    let n = i32::try_from(index).unwrap() + i32::from(changed);
    if ty.starts_with('f') {
        format!("{}.5", n + 1)
    } else if ty.starts_with('i') {
        (n - 20).to_string()
    } else {
        (n + 200).to_string()
    }
}
fn check(name: &str, ty: &str, i: usize, changed: bool) -> String {
    if ty == "raw" {
        if changed {
            format!("!{name}.f{i}.is_null()")
        } else {
            format!("probe_pointer({name}.f{i}) != 1")
        }
    } else {
        format!("{name}.f{i} != {}", value(ty, i, changed, false))
    }
}
pub(super) fn run(stage: &Path) {
    let directory = stage.join("aarch64-values-stage");
    fs::create_dir(&directory).unwrap();
    let stage = directory.as_path();
    let mut records = Vec::new();
    for (name, fields, alignment, gp) in [
        ("Byte", vec!["u8"], 0, 1),
        ("Three", vec!["u8"; 3], 0, 1),
        ("Int", vec!["i64"], 0, 1),
        ("Pair", vec!["i64", "u64"], 0, 2),
        ("Widths", vec!["i8", "u16", "i32", "u64"], 0, 2),
        ("OtherWidths", vec!["u8", "i16", "u32", "i64"], 0, 2),
        ("Twelve", vec!["i32"; 3], 0, 2),
        ("Mix", vec!["i64", "f64"], 0, 2),
        ("Pointer", vec!["raw"], 0, 1),
        ("Pointers", vec!["raw"; 2], 0, 2),
        ("First", vec!["raw", "u8", "f64"], 0, 1),
        ("Middle", vec!["u8", "raw", "f64"], 0, 1),
        ("Last", vec!["u8", "f64", "raw"], 0, 1),
        ("P16", vec!["raw"], 16, 2),
        ("I16", vec!["i64"], 16, 2),
        ("F16", vec!["f64"], 16, 2),
        ("FF16", vec!["f32"; 2], 16, 2),
        ("P32", vec!["raw"], 32, 1),
        ("DD32", vec!["f64"; 2], 32, 1),
    ] {
        records.push(Record {
            name: name.into(),
            fields,
            alignment,
            gp,
            fp: None,
        });
    }
    for ty in ["f32", "f64"] {
        for n in 1..=5 {
            records.push(Record {
                name: format!("H{ty}{n}"),
                fields: vec![ty; n],
                alignment: 0,
                gp: 1,
                fp: (n <= 4).then_some(n),
            });
        }
    }
    for (name, ty, n) in [("AlignedH4", "f32", 4), ("AlignedH2", "f64", 2)] {
        records.push(Record {
            name: name.into(),
            fields: vec![ty; n],
            alignment: 16,
            gp: 0,
            fp: Some(n),
        });
    }
    let mut c = String::from(
        "#include <stdint.h>\n#include <stddef.h>\n#include <stdlib.h>\nstatic int pointee=42;\nstatic int64_t calls;\nint64_t probe_seen(void){return calls;}\nvoid *probe_data(void){return &pointee;}\nint32_t probe_pointer(void *p){return p==&pointee && *(int*)p==42;}\nstatic void check_alignment(void *p,size_t a){volatile uintptr_t n=(uintptr_t)p;if(n%a)abort();}\n",
    );
    let mut factory = String::from(
        "module factory\nextern \"C\" fn probe_data() -> raw\nextern \"C\" fn probe_pointer(value:raw) -> i32\npub fn keep<T>(value:T) -> T = value\n",
    );
    factory.push_str("extern \"C\" fn probe_seen() -> i64\n");
    let mut body = String::from("pub fn exercise() -> i32 { unsafe {\n");
    let mut count = 0;
    for row in &records {
        let name = &row.name;
        let cf = row
            .fields
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{} f{i};", c_type(t)))
            .collect::<String>();
        let af = row
            .fields
            .iter()
            .enumerate()
            .map(|(i, t)| format!("f{i}:{t}"))
            .collect::<Vec<_>>()
            .join(",");
        let values = row
            .fields
            .iter()
            .enumerate()
            .map(|(i, t)| format!("f{i}:{}", value(t, i, false, false)))
            .collect::<Vec<_>>()
            .join(",");
        let ca = if row.alignment > 0 {
            format!("__attribute__((aligned({})))", row.alignment)
        } else {
            String::new()
        };
        let aa = if row.alignment > 0 {
            format!("align({}) ", row.alignment)
        } else {
            String::new()
        };
        c.push_str(&format!("struct {ca} {name} {{ {cf} }};\n"));
        factory.push_str(&format!("pub {aa}layout(C) {name} {{ {af} }}\n"));
        body.push_str(&format!("original{name} := {name} {{ {values} }}\n"));
        let (width, floating) = row.fp.map_or((row.gp, false), |n| (n, true));
        let mut pressure = vec![(0, false), (8, !floating)];
        for n in [8 - width, 9 - width, 8, 9] {
            pressure.push((n, floating));
        }
        pressure.sort_unstable();
        pressure.dedup();
        for (preceding, fp) in pressure {
            let suffix = format!("{name}_{preceding}_{fp}");
            let mut cp = Vec::new();
            let mut ap = Vec::new();
            let mut args = Vec::new();
            let mut guards = Vec::new();
            for i in 0..preceding {
                cp.push(format!("{} a{i}", if fp { "double" } else { "int64_t" }));
                ap.push(format!("a{i}:{}", if fp { "f64" } else { "i64" }));
                args.push(if fp {
                    format!("{}.0", i + 1)
                } else {
                    (i + 1).to_string()
                });
                guards.push(format!("a{i}!={}", i + 1));
            }
            cp.extend([
                format!("struct {name} v"),
                format!("struct {name} other"),
                "int8_t signed_byte".into(),
                "uint16_t unsigned_short".into(),
                "float tail_fp".into(),
                "int64_t tail".into(),
            ]);
            ap.extend([
                format!("v:{name}"),
                format!("other:{name}"),
                "signed_byte:i8".into(),
                "unsigned_short:u16".into(),
                "tail_fp:f32".into(),
                "tail:i64".into(),
            ]);
            args.extend([
                format!("original{name}"),
                format!("original{name}"),
                "-2".into(),
                "60000".into(),
                "0.25".into(),
                "77".into(),
            ]);
            guards.extend([
                "signed_byte!=-2".into(),
                "unsigned_short!=60000".into(),
                "tail_fp!=0.25f".into(),
                "tail!=77".into(),
            ]);
            for (i, t) in row.fields.iter().enumerate() {
                guards.push(format!("v.f{i}!={}", value(t, i, false, true)));
            }
            let other = row
                .fields
                .iter()
                .enumerate()
                .map(|(i, t)| format!("other.f{i}!={}", value(t, i, false, true)))
                .collect::<Vec<_>>()
                .join("||");
            c.push_str(&format!("struct {name} echo_{suffix}({}) {{ calls++; if({})abort(); check_alignment(&v,_Alignof(struct {name})); check_alignment(&other,_Alignof(struct {name})); v.f0={}; if({other})abort(); return v; }}\n",cp.join(","),guards.join("||"),value(row.fields[0],0,true,true)));
            factory.push_str(&format!(
                "extern \"C\" fn echo_{suffix}({}) -> {name}\n",
                ap.join(",")
            ));
            body.push_str(&format!(
                "r{suffix} := keep(echo_{suffix}({}))\n",
                args.join(",")
            ));
            for (i, t) in row.fields.iter().enumerate() {
                body.push_str(&format!(
                    "if {} || {} {{ return 1 }}\n",
                    check(&format!("r{suffix}"), t, i, i == 0),
                    check(&format!("original{name}"), t, i, false)
                ));
            }
            count += 1;
        }
    }
    body.push_str("mut index := 0\nloop { if index == 4096 { break }; repeated := echo_P32_0_false(originalP32,originalP32,-2,60000,0.25,77); if !repeated.f0.is_null() || probe_pointer(originalP32.f0) != 1 { return 2 }; index = index + 1 }\nreturn 0\n} }\n");
    body = body.replace(
        "return 0\n} }",
        &format!(
            "if probe_seen() != {} {{ return 3 }}\nreturn 0\n}} }}",
            count + 4096
        ),
    );
    factory.push_str(&body);
    super::compile_c(stage, &c);
    fs::write(stage.join("factory.align"), factory).unwrap();
    for per_unit in [false, true] {
        super::build_and_run(
            stage,
            "import factory\nfn main() -> i32 = factory.exercise()\n",
            per_unit,
            "arm-values",
        );
    }
    println!("ARM64 native record cases: {count}, whole/per-unit");
}
