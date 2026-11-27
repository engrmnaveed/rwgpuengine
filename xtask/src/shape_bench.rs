//! `cargo xtask shape-bench`: what a town of shapes costs the agent's half
//! as it grows (ADR 0029). [`town::shaped_town`](crate::town::shaped_town)
//! is `n` buildings of shapes, each a copy of one of six prefabs at one of
//! the four turns, seen in perspective at 1280 x 720.
//!
//! For each size it prints: the instances and the models in the render list
//! and the list's bytes; then the median time to build the list, to bake its
//! models into faces, to draw a frame with the reference renderer (the list,
//! its projection and the pixels with their owners, as `rwgpu shot` does
//! without the report), and to paint it as a player does. The GPU's frame of
//! the same town is printed by `cargo xtask gpu`.
//!
//! Run it as a release build: `cargo run --release -p xtask -- shape-bench`.

use std::time::Instant;

use rwgpuengine_render::{Instance, Model, Painter, RenderList, bake, project, render};
use rwgpuengine_runtime::{DataOnly, Session, SessionConfig};

use crate::{Result, town};

const SIZES: [usize; 3] = [1_000, 5_000, 50_000];

/// Median microseconds of `f`, over `runs` runs after one to warm up.
fn median_us(runs: usize, mut f: impl FnMut()) -> f64 {
    f();
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1e6
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

/// The bytes a render list holds: its instances, and the shapes of its models.
pub fn list_bytes(list: &RenderList) -> usize {
    let shapes: usize = list.models.iter().map(|m| std::mem::size_of_val(&*m.shapes)).sum();
    std::mem::size_of_val(&*list.instances) + std::mem::size_of_val(&*list.models) + shapes
}

pub fn run(args: &[String]) -> Result {
    let sizes: Vec<usize> = args.iter().filter_map(|a| a.parse().ok()).collect();
    let sizes = if sizes.is_empty() { SIZES.to_vec() } else { sizes };
    println!(
        "{:>9} {:>9} {:>9} {:>7} {:>11} {:>9} {:>9} {:>10} {:>10}",
        "buildings", "shapes", "instances", "models", "list bytes", "list us", "bake us", "frame us", "paint us"
    );
    for n in sizes {
        let session = Session::load(&town::shaped_town(n), &SessionConfig::default(), Box::new(DataOnly))
            .map_err(|d| format!("{d:?}"))?;
        let list = session.render_list(false);
        let shapes: usize = session
            .world
            .entities
            .iter()
            .filter_map(|e| e.shapes)
            .map(|s| session.world.shapes[s as usize].len())
            .sum();
        let runs = if n > 10_000 { 9 } else { 31 };
        let build = median_us(runs, || {
            std::hint::black_box(session.render_list(false));
        });
        let baking = median_us(101, || {
            for m in &list.models {
                std::hint::black_box(bake(m, &list.light));
            }
        });
        let frame = median_us(runs, || {
            std::hint::black_box(render(&project(&session.render_list(false))));
        });
        let mut painter = Painter::new();
        let mut pixels = Vec::new();
        let paint = median_us(runs, || {
            painter.paint(&session.render_list(false), &mut pixels);
            std::hint::black_box(&pixels);
        });
        println!(
            "{n:>9} {shapes:>9} {:>9} {:>7} {:>11} {build:>9.0} {baking:>9.1} {frame:>10.0} {paint:>10.0}",
            list.instances.len(),
            list.models.len(),
            list_bytes(&list),
        );
    }
    println!(
        "\nan instance is {} bytes, a model {} and a shape {}; the ground is an instance a cell",
        std::mem::size_of::<Instance>(),
        std::mem::size_of::<Model>(),
        std::mem::size_of::<rwgpuengine_data::ShapeDef>()
    );
    Ok(())
}
