use crate::entity::{Entity, NONE};

#[derive(Clone, Debug)]
pub struct Column<T> {
    sparse: Vec<u32>,
    slots: Vec<u32>,
    generations: Vec<u32>,
    values: Vec<T>,
}

impl<T> Column<T> {
    pub fn new() -> Self {
        Self {
            sparse: Vec::new(),
            slots: Vec::new(),
            generations: Vec::new(),
            values: Vec::new(),
        }
    }

    pub fn insert(&mut self, entity: Entity, value: T) {
        let slot = entity.index() as usize;
        if self.sparse.len() <= slot {
            self.sparse.resize(slot + 1, NONE);
        }
        let dense = self.sparse[slot];
        if dense != NONE {
            let dense = dense as usize;
            self.slots[dense] = entity.index();
            self.generations[dense] = entity.generation();
            self.values[dense] = value;
            return;
        }
        let dense = self.values.len() as u32;
        self.sparse[slot] = dense;
        self.slots.push(entity.index());
        self.generations.push(entity.generation());
        self.values.push(value);
    }

    pub fn get(&self, entity: Entity) -> Option<&T> {
        let dense = self.dense(entity)?;
        Some(&self.values[dense])
    }

    pub fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
        let dense = self.dense(entity)?;
        Some(&mut self.values[dense])
    }

    pub fn remove(&mut self, entity: Entity) -> Option<T> {
        let dense = self.dense(entity)?;
        self.sparse[entity.index() as usize] = NONE;
        let last = self.values.len() - 1;
        if dense != last {
            let moved = self.slots[last];
            self.sparse[moved as usize] = dense as u32;
        }
        self.slots.swap_remove(dense);
        self.generations.swap_remove(dense);
        Some(self.values.swap_remove(dense))
    }

    pub fn iter(&self) -> impl Iterator<Item = (Entity, &T)> + '_ {
        self.slots
            .iter()
            .zip(self.generations.iter())
            .zip(self.values.iter())
            .map(|((slot, generation), value)| (Entity::new(*slot, *generation), value))
    }

    fn dense(&self, entity: Entity) -> Option<usize> {
        let dense = *self.sparse.get(entity.index() as usize)?;
        if dense == NONE {
            return None;
        }
        let dense = dense as usize;
        (self.generations.get(dense).copied() == Some(entity.generation())).then_some(dense)
    }
}

impl<T> Default for Column<T> {
    fn default() -> Self {
        Self::new()
    }
}
