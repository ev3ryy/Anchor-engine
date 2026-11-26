pub(crate) const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entity {
    index: u32,
    generation: u32,
}

impl Entity {
    pub(crate) fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    pub(crate) fn index(self) -> u32 {
        self.index
    }

    pub(crate) fn generation(self) -> u32 {
        self.generation
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    StaleEntity,
    Cycle,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StaleEntity => write!(f, "stale entity"),
            Self::Cycle => write!(f, "parent cycle"),
        }
    }
}

impl std::error::Error for Error {}

pub(crate) fn next_generation(generation: u32) -> u32 {
    let next = generation.wrapping_add(1);
    if next == 0 { 1 } else { next }
}
