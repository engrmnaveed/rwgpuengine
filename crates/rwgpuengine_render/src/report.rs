//! The frame report: a text description of a rendered frame.
//!
//! For each entity: what was drawn, where (screen rectangle), how many
//! pixels and how many are still visible, and what covers it. Issues are
//! diagnostics with stable codes: off-screen (W0301), mostly hidden sprites
//! (W0302), partly off-screen entities (W0303), hidden trigger tiles (W0304)
//! and cut-off or covered text (W0305). An agent can check "is the player
//! visible?" or "does the score read 3?" without looking at the PNG.

use std::collections::BTreeMap;

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_core::hash;
use rwgpuengine_core::json::Json;
use rwgpuengine_sim::{EntityId, World};

use crate::draw::{Part, Rect};
use crate::reference::{Frame, Projected, coverage};

/// Sprites with less than this share of pixels visible are reported (W0302).
pub const OCCLUDED_BELOW: f64 = 0.5;
/// Trigger tiles (plates, goals) with less than this share visible are
/// reported (W0304). Stricter than sprites: players must see where to step.
/// Entities standing on the trigger's own cell do not count as covering it.
pub const TRIGGER_HIDDEN_BELOW: f64 = 0.75;

#[derive(Clone, Debug)]
pub struct EntityReport {
    pub id: EntityId,
    pub label: String,
    pub parts: Vec<&'static str>,
    pub cell: [i32; 2],
    /// Drawn on the screen (`screen_pos`), not in the world: `cell` is meaningless.
    pub on_screen: bool,
    /// Union of the entity's shapes, in screen pixels (may extend off-screen).
    pub bounds: Rect,
    /// Pixels inside the frame covered by any of the entity's parts, seen or not.
    pub drawn_px: u32,
    /// Of those, pixels that show the entity.
    pub visible_px: u32,
    /// Pixels area outside the frame, from the bounding rectangle.
    pub offscreen: bool,
    pub clipped: bool,
    /// Entities that show where this one is covered, with pixel counts, largest first.
    pub covered_by: Vec<(EntityId, u32)>,
    /// Label text as shown, if the entity has a label.
    pub text: Option<String>,
    /// Its shapes, in the order of its `[[shape]]` list: (kind, pixels the
    /// shape covers inside the frame, pixels that show it).
    pub shapes: Vec<(&'static str, u32, u32)>,
}

/// Pixel accounting for one entity's label.
#[derive(Default)]
struct LabelPx {
    bounds: Rect,
    drawn: u32,
    /// Lit pixels later covered by other entities.
    covered: u32,
}

impl EntityReport {
    pub fn visible_ratio(&self) -> f64 {
        if self.drawn_px == 0 { 0.0 } else { self.visible_px as f64 / self.drawn_px as f64 }
    }
}

#[derive(Clone, Debug)]
pub struct FrameReport {
    pub frame: u64,
    pub width: u32,
    pub height: u32,
    pub image_hash: u64,
    pub ground_cells: usize,
    /// World entities in file order, then screen entities in file order.
    pub entities: Vec<EntityReport>,
    pub issues: Vec<Diagnostic>,
}

/// Describe a frame. `frame` must be the rendering of `list`.
pub fn build_report(world: &World, list: &Projected, frame: &Frame) -> FrameReport {
    let cov = coverage(list, frame);
    let stats = &cov.items;
    let screen = Rect { x0: 0, y0: 0, x1: frame.width as i32, y1: frame.height as i32 };
    let mut order: Vec<EntityId> = Vec::new();
    let mut by_entity: BTreeMap<EntityId, EntityReport> = BTreeMap::new();
    let mut covered: BTreeMap<EntityId, BTreeMap<EntityId, u32>> = BTreeMap::new();
    let mut ground_cells = 0;
    let mut sprite_px: BTreeMap<EntityId, (u32, u32)> = BTreeMap::new();
    let mut trigger_tile_px: BTreeMap<EntityId, (u32, u32)> = BTreeMap::new();
    let mut label_px: BTreeMap<EntityId, LabelPx> = BTreeMap::new();
    let mut body: BTreeMap<EntityId, Rect> = BTreeMap::new();

    for (idx, item) in list.items.iter().enumerate() {
        if !item.enabled {
            continue;
        }
        let Some(id) = item.entity else {
            ground_cells += 1;
            continue;
        };
        let entry = by_entity.entry(id).or_insert_with(|| {
            order.push(id);
            EntityReport {
                id,
                label: world.get(id).map_or_else(|| format!("#{id}"), |e| e.label()),
                parts: Vec::new(),
                cell: item.cell,
                on_screen: world.get(id).is_some_and(|e| e.screen_pos.is_some()),
                bounds: Rect::EMPTY,
                drawn_px: cov.entity_px.get(&id).copied().unwrap_or(0),
                visible_px: 0,
                offscreen: false,
                clipped: false,
                covered_by: Vec::new(),
                text: world.get(id).and_then(|e| e.label_text()),
                shapes: Vec::new(),
            }
        });
        if !entry.parts.contains(&item.part.name()) {
            entry.parts.push(item.part.name());
        }
        entry.bounds = entry.bounds.union(&item.bounds);
        entry.offscreen |= item.out_of_range;
        let s = &stats[idx];
        entry.visible_px += s.visible;
        if let Part::Shape(kind) = item.part {
            entry.shapes.push((kind.name(), s.drawn, s.visible));
        }
        if item.part == Part::Tile && world.get(id).is_some_and(|e| e.trigger.is_some()) {
            // What stands on any cell of the trigger does not hide it.
            let on_it = |o: usize| o < list.world_len() && world.get(id).is_some_and(|t| t.covers(list.items[o].cell));
            let hidden: u32 = s.covered_by.iter().filter(|(o, _)| !on_it(**o)).map(|(_, n)| *n).sum();
            let p = trigger_tile_px.entry(id).or_insert((0, 0));
            p.0 += s.drawn;
            p.1 += s.drawn.saturating_sub(hidden);
        }
        if item.part == Part::Sprite {
            let p = sprite_px.entry(id).or_insert((0, 0));
            p.0 += s.drawn;
            p.1 += s.visible;
        }
        if item.part == Part::Label {
            let l = label_px.entry(id).or_default();
            l.bounds = l.bounds.union(&item.bounds);
            l.drawn += s.drawn;
            l.covered +=
                s.covered_by.iter().filter(|(o, _)| list.items[**o].entity != Some(id)).map(|(_, n)| *n).sum::<u32>();
        } else {
            let b = body.entry(id).or_insert(Rect::EMPTY);
            *b = b.union(&item.bounds);
        }
        for (&other_idx, &n) in &s.covered_by {
            if let Some(other) = list.items[other_idx].entity
                && other != id
            {
                *covered.entry(id).or_default().entry(other).or_insert(0) += n;
            }
        }
    }

    let mut issues = Vec::new();
    let mut entities = Vec::new();
    for id in order {
        let mut r = by_entity.remove(&id).expect("entity report");
        let inside = r.bounds.intersect(&screen).area();
        r.offscreen = (r.offscreen && r.bounds.is_empty()) || (!r.bounds.is_empty() && inside == 0);
        r.clipped = !r.offscreen && inside < r.bounds.area();
        let mut cov: Vec<(EntityId, u32)> = covered.remove(&id).unwrap_or_default().into_iter().collect();
        cov.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        r.covered_by = cov;
        let mut own: Vec<Diagnostic> = Vec::new();
        let [x, y, w, h] = r.bounds.xywh();
        if r.offscreen {
            let message = if r.bounds.is_empty() {
                format!("{} is out of the camera's sight (behind it, or too far away)", r.label)
            } else {
                format!("{} is drawn entirely off-screen at [{x}, {y}, {w}, {h}]", r.label)
            };
            own.push(
                Diagnostic::new(&codes::OFFSCREEN, message)
                    .hint("move it or the camera ([camera] center in the scene)"),
            );
        } else if body.get(&id).is_some_and(|b| !b.is_empty() && b.intersect(&screen).area() < b.area()) {
            own.push(Diagnostic::new(
                &codes::CLIPPED,
                format!(
                    "{} is partly off-screen at [{x}, {y}, {w}, {h}] (frame {}x{})",
                    r.label, frame.width, frame.height
                ),
            ));
        }
        if let Some(&(drawn, visible)) = trigger_tile_px.get(&id)
            && drawn > 0
            && (visible as f64) < TRIGGER_HIDDEN_BELOW * drawn as f64
        {
            let by: Vec<String> = r
                .covered_by
                .iter()
                .filter(|(o, _)| {
                    world
                        .get(*o)
                        .is_some_and(|e| e.screen_pos.is_some() || world.get(id).is_none_or(|t| !t.covers(e.cell())))
                })
                .take(3)
                .map(|(o, n)| format!("{} ({n} px)", world.get(*o).map_or_else(|| format!("#{o}"), |e| e.label())))
                .collect();
            own.push(
                Diagnostic::new(
                    &codes::HIDDEN_TRIGGER,
                    format!(
                        "trigger tile of {} at cell ({}, {}) is {}% visible (not counting what stands on it); covered by {}",
                        r.label,
                        r.cell[0],
                        r.cell[1],
                        visible * 100 / drawn,
                        if by.is_empty() { "nothing on screen".to_string() } else { by.join(", ") }
                    ),
                )
                .hint("players cannot see where to step: move or lower what covers it, or move the trigger"),
            );
        }
        if let Some(&(drawn, visible)) = sprite_px.get(&id)
            && drawn > 0
            && (visible as f64) < OCCLUDED_BELOW * drawn as f64
        {
            let by: Vec<String> = r
                .covered_by
                .iter()
                .take(3)
                .map(|(o, n)| format!("{} ({n} px)", world.get(*o).map_or_else(|| format!("#{o}"), |e| e.label())))
                .collect();
            own.push(Diagnostic::new(
                &codes::OCCLUDED,
                format!(
                    "sprite of {} is {}% visible; covered by {}",
                    r.label,
                    visible * 100 / drawn,
                    if by.is_empty() { "nothing on screen".to_string() } else { by.join(", ") }
                ),
            ));
        }
        if let Some(l) = label_px.get(&id)
            && !r.offscreen
        {
            let text = r.text.clone().unwrap_or_default();
            let [lx, ly, lw, lh] = l.bounds.xywh();
            let mut why = Vec::new();
            if l.bounds.intersect(&screen).area() < l.bounds.area() {
                why.push(format!(
                    "its text box [{lx}, {ly}, {lw}, {lh}] runs past the edge of the {}x{} frame",
                    frame.width, frame.height
                ));
            }
            if l.covered > 0 {
                let by: Vec<String> = r
                    .covered_by
                    .iter()
                    .take(3)
                    .map(|(o, _)| world.get(*o).map_or_else(|| format!("#{o}"), |e| e.label()))
                    .collect();
                why.push(format!("{} of its {} text pixels are covered by {}", l.covered, l.drawn, by.join(", ")));
            }
            if !why.is_empty() {
                own.push(
                    Diagnostic::new(&codes::LABEL_CUT, format!("label of {} ({text:?}): {}", r.label, why.join("; ")))
                        .hint("move it (`screen_pos` or label `offset`), shorten the text, or lower `scale`"),
                );
            }
        }
        // Warnings the entity declares as intended (`allow = [...]`) are not reported.
        let entity = world.get(id);
        issues.extend(own.into_iter().filter(|d| !entity.is_some_and(|e| e.allows(d.code))));
        entities.push(r);
    }

    FrameReport {
        frame: world.frame,
        width: frame.width,
        height: frame.height,
        image_hash: frame.image_hash(),
        ground_cells,
        entities,
        issues,
    }
}

impl FrameReport {
    pub fn to_json(&self, world: &World) -> Json {
        let ents: Vec<Json> = self
            .entities
            .iter()
            .map(|r| {
                let cov: Vec<Json> = r
                    .covered_by
                    .iter()
                    .map(|(o, n)| {
                        Json::object()
                            .with("entity", world.get(*o).map_or_else(|| format!("#{o}"), |e| e.label()))
                            .with("pixels", *n)
                    })
                    .collect();
                let mut j = Json::object()
                    .with("entity", r.label.as_str())
                    .with("id", r.id)
                    .with("parts", r.parts.iter().map(|p| Json::from(*p)).collect::<Vec<_>>())
                    .with("cell", if r.on_screen { Json::Null } else { Json::from(r.cell) })
                    .with("rect", r.bounds.xywh())
                    .with("drawn_px", r.drawn_px)
                    .with("visible_px", r.visible_px)
                    .with("visible", (r.visible_ratio() * 1000.0).round() / 1000.0)
                    .with("offscreen", r.offscreen)
                    .with("clipped", r.clipped)
                    .with("covered_by", cov)
                    .with("text", r.text.clone());
                // What it is built of, shape by shape: a roof that shows no pixel is hidden or faces away.
                if !r.shapes.is_empty() {
                    let shape = |(kind, drawn, visible): &(&str, u32, u32)| {
                        Json::object().with("kind", *kind).with("drawn_px", *drawn).with("visible_px", *visible)
                    };
                    j.set("shapes", r.shapes.iter().map(shape).collect::<Vec<_>>());
                }
                j
            })
            .collect();
        Json::object()
            .with("frame", self.frame)
            .with("width", self.width)
            .with("height", self.height)
            .with("image_hash", hash::hex(self.image_hash))
            .with("ground_cells", self.ground_cells)
            .with("entities", ents)
            .with("issues", self.issues.iter().map(Diagnostic::to_json).collect::<Vec<_>>())
    }

