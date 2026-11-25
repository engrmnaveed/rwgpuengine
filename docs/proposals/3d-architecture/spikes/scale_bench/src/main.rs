//! Study spike (not engine code): what moving N actors on a grid costs with
//! the storage the engine has today and with two alternatives. Each actor
//! tries one random step per tick and is refused if the cell is taken or
//! outside. No events are logged, so this is the movement cost alone.
use std::time::Instant;

/// As large as the engine's entity, laid out the way it is: everything in one struct.
#[derive(Clone)]
struct Fat {
    cell: [i32; 2],
    enabled: bool,
    _rest: [u8; FAT - 12],
}
const FAT: usize = std::mem::size_of::<rwgpuengine_sim::Entity>();

fn rng(state: &mut u64) -> u32 {
    *state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    (*state >> 33) as u32
}

fn start(n: usize) -> (i32, Vec<[i32; 2]>) {
    let mut g = 2;
    while (g / 2) * (g / 2) < n {
        g += 2;
    }
    let cells = (0..g).step_by(2).flat_map(|y| (0..g).step_by(2).map(move |x| [x as i32, y as i32])).take(n).collect();
    (g as i32, cells)
}

const STEPS: [[i32; 2]; 4] = [[0, -1], [0, 1], [-1, 0], [1, 0]];

/// Today: one array of fat entities; "is the cell free?" scans all of them.
fn scan(n: usize, ticks: u32) -> (f64, u64) {
    let (g, cells) = start(n);
    let mut e: Vec<Fat> = cells.iter().map(|c| Fat { cell: *c, enabled: true, _rest: [0; FAT - 12] }).collect();
    let mut seed = 7;
    let t = Instant::now();
    for _ in 0..ticks {
        for i in 0..n {
            let d = STEPS[(rng(&mut seed) % 4) as usize];
            let to = [e[i].cell[0] + d[0], e[i].cell[1] + d[1]];
            if to[0] < 0 || to[1] < 0 || to[0] >= g || to[1] >= g {
                continue;
            }
            if !e.iter().any(|o| o.enabled && o.cell == to) {
                e[i].cell = to;
            }
        }
    }
    (t.elapsed().as_secs_f64() * 1e3 / ticks as f64, e.iter().fold(0u64, |h, x| h.wrapping_mul(31).wrapping_add((x.cell[0] * 4099 + x.cell[1]) as u64)))
}

/// Fat entities, plus a grid that says who is on each cell.
fn indexed_fat(n: usize, ticks: u32) -> (f64, u64) {
    let (g, cells) = start(n);
    let mut e: Vec<Fat> = cells.iter().map(|c| Fat { cell: *c, enabled: true, _rest: [0; FAT - 12] }).collect();
    let mut grid = vec![0u32; (g * g) as usize];
    for (i, c) in cells.iter().enumerate() {
        grid[(c[1] * g + c[0]) as usize] = i as u32 + 1;
    }
    let mut seed = 7;
    let t = Instant::now();
    for _ in 0..ticks {
        for i in 0..n {
            let d = STEPS[(rng(&mut seed) % 4) as usize];
            let from = e[i].cell;
            let to = [from[0] + d[0], from[1] + d[1]];
            if to[0] < 0 || to[1] < 0 || to[0] >= g || to[1] >= g || grid[(to[1] * g + to[0]) as usize] != 0 {
                continue;
            }
            grid[(from[1] * g + from[0]) as usize] = 0;
            grid[(to[1] * g + to[0]) as usize] = i as u32 + 1;
            e[i].cell = to;
        }
    }
    (t.elapsed().as_secs_f64() * 1e3 / ticks as f64, e.iter().fold(0u64, |h, x| h.wrapping_mul(31).wrapping_add((x.cell[0] * 4099 + x.cell[1]) as u64)))
}

/// Positions in their own array (a column), plus the same grid.
fn indexed_columns(n: usize, ticks: u32) -> (f64, u64) {
    let (g, mut cell) = start(n);
    let mut grid = vec![0u32; (g * g) as usize];
    for (i, c) in cell.iter().enumerate() {
        grid[(c[1] * g + c[0]) as usize] = i as u32 + 1;
    }
    let mut seed = 7;
    let t = Instant::now();
    for _ in 0..ticks {
        for i in 0..n {
            let d = STEPS[(rng(&mut seed) % 4) as usize];
            let from = cell[i];
            let to = [from[0] + d[0], from[1] + d[1]];
            if to[0] < 0 || to[1] < 0 || to[0] >= g || to[1] >= g || grid[(to[1] * g + to[0]) as usize] != 0 {
                continue;
            }
            grid[(from[1] * g + from[0]) as usize] = 0;
            grid[(to[1] * g + to[0]) as usize] = i as u32 + 1;
            cell[i] = to;
        }
    }
    (t.elapsed().as_secs_f64() * 1e3 / ticks as f64, cell.iter().fold(0u64, |h, x| h.wrapping_mul(31).wrapping_add((x[0] * 4099 + x[1]) as u64)))
}

fn main() {
    println!("the engine's entity is {FAT} bytes");
    for n in [5_000usize, 50_000, 500_000] {
        // The scan is quadratic, so it gets fewer ticks; results are compared at equal ticks.
        let scan_ticks = if n <= 5_000 { 20 } else { 2 };
        let a = if n <= 50_000 { Some(scan(n, scan_ticks)) } else { None };
        let b = indexed_fat(n, 50);
        let c = indexed_columns(n, 50);
        let same = a.is_none_or(|a| a.1 == indexed_fat(n, scan_ticks).1) && b.1 == c.1;
        println!(
            "n={n:>7}: scan all entities {} ms per tick; fat entities + cell index {:.3} ms; columns + cell index {:.3} ms; same result: {same}",
            a.map_or("(not run: quadratic)".to_string(), |a| format!("{:.2}", a.0)),
            b.0,
            c.0
        );
    }
}
