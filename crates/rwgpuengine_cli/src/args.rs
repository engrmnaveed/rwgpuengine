//! The command registry and argument parser.
//!
//! Commands are data: the same table drives parsing, `rwgpu help`,
//! `rwgpu help --json` and the generated CLI reference, so they cannot drift.

use std::collections::BTreeMap;

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_core::json::Json;
use rwgpuengine_core::text::did_you_mean;

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
    "Project folder, scene file or playtest file (default: current folder). The project is found by walking up to rwgpu.toml.",
);

/// `rwgpu play`'s details: also the native player's `--help`, which takes the
/// same arguments. The bindings are the player's (players/rwgpuengine_player,
/// `input.rs`); a test there keeps this text and its table equal.
pub const PLAY_DETAILS: &str = "Starts the native player, `rwgpu-player`, which `rwgpu` looks for next to itself and \
then on PATH: rwgpu itself never needs a GPU. The player steps the simulation at the project's tick rate whatever \
the display does, draws with the GPU only when the picture changed, and scales the frame to the window with bars \
around it. When the window closes it prints the frame and the state hash: with --input they match `rwgpu run` \
for the same file.

Keys and taps, as in the web player: arrows or W A S D move, Enter or Space confirm, Escape cancel. A click is a \
tap; a drag of 12 frame pixels or more is a swipe along the iso axes (up-right is up).

Camera: Q and E turn a quarter turn; right-drag turns freely; middle-drag or Shift + right-drag pans; the wheel, \
a pinch, - and = zoom; V switches between iso and perspective; Home goes back to the scene's camera.

E0006 when the player is not installed: build it from a clone of the repository with \
`cargo install --locked --path players/rwgpuengine_player`.";

/// What `rwgpu help serve` says about the two web players.
pub const SERVE_DETAILS: &str = "Opens nothing by itself; visit the printed URL. It serves the WebGPU player when it \
finds one (--web-player, the RWGPU_WEB_PLAYER folder, `web-player` next to rwgpu, or the workspace build): the frame \
is drawn with WebGPU where the browser has it, and by the CPU reference renderer where it has not, and the page says \
which in one line. Without the WebGPU player it serves the CPU player (rwgpuengine_web.wasm), and says so. rwgpu \
itself never needs a GPU: it only serves files. URL parameters: scene=PATH, input=PATH, frames=N (run N frames \
immediately, then pause), renderer=cpu (draw with the CPU reference renderer). The page shows the frame number and \
state hash, which match `rwgpu run` for the same input.

The WebGPU player has the bindings of `rwgpu play` (see `rwgpu help play`): keys, taps, swipes and the camera, with \
a two-finger pinch on a touch screen. The CPU player has keys, taps and swipes.";

