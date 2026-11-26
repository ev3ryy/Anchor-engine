use crate::entity::NONE;

pub(crate) struct Hierarchy {
    parent: Vec<u32>,
    first_child: Vec<u32>,
    last_child: Vec<u32>,
    next_sibling: Vec<u32>,
    prev_sibling: Vec<u32>,
    first_root: u32,
    last_root: u32,
}

impl Hierarchy {
    pub(crate) fn new() -> Self {
        Self {
            parent: Vec::new(),
            first_child: Vec::new(),
            last_child: Vec::new(),
            next_sibling: Vec::new(),
            prev_sibling: Vec::new(),
            first_root: NONE,
            last_root: NONE,
        }
    }

    pub(crate) fn push_slot(&mut self) {
        self.parent.push(NONE);
        self.first_child.push(NONE);
        self.last_child.push(NONE);
        self.next_sibling.push(NONE);
        self.prev_sibling.push(NONE);
    }

    pub(crate) fn clear_slot(&mut self, slot: u32) {
        let slot = slot as usize;
        self.parent[slot] = NONE;
        self.first_child[slot] = NONE;
        self.last_child[slot] = NONE;
        self.next_sibling[slot] = NONE;
        self.prev_sibling[slot] = NONE;
    }

    pub(crate) fn parent_of(&self, slot: u32) -> u32 {
        self.parent[slot as usize]
    }

    pub(crate) fn first_child_of(&self, slot: u32) -> u32 {
        self.first_child[slot as usize]
    }

    pub(crate) fn last_child_of(&self, slot: u32) -> u32 {
        self.last_child[slot as usize]
    }

    pub(crate) fn next_siblings(&self) -> &[u32] {
        &self.next_sibling
    }

    pub(crate) fn first_root(&self) -> u32 {
        self.first_root
    }

    pub(crate) fn last_root(&self) -> u32 {
        self.last_root
    }

    pub(crate) fn link_root(&mut self, slot: u32) {
        let index = slot as usize;
        self.parent[index] = NONE;
        self.prev_sibling[index] = self.last_root;
        self.next_sibling[index] = NONE;
        if self.last_root == NONE {
            self.first_root = slot;
        } else {
            self.next_sibling[self.last_root as usize] = slot;
        }
        self.last_root = slot;
    }

    pub(crate) fn link_child(&mut self, parent: u32, child: u32) {
        let index = child as usize;
        self.parent[index] = parent;
        let last = self.last_child[parent as usize];
        self.prev_sibling[index] = last;
        self.next_sibling[index] = NONE;
        if last == NONE {
            self.first_child[parent as usize] = child;
        } else {
            self.next_sibling[last as usize] = child;
        }
        self.last_child[parent as usize] = child;
    }

    pub(crate) fn unlink(&mut self, slot: u32) {
        let index = slot as usize;
        let prev = self.prev_sibling[index];
        let next = self.next_sibling[index];
        let parent = self.parent[index];
        if prev != NONE {
            self.next_sibling[prev as usize] = next;
        }
        if next != NONE {
            self.prev_sibling[next as usize] = prev;
        }
        if parent == NONE {
            if self.first_root == slot {
                self.first_root = next;
            }
            if self.last_root == slot {
                self.last_root = prev;
            }
        } else {
            if self.first_child[parent as usize] == slot {
                self.first_child[parent as usize] = next;
            }
            if self.last_child[parent as usize] == slot {
                self.last_child[parent as usize] = prev;
            }
        }
        self.parent[index] = NONE;
        self.prev_sibling[index] = NONE;
        self.next_sibling[index] = NONE;
    }

    pub(crate) fn reaches(&self, mut cursor: u32, target: u32) -> bool {
        let limit = self.parent.len();
        let mut guard = 0;
        while cursor != NONE {
            if cursor == target {
                return true;
            }
            cursor = self.parent[cursor as usize];
            guard += 1;
            if guard > limit {
                return true;
            }
        }
        false
    }

    pub(crate) fn subtree(&self, root: u32) -> Vec<u32> {
        let mut slots = Vec::new();
        let mut stack = vec![root];
        while let Some(slot) = stack.pop() {
            slots.push(slot);
            self.push_reverse(self.last_child[slot as usize], &mut stack);
        }
        slots
    }

    pub(crate) fn push_reverse(&self, mut last: u32, stack: &mut Vec<u32>) {
        while last != NONE {
            stack.push(last);
            last = self.prev_sibling[last as usize];
        }
    }
}
