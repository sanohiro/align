//! Explicit signal lease: ordinary ownership and whole/per-unit native transport.
mod common;
use common::*;

#[test]
fn signal_owner_import_move_close_and_drop() {
    let helper = r#"module helper
import std.process
pub Lease { subscription: process.signal_subscription }
pub fn make() -> Result<Lease, Error> {
    subscription := process.signals(process.signal_set { hangup: false, interrupt: false, quit: false, terminate: true })?
    Ok(Lease { subscription: subscription })
}
pub fn empty(borrow mut subscription: process.signal_subscription) -> Result<bool, Error> {
    Ok(match subscription.next()? { None => true, Some(signal) => false })
}
pub fn identity<T>(value: T) -> T = value
pub fn discard(lease: Lease) {}
pub fn close(lease: Lease) -> Result<(), Error> {
    mut subscription := lease.subscription
    print(empty(subscription)?)
    subscription.close()?
    subscription.close()?
    print(match subscription.next() { Err(error) => true, Ok(value) => false })
    Ok(())
}
"#;
    let main = r#"import std.process
import helper
pub fn main() -> Result<(), Error> {
    lease := helper.make()?
    print(match helper.make() { Err(error) => true, Ok(other) => false })
    helper.close(helper.identity(lease))?
    helper.discard(helper.make()?)
    helper.close(helper.make()?)?
    Ok(())
}
"#;
    let files = [("main.align", main), ("helper.align", helper)];
    let checked = diff_check_multi("signal-owner", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let whole = build_and_run_multi("signal-owner", &files, "main.align");
        let unit = build_per_unit_multi("signal-owner-unit", &files, "main.align").link_and_run();
        for output in [whole, unit] {
            assert!(
                output.status.success(),
                "stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "true\ntrue\ntrue\ntrue\ntrue\n"
            );
        }
    }
}

#[test]
fn native_delivery_and_child_disposition_reset() {
    let source = r#"import std.process
pub fn main() -> Result<(), Error> {
    mut subscription := process.signals(process.signal_set { hangup: false, interrupt: false, quit: false, terminate: true })?
    command := process.command("/bin/sh", ["sh", "-c", "kill -TERM $PPID"])
    result := command.run()?
    status := result.status()
    print(match status.termination { Exited(code) => code == 0, Signaled(signal) => false })
    print(match subscription.next()? { Some(signal) => process.signal_number(signal) == process.signal_number(process.signal.Terminate), None => false })
    child := process.command("/bin/sh", ["sh", "-c", "kill -TERM $$"])
    ended := child.run()?
    end_status := ended.status()
    print(match end_status.termination { Signaled(signal) => signal == process.signal_number(process.signal.Terminate), Exited(code) => false })
    subscription.close()?
    Ok(())
}
"#;
    let files = [("main.align", source)];
    if backend_available() {
        for output in [
            build_and_run("signal-delivery", source),
            build_per_unit_multi("signal-delivery-unit", &files, "main.align").link_and_run(),
        ] {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "true\ntrue\ntrue\n"
            );
        }
    }
}

#[test]
fn signal_owner_rejects_forbidden_carriers_and_shared_mutation() {
    for (name, body) in [
        (
            "array",
            "fn bad(value: array<process.signal_subscription>) {}",
        ),
        (
            "slice",
            "fn bad(value: slice<process.signal_subscription>) {}",
        ),
        (
            "fixed",
            "fn bad(value: process.signal_subscription) { values := [value] }",
        ),
        ("box", "fn bad(value: box<process.signal_subscription>) {}"),
        (
            "tuple",
            "fn bad(value: (process.signal_subscription, i64)) {}",
        ),
        ("out", "fn bad(out value: process.signal_subscription) {}"),
        (
            "shared-next",
            "fn bad(borrow value: process.signal_subscription) { value.next() }",
        ),
        (
            "shared-close",
            "fn bad(borrow value: process.signal_subscription) { value.close() }",
        ),
        (
            "print",
            "fn bad(value: process.signal_subscription) { print(value) }",
        ),
        (
            "copy",
            "fn take(value: process.signal_subscription) {}\nfn bad(value: process.signal_subscription) { take(value)\ntake(value) }",
        ),
        (
            "immutable",
            "fn bad(value: process.signal_subscription) { value.next() }",
        ),
    ] {
        let source = format!("import std.process\n{body}\nfn main() {{}}\n");
        let checked = diff_check_multi(
            &format!("signal-negative-{name}"),
            &[("main.align", &source)],
            "main.align",
        );
        assert!(
            checked.whole_errors && checked.per_unit_errors,
            "{name}: {}\n{}",
            checked.whole_diags,
            checked.per_unit_diags
        );
    }
}
