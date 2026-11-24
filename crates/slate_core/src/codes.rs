//! The registry of stable diagnostic codes.
//!
//! Rules (docs/conventions.md#diagnostic-codes):
//! - A code is never reused or renumbered once released. Retire, don't recycle.
//! - `E` = error (fails the command), `W` = warning (reported, does not fail).
//! - Ranges: `x00xx` CLI/IO, `x01xx` file format, `x02xx` references and
//!   static checks, `x03xx` runtime and frame report, `x04xx` playtests.
//! - Every code has an explanation, printed by `slate explain <CODE>`.

use crate::diag::Severity;

/// Static description of one diagnostic code.
#[derive(Debug)]
pub struct CodeInfo {
    pub code: &'static str,
    /// Short kebab-case name, stable like the code.
    pub slug: &'static str,
    pub severity: Severity,
    pub title: &'static str,
    /// Markdown explanation: what it means, common causes, how to fix.
    pub explain: &'static str,
}

macro_rules! codes {
    ($($ident:ident = $code:literal, $slug:literal, $sev:ident, $title:literal, $explain:literal;)*) => {
        $(
            pub const $ident: CodeInfo = CodeInfo {
                code: $code,
                slug: $slug,
                severity: Severity::$sev,
                title: $title,
                explain: $explain,
            };
        )*
        /// Every registered code, in numeric order.
        pub const ALL: &[CodeInfo] = &[$($ident),*];
    };
}

