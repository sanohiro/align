use align_repl::{Config, Echo, EntryKind, Feed, Outcome, Region, Session, TimeRefusal};

fn session() -> Session {
    let config = Config {
        jobs: 1,
        time_default_n: 1,
        ..Config::default()
    };
    Session::new(config).unwrap_or_else(|error| panic!("start align-repl session: {error}"))
}

fn applied(outcome: Outcome) -> (Vec<u32>, Vec<u32>, Echo, align_repl::RunOutput) {
    match outcome {
        Outcome::Applied {
            ordinals,
            replaced,
            echo,
            out,
        } => (ordinals, replaced, echo, out),
        other => panic!("expected an applied entry, got {other:?}"),
    }
}

#[test]
fn render_and_no_op_keep_the_fixed_program() {
    let mut repl = session();
    let empty = repl.render();
    assert!(empty.contains("fn main() -> Result<(), Error>"));
    assert!(empty.ends_with("  return Ok(())\n}\n"));

    for text in ["", "   ", "// comment"] {
        assert!(matches!(repl.submit(text), Outcome::NoOp));
        assert_eq!(repl.render(), empty);
        assert!(repl.entries().is_empty());
    }
}

#[test]
fn printable_values_run_the_native_program() {
    let mut repl = session();
    let (ordinals, replaced, echo, out) = applied(repl.submit("1 + 2"));
    assert_eq!(ordinals, [1]);
    assert!(replaced.is_empty());
    assert_eq!(echo, Echo::Printed);
    assert_eq!(out.stdout_shown, b"3\n");
    assert_eq!(repl.entries()[0].kind, EntryKind::Printed);
    assert_eq!(repl.entries()[0].emitted, "print(1 + 2)");
}

#[test]
fn replacement_is_in_place_and_undo_restores_it() {
    let mut repl = session();
    applied(repl.submit("x := 1"));
    applied(repl.submit("print(x)"));

    let (_, replaced, _, out) = applied(repl.submit("x := 2"));
    assert_eq!(replaced, [1]);
    assert!(out.diverged);
    assert_eq!(out.stdout_shown, b"2\n");
    assert_eq!(repl.entries()[0].ordinal, 1);
    assert_eq!(repl.entries()[0].text, "x := 2");

    let (_, _, _, out) = applied(repl.undo());
    assert_eq!(out.stdout_shown, b"1\n");
    assert_eq!(repl.entries()[0].ordinal, 1);
    assert_eq!(repl.entries()[0].text, "x := 1");
}

#[test]
fn failed_replacement_rolls_back_entries_and_ordinal_counter() {
    let mut repl = session();
    applied(repl.submit("x := 1"));
    applied(repl.submit("y := x + 1"));
    let before = repl.render();

    match repl.submit("x := y + missing") {
        Outcome::CompileFailed { replacing, .. } => assert_eq!(replacing, [1]),
        other => panic!("expected failed replacement, got {other:?}"),
    }
    assert_eq!(repl.render(), before);

    let (ordinals, _, _, _) = applied(repl.submit("z := 3"));
    assert_eq!(ordinals, [3]);
}

#[test]
fn mixed_paste_is_split_and_undone_atomically() {
    let mut repl = session();
    let paste = "import core.math\nfn twice(x: i64) -> i64 = x * 2";
    let (ordinals, _, _, out) = applied(repl.submit(paste));
    assert_eq!(ordinals, [1, 2]);
    assert!(String::from_utf8_lossy(&out.stderr_shown).contains("unused import"));
    assert_eq!(repl.entries().len(), 2);
    assert_eq!(repl.entries()[0].region, Region::Import);
    assert_eq!(repl.entries()[1].region, Region::Decl);
    assert_eq!(repl.entries()[1].text, "fn twice(x: i64) -> i64 = x * 2");

    applied(repl.undo());
    assert!(repl.entries().is_empty());
}

#[test]
fn const_and_main_names_cannot_cross_regions() {
    let mut repl = session();
    applied(repl.add_const("WIDTH := 6"));
    assert!(matches!(
        repl.submit("WIDTH := 7"),
        Outcome::RegionConflict { name, ordinal: 1 } if name == "WIDTH"
    ));
    assert_eq!(repl.entries().len(), 1);
    assert_eq!(repl.entries()[0].region, Region::Const);
}

