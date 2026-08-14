//! The `align-repl` binary: a stdin loop over [`align_repl::Session`]
//! (`docs/impl/22-repl-plan.md` §8, §11 B1–B4).

use std::io::{BufRead, Write};

use align_repl::{Config, Feed, Outcome, SaveError, Session, TimeRefusal, cmd};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [arg] if matches!(arg.as_str(), "--help" | "-h") => {
            print!("usage: align-repl\n\n{}", cmd::HELP);
            return std::process::ExitCode::SUCCESS;
        }
        [arg] if matches!(arg.as_str(), "--version" | "-V") => {
            println!("align-repl {}", env!("CARGO_PKG_VERSION"));
            return std::process::ExitCode::SUCCESS;
        }
        [first, ..] => {
            eprintln!("align-repl: unexpected argument `{first}` (try --help)");
            return std::process::ExitCode::FAILURE;
        }
        [] => {}
    }

    let mut session = match Session::new(Config::default()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("align-repl: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // A non-tty stdin suppresses the prompt, so a piped script's output is clean.
    let interactive = std::io::IsTerminal::is_terminal(&std::io::stdin());
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        prompt(interactive, session.continuing());
        let Some(line) = lines.next() else { break };
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("align-repl: cannot read stdin: {e}");
                return std::process::ExitCode::FAILURE;
            }
        };
        if !session.continuing()
            && let Some(command) = cmd::parse(&line)
        {
            if matches!(command, cmd::Command::Quit) {
                break;
            }
            run_command(&mut session, command);
            continue;
        }
        match session.feed(&line) {
            Feed::NeedMore => continue,
            Feed::Ready(outcome) => report(&outcome),
        }
    }
    std::process::ExitCode::SUCCESS
}

fn prompt(interactive: bool, continuing: bool) {
    if !interactive {
        return;
    }
    print!("{}", if continuing { "...   " } else { "align> " });
    let _ = std::io::stdout().flush();
}

fn run_command(session: &mut Session, command: cmd::Command) {
    match command {
        cmd::Command::Quit => {}
        cmd::Command::Help => print!("{}", cmd::HELP),
        cmd::Command::List => print!("{}", session.listing()),
        cmd::Command::Type(expr) => match session.type_of(&expr) {
            Ok(rendered) => println!("<{rendered}>"),
            Err(rendered) => eprint!("{rendered}"),
        },
        cmd::Command::Const(text) => report(&session.add_const(&text)),
        cmd::Command::Save { path, force } => {
            let path = std::path::PathBuf::from(&path);
            match session.save(&path, force) {
                Ok(()) => println!(
                    "align-repl: wrote {}\n  build it with: alignc build {}",
                    path.display(),
                    path.display()
                ),
                Err(SaveError::Exists) => eprintln!(
                    "align-repl: {} already exists (use `:save!` to overwrite)",
                    path.display()
                ),
                Err(SaveError::ParentMissing) => {
                    eprintln!("align-repl: the parent directory of {} does not exist", path.display())
                }
                Err(SaveError::Io(e)) => eprintln!("align-repl: cannot write {}: {e}", path.display()),
            }
        }
        cmd::Command::Undo => report(&session.undo()),
        cmd::Command::Drop(n) => report(&session.drop_entry(n)),
        cmd::Command::Clear => report(&session.clear()),
        cmd::Command::Out => match session.last_output() {
            Some(text) => print!("{text}"),
            None if session.last_output_was_truncated() => {
                eprintln!("align-repl: the last output exceeded the retention cap and cannot be reprinted")
            }
            None => eprintln!("align-repl: nothing has run yet"),
        },
        cmd::Command::Time { n, force } => match session.time(n.unwrap_or(0), force) {
            Ok(t) => {
                if let Some(from) = t.clamped_from {
                    println!("align-repl: {from} clamped to {}", t.n);
                }
                println!(
                    "{} runs: min {:.1} ms, median {:.1} ms, max {:.1} ms",
                    t.n, t.min_ms, t.median_ms, t.max_ms
                );
                println!(
                    ":time measures the whole session program, not the last entry, and each \
                     sample includes process spawn. Spawn floor on this host: {:.1} ms (measured \
                     at startup from an empty program). Compilation is not included; the binary \
                     is already built.",
                    t.floor_ms
                );
            }
            Err(TimeRefusal::NoBinary) => {
                eprintln!("align-repl: there is no built binary to time yet")
            }
            Err(TimeRefusal::Projected { secs }) => {
                eprintln!("align-repl: that would take about {secs:.1} s — rerun as `:time!` to go ahead")
            }
        },
        cmd::Command::Unknown(message) => eprintln!("align-repl: {message}"),
    }
}

fn report(outcome: &Outcome) {
    match outcome {
        Outcome::NoOp => {}
        Outcome::Command(cmd::CmdResult::Message(message)) => eprintln!("{message}"),
        Outcome::Applied {
            replaced, echo, out, ..
        } => {
            show_output(out);
            if !replaced.is_empty() {
                let list: Vec<String> = replaced.iter().map(u32::to_string).collect();
                println!("align-repl: replaced entry {}", list.join(", "));
            }
            if let Some(text) = echo.render() {
                println!("{text}");
            }
        }
        Outcome::RanAndFailed { status, out } => {
            show_output(out);
            eprintln!(
                "align-repl: the program exited {} — the entry was kept because it compiles; \
                 use `:undo` to remove it",
                status
                    .code()
                    .map_or_else(|| String::from("abnormally"), |c| c.to_string())
            );
        }
        Outcome::CompileFailed { rendered, replacing } => {
            eprint!("{rendered}");
            if !replacing.is_empty() {
                let list: Vec<String> = replacing.iter().map(u32::to_string).collect();
                eprintln!(
                    "align-repl: this entry was being read as a replacement of entry {}; the \
                     session is unchanged",
                    list.join(", ")
                );
            }
        }
        Outcome::RegionConflict { name, ordinal } => eprintln!(
            "align-repl: `{name}` is already a top-level constant (entry {ordinal}); a `main` \
             binding cannot use that name — pick another name, or replace the constant with \
             `:const {name} := …`"
        ),
    }
}

fn show_output(out: &align_repl::RunOutput) {
    if out.truncated {
        eprintln!(
            "align-repl: output exceeded the retention cap; suffix elision is disabled until the \
             next :clear"
        );
    }
    if out.diverged {
        eprintln!(
            "align-repl: re-execution differs from the previous run (a replaced binding, \
             nondeterminism, or an external side effect) — full output follows"
        );
    }
    print!("{}", out.stdout_shown);
    eprint!("{}", out.stderr_shown);
}
