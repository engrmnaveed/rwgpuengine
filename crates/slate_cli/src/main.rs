//! The `slate` binary: the CLI for data-only projects.

use slate_runtime::{DataOnly, Game};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = slate_cli::main_with(args, &|| Box::new(DataOnly) as Box<dyn Game>);
    std::process::exit(code);
}