#[test]
fn annotated_binding_ignores_type_names_for_region_conflicts() {
    let mut repl = session();
    applied(repl.add_const("i64 := 7"));
    let (ordinals, replaced, _, _) = applied(repl.submit("x: i64 := 1"));
    assert_eq!(ordinals, [2]);
    assert!(replaced.is_empty());
    assert_eq!(repl.entries()[1].names, ["x"]);
}

#[test]
fn malformed_main_entry_reports_the_compiler_syntax_error() {
    let mut repl = session();
    let before = repl.render();
    match repl.submit("1 +") {
        Outcome::CompileFailed { rendered, replacing } => {
            assert!(!rendered.is_empty());
            assert!(rendered.contains("error:"), "{rendered}");
            assert!(replacing.is_empty());
        }
        other => panic!("expected syntax failure, got {other:?}"),
    }
    assert_eq!(repl.render(), before);
}

#[test]
fn continuation_abandons_on_a_blank_line() {
    let mut repl = session();
    assert!(matches!(repl.feed("if true {"), Feed::NeedMore));
    assert!(repl.continuing());
    assert!(matches!(
        repl.feed(""),
        Feed::Ready(Outcome::Command(align_repl::cmd::CmdResult::Message(message)))
            if message.contains("abandoned")
    ));
    assert!(!repl.continuing());
    assert!(repl.entries().is_empty());
}

#[test]
fn listing_has_line_ordinals_and_regions() {
    let mut repl = session();
    applied(repl.add_const("K := 4"));
    applied(repl.submit("x := K"));
    let listing = repl.listing();
    assert!(listing.contains("   1  const | K := 4"));
    assert!(listing.contains("   2   main |   x := K"));
    assert!(listing.contains("fn main() -> Result<(), Error>"));
}

#[test]
fn save_writes_the_exact_compiled_program() {
    let mut repl = session();
    applied(repl.submit("40 + 2"));
    let stage = align_driver::ArtifactStage::temp("align-repl-save-test")
        .unwrap_or_else(|error| panic!("create save stage: {error}"));
    let path = stage.path().join("saved.align");
    repl.save(&path, false)
        .unwrap_or_else(|error| panic!("save session: {error:?}"));
    let saved = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read saved session: {error}"));
    assert_eq!(saved, repl.render());
    assert!(matches!(repl.save(&path, false), Err(align_repl::SaveError::Exists)));
    let missing_parent = stage.path().join("missing").join("saved.align");
    assert!(matches!(
        repl.save(&missing_parent, false),
        Err(align_repl::SaveError::ParentMissing)
    ));
}

#[test]
fn main_signature_is_constant_when_question_is_added_and_undone() {
    let mut repl = session();
    assert_eq!(repl.render().matches("fn main() -> Result<(), Error>").count(), 1);
    applied(repl.submit("fn fallible() -> Result<i64, Error> = Ok(7)"));
    applied(repl.submit("x := fallible()?"));
    assert_eq!(repl.render().matches("fn main() -> Result<(), Error>").count(), 1);
    assert!(repl.render().ends_with("  return Ok(())\n}\n"));
    applied(repl.undo());
    assert_eq!(repl.render().matches("fn main() -> Result<(), Error>").count(), 1);
    assert!(repl.render().ends_with("  return Ok(())\n}\n"));
}

#[test]
fn type_of_does_not_mutate_the_session() {
    let mut repl = session();
    applied(repl.submit("x := 5"));
    let before = repl.render();
    assert_eq!(repl.type_of("x + 1"), Ok("i64".to_string()));
    assert_eq!(repl.render(), before);
}

#[test]
fn emitted_forms_are_exhaustive() {
    let mut repl = session();
    applied(repl.submit("x := 1"));
    applied(repl.submit("x + 1"));
    applied(repl.submit("print(x)"));
    for entry in repl.entries() {
        let valid = entry.emitted == entry.text
            || entry.emitted == format!("print({})", entry.text)
            || entry.emitted == format!("_ := ({})", entry.text);
        assert!(valid, "unexpected synthetic form: {}", entry.emitted);
    }
}

