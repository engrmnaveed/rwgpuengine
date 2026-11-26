//! Release builds that carry no path of the machine that built them
//! ([ADR 0023](../../docs/decisions/0023-release-builds-without-local-paths.md)).
//!
//! Every release build that xtask makes goes through [`cargo_release`]: the
//! `rwgpu` command, the web module, the players' programs and the release
//! builds the budgets time. It passes rustc one `--remap-path-prefix` for each
//! folder of this machine that source files are read from. After a build,
//! [`check_artifacts`] reads each artifact's bytes for those folders; a hit is
//! an error that names the artifact and the first strings found.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

/// How many of the strings found are shown for one artifact.
const SHOWN: usize = 5;

/// The folders of the build machine that must not be in an artifact.
#[derive(Debug, Clone)]
pub struct Machine {
    /// The home folder (`HOME`, and on Windows also `USERPROFILE`).
    pub homes: Vec<String>,
    /// The repository: the agent's workspace, with the players' inside it.
    pub root: String,
    /// Cargo's home (`CARGO_HOME`, or `.cargo` in the home folder).
    pub cargo_home: String,
    /// The folders under `<cargo home>/registry/src`, one per registry.
    pub registries: Vec<String>,
    /// The separator this system writes between folders.
    pub sep: char,
}

impl Machine {
    /// This machine, for the repository at `root`.
    pub fn here(root: &Path) -> Result<Machine> {
        let text = |p: &Path| p.to_str().map(str::to_string).ok_or(format!("a folder name that is not UTF-8: {p:?}"));
        let mut homes = Vec::new();
        for var in ["HOME", "USERPROFILE"] {
            if let Some(h) = std::env::var_os(var).filter(|h| !h.is_empty()) {
                let h = text(Path::new(&h))?;
                if !homes.contains(&h) {
                    homes.push(h);
                }
            }
        }
        let cargo_home = match std::env::var_os("CARGO_HOME").filter(|h| !h.is_empty()) {
            // Cargo reads a relative CARGO_HOME from the folder it runs in.
            Some(h) => std::env::current_dir().map_err(|e| e.to_string())?.join(h),
            None => Path::new(homes.first().ok_or("neither HOME nor USERPROFILE is set")?).join(".cargo"),
        };
        let mut registries = Vec::new();
        if let Ok(entries) = std::fs::read_dir(cargo_home.join("registry").join("src")) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    registries.push(text(&entry.path())?);
                }
            }
        }
        registries.sort();
        Ok(Machine {
            homes,
            root: text(root)?,
            cargo_home: text(&cargo_home)?,
            registries,
            sep: std::path::MAIN_SEPARATOR,
        })
    }

    /// The rustc flags that rewrite this machine's folders in what it builds:
    ///
    /// - the repository → nothing: `crates/rwgpuengine_sim/src/world.rs`;
    /// - a registry's folder → nothing: `png-0.18.1/src/common.rs`, the
    ///   crate and its version, as Cargo's own `trim-paths` would write it;
    /// - Cargo's git checkouts → nothing: `<repository>-<hash>/<commit>/...`;
    /// - anything else in the home folder → `~`, so no user name is left.
    ///
    /// rustc applies the last flag whose folder matches, so the longest
    /// folder comes last and the most specific rule wins. The flags depend
    /// only on where things are on this machine, so they stay the same from
    /// one build to the next and never make Cargo rebuild.
    pub fn remap_flags(&self) -> Vec<String> {
        let sep = self.sep;
        let dir = |p: &str| if p.ends_with(sep) { p.to_string() } else { format!("{p}{sep}") };
        let mut maps: Vec<(String, String)> = Vec::new();
        for home in self.homes.iter().filter(|h| !is_top(h)) {
            maps.push((dir(home), format!("~{sep}")));
        }
        maps.push((dir(&format!("{}{sep}git{sep}checkouts", self.cargo_home)), String::new()));
        for registry in &self.registries {
            maps.push((dir(registry), String::new()));
        }
        maps.push((dir(&self.root), String::new()));
        maps.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then(a.0.cmp(&b.0)));
        maps.dedup_by(|a, b| a.0 == b.0);
        maps.into_iter().map(|(from, to)| format!("--remap-path-prefix={from}={to}")).collect()
    }

    /// The rustflags for a release build: those already in the environment
    /// (`CARGO_ENCODED_RUSTFLAGS`, else `RUSTFLAGS`) and the remapping. Set
    /// as `CARGO_ENCODED_RUSTFLAGS`, they replace the rustflags of Cargo's
    /// configuration files for that build.
    fn encoded_rustflags(&self) -> String {
        let mut flags: Vec<String> = match std::env::var("CARGO_ENCODED_RUSTFLAGS") {
            Ok(v) if !v.is_empty() => v.split('\x1f').map(str::to_string).collect(),
            _ => std::env::var("RUSTFLAGS").unwrap_or_default().split_whitespace().map(str::to_string).collect(),
        };
        flags.extend(self.remap_flags());
        flags.join("\x1f")
    }

    /// What an artifact is searched for: each folder in the forms it can take
    /// on this system, each with the name it is shown under.
    ///
    /// A folder with backslashes (Windows) is also searched with forward
    /// slashes and with doubled backslashes (as a string literal writes it).
    /// The search ignores ASCII case and also looks for UTF-16 (see [`scan`]).
    pub fn needles(&self) -> Vec<(String, &'static str)> {
        let mut bases: Vec<(&str, &'static str)> =
            self.homes.iter().filter(|h| !is_top(h)).map(|h| (h.as_str(), "<home>")).collect();
        bases.push((&self.root, "<repository>"));
        bases.push((&self.cargo_home, "<cargo home>"));
        let mut needles: Vec<(String, &'static str)> = Vec::new();
        for (base, name) in bases {
            let base = base.trim_end_matches(['/', '\\']);
            for form in [base.to_string(), base.replace('\\', "/"), base.replace('\\', "\\\\")] {
                if !needles.iter().any(|(n, _)| n.eq_ignore_ascii_case(&form)) {
                    needles.push((form, name));
                }
            }
        }
        needles
    }
}

/// `/`, `C:\` and the like: a home folder there names nobody and would match
/// every path.
fn is_top(p: &str) -> bool {
    let p = p.trim_end_matches(['/', '\\']);
    p.is_empty() || (p.len() == 2 && p.ends_with(':'))
}

/// Run `cargo <args>` in `dir` as a release build: with this machine's
/// folders remapped. Every release build of xtask goes through here, so the
/// gates, the budgets and people build the same artifacts.
pub fn cargo_release(machine: &Machine, dir: &Path, args: &[&str]) -> Result {
    eprintln!("$ cargo {}   (a release build: this machine's folders remapped)", args.join(" "));
    let status = release_command(machine, dir, args).status().map_err(|e| format!("cargo: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("`cargo {}` failed ({status})", args.join(" "))) }
}

/// The command [`cargo_release`] runs, for a caller that reads its output.
pub fn release_command(machine: &Machine, dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.args(args).current_dir(dir).env("CARGO_ENCODED_RUSTFLAGS", machine.encoded_rustflags()).env_remove("RUSTFLAGS");
    cmd
}

/// One place in an artifact where a folder of the machine was found: the
/// name the folder is shown under and the text that follows it.
#[derive(Debug, PartialEq, Eq)]
pub struct Hit {
    pub folder: &'static str,
    pub text: String,
}

/// Every place in `bytes` where one of `needles` starts, ignoring ASCII case,
/// in UTF-8 and in UTF-16 (little-endian, as Windows writes wide strings).
/// Where several needles match at one place, the longest names it.
pub fn scan(bytes: &[u8], needles: &[(String, &'static str)]) -> Vec<Hit> {
    // The standard library's substring search is fast even in xtask's debug
    // build. Lossy decoding keeps every ASCII byte where it was, and NUL is
    // valid UTF-8, so UTF-16 text of ASCII is found too.
    let text = String::from_utf8_lossy(bytes);
    let lower = text.to_ascii_lowercase();
    let mut found: Vec<(usize, usize, &'static str, bool)> = Vec::new();
    for (needle, name) in needles {
        let wide: String = needle.chars().flat_map(|c| [c, '\0']).collect();
        for (form, utf16) in [(needle.to_ascii_lowercase(), false), (wide.to_ascii_lowercase(), true)] {
            let mut from = 0;
            while let Some(at) = lower[from..].find(&form) {
                found.push((from + at, form.len(), name, utf16));
                from += at + 1;
            }
        }
    }
    // By place, the longest first; a match inside a longer one is the same place.
    found.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut hits = Vec::new();
    let mut end = 0;
    for (at, len, name, utf16) in found {
        if at < end {
            continue;
        }
        end = at + len;
        hits.push(Hit { folder: name, text: following(text.as_bytes(), end, utf16) });
    }
    hits
}

/// The printable text that follows `at`, up to 80 characters.
fn following(bytes: &[u8], at: usize, utf16: bool) -> String {
    let step = if utf16 { 2 } else { 1 };
    let mut out = String::new();
    let mut i = at;
    while i + step <= bytes.len() && out.len() < 80 {
        let (b, high) = (bytes[i], if utf16 { bytes[i + 1] } else { 0 });
        if high != 0 || !(0x20..0x7f).contains(&b) {
            break;
        }
        out.push(b as char);
        i += step;
    }
    out
}

/// Read each artifact and fail if a folder of this machine is in it.
pub fn check_artifacts(machine: &Machine, root: &Path, artifacts: &[PathBuf]) -> Result {
    let needles = machine.needles();
    let mut failed = Vec::new();
    for artifact in artifacts {
        let shown = artifact.strip_prefix(root).unwrap_or(artifact).display().to_string();
        let bytes = std::fs::read(artifact).map_err(|e| format!("{shown}: {e}"))?;
        let hits = scan(&bytes, &needles);
        if hits.is_empty() {
            eprintln!("ok: no path of this machine in {shown} ({} bytes)", bytes.len());
            continue;
        }
        let mut lines: Vec<String> = Vec::new();
        for hit in &hits {
            let line = format!("    {}{}", hit.folder, hit.text);
            if !lines.contains(&line) && lines.len() < SHOWN {
                lines.push(line);
            }
        }
        eprintln!("FAIL: {shown} holds {} paths of this machine, for example:\n{}", hits.len(), lines.join("\n"));
        failed.push(shown);
    }
    if failed.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} hold paths of the machine that built them. Build release artifacts with xtask (`cargo xtask dist`), \
         which remaps this machine's folders. If they were built that way, a new kind of path got in: find where \
         it comes from (an `env!(\"CARGO_MANIFEST_DIR\")` in a program, a build script, a new folder sources are \
         read from) and remove it, or add its folder to `Machine::remap_flags` in xtask/src/dist.rs (ADR 0023)",
        failed.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The home folders below are made up, and written in pieces so that the
    // repository check (which reads this file) does not see a home path.
    fn unix() -> Machine {
        let home = format!("/{}/someone", "Users");
        Machine {
            homes: vec![home.clone()],
            root: format!("{home}/work/rwgpuengine"),
            cargo_home: format!("{home}/.cargo"),
            registries: vec![format!("{home}/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f")],
            sep: '/',
        }
    }

    fn windows() -> Machine {
        let home = format!("C:\\{}\\runneradmin", "Users");
        Machine {
            homes: vec![home.clone()],
            root: "D:\\a\\rwgpuengine\\rwgpuengine".into(),
            cargo_home: format!("{home}\\.cargo"),
            registries: vec![format!("{home}\\.cargo\\registry\\src\\index.crates.io-1949cf8c6b5b557f")],
            sep: '\\',
        }
    }

    /// What rustc does with the flags: the last folder that matches wins.
    fn remap(flags: &[String], path: &str) -> String {
        for flag in flags.iter().rev() {
            let (from, to) = flag.trim_start_matches("--remap-path-prefix=").rsplit_once('=').unwrap();
            if let Some(rest) = path.strip_prefix(from) {
                return format!("{to}{rest}");
            }
        }
        path.to_string()
    }

    #[test]
    fn remapped_paths_name_the_repository_file_or_the_crate_and_version() {
        for m in [unix(), windows()] {
            let flags = m.remap_flags();
            let s = m.sep;
            let reg = format!("{}{s}png-0.18.1{s}src{s}common.rs", m.registries[0]);
            assert_eq!(remap(&flags, &reg), format!("png-0.18.1{s}src{s}common.rs"));
            let own = format!("{}{s}crates{s}rwgpuengine_sim{s}src{s}world.rs", m.root);
            assert_eq!(remap(&flags, &own), format!("crates{s}rwgpuengine_sim{s}src{s}world.rs"));
            let git = format!("{}{s}git{s}checkouts{s}foo-1a2b{s}c3d4e5f{s}src{s}lib.rs", m.cargo_home);
            assert_eq!(remap(&flags, &git), format!("foo-1a2b{s}c3d4e5f{s}src{s}lib.rs"));
            let other = format!("{}{s}.rustup{s}x.rs", m.homes[0]);
            assert_eq!(remap(&flags, &other), format!("~{s}.rustup{s}x.rs"));
            // Every flag's folder ends in a separator: a folder is never a
            // prefix of a longer name beside it.
            assert!(flags.iter().all(|f| f.rsplit_once('=').unwrap().0.ends_with(s)), "{flags:?}");
        }
    }

    #[test]
    fn a_home_folder_at_the_top_is_left_alone() {
        let mut m = unix();
        m.homes = vec!["/".into()];
        assert!(m.remap_flags().iter().all(|f| !f.contains("=~")));
        assert!(m.needles().iter().all(|(n, _)| n != "/" && !n.is_empty()));
    }

    #[test]
    fn needles_cover_the_forms_windows_writes() {
        let needles = windows().needles();
        let forms: Vec<&str> = needles.iter().map(|(n, _)| n.as_str()).collect();
        let home = format!("C:\\{}\\runneradmin", "Users");
        assert!(forms.contains(&home.as_str()));
        assert!(forms.contains(&home.replace('\\', "/").as_str()));
        assert!(forms.contains(&home.replace('\\', "\\\\").as_str()));
        assert!(forms.contains(&"D:/a/rwgpuengine/rwgpuengine"));
        // On Unix there is one form of each.
        assert_eq!(unix().needles().len(), 3);
    }

    #[test]
    fn a_planted_path_is_found_and_shown_under_its_folder_name() {
        let m = unix();
        let mut bytes = b"\x00\x01called `Option::unwrap()`".to_vec();
        bytes.extend_from_slice(format!("{}/png-0.18.1/src/common.rs\x00", m.registries[0]).as_bytes());
        bytes.extend_from_slice(format!("{}/crates/x.rs", m.root).as_bytes());
        let hits = scan(&bytes, &m.needles());
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].folder, "<cargo home>");
        assert_eq!(hits[0].text, "/registry/src/index.crates.io-1949cf8c6b5b557f/png-0.18.1/src/common.rs");
        assert_eq!(hits[1], Hit { folder: "<repository>", text: "/crates/x.rs".into() });
    }

    #[test]
    fn windows_forms_case_and_utf16_are_found() {
        let m = windows();
        let needles = m.needles();
        let home = format!("C:\\{}\\runneradmin", "Users");
        for planted in [
            format!("{home}\\.rustup\\x"),
            format!("{}\\.rustup\\x", home.to_lowercase()),
            format!("{}/.rustup/x", home.replace('\\', "/")),
            format!("{}\\\\.rustup", home.replace('\\', "\\\\")),
            "d:\\A\\rwgpuengine\\rwgpuengine\\target\\release\\rwgpu.pdb".into(),
        ] {
            let bytes = format!("\x00\x7fELF..{planted}\x00").into_bytes();
            assert_eq!(scan(&bytes, &needles).len(), 1, "{planted}");
        }
        let wide: Vec<u8> = format!("{home}\\x.pdb").encode_utf16().flat_map(u16::to_le_bytes).collect();
        let hits = scan(&wide, &needles);
        assert_eq!(hits, vec![Hit { folder: "<home>", text: "\\x.pdb".into() }]);
    }

    #[test]
    fn an_artifact_without_the_machines_paths_passes() {
        for m in [unix(), windows()] {
            let bytes = b"png-0.18.1/src/common.rs\x00crates/rwgpuengine_sim/src/world.rs\x00/rustc/0ef1bc386059596dada8fb65b50425aab2ea637a/library/core/src/option.rs\x00/Library/Frameworks/Metal.framework\x00~/.rustup";
            assert_eq!(scan(bytes, &m.needles()), vec![]);
        }
    }
}
