pub mod dir;
pub mod file;

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum DiffSide {
    Left,
    Right,
}

impl DiffSide {
    pub fn oppsite(&self) -> Self {
        match self {
            DiffSide::Left => DiffSide::Right,
            DiffSide::Right => DiffSide::Left,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffState {
    Unknown,
    Orphan(DiffSide),
    Different,
    Same,
}

impl DiffState {
    pub fn is_orphan(&self, side: DiffSide) -> bool {
        if let DiffState::Orphan(s) = self {
            *s == side
        } else {
            false
        }
    }
}
