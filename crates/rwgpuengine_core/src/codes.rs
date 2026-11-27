//! The registry of stable diagnostic codes.
//!
//! Rules (docs/conventions.md#diagnostic-codes):
//! - A code is never reused or renumbered once released. Retire, don't recycle.
//! - `E` = error (fails the command), `W` = warning (reported, does not fail).
//! - Ranges: `x00xx` CLI/IO, `x01xx` file format, `x02xx` references and
//!   static checks, `x03xx` runtime and frame report, `x04xx` playtests,
//!   `x05xx` the players (graphics adapter and window).
//! - Every code has an explanation, printed by `rwgpu explain <CODE>`.

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
holds `rwgpu.toml`). Paths on the command line are relative to the current
directory.

Fix: check the spelling, or list the folder (`ls scenes/`).";

    PROJECT_NOT_FOUND = "E0002", "project-not-found", Error, "No rwgpu.toml found",
"rwgpu looks for `rwgpu.toml` in the given folder and then in each parent
folder, like cargo does with `Cargo.toml`. None was found.

Fix: run the command inside a project, pass the project folder as the first
argument, or create one with `rwgpu new <path>`.";

    BAD_ARGUMENT = "E0003", "bad-argument", Error, "Invalid command-line argument",
"An unknown flag, a missing flag value, or a value of the wrong type was passed.

Fix: run `rwgpu help <command>` (or `rwgpu help --json`) for the exact flags.";

    IO_ERROR = "E0004", "io-error", Error, "Could not read or write a file",
"The operating system refused a read or write: missing permissions, a full
disk, or a path that is a folder.

Fix: check the path in the message and that its parent folder is writable.";

    ALREADY_EXISTS = "E0005", "already-exists", Error, "Target folder is not empty",
"`rwgpu new` and `rwgpu export web` refuse to write into a folder that already
has files, so they can never overwrite your work. `rwgpu export web --force`
writes its own files into such a folder and leaves everything else as it is,
but it never writes through a link, or over a folder, that has the name of one
of its files.

Fix: choose a new folder name, or empty the folder first.";

    PLAYER_NOT_FOUND = "E0006", "player-not-found", Error, "The native player is not installed",
"`rwgpu play` starts a separate program, `rwgpu-player`, which draws the game
with the GPU in a window. `rwgpu` itself never needs a GPU, so the player is
built and installed on its own. `rwgpu play` looks for it next to the `rwgpu`
program first, then in each folder on `PATH`.

Fix: from a clone of the rwgpuengine repository, run
`cargo install --locked --path players/rwgpuengine_player`. It puts
`rwgpu-player` in cargo's bin folder, which is on `PATH` after a normal Rust
install. Or copy `rwgpu-player` next to `rwgpu`.";

    WEB_TOOL_MISSING = "E0007", "web-tool-missing", Error, "The wasm-bindgen tool is missing",
"The WebGPU web player is a wasm module and the JavaScript glue that the
`wasm-bindgen` tool writes for it. The tool must be the exact version of the
`wasm-bindgen` crate in `players/Cargo.lock`, or its glue does not fit the
module. `cargo xtask web-player` (and `cargo xtask gpu`, `cargo xtask dist`)
looks for that version in `WASM_BINDGEN`, then in `target/tools/`, then on
`PATH`, and installs it into `target/tools/` with `cargo install --locked` when
it finds none. This error means that the install failed (with no network, for
example) or that the tool found does not run.

Fix: run the command again with a network, or install the tool by hand with
the command in the message and name it in `WASM_BINDGEN` or put it on `PATH`.
Nothing else needs it: without the WebGPU player, `rwgpu serve` serves the CPU
player.";

    WEB_PLAYER_NOT_FOUND = "E0008", "web-player-not-found", Error, "No web player was found",
"`rwgpu serve` and `rwgpu export web` need a web player, which is built on its
own. There are two. The WebGPU player (ADR 0024) is a folder of four files,
`index.html`, `player.js`, `rwgpu_web.js` and `rwgpu_web_bg.wasm`, that
`cargo xtask web-player` writes to `target/web-player`; where a browser has no
WebGPU it draws with the CPU reference renderer. The CPU player (ADR 0008) is
the module `rwgpuengine_web.wasm`, which `cargo xtask web` builds; its page and
script are inside `rwgpu`.

