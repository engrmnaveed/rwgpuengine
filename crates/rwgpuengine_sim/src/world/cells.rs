//! Which entities are on each cell: the structure under the cell index
//! (ADR 0026).
//!
//! Every cell has a list of entity ids, kept in id order. The ground's cells
//! are an array, one number per cell, so finding a cell's list is one read.
//! Cells outside the ground, and every cell of a world without a ground, are
//! in a sorted map. The lists themselves are chains through one pool of
//! nodes, so a move frees a node and takes it again and nothing is allocated.

use std::collections::BTreeMap;

use super::EntityId;

/// The ground is an array up to this many cells (4,096 x 4,096, 64 MB of
/// list heads). A larger ground is kept in the sorted map, like the cells
/// outside it: the same answers, more slowly.
const DENSE_CELLS: usize = 1 << 24;

#[derive(Clone, Copy, Debug)]
struct Node {
    id: EntityId,
    /// The next node of the cell's list, plus one; 0 ends the list.
    next: u32,
}

#[derive(Clone, Debug)]
pub(super) struct Cells {
    /// The size of the array part: the ground's, or [0, 0].
    size: [i32; 2],
    /// Per ground cell, row by row: its first node plus one; 0 is an empty cell.
    heads: Vec<u32>,
    /// The same for every other cell that has someone on it.
    outside: BTreeMap<[i32; 2], u32>,
    nodes: Vec<Node>,
    /// The first unused node plus one (they are chained through `next`).
    free: u32,
    entries: usize,
}

impl Cells {
    pub fn new(ground: Option<[i32; 2]>) -> Cells {
        let [w, h] = ground.map_or([0, 0], |[w, h]| [w.max(0), h.max(0)]);
        let dense = (w as usize).checked_mul(h as usize).filter(|n| *n <= DENSE_CELLS);
        let size = if dense.is_some() { [w, h] } else { [0, 0] };
        Cells {
            size,
            heads: vec![0; dense.unwrap_or(0)],
            outside: BTreeMap::new(),
            nodes: Vec::new(),
            free: 0,
            entries: 0,
        }
    }

    fn slot(&self, c: [i32; 2]) -> Option<usize> {
        (c[0] >= 0 && c[1] >= 0 && c[0] < self.size[0] && c[1] < self.size[1])
            .then(|| c[1] as usize * self.size[0] as usize + c[0] as usize)
    }

    fn head(&self, c: [i32; 2]) -> u32 {
        match self.slot(c) {
            Some(i) => self.heads[i],
            None => self.outside.get(&c).copied().unwrap_or(0),
        }
    }

    fn set_head(&mut self, c: [i32; 2], head: u32) {
        match self.slot(c) {
            Some(i) => self.heads[i] = head,
            None if head == 0 => {
                self.outside.remove(&c);
            }
            None => {
                self.outside.insert(c, head);
            }
        }
    }

    /// Put `id` on `cell`, keeping the cell's list in id order. Putting it
    /// there twice changes nothing.
    pub fn add(&mut self, cell: [i32; 2], id: EntityId) {
        let head = self.head(cell);
        // The node to link from (0: the cell's head) and the one to link to.
        let (mut before, mut at) = (0, head);
        while at != 0 && self.nodes[at as usize - 1].id < id {
            before = at;
            at = self.nodes[at as usize - 1].next;
        }
        if at != 0 && self.nodes[at as usize - 1].id == id {
            return;
        }
        let node = Node { id, next: at };
        let new = match self.free {
            0 => {
                self.nodes.push(node);
                self.nodes.len() as u32
            }
            free => {
                self.free = self.nodes[free as usize - 1].next;
                self.nodes[free as usize - 1] = node;
                free
            }
        };
        match before {
            0 => self.set_head(cell, new),
            b => self.nodes[b as usize - 1].next = new,
        }
        self.entries += 1;
    }

    /// Take `id` off `cell`. Taking what is not there changes nothing.
    pub fn remove(&mut self, cell: [i32; 2], id: EntityId) {
        let (mut before, mut at) = (0, self.head(cell));
        while at != 0 && self.nodes[at as usize - 1].id < id {
            before = at;
            at = self.nodes[at as usize - 1].next;
        }
        if at == 0 || self.nodes[at as usize - 1].id != id {
            return;
        }
        let next = self.nodes[at as usize - 1].next;
        match before {
            0 => self.set_head(cell, next),
            b => self.nodes[b as usize - 1].next = next,
        }
        self.nodes[at as usize - 1].next = self.free;
        self.free = at;
        self.entries -= 1;
    }