#[test]
fn synthetic_consumption_matrix_preserves_borrowed_move_values() {
    let mut repl = session();
    applied(repl.submit("Owned { text: string }"));
    applied(repl.submit("s := \"owned\".clone()"));
    let (_, _, echo, out) = applied(repl.submit("s"));
    assert_eq!(echo, Echo::Printed);
    assert_eq!(out.stdout_shown, b"owned\n");
    let (_, _, _, out) = applied(repl.submit("s.len()"));
    assert_eq!(out.stdout_shown, b"5\n");

    applied(repl.submit("value := Owned { text: \"field\".clone() }"));
    let (_, _, echo, out) = applied(repl.submit("value.text"));
    assert_eq!(echo, Echo::Printed);
    assert_eq!(out.stdout_shown, b"field\n");
    let (_, _, _, out) = applied(repl.submit("value.text.len()"));
    assert_eq!(out.stdout_shown, b"5\n");

    let (_, _, echo, out) = applied(repl.submit("\"temporary\".clone()"));
    assert_eq!(echo, Echo::Printed);
    assert_eq!(out.stdout_shown, b"temporary\n");

    let (_, _, echo, _) = applied(repl.submit("value"));
    assert_eq!(
        echo,
        Echo::TypeOnly {
            rendered: "Owned".to_string(),
        }
    );
    let (_, _, _, out) = applied(repl.submit("value.text.len()"));
    assert_eq!(out.stdout_shown, b"5\n");
}

#[test]
fn child_output_remains_byte_exact_for_invalid_utf8() {
    let mut repl = session();
    applied(repl.submit("import std.io"));
    applied(repl.submit("mut bytes := buffer(1)"));
    applied(repl.submit("bytes.put_u8(255)"));
    let (_, _, _, out) = applied(repl.submit("io.stdout.write(bytes.bytes())?"));
    assert_eq!(out.stdout_shown, [255]);
    assert!(out.stderr_shown.is_empty());

    let (_, _, _, suffix) = applied(repl.submit("io.stdout.write(\"x\")?"));
    assert_eq!(suffix.stdout_shown, b"x");
    assert_eq!(repl.last_output(), Some((vec![255, b'x'], Vec::new())));
}

#[test]
fn fallible_expression_uses_the_disclosed_result_binding() {
    let mut repl = session();
    applied(repl.submit("fn fallible() -> Result<i64, Error> = Ok(7)"));
    let before_type = repl.render();
    assert_eq!(repl.type_of("fallible()"), Ok("Result<i64, Error>".to_string()));
    assert_eq!(repl.render(), before_type);
    let (_, _, echo, _) = applied(repl.submit("fallible()"));
    assert_eq!(
        echo,
        Echo::ResultBound {
            rendered: "Result<i64, Error>".to_string(),
        }
    );
    assert!(echo.render().unwrap_or_default().contains("a Result must be handled"));
    assert_eq!(repl.entries()[1].emitted, "_ := (fallible())");

    applied(repl.submit("fallible()"));
    assert_eq!(
        repl.entries()
            .iter()
            .filter(|entry| entry.kind == EntryKind::ResultBound)
            .count(),
        2
    );
}

#[test]
fn result_place_move_is_disclosed_and_undoable() {
    let mut repl = session();
    applied(repl.submit("fn fallible() -> Result<string, Error> = Ok(\"owned\".clone())"));
    applied(repl.submit("r := fallible()"));
    let (_, _, echo, _) = applied(repl.submit("r"));
    assert!(matches!(echo, Echo::ResultBound { .. }));
    assert!(echo.render().unwrap_or_default().contains("a Result must be handled"));

    match repl.submit("r") {
        Outcome::CompileFailed { rendered, .. } => {
            assert!(rendered.contains("use of moved value 'r'"), "{rendered}")
        }
        other => panic!("expected moved-value failure, got {other:?}"),
    }
    applied(repl.undo());
    let (_, _, echo, _) = applied(repl.submit("r"));
    assert!(matches!(echo, Echo::ResultBound { .. }));
}

#[test]
fn name_delta_excludes_ignored_and_nested_bindings() {
    let mut repl = session();
    applied(repl.submit("i64 := 8"));
    applied(repl.submit("typed: i64 := 1"));
    assert_eq!(repl.entries()[1].names, ["typed"]);

    applied(repl.submit("(a, _) := (1, 2)"));
    assert_eq!(repl.entries()[2].names, ["a"]);

    applied(repl.submit("arena { p := 4; print(p) }"));
    assert!(repl.entries()[3].names.is_empty());
    let (ordinals, replaced, _, _) = applied(repl.submit("p := 9"));
    assert_eq!(ordinals, [5]);
    assert!(replaced.is_empty());
    assert_eq!(repl.entries().len(), 5);
}

