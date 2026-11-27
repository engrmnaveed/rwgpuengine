//! `[[connect]]` wiring with its names resolved: who listens to which signal,
//! on what conditions, and what it does.

use std::collections::BTreeMap;

use rwgpuengine_data::{CondTest, ConditionDef, ConnectAction, ConnectFrom, PropValue, SceneDef};

use super::{Entity, EntityId};

/// What a connection listens to.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Entity(EntityId),
    /// Every entity made from this prefab (placed or spawned).
    Prefab(String),
    /// Taps on ground cells that hit no entity.
    Ground,
}

/// What a connection acts on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Entity(EntityId),
    /// The entity that emitted the signal (`to = "self"`).
    Emitter,
    /// `spawn` has no target.
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    pub from: Source,
    pub signal: String,
    /// Run in order; a refused `spawn` stops the rest.
    pub actions: Vec<Act>,
    /// All must hold when the signal arrives (checked once, before the first action).
    pub when: Vec<Condition>,
}

/// One action of a connection, with its target resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Act {
    pub to: Target,
    pub action: ConnectAction,
    /// With add/set.
    pub prop: Option<String>,
    pub value: Option<PropValue>,
    /// With spawn.
    pub prefab: Option<String>,
    /// With spawn: who pays, from which prop, how much.
    pub cost: Option<Cost>,
}

/// A spawn's price, with the payer resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Cost {
    pub entity: EntityId,
    pub prop: String,
    pub amount: PropValue,
}

impl Connection {
    /// Does this connection listen to `signal` from `emitter` (`None` = the ground)?
    pub(super) fn listens(&self, entities: &[Entity], emitter: Option<EntityId>, signal: &str) -> bool {
        self.signal == signal
            && match (&self.from, emitter) {
                (Source::Entity(id), Some(e)) => *id == e,
                (Source::Prefab(p), Some(e)) => {
                    entities.get(e as usize).and_then(|x| x.prefab.as_deref()) == Some(p.as_str())
                }
                (Source::Ground, None) => true,
                _ => false,
            }
    }
}

/// A `when` condition with its entity resolved (`None` for `reason`).
#[derive(Clone, Debug, PartialEq)]
pub struct Condition {
    pub entity: Option<EntityId>,
    pub def: ConditionDef,
}

