//! The repository check: what must never be committed
//! ([ADR 0023](../../docs/decisions/0023-release-builds-without-local-paths.md)).
//!
//! It runs first in `cargo xtask ci`, before anything is built, and on its own
//! as `cargo xtask repo-check`. It reads the files git tracks (the index, so a
//! file that is staged counts) and fails on:
//!
//! - an absolute home path of any user, in a file's text or bytes;
//! - build output: a `target` or `target-*` folder anywhere, compiler and
//!   linker output by name (Cargo's `.d` files, `.rlib`, `.o`, ...), or a
//!   compiled program or library by its first bytes;
//! - a file larger than `[repo] file_bytes` in budgets.toml.
//!
//! Untracked files that `.gitignore` does not cover are checked for build
//! output too: a build folder that is not ignored is one `git add` away from
//! being published.

use std::path::Path;
use std::process::Command;

use crate::Result;

/// Names in a home path that stand for nobody. One list, with the reason for
/// each entry; keep it short.
const PLACEHOLDER_NAMES: [(&str, &str); 1] =
    [("someone", "the made-up user name that examples in the docs use (docs/conventions.md#privacy)")];

/// Folder names that only a build makes.
fn is_build_folder(name: &str) -> bool {
    name == "target" || name.starts_with("target-") || name.ends_with(".dSYM")
}

/// File extensions of compiler and linker output.
const BUILD_EXTENSIONS: [&str; 16] =
    ["d", "rlib", "rmeta", "o", "obj", "a", "lib", "so", "dylib", "dll", "exe", "pdb", "ilk", "exp", "wasm", "profraw"];

/// Where macOS keeps home folders. Written in pieces, so that no file of the
/// repository holds the text itself (docs/conventions.md#privacy: outgoing
/// commits are searched for it).
const MAC_HOMES: &str = concat!("/", "Users", "/");

/// How many problems are printed; the count says how many there were.
const SHOWN: usize = 20;

/// One problem: where it is and what to do.
#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    pub path: String,
    /// `:line` for text, ` (byte N)` for other files, or nothing.
    pub at: String,
    pub what: String,
}

/// The check, on the repository at `root`.
pub fn check(root: &Path, max_bytes: u64) -> Result {
    let Some(tracked) = git_paths(root, &["ls-files", "-z"])? else {
        return Ok(());
    };
    let untracked = git_paths(root, &["ls-files", "-z", "--others", "--exclude-standard"])?.unwrap_or_default();
    let mut files = Vec::new();
    for path in &tracked {
        let full = root.join(path);
        let Ok(meta) = std::fs::symlink_metadata(&full) else {
            continue; // deleted in the working tree; the commit will delete it too
        };
        // A link is checked by where it points, which is what git stores.
        let bytes = if meta.file_type().is_symlink() {
            std::fs::read_link(&full).map(|t| t.to_string_lossy().into_owned().into_bytes()).unwrap_or_default()
        } else if meta.is_file() {
            std::fs::read(&full).map_err(|e| format!("{path}: {e}"))?
        } else {
            continue; // a submodule
        };
        files.push((path.clone(), bytes));
    }
    let largest = files.iter().max_by_key(|(_, b)| b.len()).map(|(p, b)| (p.as_str(), b.len())).unwrap_or(("", 0));
    let mut findings = check_files(&files, max_bytes);
    findings.extend(check_untracked(&untracked));
    eprintln!(
        "{} tracked files read, {} untracked ones not ignored; the largest is {} ({} bytes, limit {max_bytes})",
        files.len(),
        untracked.len(),
        largest.0,
        largest.1
    );
    if findings.is_empty() {
        eprintln!("ok: no home path, no build output, no file over the limit");
        return Ok(());
    }
    for f in findings.iter().take(SHOWN) {
        eprintln!("{}{}: {}", f.path, f.at, f.what);
    }
    if findings.len() > SHOWN {
        eprintln!("... and {} more", findings.len() - SHOWN);
    }
    let home = findings.iter().any(|f| f.what.starts_with("an absolute home path"));
    Err(format!(
        "{} problems in the repository (above).{}",
        findings.len(),
        if home {
            " If a home path was already committed, it is in the history too: do not push; tell the owner, who \
             decides whether the history is rewritten."
        } else {
            ""
        }
    ))
}

