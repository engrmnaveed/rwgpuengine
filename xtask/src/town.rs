//! The generated town both renderers are timed on. `cargo xtask budgets`
//! draws it with the CPU reference renderer; the GPU check
//! (`players/gpu_check`) includes this file and draws the same town with both.

/// A project with one scene: `n` boxes, one per cell of a square ground, with
/// heights from 0.4 to 3 and six colors (the town of the 3D study's spikes),
/// seen at 1280x720 by a perspective camera far enough out to see all of it.
pub fn town(n: usize) -> rwgpuengine_data::Vfs {
    let side = (1..).find(|g| g * g >= n).unwrap_or(1);
    let mut seed: u32 = 0x1234_5678;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed >> 8
    };
    let palette = ["#b5651d", "#8b2f2f", "#3f8a3f", "#c8bea0", "#5a6ea0", "#e6c850"];
    let mut scene = format!("[camera]\nview = \"perspective\"\nzoom = 0.2\n\n[ground]\nsize = [{side}, {side}]\n");
    for i in 0..n {
        // Quarter units, so every height is an exact number in the file.
        let height = 0.5 + (next() % 11) as f32 * 0.25;
        let color = palette[(next() % 6) as usize];
        scene.push_str(&format!(
            "\n[[entity]]\npos = [{}, {}]\nblock = {{ color = \"{color}\", height = {height} }}\n",
            i % side,
            i / side
        ));
    }
    let mut vfs = rwgpuengine_data::Vfs::new();
    vfs.insert(
        "rwgpu.toml",
        "[project]\nname = \"town\"\nmain_scene = \"scenes/main.toml\"\n[display]\nwidth = 1280\nheight = 720\n",
    );
    vfs.insert("scenes/main.toml", scene);
    vfs
}

