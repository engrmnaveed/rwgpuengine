//! `slate_cli` — the agent surface: files in, commands, JSON out.
//!
//! The CLI is a library so a game with Rust code can ship its own binary
//! that is "slate plus my systems":
//!
//! ```no_run
//! use slate_runtime::{DataOnly, Game};
//! fn main() {
//!     let code = slate_cli::main_with(std::env::args().skip(1).collect(), &|| Box::new(DataOnly) as Box<dyn Game>);
//!     std::process::exit(code);
//! }
//! ```

pub mod args;
pub mod commands;
pub mod docs;
pub mod out;
pub mod project;
pub mod serve;

use slate_runtime::Game;

/// Builds a fresh [`Game`] for each session.
pub type GameFactory = dyn Fn() -> Box<dyn Game>;

/// Run the CLI with `args` (without the program name). Returns the exit code.
pub fn main_with(args: Vec<String>, games: &GameFactory) -> i32 {
    let json_requested = args.iter().any(|a| a == "--json");
    let parsed = match args::parse(&args) {
        Ok(p) => p,
        Err(d) => {
            let name = args.first().map_or("slate", String::as_str);
            return out::emit(name, out::Outcome::usage(d), json_requested);
        }
    };
    let name = parsed.cmd.name;
    if parsed.help() {
        let mut o = out::Outcome::new(true);
        o.text = args::command_text(parsed.cmd);
        o.data = args::command_json(parsed.cmd);
        return out::emit("help", o, parsed.json());
    }
    let outcome = match name {
        "new" => commands::new(&parsed),
        "check" => commands::check(&parsed),
        "run" => commands::run(&parsed, games),
        "shot" => commands::shot(&parsed, games),
        "inspect" => commands::inspect(&parsed, games),
        "test" => commands::test(&parsed, games),
        "explain" => commands::explain(&parsed),
        "schema" => commands::schema_cmd(&parsed),
        "docs" => commands::docs_cmd(&parsed),
        "serve" => serve::serve(&parsed),
        "help" => commands::help(&parsed),
        "version" => commands::version(),
        other => unreachable!("command `{other}` is registered but not dispatched"),
    };
    out::emit(name, outcome, parsed.json())
}