/// `git <args>` in `root`, as paths. `None` (with a note) when git cannot
/// run here, outside CI; in CI that is an error.
fn git_paths(root: &Path, args: &[&str]) -> Result<Option<Vec<String>>> {
    let out = Command::new("git").args(args).current_dir(root).output();
    match out {
        Ok(out) if out.status.success() => Ok(Some(
            out.stdout
                .split(|&b| b == 0)
                .filter(|p| !p.is_empty())
                .map(|p| String::from_utf8_lossy(p).into())
                .collect(),
        )),
        other => {
            let why = match other {
                Ok(out) => String::from_utf8_lossy(&out.stderr).trim().to_string(),
                Err(e) => e.to_string(),
            };
            if std::env::var_os("CI").is_some() {
                return Err(format!("`git {}` failed ({why}); CI must run the repository check", args.join(" ")));
            }
            eprintln!("skipped: `git {}` failed ({why}). Not a git checkout?", args.join(" "));
            Ok(None)
        }
    }
}

/// The checks of tracked files, given each one's path and bytes.
pub fn check_files(files: &[(String, Vec<u8>)], max_bytes: u64) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (path, bytes) in files {
        let mut found = |at: String, what: String| findings.push(Finding { path: path.clone(), at, what });
        // For build output, untracking it is the one thing to do, whatever is inside.
        if let Some((why, rm)) = build_output_path(path) {
            found(
                String::new(),
                format!(
                    "build output is tracked ({why}). Untrack it with `git rm -r --cached {rm}` and make sure \
                     .gitignore covers it; build outside the repository or in an ignored folder"
                ),
            );
            continue;
        }
        if let Some(kind) = compiled(bytes) {
            found(
                String::new(),
                format!(
                    "a compiled {kind} is tracked. Untrack it with `git rm --cached {path}`: programs are built, \
                     never committed"
                ),
            );
            continue;
        }
        let fix = "Write the path relative to the repository, or with a made-up name such as `/home/someone/`";
        let homes = home_paths(bytes);
        if bytes.iter().take(8000).any(|&b| b == 0) {
            // Not text: the first place, and how many there are.
            if let Some((offset, shown)) = homes.first() {
                let more = match homes.len() {
                    1 => String::new(),
                    n => format!(" (and {} more in this file)", n - 1),
                };
                found(format!(" (byte {offset})"), format!("an absolute home path, `{shown}`{more}. {fix}"));
            }
        } else {
            for (offset, shown) in homes {
                let line = 1 + bytes[..offset].iter().filter(|&&b| b == b'\n').count();
                found(format!(":{line}"), format!("an absolute home path, `{shown}`. {fix}"));
            }
        }
        if bytes.len() as u64 > max_bytes {
            found(
                String::new(),
                format!(
                    "{} bytes, over the limit of {max_bytes} for a tracked file (budgets.toml, [repo] file_bytes). \
                     Split a source file that large; build output and downloads do not belong in the repository",
                    bytes.len()
                ),
            );
        }
    }
    findings
}

/// Untracked files that are not ignored: only build output is a problem.
fn check_untracked(paths: &[String]) -> Vec<Finding> {
    paths
        .iter()
        .filter_map(|path| {
            let (why, folder) = build_output_path(path)?;
            Some(Finding {
                path: path.clone(),
                at: String::new(),
                what: format!(
                    "build output that .gitignore does not cover ({why}): one `git add` would publish it. Add \
                     `/{folder}/` to .gitignore, or build outside the repository"
                ),
            })
        })
        .collect()
}

/// Why `path` is build output, and what to untrack: (reason, path to give `git rm`).
fn build_output_path(path: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = path.split('/').collect();
    let (name, folders) = parts.split_last()?;
    if let Some(i) = folders.iter().position(|f| is_build_folder(f)) {
        return Some((format!("a `{}` folder", folders[i]), parts[..=i].join("/")));
    }
    let ext = name.rsplit_once('.').map(|(stem, e)| (stem, e.to_ascii_lowercase()));
    match ext {
        Some((stem, e)) if !stem.is_empty() && BUILD_EXTENSIONS.contains(&e.as_str()) => {
            Some((format!("a `.{e}` file"), path.to_string()))
        }
        _ => None,
    }
}

/// A compiled program or library, known by its first bytes.
fn compiled(bytes: &[u8]) -> Option<&'static str> {
    let start = |m: &[u8]| bytes.starts_with(m);
    if start(b"\x7fELF") {
        Some("ELF program or library")
    } else if start(&[0xcf, 0xfa, 0xed, 0xfe]) || start(&[0xce, 0xfa, 0xed, 0xfe]) || start(&[0xca, 0xfe, 0xba, 0xbe]) {
        Some("Mach-O program or library")
    } else if start(b"\0asm") {
        Some("WebAssembly module")
    } else if start(b"!<arch>\n") {
        Some("static library or rlib")
    } else if start(b"MZ") && bytes.len() >= 0x40 {
        // A PE file says where its "PE\0\0" header is at offset 0x3c.
        let at = u32::from_le_bytes([bytes[0x3c], bytes[0x3d], bytes[0x3e], bytes[0x3f]]) as usize;
        (bytes.get(at..at + 4) == Some(b"PE\0\0")).then_some("Windows program or library")
    } else {
        None
    }
}

