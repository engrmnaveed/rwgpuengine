//! The state hash: every field of the simulation's state, one by one.

use rwgpuengine_core::hash::Hasher64;
use rwgpuengine_data::PropValue;

use super::World;

impl World {
    /// Hash of the full simulation state. Equal hashes mean equal worlds:
    /// the cheap way to compare runs across machines and builds.
    pub fn state_hash(&self) -> u64 {
        let mut h = Hasher64::new();
        h.u64(self.frame).u64(self.seed);
        let (s, inc) = self.rng.state();
        h.u64(s).u64(inc);
        // A terrain map is part of the world (plain ground is not, so older
        // scenes keep their hashes).
        if let Some(g) = self.ground.as_ref().filter(|g| g.has_map) {
            h.u8(0xA0).i64(g.size[0] as i64).i64(g.size[1] as i64).bytes(&g.cells);
            for t in &g.terrains {
                h.str(&t.name).bool(t.build);
            }
        }
        // The shape lists, each once (none in a scene without shapes, so
        // older scenes keep their hashes); an entity then says which is its.
        if !self.shapes.is_empty() {
            h.u8(0xA1).u64(self.shapes.len() as u64);
            for list in &self.shapes {
                h.u64(list.len() as u64);
                for s in list.iter() {
                    h.u8(s.kind as u8).u8(s.turn).bytes(&[s.color.r, s.color.g, s.color.b, s.color.a]);
                    for v in s.at.iter().chain(&s.size) {
                        h.f32(*v);
                    }
                }
            }
        }
        for e in &self.entities {
            h.u32(e.id).bool(e.enabled).bool(e.solid);
            if e.pushable {
                h.u8(10);
            }
            if e.footprint != [1, 1] {
                h.u8(11).i64(e.footprint[0] as i64).i64(e.footprint[1] as i64);
            }
            h.f32(e.pos[0]).f32(e.pos[1]).f32(e.pos[2]);
            if let Some(t) = &e.tile {
                h.u8(1).bytes(&[t.color.r, t.color.g, t.color.b, t.color.a]);
            }
            if let Some(b) = &e.block {
                h.u8(2).bytes(&[b.color.r, b.color.g, b.color.b, b.color.a]).f32(b.height);
                if b.top != b.color {
                    h.u8(12).bytes(&[b.top.r, b.top.g, b.top.b, b.top.a]);
                }
            }
            if let Some(s) = &e.sprite {
                h.u8(3).bytes(&[s.color.r, s.color.g, s.color.b, s.color.a]).f32(s.size[0]).f32(s.size[1]);
            }
            if let Some(m) = &e.mover {
                h.u8(4).u32(m.cooldown);
            }
            if let Some(p) = &e.patrol {
                h.u8(5).u64(p.target as u64).i64(p.dir as i64).u32(p.timer);
            }
            if let Some(w) = &e.wander {
                h.u8(6).u32(w.timer);
            }
            if let Some(t) = &e.trigger {
                h.u8(7).u64(t.occupants.len() as u64);
                for o in &t.occupants {
                    h.u32(*o);
                }
            }
            if let Some(l) = &e.label {
                h.u8(8).str(&l.text).bytes(&[l.color.r, l.color.g, l.color.b, l.color.a]).u32(l.scale);
            }
            if let Some([x, y]) = e.screen_pos {
                h.u8(9).f32(x).f32(y);
            }
            if let Some(list) = e.shapes {
                h.u8(13).u32(list);
            }
            if e.turn != 0 {
                h.u8(14).u8(e.turn);
            }
            for (k, v) in &e.props {
                h.str(k);
                match v {
                    PropValue::Bool(b) => h.u8(0).bool(*b),
                    PropValue::Int(i) => h.u8(1).i64(*i),
                    PropValue::Float(f) => h.u8(2).f64(*f),
                    PropValue::Str(s) => h.u8(3).str(s),
                };
            }
        }
        h.finish()
    }
}