    /// The entities on `cell`, in id order.
    pub fn at(&self, cell: [i32; 2]) -> At<'_> {
        At { nodes: &self.nodes, next: self.head(cell) }
    }

    /// Every cell with someone on it and who is there: the ground's cells row
    /// by row, then the others in the map's order. Two indexes hold the same
    /// thing when these are equal, wherever their nodes are.
    pub fn listing(&self) -> Vec<([i32; 2], Vec<EntityId>)> {
        let ground = (0..self.heads.len())
            .filter(|i| self.heads[*i] != 0)
            .map(|i| [(i % self.size[0] as usize) as i32, (i / self.size[0] as usize) as i32]);
        ground.chain(self.outside.keys().copied()).map(|c| (c, self.at(c).collect())).collect()
    }

    /// (cells of the array, entries, bytes held).
    pub fn stats(&self) -> (usize, usize, usize) {
        let bytes = self.heads.capacity() * 4
            + self.nodes.capacity() * std::mem::size_of::<Node>()
            // A sorted map's entry: the cell, the head, and its share of a node.
            + self.outside.len() * 24;
        (self.heads.len(), self.entries, bytes)
    }
}

/// The entities on one cell, in id order.
pub(super) struct At<'a> {
    nodes: &'a [Node],
    next: u32,
}

impl Iterator for At<'_> {
    type Item = EntityId;

    fn next(&mut self) -> Option<EntityId> {
        let node = self.nodes.get((self.next as usize).checked_sub(1)?)?;
        self.next = node.next;
        Some(node.id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use rwgpuengine_core::rng::Rng;

    use super::Cells;

    /// Random adds and removes, on and off the ground, against a sorted set
    /// per cell: the same lists in the same order after every change.
    #[test]
    fn lists_stay_in_id_order_on_and_off_the_ground() {
        for (seed, ground) in [(1, Some([6, 5])), (2, None), (3, Some([1, 1])), (4, Some([0, 0]))] {
            let mut rng = Rng::new(seed);
            let mut cells = Cells::new(ground);
            let mut model: BTreeMap<[i32; 2], BTreeSet<u32>> = BTreeMap::new();
            for _ in 0..20_000 {
                let c = [rng.range_i32(-3, 8), rng.range_i32(-3, 8)];
                let id = rng.below(12);
                if rng.below(2) == 0 {
                    cells.add(c, id);
                    model.entry(c).or_default().insert(id);
                } else {
                    cells.remove(c, id);
                    if let Some(set) = model.get_mut(&c) {
                        set.remove(&id);
                        if set.is_empty() {
                            model.remove(&c);
                        }
                    }
                }
                let want: Vec<u32> = model.get(&c).map(|s| s.iter().copied().collect()).unwrap_or_default();
                assert_eq!(cells.at(c).collect::<Vec<_>>(), want, "cell {c:?}");
            }
            let mut listing = cells.listing();
            listing.sort();
            let want: Vec<([i32; 2], Vec<u32>)> =
                model.iter().map(|(c, s)| (*c, s.iter().copied().collect())).collect();
            assert_eq!(listing, want);
            let entries: usize = model.values().map(BTreeSet::len).sum();
            assert_eq!(cells.stats().1, entries);
            // Nothing leaks: there are never more nodes than entries at the fullest.
            assert!(cells.nodes.len() <= 12 * 144, "{} nodes", cells.nodes.len());
        }
    }

    #[test]
    fn a_ground_too_large_for_an_array_goes_to_the_map() {
        let mut cells = Cells::new(Some([1 << 20, 1 << 20]));
        assert_eq!(cells.stats().0, 0);
        cells.add([70_000, 900_000], 7);
        cells.add([70_000, 900_000], 3);
        assert_eq!(cells.at([70_000, 900_000]).collect::<Vec<_>>(), [3, 7]);
        cells.remove([70_000, 900_000], 3);
        cells.remove([70_000, 900_000], 7);
        assert!(cells.listing().is_empty());
    }
}