/// Every absolute home path in `bytes`: (offset, the path with its user
/// name shown as `<name>`). The forms: `/home/<name>`, the same under
/// `Users` (macOS, see [`MAC_HOMES`]), and
/// `<drive>:\Users\<name>` with `\`, `/` or `\\` between the folders (any
/// case). A path inside a web address (`https://docs.rs/home/...`) is not
/// one, and nor is a placeholder (`<name>`, `$USER`, [`PLACEHOLDER_NAMES`]).
fn home_paths(bytes: &[u8]) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        let (folder_end, shown) = match b {
            b'/' if bytes[i..].starts_with(MAC_HOMES.as_bytes()) => (i + MAC_HOMES.len(), MAC_HOMES.to_string()),
            b'/' if bytes[i..].starts_with(b"/home/") => (i + 6, "/home/".to_string()),
            b':' if i > 0 && bytes[i - 1].is_ascii_alphabetic() => {
                let Some((end, shown)) = drive_users(bytes, i + 1) else { continue };
                (end, format!("{}:{shown}", bytes[i - 1] as char))
            }
            _ => continue,
        };
        let start = if b == b':' { i - 1 } else { i };
        let name_len = bytes[folder_end..].iter().take_while(|&&c| is_name_byte(c)).count();
        let name = &bytes[folder_end..folder_end + name_len];
        let named = name.iter().any(|c| c.is_ascii_alphanumeric() || *c >= 0x80);
        let placeholder = PLACEHOLDER_NAMES.iter().any(|(p, _)| p.as_bytes() == name);
        if !named || placeholder || in_web_address(bytes, start) {
            continue;
        }
        found.push((start, format!("{shown}<name>")));
    }
    found
}

/// After `<drive>:`, the rest of `\Users\` in one of its forms: where the
/// user name starts, and the form as written.
fn drive_users(bytes: &[u8], at: usize) -> Option<(usize, String)> {
    for sep in [&b"\\\\"[..], b"\\", b"/"] {
        let users = at + sep.len();
        let after = users + 5;
        if bytes[at..].starts_with(sep)
            && bytes.get(users..after).is_some_and(|u| u.eq_ignore_ascii_case(b"users"))
            && bytes[after..].starts_with(sep)
        {
            let sep = String::from_utf8_lossy(sep);
            let users = String::from_utf8_lossy(&bytes[users..after]);
            return Some((after + sep.len(), format!("{sep}{users}{sep}")));
        }
    }
    None
}

fn is_name_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-') || c >= 0x80
}

