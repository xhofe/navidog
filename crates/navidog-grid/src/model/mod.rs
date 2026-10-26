//! Grid data model: the current page, the edit overlay, inserted and pending-delete rows, and the
//! rectangular selection (21-table-viewer.md §2). No UI here; unit tests cover the change set and copy/paste.

use std::collections::{BTreeMap, BTreeSet};

use gpui_kit::SharedString;
use navidog_util::sql_type::{TypeKind, type_kind};

/// A cell value; `None` inside the `Option` is NULL, and a missing cell is an unfilled new row.
pub type Cell = Option<SharedString>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridColumn {
    pub name: SharedString,
    pub type_name: SharedString,
    pub primary_key: bool,
    pub comment: Option<SharedString>,
    /// Shown but not written back (SQLite ROWID)
    pub read_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowState {
    Clean,
    Edited,
    Inserted,
    Deleted,
}

/// Row and column coordinates are data coordinates (they exclude the marker column on the left).
pub type Pos = (usize, usize);

/// A move-only transfer of completely unfilled insert drafts across an accepted page replacement.
/// It contains no cell values or copy of the accepted result rows.
#[must_use = "Restore the blank insert drafts after replacing or accepting the page"]
pub struct BlankInsertDrafts {
    count: usize,
    anchor: Option<Pos>,
    head: Option<Pos>,
}

#[derive(Default)]
pub struct GridModel {
    pub columns: Vec<GridColumn>,
    rows: Vec<Vec<Cell>>,
    /// Edits of existing rows: (row, column) to the new value
    edits: BTreeMap<Pos, Cell>,
    /// Inserted rows; `None` means that column was left empty and the insert uses the default
    new_rows: Vec<Vec<Option<Cell>>>,
    deleted: BTreeSet<usize>,
    /// Cells whose text is SQL rather than a string literal (raw data mode)
    raw: BTreeSet<Pos>,
    /// Hidden columns and their values in each loaded row. Rows are still located by them, so
    /// hiding the primary key (or any column of a table without one) does not widen a WHERE.
    hidden_columns: Vec<GridColumn>,
    hidden_rows: Vec<Vec<Cell>>,
    /// While a commit or a load is running, edits are refused: the commit's success reloads the
    /// page (or marks every change committed), and a load replaces the rows, either way losing them.
    pub locked: bool,
    kinds: Vec<TypeKind>,
    text_columns: Vec<bool>,
    anchor: Pos,
    head: Pos,
}

impl GridModel {
    /// Next cell at or after `from` whose text contains `query`. Wraps around the page.
    /// `after` starts at the following cell, so Find Next does not stay on the current hit.
    pub fn find_cell(
        &self,
        query: &str,
        from: Pos,
        after: bool,
        case_sensitive: bool,
    ) -> Option<Pos> {
        if query.is_empty() || self.columns.is_empty() || self.row_count() == 0 {
            return None;
        }
        let cols = self.columns.len();
        let start = if after {
            let (r, c) = from;
            if c + 1 < cols {
                (r, c + 1)
            } else {
                (r.saturating_add(1), 0)
            }
        } else {
            from
        };
        let total = self.row_count() * cols;
        // Past the last cell wraps to the first, so Find Next from the last hit starts over
        let start_i = (start.0 * cols + start.1) % total;
        (0..total).find_map(|step| {
            let i = (start_i + step) % total;
            let pos = (i / cols, i % cols);
            self.cell_matches(pos.0, pos.1, query, case_sensitive)
                .then_some(pos)
        })
    }

    /// Next column name containing `query`, wrapping around.
    pub fn find_column_name(
        &self,
        query: &str,
        from: usize,
        after: bool,
        case_sensitive: bool,
    ) -> Option<usize> {
        let n = self.columns.len();
        if query.is_empty() || n == 0 {
            return None;
        }
        let start = if after {
            from.saturating_add(1) % n
        } else {
            from.min(n - 1)
        };
        (0..n).find_map(|step| {
            let c = (start + step) % n;
            contains(&self.columns[c].name, query, case_sensitive).then_some(c)
        })
    }

    pub fn cell_matches(&self, r: usize, c: usize, query: &str, case_sensitive: bool) -> bool {
        self.value(r, c)
            .and_then(|v| v.as_deref())
            .is_some_and(|s| contains(s, query, case_sensitive))
    }
}

mod edit;
mod projection;
mod storage;
#[cfg(test)]
mod tests;
mod tsv;

pub use projection::ColumnLayout;
use tsv::*;
