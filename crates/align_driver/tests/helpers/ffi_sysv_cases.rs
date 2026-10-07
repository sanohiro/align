//! One native input matrix shared by driver execution and target-independent codegen owners.
struct Record {
    name: String,
    fields: Vec<&'static str>,
    alignment: usize,
    size: usize,
    gp: usize,
    fp: usize,
}

fn c_type(ty: &str) -> &str {
    match ty {
        "i8" => "I8",
        "u8" => "U8",
        "i16" => "I16",
        "u16" => "U16",
        "i32" => "I32",
        "u32" => "U32",
        "i64" => "I64",
        "u64" => "U64",
        "f32" => "float",
        "f64" => "double",
        "raw" => "void *",
        _ => "invalid_case_type",
    }
}

fn value(ty: &str, index: usize, changed: bool, c: bool) -> String {
    let n = index + usize::from(changed);
    if ty == "raw" {
        if changed {
            if c { "((void*)0)" } else { "raw.null()" }
        } else if c {
            "&pointee"
        } else {
            "sysv_data()"
        }
        .into()
    } else if ty.starts_with('f') {
        format!("{}.5", n + 1)
    } else if ty.starts_with('i') {
        format!("-{}", n + 1)
    } else {
        (n + 20).to_string()
    }
}

fn check(name: &str, ty: &str, i: usize, changed: bool) -> String {
    if ty == "raw" {
        if changed {
            format!("!{name}.f{i}.is_null()")
        } else {
            format!("sysv_pointer({name}.f{i}) != 1")
        }
    } else {
        format!("{name}.f{i} != {}", value(ty, i, changed, false))
    }
}

