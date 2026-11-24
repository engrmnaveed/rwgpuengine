//! The command registry and argument parser.
//!
//! Commands are data: the same table drives parsing, `slate help`,
//! `slate help --json` and the generated CLI reference, so they cannot drift.

use std::collections::BTreeMap;

use slate_core::codes;
use slate_core::diag::Diagnostic;
use slate_core::json::Json;
use slate_core::text::did_you_mean;

#[derive(Debug)]
pub struct Flag {
    pub name: &'static str,
    /// Value placeholder (`N`, `FILE`), or `None` for a switch.
    pub value: Option<&'static str>,
    pub doc: &'static str,
}

#[derive(Debug)]
pub struct Command {
    pub name: &'static str,
    pub summary: &'static str,
    /// Positional arguments: (name, doc). Names in brackets are optional.
    pub args: &'static [(&'static str, &'static str)],
    pub flags: &'static [Flag],
    pub details: &'static str,
    pub examples: &'static [&'static str],
}

const fn flag(name: &'static str, value: Option<&'static str>, doc: &'static str) -> Flag {
    Flag { name, value, doc }
}

const JSON: Flag = flag("--json", None, "Print one JSON object on stdout instead of text.");
const INPUT: Flag =
    flag("--input", Some("FILE"), "Drive the scene with a playtest's [[input]] (and use its scene and seed).");
const SEED: Flag = flag("--seed", Some("N"), "Override the random seed.");

const PATH_ARG: (&str, &str) = (
    "[PATH]",
    "Project folder, scene file or playtest file (default: current folder). The project is found by walking up to slate.toml.",
);