    /// Plain-text table, one line per entity, back to front.
    pub fn to_text(&self) -> String {
        let mut out = format!(
            "frame {} — {}x{}, {} ground cells, {} entities drawn, image {}\n",
            self.frame,
            self.width,
            self.height,
            self.ground_cells,
            self.entities.len(),
            hash::hex(self.image_hash)
        );
        for r in &self.entities {
            let [x, y, w, h] = r.bounds.xywh();
            let at = if r.on_screen { "screen".to_string() } else { format!("cell ({}, {})", r.cell[0], r.cell[1]) };
            let mut line = format!(
                "  {:<14} {:<13} {at:<12} rect [{x}, {y}, {w}, {h}]  visible {:>3}%",
                r.label,
                r.parts.join("+"),
                (r.visible_ratio() * 100.0).round() as i64
            );
            if let Some(t) = &r.text {
                line.push_str(&format!("  text {t:?}"));
            }
            if let Some((o, n)) = r.covered_by.first() {
                let same_cell =
                    !r.on_screen && self.entities.iter().any(|x| x.id == *o && !x.on_screen && x.cell == r.cell);
                let note = if same_cell { " standing on it" } else { "" };
                let who =
                    self.entities.iter().find(|x| x.id == *o).map_or_else(|| format!("#{o}"), |x| x.label.clone());
                line.push_str(&format!("  (covered most by {who}{note}: {n} px)"));
            }
            if r.offscreen {
                line.push_str("  OFFSCREEN");
            } else if r.clipped {
                line.push_str("  CLIPPED");
            }
            out.push_str(&line);
            out.push('\n');
        }
        out
    }
}