/// A scene's `[[connect]]` tables with their names resolved. A table that
/// names an entity the scene does not have is left out (the loader reports it).
pub(super) fn resolve(scene: &SceneDef, names: &BTreeMap<String, EntityId>) -> Vec<Connection> {
    scene
        .connections
        .iter()
        .filter_map(|c| {
            let when = c
                .when
                .iter()
                .map(|w| {
                    let entity = match w.test {
                        CondTest::Reason(_) => None,
                        _ => Some(*names.get(&w.entity)?),
                    };
                    Some(Condition { entity, def: w.clone() })
                })
                .collect::<Option<Vec<_>>>()?;
            let from = match &c.from {
                ConnectFrom::Entity(n) => Source::Entity(*names.get(n)?),
                ConnectFrom::Prefab(p) => Source::Prefab(p.clone()),
                ConnectFrom::Ground => Source::Ground,
            };
            let actions = c
                .actions
                .iter()
                .map(|a| {
                    let to = match (a.action, a.to.as_deref()) {
                        (ConnectAction::Spawn, _) => Target::None,
                        (_, Some("self")) => Target::Emitter,
                        (_, Some(n)) => Target::Entity(*names.get(n)?),
                        (_, None) => return None,
                    };
                    let cost = match &a.cost {
                        Some(k) => Some(Cost {
                            entity: *names.get(&k.entity)?,
                            prop: k.prop.clone(),
                            amount: k.amount.clone(),
                        }),
                        None => None,
                    };
                    Some(Act {
                        to,
                        action: a.action,
                        prop: a.prop.clone(),
                        value: a.value.clone(),
                        prefab: a.prefab.clone(),
                        cost,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(Connection { from, signal: c.signal.clone(), actions, when })
        })
        .collect()
}

/// Who listens to what: every connection under its signal and the place it
/// listens to, so that a signal finds its connections without looking at all
/// of them (ADR 0026). Built once: a world's connections do not change.
#[derive(Clone, Debug, Default)]
pub(super) struct Routes {
    /// The signal and prefab names that connections use, each with a number.
    signals: BTreeMap<String, u32>,
    prefabs: BTreeMap<String, u32>,
    /// (signal, where it listens, the connection's place in the file),
    /// sorted: the connections of one signal from one place are a run, in
    /// file order.
    routes: Vec<(u32, Place, u32)>,
}

/// Where a connection listens, with names as numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Place {
    Ground,
    Entity(EntityId),
    Prefab(u32),
}

/// The number of `name`, a new one if it has none yet.
fn numbered(names: &mut BTreeMap<String, u32>, name: &str) -> u32 {
    if let Some(n) = names.get(name) {
        return *n;
    }
    let n = names.len() as u32;
    names.insert(name.to_string(), n);
    n
}

impl Routes {
    pub fn build(connections: &[Connection]) -> Routes {
        let (mut signals, mut prefabs) = (BTreeMap::new(), BTreeMap::new());
        let mut routes: Vec<(u32, Place, u32)> = connections
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let from = match &c.from {
                    Source::Ground => Place::Ground,
                    Source::Entity(id) => Place::Entity(*id),
                    Source::Prefab(p) => Place::Prefab(numbered(&mut prefabs, p)),
                };
                (numbered(&mut signals, &c.signal), from, i as u32)
            })
            .collect();
        routes.sort_unstable();
        Routes { signals, prefabs, routes }
    }

    /// The connections of `signal` that listen to `from`, in file order.
    fn run(&self, signal: u32, from: Place) -> &[(u32, Place, u32)] {
        let start = self.routes.partition_point(|r| (r.0, r.1) < (signal, from));
        let len = self.routes[start..].partition_point(|r| (r.0, r.1) == (signal, from));
        &self.routes[start..start + len]
    }

    /// The two runs a signal from an entity can reach: the connections from
    /// that entity, and those from its prefab. For the ground, one run.
    fn runs(&self, emitter: Option<EntityId>, prefab: Option<&str>, signal: &str) -> [&[(u32, Place, u32)]; 2] {
        let Some(&signal) = self.signals.get(signal) else { return [&[], &[]] };
        match emitter {
            None => [self.run(signal, Place::Ground), &[]],
            Some(e) => [
                self.run(signal, Place::Entity(e)),
                prefab.and_then(|p| self.prefabs.get(p)).map_or(&[], |p| self.run(signal, Place::Prefab(*p))),
            ],
        }
    }

    /// The connections that listen to `signal` from `emitter` (`None` = the
    /// ground), by their place in the file, in file order. `prefab` is the
    /// emitter's.
    pub fn listeners(&self, emitter: Option<EntityId>, prefab: Option<&str>, signal: &str) -> Vec<u32> {
        let [a, b] = self.runs(emitter, prefab, signal);
        // Both are in file order and no connection is in both: merge them.
        let mut out = Vec::with_capacity(a.len() + b.len());
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            if j == b.len() || (i < a.len() && a[i].2 < b[j].2) {
                out.push(a[i].2);
                i += 1;
            } else {
                out.push(b[j].2);
                j += 1;
            }
        }
        out
    }

    pub fn any(&self, emitter: Option<EntityId>, prefab: Option<&str>, signal: &str) -> bool {
        self.runs(emitter, prefab, signal).iter().any(|run| !run.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use rwgpuengine_core::rng::Rng;
    use rwgpuengine_data::{SceneLoader, Vfs};

    use super::super::{World, scan};
    use super::{Connection, Routes, Source};

    /// Random wiring, every emitter and every signal: the routes give the
    /// connections the scan gives, in the same order.
    #[test]
    fn routes_find_the_connections_the_scan_finds_in_file_order() {
        let prefabs = ["p0", "p1", "p2"];
        let signals = ["tapped", "pressed", "changed"];
        let mut vfs = Vfs::new();
        for p in prefabs {
            vfs.insert(format!("prefabs/{p}.toml"), "sprite = {}\n");
        }
        // Eight entities: copies of each prefab, and two that are copies of nothing.
        let scene: String = (0..8)
            .map(|i| {
                [
                    "[[entity]]\nprefab = \"p0\"\n",
                    "[[entity]]\nprefab = \"p1\"\n",
                    "[[entity]]\nprefab = \"p2\"\n",
                    "[[entity]]\n",
                ][i % 4]
            })
            .collect();
        vfs.insert("scenes/main.toml", scene);
        let mut loader = SceneLoader::new(&vfs);
        let def = loader.load_scene("scenes/main.toml").unwrap_or_else(|| panic!("{:#?}", loader.diags));
        let world = World::new(&def, 1);
        let mut rng = Rng::new(5);
        for round in 0..300 {
            let connections: Vec<Connection> = (0..rng.below(40))
                .map(|_| Connection {
                    from: match rng.below(5) {
                        0 => Source::Ground,
                        1 | 2 => Source::Entity(rng.below(10)),
                        _ => Source::Prefab(["p0", "p1", "p2", "p9"][rng.below(4) as usize].to_string()),
                    },
                    signal: signals[rng.below(2) as usize].to_string(),
                    actions: Vec::new(),
                    when: Vec::new(),
                })
                .collect();
            let routes = Routes::build(&connections);
            // Emitters: the ground, every entity, and two ids no entity has.
            for emitter in std::iter::once(None).chain((0..10).map(Some)) {
                for signal in signals {
                    let prefab = emitter.and_then(|e| world.get(e)).and_then(|e| e.prefab.as_deref());
                    let scanned = scan::listeners(&connections, &world.entities, emitter, signal);
                    assert_eq!(
                        routes.listeners(emitter, prefab, signal),
                        scanned,
                        "round {round}, {emitter:?} {signal}"
                    );
                    assert_eq!(routes.any(emitter, prefab, signal), !scanned.is_empty(), "round {round}");
                }
            }
        }
    }
}