pub static COMMANDS: &[Command] = &[
    Command {
        name: "new",
        summary: "Create a project from the starter template",
        args: &[("PATH", "Folder to create (must not exist or be empty).")],
        flags: &[flag("--name", Some("NAME"), "Project name (default: the folder name)."), JSON],
        details: "Writes rwgpu.toml, a starter scene, prefabs, a playtest and an AGENTS.md generated for this \
                  rwgpu version, then checks the result.",
        examples: &["rwgpu new my-game", "rwgpu new games/maze --name \"Maze\""],
    },
    Command {
        name: "check",
        summary: "Validate every project file without running anything",
        args: &[PATH_ARG],
        flags: &[JSON],
        details: "Parses rwgpu.toml, every scene, prefab and playtest; checks types, unknown keys, references \
                  between files and static problems (overlapping solids, signals that can never fire). Exit code 1 \
                  if there are errors; warnings do not fail.",
        examples: &["rwgpu check", "rwgpu check my-game --json"],
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
            "rwgpu run --frames 300",
            "rwgpu run tests/walkthrough.toml --events",
            "rwgpu run --frames 120 --track guard",
            "rwgpu run scenes/garden.toml --input tests/walkthrough.toml --json",
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
        details: "The picture is drawn by a deterministic CPU renderer, in 3D, from the camera as it is at \
                  that frame: identical pixels on every machine. The frame report lists each entity's screen \
                  rectangle, visible pixels and what covers it, and flags off-screen (W0301), hidden (W0302) \
                  and clipped (W0303) entities.",
        examples: &[
            "rwgpu shot",
            "rwgpu shot --input tests/walkthrough.toml --report",
            "rwgpu shot --frame 120 --out a.png --json",
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
            flag(
                "--place",
                Some("PREFAB[@X,Y]"),
                "Can I build here? With a cell: whether a `spawn` of PREFAB fits there at this frame, and why not. \
                 Without: every cell where it fits. Prints only that answer (add --entity for the table).",
            ),
            JSON,
        ],
        details: "Answers what exists, where it is, and why something happened. The text table is a summary \
                  (cell, components, solid, cooldown, patrol target, screen rectangle, tap point); --json has every \
                  component and all runtime state, plus `ground` (size, and the map and terrain legend for terrain \
                  maps). `tap` is a pixel where a tap is guaranteed to hit the entity. `--place` applies the same \
                  rules as the `spawn` action (inside the ground, buildable terrain, nothing enabled standing there).",
        examples: &[
            "rwgpu inspect --json",
            "rwgpu inspect --input tests/walkthrough.toml --frame 60 --entity player --events",
            "rwgpu inspect --input tests/build.toml --frame 120 --place house@3,2",
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
        examples: &["rwgpu test", "rwgpu test --filter walk --json", "rwgpu test --update-refs"],
    },
    Command {
        name: "explain",
        summary: "Explain a diagnostic code, a file format/component, or a command",
        args: &[(
            "[TOPIC]",
            "A code (E0103), a component or format (sprite, entity, connect, playtest) or a command. Omit to list topics.",
        )],
        flags: &[JSON],
        details: "Like `rustc --explain`, for rwgpu's codes and file formats.",
        examples: &["rwgpu explain", "rwgpu explain E0201", "rwgpu explain sprite", "rwgpu explain trigger --json"],
    },
    Command {
        name: "schema",
        summary: "Print the JSON Schema of a file format",
        args: &[("[KIND]", "project, scene, prefab or playtest. Omit to list them.")],
        flags: &[flag("--out", Some("DIR"), "Write all schemas into DIR as <kind>.schema.json."), JSON],
        details: "Editors (e.g. VS Code + Even Better TOML) can use these for completion and inline errors.",
        examples: &["rwgpu schema scene", "rwgpu schema --out .rwgpu/schema"],
    },
    Command {
        name: "docs",
        summary: "Print the reference docs for this exact rwgpu version",
        args: &[],
        flags: &[
            flag("--agents", None, "Print the AGENTS.md that `rwgpu new` writes into projects."),
            flag("--out", Some("DIR"), "Write cli.md, formats.md, errors.md and schemas/ into DIR."),
            JSON,
        ],
        details: "Docs are generated from the same registries the engine uses, so they always match the \
                  installed version.",
        examples: &["rwgpu docs", "rwgpu docs --agents > AGENTS.md"],
    },
    Command {
        name: "serve",
        summary: "Serve the web player for a project (reloads when files change)",
        args: &[PATH_ARG],
        flags: &[
            flag("--port", Some("N"), "Port on 127.0.0.1 (default 4410; 0 picks a free one and prints it)."),
            flag(
                "--web-player",
                Some("DIR"),
                "Folder of the WebGPU player (default: `web-player` next to rwgpu, then the workspace build, see \
                 `cargo xtask web-player`).",
            ),
            flag(
                "--wasm",
                Some("FILE"),
                "Path to rwgpuengine_web.wasm, the CPU player served when there is no WebGPU player (default: the \
                 workspace build, see `cargo xtask web`).",
            ),
        ],
        details: SERVE_DETAILS,
        examples: &[
            "rwgpu serve examples/tiny_town",
            "rwgpu serve --port 8080",
            "rwgpu serve examples/iso_garden --wasm target/wasm32-unknown-unknown/web/rwgpuengine_web.wasm",
        ],
    },
    Command {
        name: "play",
        summary: "Play a project in a window, drawn with the GPU (needs the native player)",
        args: &[PATH_ARG],
        flags: &[
            INPUT,
            SEED,
            flag(
                "--frames",
                Some("N"),
                "Close after N frames and print the result. Default: the playtest's frames with --input, else play \
                 until the window is closed.",
            ),
            flag(
                "--headless",
                None,
                "No window: take the steps as fast as they go and draw each changed frame off screen. Needs --frames \
                 or --input.",
            ),
            flag(
                "--screenshot",
                Some("FILE"),
                "At the end, save what the window shows as a PNG (with --headless, the frame itself).",
            ),
            JSON,
        ],
        details: PLAY_DETAILS,
        examples: &[
            "rwgpu play examples/tiny_town",
            "rwgpu play examples/tiny_town --input tests/turn_and_build.toml --screenshot out/turned.png",
            "rwgpu play tests/build_town.toml --headless --json",
        ],
    },
    Command {
        name: "help",
        summary: "Show help for rwgpu or a command",
        args: &[("[COMMAND]", "Command to describe.")],
        flags: &[JSON],
        details: "`rwgpu help --json` lists every command, argument and flag as data.",
        examples: &["rwgpu help", "rwgpu help shot", "rwgpu help --json"],
    },
    Command {
        name: "version",
        summary: "Print the rwgpu version",
        args: &[],
        flags: &[JSON],
        details: "",
        examples: &["rwgpu version --json"],
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
                    .unwrap_or_else(|| "run `rwgpu help` for the list of commands".into());
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
        // A short flag (`-o`) is a mistake, not a path: rwgpu has long flags only.
        if arg.len() >= 2 && arg.starts_with('-') && arg.as_bytes()[1].is_ascii_alphabetic() {
            let long: Vec<&str> =
                cmd.flags.iter().map(|f| f.name).filter(|n| n.as_bytes().get(2) == Some(&arg.as_bytes()[1])).collect();
            let hint = match long.as_slice() {
                [one] => format!("rwgpu has long flags only: did you mean `{one}`?"),
                [] => format!("rwgpu has long flags only: run `rwgpu help {}` for its flags", cmd.name),
                many => format!("rwgpu has long flags only: one of `{}`", many.join("`, `")),
            };
            return Err(bad_arg(format!("`rwgpu {}` has no flag `{arg}`", cmd.name), Some(hint)));
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
                .unwrap_or_else(|| format!("run `rwgpu help {}` for its flags", cmd.name));
            return Err(bad_arg(format!("`rwgpu {}` has no flag `{name}`", cmd.name), Some(hint)));
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
                "`rwgpu {}` takes at most {max_pos} argument(s), got {}: {}",
                cmd.name,
                positionals.len(),
                positionals.join(" ")
            ),
            Some(format!("run `rwgpu help {}`", cmd.name)),
        ));
    }
    Ok(Parsed { cmd, positionals, flags })
}