A flag names the one place looked at, and beats a variable: `--web-player
DIR`, `--wasm FILE`, then `RWGPU_WEB_PLAYER`, `RWGPU_WEB_WASM`. With none, the
WebGPU player is looked for in `web-player` next to the `rwgpu` program, then,
when `rwgpu` runs from the source tree (a debug build), in `target/web-player`;
then the CPU player's module in the same two places.

Fix: from a clone of the rwgpuengine repository, `cargo xtask dist` builds
`rwgpu` and puts both players next to it (`target/release/`; with `--agent`,
the CPU player only). Or build one and name it with the flag or the variable,
or copy it next to `rwgpu`.";

    EXPORT_EXTRA_FILES = "W0001", "export-extra-files", Warning, "The export folder holds other files",
"`rwgpu export web --force` wrote into a folder that already held files it
does not write. They were left as they are. A web host that publishes the
folder publishes them too.

Fix: look at the files the message names. To publish only the game, export
into a new or empty folder.";

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

Fix: use the type the message names. `rwgpu explain <component>` lists every
key of a component with its type, for example `rwgpu explain sprite`.";

    UNKNOWN_KEY = "E0103", "unknown-key", Error, "Unknown key",
"The table has a key rwgpu does not know. Unknown keys are errors, never
silently ignored, so typos cannot hide.

Fix: use the suggested spelling, or see the valid keys with
`rwgpu explain entity` (or the component's name).";

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
"A `[[connect]]` (including its `when` conditions) or a playtest `[[expect]]`
names an entity that the scene does not define. Only entities with a `name` can
be referenced.

Fix: add `name = \"...\"` to the entity, or use the suggested name.";

    DUPLICATE_NAME = "E0203", "duplicate-name", Error, "Duplicate entity name",
"Two entities in one scene have the same `name`. Names must be unique so that
connections, playtests and `rwgpu inspect --entity` are unambiguous.

Fix: rename one of them.";

    UNKNOWN_SCENE = "E0204", "unknown-scene", Error, "Scene file not found",
"`main_scene` in rwgpu.toml, or `scene` in a playtest, points to a file that
does not exist.

Fix: correct the path (relative to the project root), or create the scene.";

    NAME_WITH_CELLS = "E0205", "name-with-cells", Error, "`name` cannot be used with `cells`",
"`cells` places one copy of the entity on every listed cell. Names must be
unique, so a multi-cell entry cannot have a `name`.

Fix: remove `name`, or split the entry into separate `[[entity]]` tables with
`pos`.";

    UNKNOWN_PROP = "E0206", "unknown-prop", Error, "Unknown prop",
"A `[[connect]]` (`prop`, or a `when` condition) or a label placeholder (`{score}`)
names a prop the entity does not have. Props only exist if they are declared in
the entity's (or its prefab's) `props` table, so typos are caught before running.

Fix: declare it with a starting value, e.g. `props = { score = 0 }` on that
entity, or use the suggested name.";

    PROP_TYPE = "E0207", "prop-type", Error, "Value does not fit the prop's type",
"A prop keeps the type of its declared starting value. `add` needs a number prop,
`set` and `equals` need a value of the prop's type (an integer prop takes
integers; a number prop takes integers or decimals), and `at_least`/`at_most`
need a number prop.

Fix: change the value, or declare the prop with the type you mean (`0` for a
count, `0.0` for a decimal, `\"\"` for text, `false` for a flag).";

    UNKNOWN_TERRAIN = "E0208", "unknown-terrain", Error, "Map uses an undeclared terrain character",
"A row of `[ground] map` has a character that no `[[terrain]]` table declares
with `char`, so rwgpu does not know what that cell is.

Fix: add a `[[terrain]]` table with `char = \"...\"` for it (with a `name`,
`color` and `build`), or change the character to a declared one. Every cell
needs a terrain: write plain ground as a declared character too.";

    TOO_LARGE = "E0209", "too-large", Error, "Larger than the engine allows",
"A file, a scene or a playtest is larger than this version of rwgpu allows.
The message says which limit and by how much:

- a file of more than 67108864 bytes (64 MiB);
- a scene of more than 1000000 entities (each cell of `cells` is one);
- a ground of more than 16777216 cells (4096 x 4096);
- a scene whose entities cover more than 4194304 cells (2048 x 2048), every
  `footprint` counted in full;
