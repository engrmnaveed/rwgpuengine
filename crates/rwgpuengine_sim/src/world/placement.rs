//! Placement: whether a prefab fits on the ground, and `spawn`.

use rwgpuengine_core::json::Json;
use rwgpuengine_core::limits::ENTITIES;
use rwgpuengine_data::{EntityDef, footprint_cells};

use super::counting::Stale;
use super::entity::make_entity;
use super::signals::Pending;
use super::wiring::Cost;
use super::{Entity, EntityId, World, fmt_cell, scan};
use crate::dump::prop_json;
use crate::events::EventKind;

/// Why a spawn could not be placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    /// `edge`, `terrain` or `occupied`.
    pub reason: &'static str,
    /// The first cell of the footprint that failed.
    pub cell: [i32; 2],
    /// The entity covering that cell (for `occupied`).
    pub by: Option<EntityId>,
    /// The terrain there (for `terrain`).
    pub terrain: Option<String>,
    /// "cell (4, 4) is water (build = false)".
    pub why: String,
}

impl World {
    /// The spawn template for `prefab`, if a `spawn` action uses it.
    pub fn template(&self, prefab: &str) -> Option<&EntityDef> {
        self.templates.iter().find(|t| t.prefab.as_deref() == Some(prefab))
    }

    pub(super) fn terrain_name(&self, cell: [i32; 2]) -> Option<String> {
        let g = self.ground.as_ref().filter(|g| g.has_map)?;
        g.terrain_at(cell).map(|t| t.name.clone())
    }

    /// The first entity, in id order, that takes up `cell` for placement.
    fn taking(&self, cell: [i32; 2]) -> Option<EntityId> {
        let found = self.index.taking(cell);
        if self.checking() {
            let scanned = scan::taking(&self.entities, cell);
            assert_eq!(found, scanned, "the index and the scan disagree on what takes up {cell:?}");
        }
        found
    }

    /// Can `prefab` be placed with its back corner on `at` right now? Returns
    /// the cells it would cover, or why not. Every covered cell must be inside
    /// the ground, on buildable terrain, and not covered by an enabled entity.
    pub fn can_place(&self, prefab: &str, at: [i32; 2]) -> Result<Vec<[i32; 2]>, Refusal> {
        let footprint = self.template(prefab).map_or([1, 1], |t| t.footprint);
        let cells = footprint_cells(at, footprint);
        for &c in &cells {
            match &self.ground {
                Some(g) if !g.contains(c) => {
                    let why = format!("cell {} is outside the {}x{} ground", fmt_cell(c), g.size[0], g.size[1]);
                    return Err(Refusal { reason: "edge", cell: c, by: None, terrain: None, why });
                }
                Some(g) => {
                    if let Some(t) = g.terrain_at(c).filter(|t| !t.build) {
                        let why =
                            format!("cell {} is {}, where nothing can be built (build = false)", fmt_cell(c), t.name);
                        return Err(Refusal {
                            reason: "terrain",
                            cell: c,
                            by: None,
                            terrain: Some(t.name.clone()),
                            why,
                        });
                    }
                }
                None => {}
            }
            if let Some(e) = self.taking(c).map(|by| &self.entities[by as usize]) {
                let why = format!("cell {} is taken by {}", fmt_cell(c), e.describe());
                return Err(Refusal { reason: "occupied", cell: c, by: Some(e.id), terrain: None, why });
            }
        }
        Ok(cells)
    }