// ---------------------------------------------------------------------------
// Help

pub fn usage_line(c: &Command) -> String {
    let mut s = format!("rwgpu {}", c.name);
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
        "rwgpu {} — an agent-native 2D/2.5D game engine. Text files in, JSON out.\n\nUsage: rwgpu <COMMAND> [ARGS] [FLAGS]\n\nCommands:\n",
        rwgpuengine_core::ENGINE_VERSION
    );
    for c in COMMANDS {
        out.push_str(&format!("  {:<9} {}\n", c.name, c.summary));
    }
    out.push_str(
        "\nEvery command accepts --json. Exit codes: 0 ok, 1 errors or failed checks, 2 bad command line.\n\
         Start here: `rwgpu new my-game`, then read my-game/AGENTS.md. Details: `rwgpu help <command>`.\n",
    );
    out
}

pub fn command_text(c: &Command) -> String {
    let mut out = format!("rwgpu {} — {}\n\nUsage: {}\n", c.name, c.summary, usage_line(c));
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
        // Short flags are not paths: name the long one.
        let e = parse(&args("shot -o out/x.png")).unwrap_err();
        assert_eq!(e.message, "`rwgpu shot` has no flag `-o`");
        assert_eq!(e.hint.as_deref(), Some("rwgpu has long flags only: did you mean `--out`?"));
        // `-h` still means help; a value like -1 after a flag is still a value.
        assert!(parse(&args("shot -h")).is_ok());
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