- a scene that copies more than 134217728 bytes (128 MiB) of prefab files into
  its entities: each prefab's size, once for every entity made from it;
- a name or a key of more than 128 characters;
- a playtest of more than 100000 `[[input]]`, `[[expect]]` and `[[shot]]`
  tables;
- a list of more than 64 `[[shape]]` tables.

The limits keep `rwgpu check`, loading and the first step of a game short
whatever a file says. A project over one is refused where it goes over,
before the work it would cause, and nothing after that point in the file is
read. Each is far above what the example games need; a town of 500,000
actors is allowed. `rwgpu explain limits` lists every limit.

Fix: make the named part smaller. This version cannot load a larger one.";

    SOLID_OVERLAP = "W0201", "solid-overlap", Warning, "Two solid entities share a cell",
"Two enabled solid entities start on the same grid cell (for an entity with a
`footprint`, any cell it covers). This is usually a copy-paste mistake and makes
the cell impossible to enter.

Fix: move one of them, or make one non-solid. If it is deliberate (for example
two stacked door leaves, each opened by its own plate, to build an AND), add
`allow = [\"W0201\"]` to either entity.";

    OUTSIDE_GROUND = "W0202", "outside-ground", Warning, "Entity is outside the ground",
"The entity's cell (or a cell of its `footprint`) is outside the ground
(`[ground] size`, or the size of `[ground] map`). Movers can never reach it, and
it may be drawn off-screen.

Fix: move it inside 0..size, or grow the ground.";

    SIGNAL_NEVER_EMITTED = "W0203", "signal-never-emitted", Warning, "Connected signal is never emitted",
"A `[[connect]]` listens for a signal its `from` entity can never emit. Every
entity emits `tapped`; an entity with a `trigger` emits its `on_enter` and
`on_exit` signals (default `pressed` and `released`); an entity whose props are
changed by a `[[connect]]` `add` or `set` emits `changed`; an entity made by a
`spawn` action emits `spawned`; the ground (`from = \"ground\"`) emits only
`tapped`. With `from_prefab`, the prefab's own components decide. Rust game code
may emit other signals, in which case this warning can be ignored.

Fix: add the component that emits the signal, or fix the signal name.";

    INPUT_AFTER_END = "W0204", "input-after-end", Warning, "Input is scheduled after the last frame",
"A playtest `[[input]]` has a `frame` at or after the playtest's `frames`, so
it is never applied.

Fix: lower the frame, or raise `frames` in `[test]`.";

    ENGINE_MISMATCH = "W0205", "engine-mismatch", Warning, "Project targets a different engine version",
"`engine` in rwgpu.toml does not match the running rwgpu version. File formats
may differ. Docs for the running version: `rwgpu docs`.

Fix: update `engine`, or install the matching rwgpu version.";

    ACTOR_IN_SOLID = "W0206", "actor-in-solid", Warning, "Actor starts inside a solid entity",
"An actor (an entity with mover, patrol or wander) starts on a cell that also holds
an enabled solid entity. It can walk out, but nothing can walk in, and it usually
means a typo in `pos` (a player spawned inside a wall).

Fix: move one of them. If it is deliberate (a hidden stationary actor used as a
logic piece inside a wall), add `allow = [\"W0206\"]` to the actor.";

    UNSUPPORTED_CHAR = "W0207", "unsupported-char", Warning, "Label text has characters the font cannot draw",
"Labels use rwgpu's built-in pixel font, which has the printable ASCII characters
(letters a-z and A-Z, digits, punctuation and space) and line breaks. Other
characters (accents, other scripts, emoji) are drawn as a hollow box.

Fix: use ASCII text. Fonts for other scripts are not in this version.";

    SHAPE_OUTSIDE_FOOTPRINT = "W0208", "shape-outside-footprint", Warning, "A shape reaches outside its entity's cells",
"A `[[shape]]` reaches past the cells its entity covers: `at` is below 0, or
`at` plus `size` is past the `footprint`, along x or y. Placement, blocking
and taps go by the footprint, so the part outside stands on cells that count
as free, and it is drawn into whatever is built there.

Fix: make the shape smaller or move it (`at`), or give the entity the
`footprint` it needs. If it is meant (the eaves of a roof), add
`allow = [\"W0208\"]`.";

    SHAPES_IN_ONE_PLANE = "W0209", "shapes-in-one-plane", Warning, "Two shapes have a side in the same plane",
