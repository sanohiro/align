//! Shared numeric C-field fixtures for native execution and physical ABI inspection.

#[derive(Clone)]
pub struct Field {
    pub ty: &'static str,
    pub count: Option<usize>,
}
#[derive(Clone)]
pub struct Record {
    pub name: String,
    pub fields: Vec<Field>,
    pub alignment: usize,
}
fn array(ty: &'static str, count: usize) -> Field {
    Field {
        ty,
        count: Some(count),
    }
}
fn scalar(ty: &'static str) -> Field {
    Field { ty, count: None }
}

pub fn records() -> Vec<Record> {
    let mut rows = Vec::new();
    for ty in [
        "i8", "u8", "i16", "u16", "i32", "u32", "i64", "u64", "f32", "f64",
    ] {
        rows.push(Record {
            name: format!("Cfa{ty}"),
            fields: vec![array(ty, 3)],
            alignment: 0,
        });
    }
    for ty in ["f32", "f64"] {
        for count in [1, 2, 4, 5] {
            rows.push(Record {
                name: format!("Cfa{ty}n{count}"),
                fields: vec![array(ty, count)],
                alignment: 0,
            });
        }
    }
    for (name, fields, alignment) in [
        ("CfaMixedIS", vec![scalar("i32"), array("f32", 3)], 0),
        ("CfaMixedSI", vec![array("f32", 2), scalar("i32")], 0),
        ("CfaMerged", vec![array("i16", 2), array("f32", 1)], 0),
        ("CfaHfaJoined", vec![array("f32", 2), scalar("f32")], 0),
        ("CfaPointer", vec![array("i16", 3), scalar("raw")], 0),
        ("CfaAlignedHfa", vec![array("f32", 4)], 16),
        ("CfaPadded", vec![array("f32", 3)], 16),
        (
            "CfaWide",
            vec![scalar("i8"), array("f64", 2), scalar("u16")],
            32,
        ),
        ("CfaShape", vec![array("i64", 8), array("i64", 8)], 0),
        ("CfaKey", vec![array("i64", 41)], 0),
    ] {
        rows.push(Record {
            name: name.into(),
            fields,
            alignment,
        });
    }
    rows
}
fn c_type(ty: &str) -> &'static str {
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
        _ => panic!("fixture type"),
    }
}
fn value(ty: &str, field: usize, element: usize, changed: bool, c: bool) -> String {
    let base: i128 = match ty {
        "i8" => -7,
        "i16" => -300,
        "i32" => -70000,
        "i64" => -9000000000,
        "u8" => 180,
        "u16" => 50000,
        "u32" => 3000000000,
        "u64" => 18000000000000000000,
        _ => 3,
    };
    let number = base + field as i128 * 3 + element as i128 + i128::from(changed) * 50;
    if matches!(ty, "f32" | "f64") {
        format!("{number}.25")
    } else if c && ty == "u64" {
        format!("{number}ULL")
    } else {
        number.to_string()
    }
}
fn initializer(record: &Record) -> String {
    let fields = record
        .fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let value = if let Some(n) = field.count {
                format!(
                    "[{}]",
                    (0..n)
                        .map(|j| value(field.ty, i, j, false, false))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            } else if field.ty == "raw" {
                "raw.null()".into()
            } else {
                value(field.ty, i, 0, false, false)
            };
            format!("f{i}: {value}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{} {{ {fields} }}", record.name)
}
fn align_checks(record: &Record, variable: &str, changed: bool) -> String {
    let mut result = String::new();
    for (i, field) in record.fields.iter().enumerate() {
        for j in 0..field.count.unwrap_or(1) {
            let place = if field.count.is_some() {
                format!("{variable}.f{i}[{j}]")
            } else {
                format!("{variable}.f{i}")
            };
            if field.ty == "raw" {
                result.push_str(&format!("if !{place}.is_null() {{ return 11 }}\n"));
            } else {
                result.push_str(&format!(
                    "if {place} != {} {{ return 12 }}\n",
                    value(field.ty, i, j, changed, false)
                ));
            }
        }
    }
    result
}

/// Returns the C oracle, Align declarations/exercise function, and exact foreign call count.
pub fn sources() -> (String, String, usize) {
    sources_for_revision(0)
}

pub fn sources_for_revision(revision: u8) -> (String, String, usize) {
    let mut records = records();
    match revision {
        0 => {}
        1 => records[0].fields[0].ty = "i16",
        2 => records[0].fields[0].count = Some(5),
        _ => panic!("fixture revision"),
    }
    let mut c = String::from(
        "#include <stdint.h>\n#include <stddef.h>\n#include <stdlib.h>\n#include <string.h>\n#include <stdalign.h>\nstatic int64_t seen;\nstatic unsigned char storage[4096];\nint64_t cfa_seen(void){return seen;}\nvoid *cfa_storage(void){memset(storage,0xA5,sizeof(storage));return storage;}\n",
    );
    let mut source = String::from(
        "extern \"C\" fn cfa_seen() -> i64\nextern \"C\" fn cfa_storage() -> raw\npub fn keep<T>(x:T) -> T = x\n",
    );
    let mut main = String::from("pub fn exercise() -> i32 { unsafe {\n");
    let mut count = 0;
    for (ordinal, record) in records.iter().enumerate() {
        let name = &record.name;
        let mut body = format!("fn exercise_{ordinal}() -> i32 {{ unsafe {{\n");
        main.push_str(&format!("if exercise_{ordinal}() != 0 {{ return 20 }}\n"));
        source.push_str("pub ");
        if record.alignment > 0 {
            source.push_str(&format!("align({}) ", record.alignment));
        }
        source.push_str(&format!("layout(C) {name} {{ "));
        c.push_str("typedef struct ");
        if record.alignment > 0 {
            c.push_str(&format!("__attribute__((aligned({}))) ", record.alignment));
        }
        c.push_str("{ ");
        for (i, field) in record.fields.iter().enumerate() {
            let ty = field.ty;
            if let Some(n) = field.count {
                source.push_str(&format!("f{i}: [{ty}; {n}], "));
                c.push_str(&format!("{} f{i}[{n}]; ", c_type(ty)));
            } else {
                source.push_str(&format!("f{i}: {ty}, "));
                c.push_str(&format!("{} f{i}; ", c_type(ty)));
            }
        }
        source.push_str("}\n");
        c.push_str(&format!("}} {name};\n"));
        // Pin C's independent layout, including tail padding, rather than only reading values.
        let mut offset = 0usize;
        let mut alignment = record.alignment.max(1);
        for (i, field) in record.fields.iter().enumerate() {
            let bytes = match field.ty {
                "i8" | "u8" => 1,
                "i16" | "u16" => 2,
                "i32" | "u32" | "f32" => 4,
                _ => 8,
            };
            alignment = alignment.max(bytes);
            offset = offset.next_multiple_of(bytes);
            c.push_str(&format!(
                "_Static_assert(offsetof({name},f{i})=={offset},\"field offset\");\n"
            ));
            offset += bytes * field.count.unwrap_or(1);
        }
        let size = offset.next_multiple_of(alignment);
        c.push_str(&format!("_Static_assert(sizeof({name})=={size},\"record size\");\n_Static_assert(alignof({name})=={alignment},\"record alignment\");\n"));
        c.push_str(&format!(
            "static void check_{name}({name} x,int changed){{\n"
        ));
        for (i, field) in record.fields.iter().enumerate() {
            for j in 0..field.count.unwrap_or(1) {
                let place = if field.count.is_some() {
                    format!("x.f{i}[{j}]")
                } else {
                    format!("x.f{i}")
                };
                let test = if field.ty == "raw" {
                    format!("{place}!=NULL")
                } else {
                    format!("{place}!={}+changed*50", value(field.ty, i, j, false, true))
                };
                c.push_str(&format!("if({test})abort();\n"));
            }
        }
        c.push_str("}\n");
        c.push_str(&format!("int64_t cfa_size_{ordinal}(void){{return sizeof({name});}}\nvoid cfa_raw_{ordinal}(void *p,int32_t changed){{{name} value; memcpy(&value,(char*)p+1,sizeof(value));check_{name}(value,changed);if(storage[0]!=0xA5||storage[sizeof(value)+1]!=0xA5)abort();}}\n"));
        source.push_str(&format!("extern \"C\" fn cfa_size_{ordinal}() -> i64\nextern \"C\" fn cfa_raw_{ordinal}(p:raw,changed:i32)\n"));
        let original = format!("original{ordinal}");
        let initial = if name == "Cfaf32" {
            format!("composed({}) else {{ return 21 }}", initializer(record))
        } else {
            initializer(record)
        };
        body.push_str(&format!("{original} := {}\ncopy{ordinal} := keep({original})\np{ordinal} := cfa_storage()\nraw.store(p{ordinal},1,{original})\ncfa_raw_{ordinal}(p{ordinal},0)\nmut loaded{ordinal}: {name} := raw.load(p{ordinal},1)\n", initial));
        body.push_str(&align_checks(record, &format!("loaded{ordinal}"), false));
        for (i, field) in record.fields.iter().enumerate() {
            if field.ty == "raw" {
                continue;
            }
            for j in 0..field.count.unwrap_or(1) {
                let place = if field.count.is_some() {
                    format!("loaded{ordinal}.f{i}[{j}]")
                } else {
                    format!("loaded{ordinal}.f{i}")
                };
                body.push_str(&format!(
                    "{place} = {}\n",
                    value(field.ty, i, j, true, false)
                ));
            }
        }
        body.push_str(&format!(
            "raw.store(p{ordinal},1,loaded{ordinal})\ncfa_raw_{ordinal}(p{ordinal},1)\n"
        ));
        // Width/stride and padding are checked through independent C reads and boundary sentinels.
        body.push_str(&format!("if cfa_size_{ordinal}() < 1 {{ return 13 }}\n"));
        for (gp, fp) in [
            (0, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0),
            (8, 0),
            (9, 0),
            (0, 4),
            (0, 5),
            (0, 6),
            (0, 7),
            (0, 8),
            (0, 9),
            (9, 9),
        ] {
            let symbol = format!("cfa_echo_{ordinal}_{gp}_{fp}");
            let mut c_params = Vec::new();
            let mut a_params = Vec::new();
            let mut args = Vec::new();
            let mut guards = String::new();
            for n in 0..gp {
                c_params.push(format!("int64_t g{n}"));
                a_params.push(format!("g{n}:i64"));
                args.push((100 + n).to_string());
                guards.push_str(&format!("if(g{n}!={})abort();", 100 + n));
            }
            for n in 0..fp {
                c_params.push(format!("double d{n}"));
                a_params.push(format!("d{n}:f64"));
                args.push(format!("{}.0", 200 + n));
                guards.push_str(&format!("if(d{n}!={})abort();", 200 + n));
            }
            c_params.extend([
                format!("{name} x"),
                "int8_t tail_i".into(),
                "double tail_f".into(),
                format!("{name} y"),
            ]);
            a_params.extend([
                format!("x:{name}"),
                "tail_i:i8".into(),
                "tail_f:f64".into(),
                format!("y:{name}"),
            ]);
            args.extend([
                original.clone(),
                "-7".into(),
                "250.5".into(),
                format!("copy{ordinal}"),
            ]);
            c.push_str(&format!("{name} {symbol}({}){{{guards}if(tail_i!=-7||tail_f!=250.5)abort();check_{name}(x,0);check_{name}(y,0);seen++;",c_params.join(",")));
            for (i, field) in record.fields.iter().enumerate() {
                if field.ty == "raw" {
                    continue;
                }
                for j in 0..field.count.unwrap_or(1) {
                    let place = if field.count.is_some() {
                        format!("x.f{i}[{j}]")
                    } else {
                        format!("x.f{i}")
                    };
                    c.push_str(&format!("{place}={};", value(field.ty, i, j, true, true)));
                }
            }
            c.push_str("return x;}\n");
            source.push_str(&format!(
                "extern \"C\" fn {symbol}({}) -> {name}\n",
                a_params.join(",")
            ));
            let returned = format!("r{ordinal}_{gp}_{fp}");
            body.push_str(&format!("{returned} := {symbol}({})\n", args.join(",")));
            body.push_str(&align_checks(record, &returned, true));
            count += 1;
        }
        body.push_str(&align_checks(record, &original, false));
        body.push_str(&align_checks(record, &format!("copy{ordinal}"), false));
        body.push_str("return 0\n} }\n");
        source.push_str(&body);
    }
    main.push_str(&format!(
        "if cfa_seen() != {count} {{ return 14 }}\nreturn 0\n}} }}\n"
    ));
    source.push_str(
        r#"
fn cfa_ok(value:Cfaf32) -> Result<Cfaf32,i64> = Ok(value)
fn cfa_none() -> Option<Cfaf32> = None
pub fn composed(value:Cfaf32) -> Result<Cfaf32,i64> {
  a := if true { value } else { value }
  b := match 0 { 0 => a, _ => value }
  c := cfa_none() else b
  d := cfa_ok(c)?
  e := cfa_ok(d).map_err(fn error:i64 { error })?
  mut state := e
  mut count := 0
  loop { if count == 2 { break }; state = e; count = count + 1 }
  return Ok(state)
}
"#,
    );
    source.push_str(&main);
    (c, source, count)
}