pub static COMMANDS: &[Command] = &[
    Command {
        name: "new",
        summary: "Create a project from the starter template",
        args: &[("PATH", "Folder to create (must not exist or be empty).")],
        flags: &[flag("--name", Some("NAME"), "Project name (default: the folder name)."), JSON],
        details: "Writes slate.toml, a starter scene, prefabs, a playtest and an AGENTS.md generated for this \
                  slate version, then checks the result.",
        examples: &["slate new my-game", "slate new games/maze --name \"Maze\""],
    },
    Command {
        name: "check",
        summary: "Validate every project file without running anything",
        args: &[PATH_ARG],
        flags: &[JSON],
        details: "Parses slate.toml, every scene, prefab and playtest; checks types, unknown keys, references \
                  between files and static problems (overlapping solids, signals that can never fire). Exit code 1 \
                  if there are errors; warnings do not fail.",
        examples: &["slate check", "slate check my-game --json"],
    },
    Command {
        name: "run",
        summary: "Run a scene headless for N frames",
        args: &[PATH_ARG],
        flags: &[
            flag("--frames", Some("N"), "Frames (fixed steps) to run. Default: the playtest's frames, else 300."),
            INPUT,
            SEED,
            flag("--events", None, "Include the event log."),
            flag("--hash-every", Some("N"), "Record the state hash every N frames (to find where two runs diverge)."),
            flag(
                "--track",
                Some("NAME"),
                "Print the cells an entity occupies, with frame ranges (timing puzzles, patrols).",
            ),
            JSON,
        ],
        details: "Deterministic: the same files, input and seed always give the same state hash, on every \
                  platform. No window and no GPU are used.",
        examples: &[
            "slate run --frames 300",
            "slate run tests/walkthrough.toml --events",
            "slate run --frames 120 --track guard",
            "slate run scenes/garden.toml --input tests/walkthrough.toml --json",
        ],
    },
    Command {
        name: "shot",
        summary: "Render a frame to PNG, with a text report of what is drawn where",
        args: &[PATH_ARG],
        flags: &[
            flag("--frame", Some("N"), "Frame to render. Default: the playtest's last frame, else 0."),
            INPUT,
            SEED,
            flag("--out", Some("FILE"), "PNG path. Default: out/<scene>_f<frame>.png in the project."),
            flag(
                "--compare",
                Some("FILE"),
                "Compare with a reference PNG; exit 1 if it differs (writes <out>.diff.png).",
            ),
            flag("--max-diff", Some("N"), "With --compare: pixels allowed to differ (default 0)."),
            flag("--report", None, "Print the frame report (always included with --json)."),
            JSON,
        ],
        details: "Rendering is a deterministic CPU rasterizer: identical pixels on every machine. The frame \
                  report lists each entity's screen rectangle, visible pixels and what covers it, and flags \
                  off-screen (W0301), hidden (W0302) and clipped (W0303) entities.",
        examples: &[
            "slate shot",
            "slate shot --input tests/walkthrough.toml --report",
            "slate shot --frame 120 --out a.png --json",
        ],
    },
    Command {
        name: "inspect",
        summary: "Dump the world state at a frame as JSON (or a table)",
        args: &[PATH_ARG],
        flags: &[
            flag("--frame", Some("N"), "Frame to inspect. Default: the playtest's last frame, else 0."),
            INPUT,
            SEED,
            flag("--entity", Some("NAME"), "Only this entity (and, with --events, its events)."),
            flag("--component", Some("NAME"), "Only entities with this component (sprite, trigger, ...)."),
            flag("--events", None, "Include the event log: what happened and what was refused, and why."),
            flag("--since", Some("N"), "With --events: only events from frame N on."),
            JSON,
        ],
        details: "Answers what exists, where it is, and why something happened. The text table is a summary \
                  (cell, components, solid, cooldown, patrol target, screen rectangle, tap point); --json has every \
                  component and all runtime state. `tap` is a pixel where a tap is guaranteed to hit the entity.",
        examples: &[
            "slate inspect --json",
            "slate inspect --input tests/walkthrough.toml --frame 60 --entity player --events",
        ],
    },
    Command {
        name: "test",
        summary: "Run playtests: scripted input, expectations and reference screenshots",
        args: &[("[PATH]", "Project folder or a single playtest file (default: current folder).")],
        flags: &[
            flag("--filter", Some("TEXT"), "Only playtests whose name contains TEXT."),
            flag("--update-refs", None, "Write reference images from the current output instead of comparing."),
            JSON,
        ],
        details: "Runs every tests/*.toml. Failed expectations are E0401 with the entity's recent events; image \
                  mismatches are E0402 and write the actual and diff images under out/tests/.",
        examples: &["slate test", "slate test --filter walk --json", "slate test --update-refs"],
    },
    Command {
        name: "explain",
        summary: "Explain a diagnostic code, a file format/component, or a command",
        args: &[(
            "[TOPIC]",
            "A code (E0103), a component or format (sprite, entity, connect, playtest) or a command. Omit to list topics.",
        )],
        flags: &[JSON],
        details: "Like `rustc --explain`, for slate's codes and file formats.",
        examples: &["slate explain", "slate explain E0201", "slate explain sprite", "slate explain trigger --json"],
    },
    Command {
        name: "schema",
        summary: "Print the JSON Schema of a file format",
        args: &[("[KIND]", "project, scene, prefab or playtest. Omit to list them.")],
        flags: &[flag("--out", Some("DIR"), "Write all schemas into DIR as <kind>.schema.json."), JSON],
        details: "Editors (e.g. VS Code + Even Better TOML) can use these for completion and inline errors.",
        examples: &["slate schema scene", "slate schema --out .slate/schema"],
    },
    Command {
        name: "docs",
        summary: "Print the reference docs for this exact slate version",
        args: &[],
        flags: &[
            flag("--agents", None, "Print the AGENTS.md that `slate new` writes into projects."),
            flag("--out", Some("DIR"), "Write cli.md, formats.md, errors.md and schemas/ into DIR."),
            JSON,
        ],
        details: "Docs are generated from the same registries the engine uses, so they always match the \
                  installed version.",
        examples: &["slate docs", "slate docs --agents > AGENTS.md"],
    },
    Command {
        name: "serve",
        summary: "Serve the web player for a project (reloads when files change)",
        args: &[PATH_ARG],
        flags: &[
            flag("--port", Some("N"), "Port on 127.0.0.1 (default 4410)."),
            flag(
                "--wasm",
                Some("FILE"),
                "Path to slate_web.wasm (default: the workspace build, see `cargo xtask web`).",
            ),
        ],
        details: "Opens nothing by itself; visit the printed URL. URL parameters: scene=PATH, input=PATH, \
                  frames=N (run N frames immediately, then pause). The page shows the frame number and state hash, \
                  which match `slate run` for the same input.",
        examples: &["slate serve examples/iso_garden", "slate serve --port 8080"],
    },
    Command {
        name: "help",
        summary: "Show help for slate or a command",
        args: &[("[COMMAND]", "Command to describe.")],
        flags: &[JSON],
        details: "`slate help --json` lists every command, argument and flag as data.",
        examples: &["slate help", "slate help shot", "slate help --json"],
    },
    Command {
        name: "version",
        summary: "Print the slate version",
        args: &[],
        flags: &[JSON],
        details: "",
        examples: &["slate version --json"],
    },
];