"Two shapes of one list each have an upright side in the same plane, facing
the same way, and the two sides overlap: a chimney flush with a wall. No rule
says which of two sides in one plane shows, so the picture there can differ
between two cameras and between the two renderers.

Fix: move one shape a little (`at`), so that its side is in front of the
other or behind it, or make them meet without overlapping.";

    // ---- x03xx: runtime and frame report ------------------------------------
    SIGNAL_LIMIT = "E0301", "signal-limit", Error, "Too many signals in one frame",
"More than 1000 signals were dispatched in a single frame. This almost always
means a loop: A's signal enables B, whose signal toggles A, and so on.

Fix: run `rwgpu inspect --events` and look at the `signal` events for the
repeating chain.";

    ACTION_LIMIT = "E0302", "action-limit", Error, "Too many actions in one frame",
"More than 10000 `[[connect]]` actions ran (or were skipped by their `when`) in
a single frame. The step stopped there: the signals still queued were dropped,
and an `error` event names the action it stopped at. One signal can reach many
connections, each with an `actions` list, and every `add` or `set` that changes
a prop can emit `changed` again: a few connections that feed each other reach
thousands of actions within the 1000 signals a frame allows (E0301).

Fix: run `rwgpu inspect --frame N --events` at the frame the diagnostic names
and look at which connection repeats. Listen to a narrower signal, add a `when`
that stops the chain, or keep the actions that feed each other apart.
`rwgpu explain limits` lists every limit.";

    COUNT_LIMIT = "E0303", "count-limit", Error, "Too many count changes in one frame",
"More than 10000 `counts` props changed in a single frame. Each spawn, enable,
disable or toggle of a copy changes every `counts` prop that counts its prefab,
so many counters times many changes grows fast (a prefab that counts its own
copies and spawns more of itself does it in a few frames). The step stopped
there: the signals still queued were dropped, the counts not yet brought up to
date are brought up to date by the next frame, and an `error` event names the
prefab.

Fix: keep `counts` on a few entities (a HUD or a job card), not on the copies
being counted. `rwgpu explain limits` lists every limit.";

    ENTITY_LIMIT = "E0304", "entity-limit", Error, "Too many entities in the world",
"A `spawn` would make the world hold more than 1000000 entities. Entities are
never removed (a disabled one keeps its id), so a spawn that runs every frame,
or a chain where every new copy spawns more (`from_prefab` on `spawned`), grows
the world until memory runs out. The step stopped at that spawn: nothing was
placed and the signals still queued were dropped. Every later frame that tries
to spawn stops there too.

Fix: run `rwgpu run --frames N` and read `entities` to see how fast the world
grows; give the spawn a `cost` or a `when` that ends it, or `disable` copies and
`enable` them again instead of spawning new ones. `rwgpu explain limits` lists
every limit.";

    TAP_LIMIT = "E0305", "tap-limit", Error, "Too many taps in one frame",
"More than 256 taps arrived for a single frame (from a playtest's `[[input]]`
tables, or from a player). The first 256 were applied in order; the rest were
not looked at, and an `error` event says how many. Each tap is resolved
against the drawn frame, so their number decides how long a frame takes.

Fix: spread the taps over several frames (`frame = N`).
`rwgpu explain limits` lists every limit.";

    OFFSCREEN = "W0301", "offscreen", Warning, "Entity is drawn entirely off-screen",
"The entity is enabled and visible but nothing of it lands inside the frame.

Fix: move the camera (`[camera] center`), move the entity, or make the display
larger in rwgpu.toml.";

    OCCLUDED = "W0302", "occluded", Warning, "Sprite is mostly hidden",
"Less than half of a sprite's pixels are visible in the final frame because
other things are drawn over it: usually blocks in front of it, sometimes other
sprites on the same cell (later entities in the file are drawn on top).

Fix: move the sprite, lower what covers it, or, if it is intentional (a
character behind a wall, a deliberately layered sprite), add
`allow = [\"W0302\"]` to that entity.";

    CLIPPED = "W0303", "clipped", Warning, "Entity is partly off-screen",
"Part of the entity is outside the frame.

Fix: adjust the camera or display size if the whole entity should be visible.";

    HIDDEN_TRIGGER = "W0304", "hidden-trigger", Warning, "Trigger tile is less than 75% visible behind things in front of it",
