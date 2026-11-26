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
