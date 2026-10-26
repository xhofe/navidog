use super::*;

use std::collections::HashSet;

use gpui_kit::SharedString;

pub(super) fn server_order(server: &[SharedString], present: &[SharedString]) -> Vec<SharedString> {
    let mut out = Vec::with_capacity(present.len());
    for name in server {
        if present.iter().any(|p| p == name) && !out.iter().any(|n| n == name) {
            out.push(name.clone());
        }
    }
    for name in present {
        if !out.iter().any(|n| n == name) {
            out.push(name.clone());
        }
    }
    out
}

pub(super) fn permute_cells(row: &[Cell], order: &[usize]) -> Vec<Cell> {
    order
        .iter()
        .map(|&i| row.get(i).cloned().flatten())
        .collect()
}

pub(super) fn remap_col(old: usize, order: &[usize]) -> Option<usize> {
    order.iter().position(|&i| i == old)
}

/// Hiding every column hides none, since the grid could not show anything to unhide.
pub(super) fn hides(columns: &[GridColumn], hidden: &HashSet<String>, col: &GridColumn) -> bool {
    hidden.len() < columns.len() && hidden.contains(col.name.as_ref())
}

pub(super) fn select_columns(
    columns: &[GridColumn],
    rows: &[Vec<Cell>],
    pick: impl Fn(&GridColumn) -> bool,
) -> (Vec<GridColumn>, Vec<Vec<Cell>>) {
    let ix: Vec<usize> = (0..columns.len()).filter(|&i| pick(&columns[i])).collect();
    let columns = ix.iter().map(|&i| columns[i].clone()).collect();
    let rows = rows.iter().map(|r| permute_cells(r, &ix)).collect();
    (columns, rows)
}

/// Column order for one open grid. `home` is the order with freeze-induced moves undone.
/// `frozen` is the frozen names, left to right. Display order pulls frozen names to the front.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnLayout {
    home: Vec<SharedString>,
    frozen: Vec<SharedString>,
    /// Column order returned by the server. Drags and freezes do not change it.
    server: Vec<SharedString>,
}

impl ColumnLayout {
    pub fn clear(&mut self) {
        self.home.clear();
        self.frozen.clear();
        self.server.clear();
    }

    /// The query's column order, before this grid permutes it for display.
    pub fn remember_server(&mut self, present: &[SharedString]) {
        self.server = present.to_vec();
    }

    /// Puts `home` back to the server order and clears the freeze.
    pub fn restore_server(&mut self) {
        self.home = self.server.clone();
        self.frozen.clear();
    }

    /// True when the display order is the server order and nothing is frozen.
    pub fn is_server_order(&self, present: &[SharedString]) -> bool {
        !self.any_frozen() && self.display_names(present) == server_order(&self.server, present)
    }

    pub fn is_frozen(&self, name: &str) -> bool {
        self.frozen.iter().any(|n| n.as_ref() == name)
    }

    pub fn any_frozen(&self) -> bool {
        !self.frozen.is_empty()
    }

    /// How many of `columns` are frozen. They occupy the left of the display order.
    pub fn pinned(&self, columns: &[GridColumn]) -> usize {
        self.frozen
            .iter()
            .filter(|n| columns.iter().any(|c| &c.name == *n))
            .count()
    }

    /// Remembers new names at the end of `home`. Names that are absent this page stay put,
    /// so hiding a column and showing it again puts it back.
    pub fn sync(&mut self, present: &[SharedString]) {
        for name in present {
            if !self.home.iter().any(|n| n == name) {
                self.home.push(name.clone());
            }
        }
    }

    pub fn display_names(&self, present: &[SharedString]) -> Vec<SharedString> {
        let has = |n: &SharedString| present.iter().any(|p| p == n);
        let mut out = Vec::with_capacity(present.len());
        for name in &self.frozen {
            if has(name) && !out.iter().any(|n| n == name) {
                out.push(name.clone());
            }
        }
        for name in &self.home {
            if has(name) && !self.is_frozen(name) && !out.iter().any(|n| n == name) {
                out.push(name.clone());
            }
        }
        for name in present {
            if !out.iter().any(|n| n == name) {
                out.push(name.clone());
            }
        }
        out
    }

    /// `from` and `to` are indexes in [`Self::display_names`]. A frozen column only
    /// reorders `frozen`; any other column only reorders the unfrozen slots of `home`.
    pub fn drag(&mut self, present: &[SharedString], from: usize, to: usize) {
        let mut display = self.display_names(present);
        if from >= display.len() || to >= display.len() || from == to {
            return;
        }
        let name = display.remove(from);
        let frozen = self.is_frozen(&name);
        display.insert(to, name);
        if frozen {
            let prefix: Vec<_> = display.into_iter().filter(|n| self.is_frozen(n)).collect();
            rewrite(&mut self.frozen, &prefix);
        } else {
            let rest: Vec<_> = display.into_iter().filter(|n| !self.is_frozen(n)).collect();
            rewrite(&mut self.home, &rest);
        }
    }

    /// Inserts `name` just after the frozen columns. Already frozen, or an unknown name, is a no-op.
    pub fn freeze_column(&mut self, present: &[SharedString], name: &SharedString) -> bool {
        if self.is_frozen(name) || !present.iter().any(|n| n == name) {
            return false;
        }
        self.frozen.push(name.clone());
        true
    }

    /// Freezes the display prefix through `index` without moving columns.
    pub fn freeze_through(&mut self, present: &[SharedString], index: usize) {
        let display = self.display_names(present);
        if display.is_empty() {
            return;
        }
        let end = index.min(display.len() - 1);
        self.frozen = display[..=end].to_vec();
    }

    pub fn unfreeze_all(&mut self) {
        self.frozen.clear();
    }
}

/// Replaces the slots that appear in `order` with `order`, and leaves the others where they are.
pub(super) fn rewrite(slots: &mut [SharedString], order: &[SharedString]) {
    let mut it = order.iter();
    for slot in slots.iter_mut() {
        if order.iter().any(|n| n == slot)
            && let Some(next) = it.next()
        {
            *slot = next.clone();
        }
    }
}