/// A project with one scene: `n` buildings of shapes on a square ground, one
/// every three cells, each a copy of one of six prefabs (a house, a mill with
/// a round tower, a workshop, a farm with a field in rows, a tree, a market:
/// 2 to 9 shapes, 28 a building on average with their faces counted), each at
/// one of the four turns; seen like [`town`]. `cargo xtask shape-bench` and
/// the GPU check time both renderers on it (ADR 0029).
pub fn shaped_town(n: usize) -> rwgpuengine_data::Vfs {
    const PREFABS: [(&str, &str); 6] = [
        (
            "house",
            "footprint = [2, 2]\nshape = [{ kind = \"box\", at = [0.125, 0.125, 0], size = [1.75, 1.75, 1.125], color = \"#e8d8b0\" }, { kind = \"roof\", at = [0, 0, 1.125], size = [2, 2, 0.875], color = \"#b5523b\" }, { kind = \"box\", at = [1.375, 0.375, 1.125], size = [0.25, 0.25, 1], color = \"#7a6a5a\" }]\n",
        ),
        (
            "mill",
            "footprint = [2, 2]\nshape = [{ kind = \"box\", at = [0.125, 0.125, 0], size = [1, 1.75, 0.875], color = \"#d9c9a0\" }, { kind = \"roof\", at = [0, 0, 0.875], size = [1.25, 2, 0.625], color = \"#8b5a3c\", turn = 90 }, { kind = \"cylinder\", at = [1, 0.5, 0], size = [0.875, 0.875, 1.875], color = \"#c8c2b4\" }, { kind = \"cone\", at = [0.9375, 0.4375, 1.875], size = [1, 1, 0.75], color = \"#5a6ea0\" }]\n",
        ),
        (
            "workshop",
            "footprint = [2, 3]\nshape = [{ kind = \"box\", at = [0.125, 0.125, 0], size = [1.75, 2.75, 1], color = \"#b07a4a\" }, { kind = \"wedge\", at = [0, 0, 1], size = [2, 3, 0.625], color = \"#70564a\" }, { kind = \"cylinder\", at = [1.25, 2.125, 0.625], size = [0.375, 0.375, 1.5], color = \"#5a5a5a\" }]\n",
        ),
        (
            "farm",
            "footprint = [3, 3]\nshape = [{ kind = \"box\", at = [0.25, 0.25, 0], size = [1.75, 1.25, 1], color = \"#a8443a\" }, { kind = \"roof\", at = [0.125, 0.125, 1], size = [2, 1.5, 0.75], color = \"#5c4a44\" }, { kind = \"box\", at = [0.125, 1.625, 0], size = [2.75, 1.25, 0.0625], color = \"#6e5234\" }, { kind = \"box\", at = [0.25, 1.75, 0.0625], size = [2.5, 0.1875, 0.125], color = \"#7a9a3a\" }, { kind = \"box\", at = [0.25, 2.0625, 0.0625], size = [2.5, 0.1875, 0.125], color = \"#c8b040\" }, { kind = \"box\", at = [0.25, 2.375, 0.0625], size = [2.5, 0.1875, 0.125], color = \"#7a9a3a\" }, { kind = \"box\", at = [0.25, 2.625, 0.0625], size = [2.5, 0.1875, 0.125], color = \"#c8b040\" }]\n",
        ),
        (
            "tree",
            "shape = [{ kind = \"cylinder\", at = [0.375, 0.375, 0], size = [0.25, 0.25, 0.5], color = \"#7a5a3a\" }, { kind = \"cone\", at = [0.125, 0.125, 0.375], size = [0.75, 0.75, 1.25], color = \"#3f8a4a\" }]\n",
        ),
        (
            "market",
            "footprint = [3, 3]\nshape = [{ kind = \"box\", at = [0.125, 0.125, 0], size = [2.75, 2.75, 0.0625], color = \"#c9b38a\" }, { kind = \"box\", at = [0.25, 0.25, 0.0625], size = [0.125, 0.125, 1], color = \"#6a4a2a\" }, { kind = \"box\", at = [2.625, 0.25, 0.0625], size = [0.125, 0.125, 1], color = \"#6a4a2a\" }, { kind = \"box\", at = [0.25, 2.625, 0.0625], size = [0.125, 0.125, 1], color = \"#6a4a2a\" }, { kind = \"box\", at = [2.625, 2.625, 0.0625], size = [0.125, 0.125, 1], color = \"#6a4a2a\" }, { kind = \"wedge\", at = [0.1875, 0.1875, 1.0625], size = [1.3125, 1.3125, 0.5], color = \"#c0392b\" }, { kind = \"wedge\", at = [0.1875, 1.5, 1.0625], size = [1.3125, 1.3125, 0.5], color = \"#ece4d2\" }, { kind = \"wedge\", at = [1.5, 0.1875, 1.0625], size = [1.3125, 1.3125, 0.5], color = \"#ece4d2\", turn = 180 }, { kind = \"wedge\", at = [1.5, 1.5, 1.0625], size = [1.3125, 1.3125, 0.5], color = \"#c0392b\", turn = 180 }]\n",
        ),
    ];
    let side = (1..).find(|g| g * g >= n).unwrap_or(1);
    let mut seed: u32 = 0x1234_5678;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed >> 8
    };
    let cells = side * 3;
    let mut scene = format!("[camera]\nview = \"perspective\"\nzoom = 0.2\n\n[ground]\nsize = [{cells}, {cells}]\n");
    for i in 0..n {
        let (name, _) = PREFABS[(next() % 6) as usize];
        scene.push_str(&format!(
            "\n[[entity]]\nprefab = \"{name}\"\npos = [{}, {}]\nturn = {}\n",
            (i % side) * 3,
            (i / side) * 3,
            (next() % 4) * 90
        ));
    }
    let mut vfs = rwgpuengine_data::Vfs::new();
    vfs.insert(
        "rwgpu.toml",
        "[project]\nname = \"shaped-town\"\nmain_scene = \"scenes/main.toml\"\n[display]\nwidth = 1280\nheight = 720\n",
    );
    for (name, text) in PREFABS {
        vfs.insert(format!("prefabs/{name}.toml"), text);
    }
    vfs.insert("scenes/main.toml", scene);
    vfs
}
