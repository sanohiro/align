//! The eleven `:` commands (`docs/impl/22-repl-plan.md` §8).

/// What a command produced, for the caller to render.
#[derive(Clone, Debug)]
pub enum CmdResult {
    Message(String),
}

/// One parsed command line. `force` is `:save!` / `:time!`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Quit,
    List,
    Type(String),
    Const(String),
    Save { path: String, force: bool },
    Undo,
    Time { n: Option<u32>, force: bool },
    Out,
    Clear,
    Drop(u32),
    Unknown(String),
}

/// Parse a `:`-prefixed line. Returns `None` when the line is an ordinary entry.
pub fn parse(line: &str) -> Option<Command> {
    let rest = line.strip_prefix(':')?;
    let (head, tail) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim()),
        None => (rest, ""),
    };
    let no_args = |command, value| {
        if tail.is_empty() {
            command
        } else {
            Command::Unknown(format!("`:{value}` does not take arguments"))
        }
    };
    let time = |force| {
        if tail.is_empty() {
            Command::Time { n: None, force }
        } else {
            match tail.parse() {
                Ok(n) => Command::Time { n: Some(n), force },
                Err(_) => Command::Unknown(format!("`:time` needs an integer count (got `{tail}`)")),
            }
        }
    };
    Some(match head {
        "help" => no_args(Command::Help, "help"),
        "quit" => no_args(Command::Quit, "quit"),
        "list" => no_args(Command::List, "list"),
        "type" if tail.is_empty() => Command::Unknown("`:type` needs an expression".to_string()),
        "type" => Command::Type(tail.to_string()),
        "const" if tail.is_empty() => Command::Unknown("`:const` needs `NAME := expression`".to_string()),
        "const" => Command::Const(tail.to_string()),
        "save" | "save!" if tail.is_empty() => Command::Unknown(format!("`:{head}` needs a path")),
        "save" => Command::Save {
            path: tail.to_string(),
            force: false,
        },
        "save!" => Command::Save {
            path: tail.to_string(),
            force: true,
        },
        "undo" => no_args(Command::Undo, "undo"),
        "time" => time(false),
        "time!" => time(true),
        "out" => no_args(Command::Out, "out"),
        "clear" => no_args(Command::Clear, "clear"),
        "drop" => match tail.parse() {
            Ok(n) => Command::Drop(n),
            Err(_) => Command::Unknown(format!(":drop needs an ordinal (got `{tail}`)")),
        },
        other => Command::Unknown(format!("unknown command `:{other}`")),
    })
}

pub const HELP: &str = "\
align-repl — an editor for one growing Align program.

Every entry is spliced into a single program, the WHOLE program is recompiled with the real
compiler, and the real binary runs. Nothing is interpreted, and no value survives between
entries — earlier lines are re-executed, which is why behavior matches `alignc build` exactly.

  * Re-binding a name EDITS the earlier line in place; later lines then re-run against the new
    value. `:list` shows the program and the entry ordinals (gaps are removed entries).
  * A `fn` you define cannot see `main`'s bindings. Use `:const NAME := expr` for a value a
    function must reach.
  * Side effects re-run. A session that writes a file writes it again on every entry. Identical
    output is elided; anything that CHANGED is printed in full behind a banner.
  * Heap/arena values cannot span entries — `arena` and `heap.new` are block-scoped, and each
    entry is one statement. Type the whole block as one entry:
        arena {
          p: box<i32> := heap.new(42)
          print(p.get())
        }
  * A method chain broken across lines is only continued inside brackets; at top level write it
    on one line or wrap it in parentheses.
  * Ctrl-C exits align-repl — and, while your program is running, ends the program with it.

  :help              this text                :undo            remove the last entry
  :quit              exit (also Ctrl-D)       :drop N          remove entry N
  :list              show the program         :clear           drop every entry
  :type EXPR         the type of EXPR         :out             reprint the last output
  :const NAME := E   a top-level constant     :time [N]        time the built binary
  :save PATH         write a .align file      :save! PATH      … overwriting
";

#[cfg(test)]
mod tests {
    use super::{Command, parse};

    #[test]
    fn command_table_covers_the_v1_surface_and_errors() {
        assert_eq!(parse("ordinary entry"), None);
        assert_eq!(parse(":help"), Some(Command::Help));
        assert_eq!(parse(":quit"), Some(Command::Quit));
        assert_eq!(parse(":list"), Some(Command::List));
        assert_eq!(parse(":type x + 1"), Some(Command::Type("x + 1".into())));
        assert_eq!(parse(":const K := 1"), Some(Command::Const("K := 1".into())));
        assert_eq!(
            parse(":save out.align"),
            Some(Command::Save {
                path: "out.align".into(),
                force: false
            })
        );
        assert_eq!(
            parse(":save! out.align"),
            Some(Command::Save {
                path: "out.align".into(),
                force: true
            })
        );
        assert_eq!(parse(":undo"), Some(Command::Undo));
        assert_eq!(
            parse(":time 7"),
            Some(Command::Time {
                n: Some(7),
                force: false
            })
        );
        assert_eq!(parse(":time!"), Some(Command::Time { n: None, force: true }));
        assert_eq!(parse(":out"), Some(Command::Out));
        assert_eq!(parse(":clear"), Some(Command::Clear));
        assert_eq!(parse(":drop 12"), Some(Command::Drop(12)));
        for invalid in [":help now", ":type", ":save", ":time nope", ":drop nope", ":wat"] {
            assert!(matches!(parse(invalid), Some(Command::Unknown(_))), "{invalid}");
        }
    }
}