/// Is the text at `at` part of a web address? The word it is in (back to a
/// space, quote or bracket) has `://` and is not a `file:` address.
fn in_web_address(bytes: &[u8], at: usize) -> bool {
    let word_start =
        bytes[..at].iter().rposition(|&c| c.is_ascii_whitespace() || b"\"'`<>()[]{}".contains(&c)).map_or(0, |p| p + 1);
    let word = &bytes[word_start..at];
    word.windows(3).any(|w| w == b"://") && !word.starts_with(b"file:")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Home paths are written in pieces: this file is tracked, and the check
    // must not find them in its own source.
    const USERS: &str = "Users";
    const HOME: &str = "home";

    fn one(path: &str, text: &str) -> Vec<Finding> {
        check_files(&[(path.to_string(), text.as_bytes().to_vec())], 150_000)
    }

    #[test]
    fn a_clean_tree_passes() {
        let files: Vec<(String, Vec<u8>)> = [
            ("crates/rwgpuengine_sim/src/world.rs", "let p = \"crates/rwgpuengine_sim/src/world.rs:120\";".to_string()),
            ("docs/a.md", format!("Run it from `/{HOME}/someone/rwgpuengine` or `/{USERS}/<name>/`.")),
            ("docs/b.md", format!("See <https://docs.rs/{HOME}/latest/{HOME}/> and https://x.org/{USERS}/x.")),
            ("docs/c.md", format!("`/{HOME}/$USER/`, `C:\\{USERS}\\%USERNAME%\\`, `~/.cargo`, `/{HOME}/`.")),
            ("examples/tiny_town/tests/refs/a.png", b"\x89PNG\r\n\x1a\n\0\0".iter().map(|&b| b as char).collect()),
            ("players/Cargo.lock", "name = \"wgpu\"".into()),
            ("docs/targets.md", "the targets/ folder of a doc is not a build folder".into()),
        ]
        .into_iter()
        .map(|(p, t)| (p.to_string(), t.into_bytes()))
        .collect();
        assert_eq!(check_files(&files, 150_000), vec![]);
        assert_eq!(check_untracked(&["notes.md".into(), "docs/new.md".into()]), vec![]);
    }

    #[test]
    fn home_paths_of_every_form_are_found_with_their_line() {
        for (text, shown) in [
            (format!("x\ny = \"/{USERS}/alice/work\"\n"), format!("/{USERS}/<name>")),
            (format!("x\nHOME=/{HOME}/bob\n"), format!("/{HOME}/<name>")),
            (format!("x\npath C:\\{USERS}\\carol\\x\n"), format!("C:\\{USERS}\\<name>")),
            (format!("x\npath d:/{}/dave/x\n", "users"), "d:/users/<name>".into()),
            (format!("x\n\"C:\\\\{USERS}\\\\erin\\\\x\"\n"), format!("C:\\\\{USERS}\\\\<name>")),
            (format!("x\nfile:///{USERS}/frank/x\n"), format!("/{USERS}/<name>")),
            (format!("x\n/{HOME}/José/x\n"), format!("/{HOME}/<name>")),
        ] {
            let found = one("docs/a.md", &text);
            assert_eq!(found.len(), 1, "{text}: {found:?}");
            assert_eq!(found[0].at, ":2", "{text}");
            assert!(found[0].what.contains(&format!("`{shown}`")), "{text}: {}", found[0].what);
        }
    }

    #[test]
    fn a_home_path_in_a_binary_file_is_found_by_its_byte() {
        let mut bytes = b"\x89PNG\0\0\0".to_vec();
        bytes.extend_from_slice(format!("/{USERS}/alice/.cargo\0/{USERS}/alice/x").as_bytes());
        let found = check_files(&[("a.png".into(), bytes)], 150_000);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].at, " (byte 7)");
        assert!(found[0].what.contains("(and 1 more in this file)"), "{}", found[0].what);
    }

    #[test]
    fn build_output_is_found_by_folder_name_and_content() {
        for (path, rm) in [
            ("target/release/rwgpu", "target"),
            ("players/target-musl/x86_64-unknown-linux-musl/release/rwgpu.d", "players/target-musl"),
            ("players/target-release/release/gpu_check", "players/target-release"),
            ("crates/x/target/debug/build/out.txt", "crates/x/target"),
            ("rwgpu.dSYM/Contents/Info.plist", "rwgpu.dSYM"),
            ("crates/x/libfoo.rlib", "crates/x/libfoo.rlib"),
            ("deps/rwgpu-1a2b3c.d", "deps/rwgpu-1a2b3c.d"),
            ("x/main.o", "x/main.o"),
            ("web/rwgpuengine_web.wasm", "web/rwgpuengine_web.wasm"),
            ("x/rwgpu.PDB", "x/rwgpu.PDB"),
        ] {
            let found = one(path, "");
            assert_eq!(found.len(), 1, "{path}");
            assert!(found[0].what.contains(&format!("`git rm -r --cached {rm}`")), "{path}: {}", found[0].what);
        }
        let mut pe = vec![0u8; 0x80];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c] = 0x40;
        pe[0x40..0x44].copy_from_slice(b"PE\0\0");
        for bytes in [b"\x7fELF\x02\x01".to_vec(), vec![0xcf, 0xfa, 0xed, 0xfe, 7], b"\0asm\x01".to_vec(), pe] {
            let found = check_files(&[("players/rwgpu-player".into(), bytes)], 150_000);
            assert_eq!(found.len(), 1);
            assert!(found[0].what.starts_with("a compiled"), "{}", found[0].what);
        }
        // "MZ" alone is text, not a program.
        assert_eq!(one("docs/mz.md", "MZ is how a DOS program starts"), vec![]);
    }

    #[test]
    fn a_build_folder_that_is_not_ignored_is_found_before_it_is_added() {
        let found = check_untracked(&["players/target-musl/release/rwgpu".into(), "docs/notes.md".into()]);
        assert_eq!(found.len(), 1);
        assert!(found[0].what.contains("Add `/players/target-musl/` to .gitignore"), "{}", found[0].what);
    }

    #[test]
    fn a_file_over_the_limit_is_found() {
        let files = [("docs/big.md".to_string(), vec![b'a'; 1001]), ("docs/ok.md".to_string(), vec![b'a'; 1000])];
        let found = check_files(&files, 1000);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "docs/big.md");
        assert!(found[0].what.starts_with("1001 bytes, over the limit of 1000"));
    }
}