#[test]
fn ambiguous_braces_route_by_the_compiler_result() {
    let mut repl = session();
    applied(repl.submit("Point { x: i64 }"));
    assert_eq!(repl.entries()[0].region, Region::Decl);

    applied(repl.submit("x := 7"));
    let (_, _, echo, _) = applied(repl.submit("Point{x: x}"));
    assert_eq!(repl.entries()[2].region, Region::Main);
    assert_eq!(
        echo,
        Echo::TypeOnly {
            rendered: "Point".to_string(),
        }
    );

    let before = repl.render();
    assert!(matches!(
        repl.submit("Point { x: f64 }"),
        Outcome::CompileFailed { replacing, .. } if replacing == [1]
    ));
    assert_eq!(repl.render(), before);

    applied(repl.drop_entry(3));
    let (_, replaced, _, _) = applied(repl.submit("Point { x: f64 }"));
    assert_eq!(replaced, [1]);
    assert_eq!(repl.entries()[0].ordinal, 1);
    assert_eq!(repl.entries()[0].text, "Point { x: f64 }");
}

#[test]
fn extern_replacement_is_explicit() {
    let mut repl = session();
    applied(repl.submit("extern \"C\" fn abs(x: i32) -> i32"));
    let (_, replaced, _, _) = applied(repl.submit("extern \"C\" fn abs(x: i64) -> i64"));
    assert_eq!(replaced, [1]);
    assert_eq!(repl.entries().len(), 1);
    assert_eq!(repl.entries()[0].ordinal, 1);
    assert!(repl.entries()[0].text.contains("i64"));
}

#[test]
fn time_requires_a_user_program_and_does_not_recompile() {
    let mut repl = session();
    assert!(matches!(repl.time(1, false), Err(TimeRefusal::NoBinary)));
    applied(repl.submit("x := 1"));
    let before = repl.render();
    let timing = repl.time(1, false).unwrap_or_else(|_| panic!("time accepted session"));
    assert_eq!(timing.n, 1);
    assert!(timing.min_ms >= 0.0);
    assert!(timing.floor_ms >= 0.0);
    assert_eq!(repl.render(), before);

    let clamped = repl
        .time(1001, true)
        .unwrap_or_else(|_| panic!("forced timing accepted session"));
    assert_eq!(clamped.n, 1000);
    assert_eq!(clamped.clamped_from, Some(1001));

    let mut slow = session();
    applied(slow.submit("import std.time"));
    applied(slow.submit("time.sleep(20000000)"));
    assert!(matches!(
        slow.time(1000, false),
        Err(TimeRefusal::Projected { secs }) if secs > 10.0
    ));
}

#[test]
fn runtime_failure_keeps_the_compiling_entry() {
    let mut repl = session();
    applied(repl.submit("xs := [1]"));
    let outcome = repl.submit("xs[2]");
    assert!(matches!(outcome, Outcome::RanAndFailed { .. }));
    assert_eq!(repl.entries().len(), 2);
    assert_eq!(repl.entries()[1].text, "xs[2]");
    applied(repl.undo());
    assert_eq!(repl.entries().len(), 1);
}

#[test]
fn drop_rolls_back_on_dependency_and_clear_keeps_ordinal_gaps() {
    let mut repl = session();
    applied(repl.submit("x := 1"));
    applied(repl.submit("print(x)"));
    let before = repl.render();
    assert!(matches!(repl.drop_entry(1), Outcome::CompileFailed { .. }));
    assert_eq!(repl.render(), before);

    applied(repl.drop_entry(2));
    assert_eq!(
        repl.entries().iter().map(|entry| entry.ordinal).collect::<Vec<_>>(),
        [1]
    );
    applied(repl.clear());
    let (ordinals, _, _, _) = applied(repl.submit("y := 2"));
    assert_eq!(ordinals, [3]);
}

#[test]
fn accepted_build_reuses_the_candidate_frontend() {
    let mut repl = session();
    let before = align_driver::memo::stats().unit_hits;
    // Keep this source fresh across separate test-process invocations. A prior invocation's
    // persistent unit-cache entry bypasses the in-process store entirely and would make this owner
    // observe no new memo hit even though candidate and build still share the correct path.
    let entry = format!("memo_probe := {}", std::process::id());
    applied(repl.submit(&entry));
    let after = align_driver::memo::stats().unit_hits;
    assert!(after > before, "accepted build must hit the candidate's unit frontend");
}