pub fn find(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// Parsed command line.
#[derive(Debug)]
pub struct Parsed {
    pub cmd: &'static Command,
    pub positionals: Vec<String>,
    flags: BTreeMap<&'static str, Option<String>>,
}

impl Parsed {
    pub fn has(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.flags.get(name).and_then(|v| v.as_deref())
    }

    pub fn json(&self) -> bool {
        self.has("--json")
    }

    pub fn help(&self) -> bool {
        self.has("--help")
    }

    pub fn positional(&self, i: usize) -> Option<&str> {
        self.positionals.get(i).map(String::as_str)
    }

    /// A non-negative integer flag value, or an E0003 diagnostic.
    pub fn number(&self, name: &str) -> Result<Option<u64>, Diagnostic> {
        match self.value(name) {
            None => Ok(None),
            Some(v) => v.parse::<u64>().map(Some).map_err(|_| {
                bad_arg(
                    format!("`{name}` needs a non-negative whole number, got `{v}`"),
                    Some(format!("for example `{name} 300`")),
                )
            }),
        }
    }
}

pub fn bad_arg(message: String, hint: Option<String>) -> Diagnostic {
    Diagnostic::new(&codes::BAD_ARGUMENT, message).maybe_hint(hint)
}

/// Parse `args` (without the program name).
pub fn parse(args: &[String]) -> Result<Parsed, Diagnostic> {
    let Some(first) = args.first() else {
        return Ok(Parsed { cmd: find("help").expect("help"), positionals: vec![], flags: BTreeMap::new() });
    };
    let (cmd, rest): (&'static Command, &[String]) = match first.as_str() {
        "-h" | "--help" => (find("help").expect("help"), &args[1..]),
        "-V" | "--version" => (find("version").expect("version"), &args[1..]),
        name => match find(name) {
            Some(c) => (c, &args[1..]),
            None => {
                let hint = did_you_mean(name, COMMANDS.iter().map(|c| c.name))
                    .unwrap_or_else(|| "run `slate help` for the list of commands".into());
                return Err(bad_arg(format!("unknown command `{name}`"), Some(hint)));
            }
        },
    };
    let mut positionals = Vec::new();
    let mut flags = BTreeMap::new();
    let mut i = 0;
    while i < rest.len() {
        let arg = &rest[i];
        i += 1;
        if arg == "--help" || arg == "-h" {
            flags.insert("--help", None);
            continue;
        }
        if !arg.starts_with("--") || arg == "--" {
            positionals.push(arg.clone());
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        let Some(spec) = cmd.flags.iter().find(|f| f.name == name) else {
            let hint = did_you_mean(name, cmd.flags.iter().map(|f| f.name))
                .unwrap_or_else(|| format!("run `slate help {}` for its flags", cmd.name));
            return Err(bad_arg(format!("`slate {}` has no flag `{name}`", cmd.name), Some(hint)));
        };
        let value = match (spec.value, inline) {
            (None, None) => None,
            (None, Some(_)) => {
                return Err(bad_arg(format!("`{name}` does not take a value"), None));
            }
            (Some(_), Some(v)) => Some(v),
            (Some(placeholder), None) => match rest.get(i) {
                Some(v) if !v.starts_with("--") => {
                    i += 1;
                    Some(v.clone())
                }
                _ => {
                    return Err(bad_arg(format!("`{name}` needs a value ({placeholder})"), None));
                }
            },
        };
        flags.insert(spec.name, value);
    }
    let max_pos = cmd.args.len();
    if positionals.len() > max_pos {
        return Err(bad_arg(
            format!(
                "`slate {}` takes at most {max_pos} argument(s), got {}: {}",
                cmd.name,
                positionals.len(),
                positionals.join(" ")
            ),
            Some(format!("run `slate help {}`", cmd.name)),
        ));
    }
    Ok(Parsed { cmd, positionals, flags })
}

// ---------------------------------------------------------------------------
// Help

pub fn usage_line(c: &Command) -> String {
    let mut s = format!("slate {}", c.name);
    for (a, _) in c.args {
        s.push(' ');
        s.push_str(a);
    }
    if !c.flags.is_empty() {
        s.push_str(" [FLAGS]");
    }
    s
}

pub fn overview_text() -> String {
    let mut out = format!(
        "slate {} — an agent-native 2D/2.5D game engine. Text files in, JSON out.\n\nUsage: slate <COMMAND> [ARGS] [FLAGS]\n\nCommands:\n",
        slate_core::ENGINE_VERSION
    );
    for c in COMMANDS {
        out.push_str(&format!("  {:<9} {}\n", c.name, c.summary));
    }
    out.push_str(
        "\nEvery command accepts --json. Exit codes: 0 ok, 1 errors or failed checks, 2 bad command line.\n\
         Start here: `slate new my-game`, then read my-game/AGENTS.md. Details: `slate help <command>`.\n",
    );
    out
}

pub fn command_text(c: &Command) -> String {
    let mut out = format!("slate {} — {}\n\nUsage: {}\n", c.name, c.summary, usage_line(c));
    if !c.args.is_empty() {
        out.push_str("\nArguments:\n");
        for (a, d) in c.args {
            out.push_str(&format!("  {a:<12} {d}\n"));
        }
    }
    if !c.flags.is_empty() {
        out.push_str("\nFlags:\n");
        for f in c.flags {
            let left = match f.value {
                Some(v) => format!("{} {v}", f.name),
                None => f.name.to_string(),
            };
            out.push_str(&format!("  {left:<18} {}\n", f.doc));
        }
    }
    if !c.details.is_empty() {
        out.push_str(&format!("\n{}\n", c.details));
    }
    if !c.examples.is_empty() {
        out.push_str("\nExamples:\n");
        for e in c.examples {
            out.push_str(&format!("  {e}\n"));
        }
    }
    out
}

pub fn command_json(c: &Command) -> Json {
    Json::object()
        .with("name", c.name)
        .with("summary", c.summary)
        .with("usage", usage_line(c))
        .with("args", c.args.iter().map(|(a, d)| Json::object().with("name", *a).with("doc", *d)).collect::<Vec<_>>())
        .with(
            "flags",
            c.flags
                .iter()
                .map(|f| Json::object().with("name", f.name).with("value", f.value).with("doc", f.doc))
                .collect::<Vec<_>>(),
        )
        .with("details", c.details)
        .with("examples", c.examples.iter().map(|e| Json::from(*e)).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn parses_flags_in_both_forms() {
        let p = parse(&args("run scenes/a.toml --frames 30 --seed=4 --events --json")).unwrap();
        assert_eq!(p.cmd.name, "run");
        assert_eq!(p.positional(0), Some("scenes/a.toml"));
        assert_eq!(p.number("--frames").unwrap(), Some(30));
        assert_eq!(p.number("--seed").unwrap(), Some(4));
        assert!(p.has("--events") && p.json());
    }

    #[test]
    fn errors_suggest_fixes() {
        let e = parse(&args("rnu")).unwrap_err();
        assert_eq!(e.hint.as_deref(), Some("did you mean `run`?"));
        let e = parse(&args("shot --frmae 3")).unwrap_err();
        assert_eq!(e.hint.as_deref(), Some("did you mean `--frame`?"));
        let e = parse(&args("run --frames")).unwrap_err();
        assert!(e.message.contains("needs a value"));
        let e = parse(&args("run --frames x")).unwrap().number("--frames").unwrap_err();
        assert_eq!(e.code, "E0003");
    }

    #[test]
    fn every_command_has_examples_that_parse() {
        for c in COMMANDS {
            for ex in c.examples {
                let words: Vec<String> = ex
                    .split_whitespace()
                    .skip(1)
                    .filter(|w| *w != ">" && !w.ends_with(".md"))
                    .map(|w| w.trim_matches('"').to_string())
                    .collect();
                assert!(parse(&words).is_ok(), "example does not parse: {ex}");
            }
        }
    }
}