"Less than three quarters of a trigger's floor tile (a pressure plate, a goal) is visible, usually because
a tall block in front of it (larger x + y) covers it. Entities standing on the trigger's own cell do not
count. Players cannot see where to step.

Fix: move the block in front of it (in front = larger x + y), lower it, or move the trigger.";

    LABEL_CUT = "W0305", "label-cut", Warning, "Label text is cut off or covered",
"Part of a label's text is outside the frame, or other things are drawn over some
of its letters, so players may misread it (a score of 10 that shows as 1).
Screen labels (`screen_pos`) are drawn above the world; only later screen
entities can cover them.

Fix: move the label (`screen_pos`, or `offset`), make the text shorter, or use a
smaller `scale`. The frame report (`rwgpu shot --report`) shows its rectangle.";

    // ---- x04xx: playtests -----------------------------------------------------
    EXPECT_FAILED = "E0401", "expect-failed", Error, "Playtest expectation failed",
"An `[[expect]]` in a playtest did not hold at its frame. The message shows
expected and actual values; the notes list the entity's most recent events.

Fix: inspect what happened with
`rwgpu inspect --input <test> --frame <N> --events --entity <name>`.";

    IMAGE_MISMATCH = "E0402", "image-mismatch", Error, "Screenshot differs from reference image",
"The rendered frame differs from the saved reference by more pixels than
`max_diff_pixels` allows. A diff image is written next to the output (changed
pixels in red).

Fix: if the change is intended, update references with
`rwgpu test --update-refs`; otherwise find what moved with `rwgpu shot --report`.";

    REFERENCE_MISSING = "E0403", "reference-missing", Error, "Reference image not found",
"A playtest `[[shot]]` names a reference image that does not exist yet.

Fix: create it from the current output with `rwgpu test --update-refs`, then
look at the image to confirm it is right before committing it.";

    NO_TESTS = "W0401", "no-tests", Warning, "No playtests found",
"`rwgpu test` found no `.toml` files in `tests/`.

Fix: add a playtest; see the playtest format in `rwgpu docs`.";

    // ---- x05xx: the players ------------------------------------------------
    NO_GPU_ADAPTER = "E0501", "no-gpu-adapter", Error, "No graphics adapter",
"The player found no graphics adapter that can draw (and, with a window, show
the picture in it). It needs Vulkan (Linux, Windows), Metal (macOS) or
Direct3D 12 (Windows). The message names the backends it tried.

Fix: install or update the graphics driver. On Linux without a graphics card,
Mesa's software Vulkan driver works: `sudo apt-get install mesa-vulkan-drivers
libvulkan1`. `WGPU_BACKEND=vulkan` (or `metal`, `dx12`) picks a backend.
Everything else rwgpu does needs no GPU: `rwgpu run`, `shot` and `test` draw
with the CPU. In a browser the web player shows the game with the CPU
reference renderer instead, and its status line names this code.";

    NO_WINDOW_SYSTEM = "E0502", "no-window-system", Error, "No window system",
"The player could not open a window: there is no desktop session it can reach.
On Linux that means neither `WAYLAND_DISPLAY` nor `DISPLAY` leads to a running
display server, as in an SSH session or a container.

Fix: run it on a desktop, or with `--headless`, which plays the same steps and
draws the same frames with no window. On a Linux server, a virtual X display
works: `xvfb-run -a rwgpu play ...`.";

    GPU_FAILED = "E0503", "gpu-failed", Error, "The graphics device failed",
"The graphics adapter was found but refused a request: it would not give a
device, could not draw into the window, reported an error while drawing, or
could not hand a picture back for `--screenshot`. The message has the driver's
own words.

Fix: update the graphics driver, or try another backend with `WGPU_BACKEND`.
If it happens with one project only, report it with that project attached. In
a browser (a lost device, too) the web player goes on with the CPU reference
renderer, and its status line names this code.";
}

/// Look up a code such as `E0103` (case-insensitive) or a slug such as
/// `unknown-key`.
pub fn lookup(code_or_slug: &str) -> Option<&'static CodeInfo> {
    ALL.iter().find(|c| c.code.eq_ignore_ascii_case(code_or_slug) || c.slug.eq_ignore_ascii_case(code_or_slug))
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