/// C translation unit, moduleless Align factory, and exact expected native call count.
pub(crate) fn sources() -> (String, String, usize) {
    let mut rows = Vec::new();
    for (name, fields, alignment, size, gp, fp) in [
        ("Byte", vec!["u8"], 0, 1, 1, 0),
        ("Three", vec!["u8"; 3], 0, 3, 1, 0),
        ("Int", vec!["i64"], 0, 8, 1, 0),
        ("Pair", vec!["i64", "u64"], 0, 16, 2, 0),
        ("Widths", vec!["i8", "u16", "i32", "u64"], 0, 16, 2, 0),
        ("OtherWidths", vec!["u8", "i16", "u32", "i64"], 0, 16, 2, 0),
        ("Twelve", vec!["i32"; 3], 0, 12, 2, 0),
        ("Mix", vec!["i64", "f64"], 0, 16, 1, 1),
        ("Reverse", vec!["f64", "i64"], 0, 16, 1, 1),
        ("Pointer", vec!["raw"], 0, 8, 1, 0),
        ("Pointers", vec!["raw"; 2], 0, 16, 2, 0),
        ("First", vec!["raw", "u8", "f64"], 0, 24, 0, 0),
        ("Middle", vec!["u8", "raw", "f64"], 0, 24, 0, 0),
        ("Last", vec!["u8", "f64", "raw"], 0, 24, 0, 0),
        ("Bytes17", vec!["u8"; 17], 0, 17, 0, 0),
        ("P16", vec!["raw"], 16, 16, 1, 0),
        ("I16", vec!["i64"], 16, 16, 1, 0),
        ("F16", vec!["f64"], 16, 16, 0, 1),
        ("FF16", vec!["f32"; 2], 16, 16, 0, 1),
        ("P32", vec!["raw"], 32, 32, 0, 0),
        ("DD32", vec!["f64"; 2], 32, 32, 0, 0),
        ("I64", vec!["i64"], 64, 64, 0, 0),
        ("Page", vec!["raw"], 4096, 4096, 0, 0),
    ] {
        rows.push(Record {
            name: name.into(),
            fields,
            alignment,
            size,
            gp,
            fp,
        });
    }
    for (ty, bytes) in [("f32", 4), ("f64", 8)] {
        for n in 1..=5 {
            let size = n * bytes;
            rows.push(Record {
                name: format!("F{bytes}N{n}"),
                fields: vec![ty; n],
                alignment: 0,
                size,
                gp: 0,
                fp: if size <= 16 { size.div_ceil(8) } else { 0 },
            });
        }
    }
    let mut c = String::from(
        "typedef __INT8_TYPE__ I8; typedef __UINT8_TYPE__ U8;\ntypedef __INT16_TYPE__ I16; typedef __UINT16_TYPE__ U16;\ntypedef __INT32_TYPE__ I32; typedef __UINT32_TYPE__ U32;\ntypedef __INT64_TYPE__ I64; typedef __UINT64_TYPE__ U64;\ntypedef __UINTPTR_TYPE__ UP; extern void abort(void);\nstatic int pointee=42; static I64 calls;\nvoid *sysv_data(void){return &pointee;}\nI32 sysv_pointer(void *p){return p==&pointee && *(int*)p==42;}\nI64 sysv_seen(void){return calls;}\nstatic void check_alignment(void *p,UP a){volatile UP n=(UP)p;if(n%a)abort();}\nstruct Sret { I64 a,b,c; };\n",
    );
    let mut source = String::from(
        "extern \"C\" fn sysv_data() -> raw\nextern \"C\" fn sysv_pointer(value:raw) -> i32\nextern \"C\" fn sysv_seen() -> i64\npub fn keep<T>(value:T) -> T = value\npub layout(C) Sret { a:i64,b:i64,c:i64 }\n",
    );
    let mut body = String::from("pub fn exercise() -> i32 { unsafe {\n");
    let mut count = 0;
    for row in &rows {
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
        let ca = if row.alignment == 0 {
            String::new()
        } else {
            format!("__attribute__((aligned({})))", row.alignment)
        };
        let aa = if row.alignment == 0 {
            String::new()
        } else {
            format!("align({}) ", row.alignment)
        };
        c.push_str(&format!("struct {ca} {name} {{ {cf} }};\n"));
        source.push_str(&format!("pub {aa}layout(C) {name} {{ {af} }}\n"));
        body.push_str(&format!("original{name} := {name} {{ {values} }}\n"));
        for force_sret in [false, true] {
            if force_sret
                && (row.size > 16 || !matches!(name.as_str(), "Pair" | "Mix" | "Reverse" | "F8N2"))
            {
                continue;
            }
            let gp_budget = 6 - usize::from(force_sret || row.size > 16);
            let mut pressure = vec![(0, 0), (6, 0), (7, 0), (0, 8), (0, 9)];
            if row.gp > 0 {
                pressure.extend([(gp_budget - row.gp, 0), (gp_budget - row.gp + 1, 0)]);
            }
            if row.fp > 0 {
                pressure.extend([(0, 8 - row.fp), (0, 9 - row.fp)]);
            }
            if row.gp > 0 && row.fp > 0 {
                pressure.extend([(5, 8), (6, 7)]);
            }
            pressure.sort_unstable();
            pressure.dedup();
            for (gp, fp) in pressure {
                let symbol = format!("sysv_{name}_{gp}_{fp}_{force_sret}");
                let mut cp = Vec::new();
                let mut ap = Vec::new();
                let mut args = Vec::new();
                let mut guards = Vec::new();
                for (n, floating) in [(gp, false), (fp, true)] {
                    for i in 0..n {
                        let p = format!("{}p{i}", if floating { "f" } else { "g" });
                        cp.push(format!("{} {p}", if floating { "double" } else { "I64" }));
                        ap.push(format!("{p}:{}", if floating { "f64" } else { "i64" }));
                        args.push(if floating {
                            format!("{}.0", i + 1)
                        } else {
                            (i + 1).to_string()
                        });
                        guards.push(format!("{p}!={}", i + 1));
                    }
                }
                cp.extend([
                    format!("struct {name} v"),
                    format!("struct {name} other"),
                    "I8 signed_byte".into(),
                    "U16 unsigned_short".into(),
                    "float tail_fp".into(),
                    "I64 tail".into(),
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
                let ret = if force_sret { "Sret" } else { name };
                let result = if force_sret {
                    "(struct Sret){21,42,77}"
                } else {
                    "v"
                };
                c.push_str(&format!("struct {ret} {symbol}({}) {{ calls++; if({})abort(); check_alignment(&v,_Alignof(struct {name})); check_alignment(&other,_Alignof(struct {name})); v.f0={}; if({other})abort(); return {result}; }}\n",cp.join(","),guards.join("||"),value(row.fields[0],0,true,true)));
                source.push_str(&format!(
                    "extern \"C\" fn {symbol}({}) -> {ret}\n",
                    ap.join(",")
                ));
                body.push_str(&format!(
                    "result{count} := keep({symbol}({}))\n",
                    args.join(",")
                ));
                let mut checks = row
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(i, t)| check(&format!("original{name}"), t, i, false))
                    .collect::<Vec<_>>();
                if force_sret {
                    checks.push(format!(
                        "result{count}.a != 21 || result{count}.b != 42 || result{count}.c != 77"
                    ));
                } else {
                    checks.extend(
                        row.fields
                            .iter()
                            .enumerate()
                            .map(|(i, t)| check(&format!("result{count}"), t, i, i == 0)),
                    );
                }
                body.push_str(&format!("if {} {{ return 1 }}\n", checks.join(" || ")));
                count += 1;
            }
        }
    }
    // A failed pair must leave its last GP register for a later small record, and a failed
    // mixed record must retain its opposite-class register for a later aggregate.
    c.push_str("I32 sysv_rollback_gp(I64 a,I64 b,I64 c,I64 d,I64 e,struct Pair p,struct Pointer q,I64 tail){calls++;return a==1&&b==2&&c==3&&d==4&&e==5&&p.f0==-1&&p.f1==21&&q.f0==&pointee&&tail==77;}\nI32 sysv_rollback_fp(double a,double b,double c,double d,double e,double f,double g,struct F8N2 p,struct F8N1 q,double tail){calls++;return a==1&&b==2&&c==3&&d==4&&e==5&&f==6&&g==7&&p.f0==1.5&&p.f1==2.5&&q.f0==1.5&&tail==77;}\nI32 sysv_rollback_mixed(double a,double b,double c,double d,double e,double f,double g,double h,struct Mix p,struct Pointers q,I64 tail){calls++;return a==1&&b==2&&c==3&&d==4&&e==5&&f==6&&g==7&&h==8&&p.f0==-1&&p.f1==2.5&&q.f0==&pointee&&q.f1==&pointee&&tail==77;}\nI32 sysv_rollback_reverse(I64 a,I64 b,I64 c,I64 d,I64 e,I64 f,struct Reverse p,struct F8N2 q,double tail){calls++;return a==1&&b==2&&c==3&&d==4&&e==5&&f==6&&p.f0==1.5&&p.f1==-2&&q.f0==1.5&&q.f1==2.5&&tail==77;}\n");
    source.push_str("extern \"C\" fn sysv_rollback_gp(a:i64,b:i64,c:i64,d:i64,e:i64,p:Pair,q:Pointer,tail:i64) -> i32\nextern \"C\" fn sysv_rollback_fp(a:f64,b:f64,c:f64,d:f64,e:f64,f:f64,g:f64,p:F8N2,q:F8N1,tail:f64) -> i32\nextern \"C\" fn sysv_rollback_mixed(a:f64,b:f64,c:f64,d:f64,e:f64,f:f64,g:f64,h:f64,p:Mix,q:Pointers,tail:i64) -> i32\nextern \"C\" fn sysv_rollback_reverse(a:i64,b:i64,c:i64,d:i64,e:i64,f:i64,p:Reverse,q:F8N2,tail:f64) -> i32\n");
    body.push_str("if sysv_rollback_gp(1,2,3,4,5,originalPair,originalPointer,77) != 1 { return 1 }\nif sysv_rollback_fp(1.0,2.0,3.0,4.0,5.0,6.0,7.0,originalF8N2,originalF8N1,77.0) != 1 { return 1 }\nif sysv_rollback_mixed(1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,originalMix,originalPointers,77) != 1 { return 1 }\nif sysv_rollback_reverse(1,2,3,4,5,6,originalReverse,originalF8N2,77.0) != 1 { return 1 }\nmut iteration := 0\nloop { if iteration == 4096 { break }; repeated := sysv_P32_0_0_false(originalP32,originalP32,-2,60000,0.25,77); if !repeated.f0.is_null() || sysv_pointer(originalP32.f0) != 1 { return 1 }; iteration = iteration + 1 }\n");
    c.push_str("struct Sret sysv_view_pressure(I64 a,I64 b,I64 c,const char *text,struct Pair p,struct Pointer q){calls++;if(a!=1||b!=2||c!=3||text[0]!='o'||text[1]!='k'||p.f0!=-1||p.f1!=21||q.f0!=&pointee)abort();return (struct Sret){21,42,77};}\nI32 sysv_before_memory(struct Sret big,I64 a,I64 b,I64 c,I64 d,struct Pair p){calls++;return big.a==21&&big.b==42&&big.c==77&&a==1&&b==2&&c==3&&d==4&&p.f0==-1&&p.f1==21;}\n");
    source.push_str("extern \"C\" fn sysv_view_pressure(a:i64,b:i64,c:i64,text:str,p:Pair,q:Pointer) -> Sret\nextern \"C\" fn sysv_before_memory(big:Sret,a:i64,b:i64,c:i64,d:i64,p:Pair) -> i32\n");
    body.push_str("view_result := sysv_view_pressure(1,2,3,\"ok\",originalPair,originalPointer)\nif view_result.a != 21 || view_result.b != 42 || view_result.c != 77 { return 1 }\nif sysv_before_memory(Sret { a:21,b:42,c:77 },1,2,3,4,originalPair) != 1 { return 1 }\n");
    count += 6 + 4096;
    body.push_str(&format!(
        "if sysv_seen() != {count} {{ return 1 }}\nreturn 0\n}} }}\n"
    ));
    source.push_str(&body);
    (c, source, count)
}
