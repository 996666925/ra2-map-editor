//! Shared map-axis constraint for continuous terrain and object brushes.
use crate::document::Cell;

#[derive(Clone, Copy, Default)]
pub(super) struct BrushDirection {
    shift: bool,
    line: bool,
    last: Option<Cell>,
    lock: Option<AxisLock>,
}

#[derive(Clone, Copy)]
struct AxisLock {
    origin: Cell,
    axis: Option<usize>,
}

impl BrushDirection {
    pub fn set_modifiers(&mut self, shift: bool, line: bool) {
        if self.shift != shift || self.line != line {
            self.lock = None;
        }
        self.shift = shift;
        self.line = line;
        if !shift && !line {
            self.lock = None;
        }
    }

    pub fn constrain(&mut self, at: Option<Cell>, press: bool) -> Option<Cell> {
        // Leaving the map breaks interpolation and starts a fresh constraint
        // on re-entry, just as it does for the unconstrained brush path.
        if press || at.is_none() {
            self.last = None;
            self.lock = None;
        }
        let mut at = at?;
        if self.shift || self.line {
            let lock = self.lock.get_or_insert(AxisLock {
                origin: self.last.unwrap_or(at),
                axis: None,
            });
            let dx = at[0].abs_diff(lock.origin[0]);
            let dy = at[1].abs_diff(lock.origin[1]);
            if self.shift && lock.axis.is_none() && (dx != 0 || dy != 0) {
                lock.axis = Some(if dx >= dy { 0 } else { 1 });
            }
            if let Some(axis) = lock.axis {
                at[1 - axis] = lock.origin[1 - axis];
            }
        }
        self.last = Some(at);
        Some(at)
    }

    pub fn preview(mut self, at: Option<Cell>) -> Option<Cell> {
        self.constrain(at, false)
    }

    pub fn line_origin(&self) -> Option<Cell> {
        self.lock.map(|lock| lock.origin)
    }
}
