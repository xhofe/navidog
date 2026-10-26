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

impl GridModel {
    /// All columns in this result, including the columns currently hidden from the grid.
    pub fn all_columns(&self) -> impl Iterator<Item = &GridColumn> {
        self.columns.iter().chain(&self.hidden_columns)
    }

    /// Repartitions the accepted data without retaining another copy of its rows.
    /// Returns false if pending writes need to be committed or discarded first. Completely
    /// unfilled insert drafts contain no values to move, so they keep their default state.
    pub fn set_hidden(&mut self, hidden: &HashSet<String>) -> bool {
        if !self.edits.is_empty()
            || !self.deleted.is_empty()
            || self
                .new_rows
                .iter()
                .any(|row| row.iter().any(Option::is_some))
        {
            return false;
        }
        let head_name = self.columns.get(self.head.1).map(|c| c.name.clone());
        let mut columns = std::mem::take(&mut self.columns);
        columns.append(&mut self.hidden_columns);
        let mut rows = std::mem::take(&mut self.rows);
        for (ix, row) in rows.iter_mut().enumerate() {
            if let Some(values) = self.hidden_rows.get_mut(ix) {
                row.append(values);
            }
        }
        self.hidden_rows.clear();
        (self.columns, self.rows) = Self::project(&columns, &rows, hidden);
        self.cache_columns();
        self.set_hidden_columns(&columns, &rows, hidden);
        for row in &mut self.new_rows {
            *row = vec![None; self.columns.len()];
        }
        self.head.1 = head_name
            .and_then(|name| self.columns.iter().position(|c| c.name == name))
            .unwrap_or_else(|| self.head.1.min(self.columns.len().saturating_sub(1)));
        self.anchor = self.head;
        true
    }

    /// Sorts the accepted rows and their hidden key values together. Pending edits stay put.
    pub fn sort_rows(&mut self, col: usize, asc: bool) {
        if self.has_changes() || col >= self.columns.len() {
            return;
        }
        let numeric = self
            .columns
            .get(col)
            .is_some_and(|column| matches!(type_kind(&column.type_name), TypeKind::Number));
        let mut order: Vec<_> = (0..self.rows.len()).collect();
        order.sort_by(|&a, &b| {
            let value = |ix: usize| self.rows[ix].get(col).and_then(Option::as_deref);
            let ord = match (value(a), value(b)) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
                (Some(a), Some(b)) if numeric => match (a.parse::<f64>(), b.parse::<f64>()) {
                    // `total_cmp` gives NaN a fixed place instead of treating every
                    // NaN comparison as equal. Unparsable values sort after numbers
                    // and among themselves by text, which still makes a total order.
                    (Ok(a), Ok(b)) => a.total_cmp(&b),
                    (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                    (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                    (Err(_), Err(_)) => a.cmp(b),
                },
                (Some(a), Some(b)) => a.cmp(b),
            };
            if asc { ord } else { ord.reverse() }
        });
        let reorder = |rows: &mut Vec<Vec<Cell>>| {
            let mut previous: Vec<_> = std::mem::take(rows).into_iter().map(Some).collect();
            *rows = order
                .iter()
                .filter_map(|&ix| previous.get_mut(ix).and_then(Option::take))
                .collect();
        };
        reorder(&mut self.rows);
        reorder(&mut self.hidden_rows);
    }

    /// Moves one data column. Cells, edits, the raw-SQL flags and the selection follow it.
    pub fn move_column(&mut self, from: usize, to: usize) {
        let n = self.columns.len();
        if from == to || from >= n || to >= n {
            return;
        }
        let mut order: Vec<usize> = (0..n).collect();
        let ix = order.remove(from);
        order.insert(to, ix);
        self.reorder(&order);
    }

    /// `order[new_index]` is the old column index.
    pub fn reorder(&mut self, order: &[usize]) {
        if order.len() != self.columns.len() || (0..order.len()).any(|i| !order.contains(&i)) {
            return;
        }
        self.columns = order.iter().map(|&i| self.columns[i].clone()).collect();
        self.cache_columns();
        for row in &mut self.rows {
            let next = permute_cells(row, order);
            *row = next;
        }
        for row in &mut self.new_rows {
            let next: Vec<Option<Cell>> = order
                .iter()
                .map(|&i| row.get(i).cloned().flatten())
                .collect();
            *row = next;
        }
        let edits = std::mem::take(&mut self.edits);
        self.edits = edits
            .into_iter()
            .filter_map(|((r, c), v)| remap_col(c, order).map(|nc| ((r, nc), v)))
            .collect();
        let raw = std::mem::take(&mut self.raw);
        self.raw = raw
            .into_iter()
            .filter_map(|(r, c)| remap_col(c, order).map(|nc| (r, nc)))
            .collect();
        self.anchor.1 = remap_col(self.anchor.1, order).unwrap_or(0);
        self.head.1 = remap_col(self.head.1, order).unwrap_or(0);
    }

    /// Columns and cell values with `hidden` names removed. Hiding every column keeps them all.
    pub fn project(
        columns: &[GridColumn],
        rows: &[Vec<Cell>],
        hidden: &HashSet<String>,
    ) -> (Vec<GridColumn>, Vec<Vec<Cell>>) {
        select_columns(columns, rows, |c| !hides(columns, hidden, c))
    }

    /// Keeps the `hidden` columns of the unprojected page, so rows are still located by them.
    /// Call after `set_data` with the projection of the same page.
    pub fn set_hidden_columns(
        &mut self,
        columns: &[GridColumn],
        rows: &[Vec<Cell>],
        hidden: &HashSet<String>,
    ) {
        (self.hidden_columns, self.hidden_rows) =
            select_columns(columns, rows, |c| hides(columns, hidden, c));
    }
}