    /// Place a new copy of `prefab` at `at` and pay its `cost`, or log why
    /// not (funds are checked before the cell). The new entity emits
    /// `spawned` if a connection listens. Returns whether it was placed.
    pub(super) fn spawn(
        &mut self,
        prefab: &str,
        cost: Option<&Cost>,
        at: [i32; 2],
        emitter: Option<EntityId>,
        via: &str,
        held: &[&str],
    ) -> bool {
        let when = if held.is_empty() { String::new() } else { format!("; when {} held", held.join(", ")) };
        let found = self.templates.iter().position(|t| t.prefab.as_deref() == Some(prefab));
        let Some((template, shapes)) = found.map(|i| (self.templates[i].clone(), self.template_shapes[i])) else {
            let msg = format!("could not spawn '{prefab}' at {} ({via}): no such prefab", fmt_cell(at));
            let data = Json::object().with("code", "E0201").with("prefab", prefab);
            self.write(EventKind::Error, None, emitter, msg, data);
            return false;
        };
        if self.entities.len() >= self.max_entities {
            let msg = format!(
                "'{prefab}' was not spawned at {} ({via}): the world holds {} entities, the most it may; dropping \
                 the rest of this frame's signals. Usually a spawn that runs every frame or a chain of spawns (E0304)",
                fmt_cell(at),
                self.entities.len()
            );
            let data = Json::object().with("code", "E0304").with("prefab", prefab).with("at", at);
            self.cut_step(&ENTITIES, None, emitter, msg, data);
            return false;
        }
        let placed = match self.can_afford(cost, at) {
            Err(r) => Err(r),
            Ok(()) => self.can_place(prefab, at),
        };
        match placed {
            Err(r) => {
                let msg = format!("'{prefab}' could not be placed at {} ({via}): {}{when}", fmt_cell(at), r.why);
                let mut data = Json::object()
                    .with("cause", "spawn")
                    .with("prefab", prefab)
                    .with("at", at)
                    .with("reason", r.reason)
                    .with("cell", r.cell);
                if let Some(t) = &r.terrain {
                    data.set("terrain", t.as_str());
                }
                if let (Some(k), "funds") = (cost, r.reason) {
                    data.set("cost", self.cost_json(k));
                }
                // The source of the signal says so, if anything listens.
                let listened = self.is_listened(emitter, "refused");
                let mut msg = msg;
                if listened {
                    msg.push_str(&format!("; {} emits 'refused'", self.source_name(emitter)));
                    data.set("signal", "refused");
                }
                self.write(EventKind::Blocked, None, r.by, msg, data);
                if listened {
                    self.pending.push_back(Pending {
                        emitter,
                        signal: "refused".into(),
                        cell: at,
                        reason: Some(r.reason),
                    });
                }
                false
            }
            Ok(cells) => {
                let id = self.entities.len() as EntityId;
                let mut def = template;
                def.pos = [at[0] as f32, at[1] as f32, def.pos[2]];
                self.entities.0.push(make_entity(id, &def, shapes));
                self.index.enter(&self.entities[id as usize]);
                let who = self.describe(id);
                let cells_json: Vec<Json> = cells.iter().map(|c| Json::from(*c)).collect();
                let mut data = Json::object().with("prefab", prefab).with("cell", at).with("cells", cells_json);
                let mut paid = String::new();
                let mut payer_changed = None;
                if let Some(k) = cost
                    && let Some((before, after)) = self.pay(k)
                {
                    let owner = self.get(k.entity).map_or_else(|| format!("#{}", k.entity), Entity::label);
                    paid = format!(
                        "; paid {} from '{owner}'.{} ({} -> {})",
                        k.amount.display(),
                        k.prop,
                        before.literal(),
                        after.literal()
                    );
                    data.set(
                        "paid",
                        self.cost_json(k).with("before", prop_json(&before)).with("after", prop_json(&after)),
                    );
                    payer_changed = Some(k.entity);
                }
                let msg = format!("{who} was spawned at {} ({via}){paid}{when}", fmt_cell(at));
                self.write(EventKind::Spawned, Some(id), emitter, msg, data);
                if !self.entities[id as usize].counts.is_empty() {
                    self.add_counter(id);
                }
                if self.is_listened(Some(id), "spawned") {
                    self.emit(id, "spawned");
                }
                // Paying changed the payer's prop.
                if let Some(p) = payer_changed
                    && self.is_listened(Some(p), "changed")
                {
                    self.emit(p, "changed");
                }
                let why = format!("{who} was spawned");
                self.refresh_counts(&why, Stale::Prefab(Some(prefab), Some(id)));
                true
            }
        }
    }
}