codes! {
    // ---- x00xx: command line and files ---------------------------------
    FILE_NOT_FOUND = "E0001", "file-not-found", Error, "File not found",
"A path given on the command line or referenced by a project file does not exist.

Paths inside project files are relative to the project root (the folder that
holds `slate.toml`). Paths on the command line are relative to the current
directory.

Fix: check the spelling, or list the folder (`ls scenes/`).";

    PROJECT_NOT_FOUND = "E0002", "project-not-found", Error, "No slate.toml found",
"slate looks for `slate.toml` in the given folder and then in each parent
folder, like cargo does with `Cargo.toml`. None was found.

Fix: run the command inside a project, pass the project folder as the first
argument, or create one with `slate new <path>`.";

    BAD_ARGUMENT = "E0003", "bad-argument", Error, "Invalid command-line argument",
"An unknown flag, a missing flag value, or a value of the wrong type was passed.

Fix: run `slate help <command>` (or `slate help --json`) for the exact flags.";

    IO_ERROR = "E0004", "io-error", Error, "Could not read or write a file",
"The operating system refused a read or write: missing permissions, a full
disk, or a path that is a folder.

Fix: check the path in the message and that its parent folder is writable.";

    ALREADY_EXISTS = "E0005", "already-exists", Error, "Target folder is not empty",
"`slate new` refuses to write into a folder that already has files, so it can
never overwrite your work.

Fix: choose a new folder name, or empty the folder first.";

    // ---- x01xx: file format ----------------------------------------------
    TOML_SYNTAX = "E0101", "toml-syntax", Error, "Invalid TOML syntax",
"The file is not valid TOML, so nothing in it could be read.

Common causes: a missing closing quote or bracket, a bare word where a string
was meant (`color = red` instead of `color = \"red\"`), or the same key twice
in one table.

Fix: correct the syntax at the reported line. TOML reference: https://toml.io";

    WRONG_TYPE = "E0102", "wrong-type", Error, "Value has the wrong type",
"A key holds a value of the wrong kind, such as a string where a number is
expected, or a single number where an `[x, y]` array is expected.

Fix: use the type the message names. `slate explain <component>` lists every
key of a component with its type, for example `slate explain sprite`.";

    UNKNOWN_KEY = "E0103", "unknown-key", Error, "Unknown key",
"The table has a key slate does not know. Unknown keys are errors, never
silently ignored, so typos cannot hide.

Fix: use the suggested spelling, or see the valid keys with
`slate explain entity` (or the component's name).";

    MISSING_KEY = "E0104", "missing-key", Error, "Required key is missing",
"A required key is absent, for example `scene` in a playtest's `[test]` table
or `from` in a `[[connect]]`.

Fix: add the key named in the message.";

    INVALID_VALUE = "E0105", "invalid-value", Error, "Invalid value",
"The value has the right type but is not allowed: a color that is not
`#rgb`/`#rrggbb`/`#rrggbbaa`, a number out of range, a word that is not one of
the allowed choices, or a combination of keys that cannot go together.

Fix: the message names what is allowed.";

    // ---- x02xx: references and static checks --------------------------------
    UNKNOWN_PREFAB = "E0201", "unknown-prefab", Error, "Unknown prefab",
"An entity names a prefab that has no file. Prefab `wall` lives in
`prefabs/wall.toml`; prefab `props/tree` lives in `prefabs/props/tree.toml`.

Fix: create the prefab file, or use the suggested name.";

    UNKNOWN_ENTITY = "E0202", "unknown-entity", Error, "Unknown entity name",
"A `[[connect]]` or a playtest `[[expect]]` names an entity that the scene does
not define. Only entities with a `name` can be referenced.

Fix: add `name = \"...\"` to the entity, or use the suggested name.";

    DUPLICATE_NAME = "E0203", "duplicate-name", Error, "Duplicate entity name",
"Two entities in one scene have the same `name`. Names must be unique so that
connections, playtests and `slate inspect --entity` are unambiguous.

Fix: rename one of them.";

    UNKNOWN_SCENE = "E0204", "unknown-scene", Error, "Scene file not found",
"`main_scene` in slate.toml, or `scene` in a playtest, points to a file that
does not exist.

Fix: correct the path (relative to the project root), or create the scene.";

    NAME_WITH_CELLS = "E0205", "name-with-cells", Error, "`name` cannot be used with `cells`",
"`cells` places one copy of the entity on every listed cell. Names must be
unique, so a multi-cell entry cannot have a `name`.

Fix: remove `name`, or split the entry into separate `[[entity]]` tables with
`pos`.";

    SOLID_OVERLAP = "W0201", "solid-overlap", Warning, "Two solid entities share a cell",
"Two enabled solid entities start on the same grid cell. This is usually a
copy-paste mistake and makes the cell impossible to enter.

Fix: move one of them, or make one non-solid.";

    OUTSIDE_GROUND = "W0202", "outside-ground", Warning, "Entity is outside the ground",
"The entity's cell is outside `[ground] size`. Movers can never reach it, and
it may be drawn off-screen.

Fix: move it inside 0..size, or grow the ground.";

    SIGNAL_NEVER_EMITTED = "W0203", "signal-never-emitted", Warning, "Connected signal is never emitted",
"A `[[connect]]` listens for a signal its `from` entity can never emit. Every
entity emits `tapped`; an entity with a `trigger` emits its `on_enter` and
`on_exit` signals (default `pressed` and `released`). Rust game code may emit
other signals, in which case this warning can be ignored.

Fix: add the component that emits the signal, or fix the signal name.";

    INPUT_AFTER_END = "W0204", "input-after-end", Warning, "Input is scheduled after the last frame",
"A playtest `[[input]]` has a `frame` at or after the playtest's `frames`, so
it is never applied.

Fix: lower the frame, or raise `frames` in `[test]`.";

    ENGINE_MISMATCH = "W0205", "engine-mismatch", Warning, "Project targets a different engine version",
"`engine` in slate.toml does not match the running slate version. File formats
may differ. Docs for the running version: `slate docs`.

Fix: update `engine`, or install the matching slate version.";

    // ---- x03xx: runtime and frame report ------------------------------------
    SIGNAL_LIMIT = "E0301", "signal-limit", Error, "Too many signals in one frame",
"More than 1000 signals were dispatched in a single frame. This almost always
means a loop: A's signal enables B, whose signal toggles A, and so on.

Fix: run `slate inspect --events` and look at the `signal` events for the
repeating chain.";

    OFFSCREEN = "W0301", "offscreen", Warning, "Entity is drawn entirely off-screen",
"The entity is enabled and visible but nothing of it lands inside the frame.

Fix: move the camera (`[camera] center`), move the entity, or make the display
larger in slate.toml.";

    OCCLUDED = "W0302", "occluded", Warning, "Sprite is mostly hidden",
"Less than half of a sprite's pixels are visible in the final frame because
other things are drawn over it (usually blocks in front of it).

Fix: move the sprite, lower the blocks in front of it, or accept it if it is
intentional (for example a character walking behind a wall).";

    CLIPPED = "W0303", "clipped", Warning, "Entity is partly off-screen",
"Part of the entity is outside the frame.

Fix: adjust the camera or display size if the whole entity should be visible.";

    // ---- x04xx: playtests -----------------------------------------------------
    EXPECT_FAILED = "E0401", "expect-failed", Error, "Playtest expectation failed",
"An `[[expect]]` in a playtest did not hold at its frame. The message shows
expected and actual values; the notes list the entity's most recent events.

Fix: inspect what happened with
`slate inspect --input <test> --frame <N> --events --entity <name>`.";

    IMAGE_MISMATCH = "E0402", "image-mismatch", Error, "Screenshot differs from reference image",
"The rendered frame differs from the saved reference by more pixels than
`max_diff_pixels` allows. A diff image is written next to the output (changed
pixels in red).

Fix: if the change is intended, update references with
`slate test --update-refs`; otherwise find what moved with `slate shot --report`.";

    REFERENCE_MISSING = "E0403", "reference-missing", Error, "Reference image not found",
"A playtest `[[shot]]` names a reference image that does not exist yet.

Fix: create it from the current output with `slate test --update-refs`, then
look at the image to confirm it is right before committing it.";

    NO_TESTS = "W0401", "no-tests", Warning, "No playtests found",
"`slate test` found no `.toml` files in `tests/`.

Fix: add a playtest; see the playtest format in `slate docs`.";
}

/// Look up a code such as `E0103` (case-insensitive) or a slug such as
/// `unknown-key`.
pub fn lookup(code_or_slug: &str) -> Option<&'static CodeInfo> {
    ALL.iter()
        .find(|c| c.code.eq_ignore_ascii_case(code_or_slug) || c.slug.eq_ignore_ascii_case(code_or_slug))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_well_formed_and_sorted() {
        let mut seen = std::collections::BTreeSet::new();
        for c in ALL {
            assert!(seen.insert(c.code), "duplicate code {}", c.code);
            assert!(seen.insert(c.slug), "duplicate slug {}", c.slug);
            assert_eq!(c.code.len(), 5, "{}", c.code);
            let prefix = if c.severity == Severity::Error { 'E' } else { 'W' };
            assert!(c.code.starts_with(prefix), "{} has the wrong prefix for {:?}", c.code, c.severity);
            assert!(!c.explain.is_empty());
        }
    }

    #[test]
    fn lookup_by_code_or_slug() {
        assert_eq!(lookup("e0103").map(|c| c.code), Some("E0103"));
        assert_eq!(lookup("unknown-key").map(|c| c.code), Some("E0103"));
        assert!(lookup("E9999").is_none());
    }
}
