//! JSON views of the world and its events (what `slate inspect` prints).

use slate_core::hash;
use slate_core::json::Json;
use slate_data::PropValue;

use crate::events::Event;
use crate::world::{Entity, World};

/// Whole numbers print as integers (`[3, 4, 0]`), others as floats.
pub fn num(v: f32) -> Json {
    if v.fract() == 0.0 && v.abs() < 1e9 { Json::Int(v as i64) } else { Json::from(v) }
}

pub fn vec_json(v: &[f32]) -> Json {
    Json::Arr(v.iter().map(|x| num(*x)).collect())
}

pub fn prop_json(v: &PropValue) -> Json {
    match v {
        PropValue::Bool(b) => Json::Bool(*b),
        PropValue::Int(i) => Json::Int(*i),
        PropValue::Float(f) => Json::Float(*f),
        PropValue::Str(s) => Json::Str(s.clone()),
    }
}

/// One entity with every component it has. Absent components are omitted.
pub fn entity_json(world: &World, e: &Entity) -> Json {
    let mut j = Json::object()
        .with("id", e.id)
        .with("name", e.name.clone())
        .with("prefab", e.prefab.clone())
        .with("enabled", e.enabled)
        .with("pos", vec_json(&e.pos))
        .with("cell", e.cell())
        .with("solid", e.solid);
    if let Some(t) = &e.tile {
        j.set("tile", Json::object().with("color", t.color.to_hex()));
    }
    if let Some(b) = &e.block {
        j.set("block", Json::object().with("color", b.color.to_hex()).with("height", num(b.height)));
    }
    if let Some(s) = &e.sprite {
        j.set(
            "sprite",
            Json::object()
                .with("shape", s.shape.name())
                .with("color", s.color.to_hex())
                .with("size", vec_json(&s.size))
                .with("offset", vec_json(&s.offset)),
        );
    }
    if let Some(m) = &e.mover {
        j.set("mover", Json::object().with("step_frames", m.step_frames).with("cooldown", m.cooldown));
    }
    if let Some(p) = &e.patrol {
        let path: Vec<Json> = p.def.path.iter().map(|c| Json::from(*c)).collect();
        j.set(
            "patrol",
            Json::object()
                .with("path", path)
                .with("step_frames", p.def.step_frames)
                .with("mode", p.def.mode.name())
                .with("next", p.def.path.get(p.target).copied())
                .with("timer", p.timer),
        );
    }
    if let Some(w) = &e.wander {
        let area = w.def.area.map(|[a, b]| Json::Arr(vec![Json::from(a), Json::from(b)]));
        j.set(
            "wander",
            Json::object().with("step_frames", w.def.step_frames).with("area", area).with("timer", w.timer),
        );
    }
    if let Some(t) = &e.trigger {
        let occ: Vec<Json> = t.occupants.iter().map(|id| Json::from(label(world, *id))).collect();
        j.set(
            "trigger",
            Json::object()
                .with("on_enter", t.def.on_enter.as_str())
                .with("on_exit", t.def.on_exit.as_str())
                .with("only", t.def.only.clone())
                .with("occupied_by", occ),
        );
    }
    if let Some(l) = &e.label {
        j.set(
            "label",
            Json::object()
                .with("text", l.text.as_str())
                .with("shown", e.label_text())
                .with("color", l.color.to_hex())
                .with("scale", l.scale)
                .with("offset", vec_json(&l.offset)),
        );
    }
    if let Some(p) = &e.screen_pos {
        j.set("screen_pos", vec_json(p));
    }
    let mut props = Json::object();
    for (k, v) in &e.props {
        props.set(k, prop_json(v));
    }
    j.set("props", props);
    if !e.allow.is_empty() {
        j.set("allow", Json::Arr(e.allow.iter().map(Json::from).collect()));
    }
    j
}

fn label(world: &World, id: u32) -> String {
    world.get(id).map_or_else(|| format!("#{id}"), Entity::label)
}

pub fn event_json(world: &World, e: &Event) -> Json {
    let mut j = Json::object().with("frame", e.frame).with("kind", e.kind.name());
    j.set("entity", e.entity.map(|id| label(world, id)));
    if let Some(o) = e.other {
        j.set("other", label(world, o));
    }
    j.set("message", e.message.as_str());
    if let Json::Obj(entries) = &e.data {
        for (k, v) in entries {
            if k != "signal" || e.signal.is_some() {
                j.set(k, v.clone());
            }
        }
    }
    j
}

/// Summary fields shared by `run` and `inspect`.
pub fn world_summary(world: &World) -> Json {
    let counts = world.events.counts();
    let mut by_kind = Json::object();
    for (k, n) in counts {
        by_kind.set(k.name(), n);
    }
    Json::object()
        .with("frame", world.frame)
        .with("seed", world.seed)
        .with("state_hash", hash::hex(world.state_hash()))
        .with("entities", world.entities.len())
        .with("enabled", world.entities.iter().filter(|e| e.enabled).count())
        .with("event_counts", by_kind)
}
