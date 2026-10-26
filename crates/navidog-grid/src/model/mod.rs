//! Grid data model: the current page, the edit overlay, inserted and pending-delete rows, and the
//! rectangular selection (21-table-viewer.md §2). No UI here; unit tests cover the change set and copy/paste.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::RangeInclusive;

use gpui_kit::SharedString;
use navidog_db::dialect::{ChangeSet, Dialect, RowKey, RowUpdate, Writes, Written};
use navidog_util::sql_type::{TypeKind, is_string_type, type_kind};

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

    pub fn set_data(&mut self, columns: Vec<GridColumn>, rows: Vec<Vec<Cell>>) {
        self.columns = columns;
        self.rows = rows;
        self.cache_columns();
        self.hidden_columns.clear();
        self.hidden_rows.clear();
        self.discard();
        let max = (
            self.row_count().saturating_sub(1),
            self.columns.len().saturating_sub(1),
        );
        self.anchor = (self.anchor.0.min(max.0), self.anchor.1.min(max.1));
        self.head = self.anchor;
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

    pub fn row_count(&self) -> usize {
        self.rows.len() + self.new_rows.len()
    }

    /// Text the cell shows and the editor starts from. In raw data mode a string value is a SQL
    /// literal (`'PENELOPE'`), including quotes. A value already typed as an expression is left as
    /// typed. NULL and an unfilled cell are empty; the grid paints NULL itself.
    pub fn raw_text(&self, r: usize, c: usize, raw_mode: bool, dialect: Dialect) -> String {
        self.shown_text(r, c, raw_mode, dialect).to_string()
    }

    /// `raw_text` for painting: shares the cell's string unless raw data mode quotes it, so a
    /// frame does not copy every visible value.
    pub fn shown_text(&self, r: usize, c: usize, raw_mode: bool, dialect: Dialect) -> SharedString {
        let Some(Some(s)) = self.value(r, c) else {
            return SharedString::default();
        };
        let quote = raw_mode && !self.raw.contains(&(r, c)) && self.is_text(c);
        if quote {
            dialect.string_literal(s).into()
        } else {
            s.clone()
        }
    }

    /// Cached from `columns` on every column change; the delegate calls this per cell per frame.
    pub fn kind(&self, c: usize) -> TypeKind {
        self.kinds.get(c).copied().unwrap_or(TypeKind::Other)
    }

    pub fn set_column_type(&mut self, c: usize, type_name: impl Into<SharedString>) {
        if let Some(column) = self.columns.get_mut(c) {
            column.type_name = type_name.into();
            self.cache_columns();
        }
    }

    pub fn is_text(&self, c: usize) -> bool {
        self.text_columns.get(c).copied().unwrap_or(false)
    }

    fn cache_columns(&mut self) {
        self.kinds = self
            .columns
            .iter()
            .map(|column| type_kind(&column.type_name))
            .collect();
        self.text_columns = self
            .columns
            .iter()
            .map(|column| is_string_type(&column.type_name))
            .collect();
    }

    pub fn value(&self, r: usize, c: usize) -> Option<&Cell> {
        match r.checked_sub(self.rows.len()) {
            None => self.edits.get(&(r, c)).or_else(|| self.rows[r].get(c)),
            Some(n) => self.new_rows.get(n)?.get(c)?.as_ref(),
        }
    }

    pub fn set_value(&mut self, r: usize, c: usize, v: Cell) {
        self.write(r, c, v, false);
    }

    /// Like `set_value`, but a non-empty value is submitted as SQL (raw data mode).
    pub fn set_raw(&mut self, r: usize, c: usize, v: Cell) {
        self.write(r, c, v, true);
    }

    fn write(&mut self, r: usize, c: usize, v: Cell, raw: bool) {
        if self.locked || self.columns.get(c).is_some_and(|col| col.read_only) {
            return;
        }
        let stored = match r.checked_sub(self.rows.len()) {
            None if self.rows[r].get(c) == Some(&v) => {
                self.edits.remove(&(r, c));
                false
            }
            None => {
                self.edits.insert((r, c), v);
                true
            }
            Some(n) => {
                if let Some(cell) = self.new_rows.get_mut(n).and_then(|row| row.get_mut(c)) {
                    *cell = Some(v);
                    true
                } else {
                    false
                }
            }
        };
        if stored && raw {
            self.raw.insert((r, c));
        } else {
            self.raw.remove(&(r, c));
        }
    }

    pub fn row_state(&self, r: usize) -> RowState {
        // Past the last row is not an insert. An empty grid used to report row 0 as
        // Inserted, so "add row" tried to commit nothing and did not insert.
        if r >= self.row_count() {
            RowState::Clean
        } else if r >= self.rows.len() {
            RowState::Inserted
        } else if self.deleted.contains(&r) {
            RowState::Deleted
        } else if self.edits.range((r, 0)..=(r, usize::MAX)).next().is_some() {
            RowState::Edited
        } else {
            RowState::Clean
        }
    }

    /// The row has edits, is a new row, or is pending deletion, so leaving it commits (F-DATA-003).
    pub fn row_dirty(&self, r: usize) -> bool {
        !matches!(self.row_state(r), RowState::Clean)
    }

    pub fn is_edited(&self, r: usize, c: usize) -> bool {
        self.edits.contains_key(&(r, c)) || (r >= self.rows.len() && self.value(r, c).is_some())
    }

    /// Appends an empty row, selects it and returns its row number. Locked, it returns the current row.
    pub fn insert_row(&mut self) -> usize {
        if self.locked {
            return self.head.0;
        }
        self.new_rows.push(vec![None; self.columns.len()]);
        let r = self.row_count() - 1;
        self.select((r, 0), false);
        r
    }

    /// Deletes the rows the selection covers: inserted rows go away, existing rows are marked pending.
    pub fn delete_selected_rows(&mut self) {
        if self.locked {
            return;
        }
        let rows = self.selection().0;
        for r in rows.clone().rev() {
            match r.checked_sub(self.rows.len()) {
                Some(n) if n < self.new_rows.len() => {
                    let abs = self.rows.len() + n;
                    self.new_rows.remove(n);
                    self.raw = self
                        .raw
                        .iter()
                        .filter_map(|&(row, col)| match row.cmp(&abs) {
                            std::cmp::Ordering::Equal => None,
                            std::cmp::Ordering::Greater => Some((row - 1, col)),
                            std::cmp::Ordering::Less => Some((row, col)),
                        })
                        .collect();
                }
                Some(_) => {}
                None => {
                    self.deleted.insert(r);
                }
            }
        }
        let r = (*rows.start()).min(self.row_count().saturating_sub(1));
        self.select((r, self.head.1), false);
    }

    pub fn has_changes(&self) -> bool {
        !self.edits.is_empty() || !self.new_rows.is_empty() || !self.deleted.is_empty()
    }

    pub fn discard(&mut self) {
        self.edits.clear();
        self.new_rows.clear();
        self.deleted.clear();
        self.raw.clear();
    }

    /// Moves completely unfilled insert drafts out of the model. Other pending writes retain
    /// their row coordinates and raw-SQL flags; restoring the token uses the current column width.
    pub fn take_blank_insert_drafts(&mut self) -> BlankInsertDrafts {
        let start = self.rows.len();
        let mut drafts = BlankInsertDrafts {
            count: 0,
            anchor: None,
            head: None,
        };
        let mut retained = Vec::with_capacity(self.new_rows.len());
        let mut rows = Vec::with_capacity(self.new_rows.len());
        for (index, row) in std::mem::take(&mut self.new_rows).into_iter().enumerate() {
            if row.iter().all(Option::is_none) {
                if self.anchor.0 == start + index {
                    drafts.anchor = Some((drafts.count, self.anchor.1));
                }
                if self.head.0 == start + index {
                    drafts.head = Some((drafts.count, self.head.1));
                }
                drafts.count += 1;
                retained.push(None);
            } else {
                retained.push(Some(rows.len()));
                rows.push(row);
            }
        }
        self.new_rows = rows;
        self.raw = std::mem::take(&mut self.raw)
            .into_iter()
            .filter_map(|(row, column)| match row.checked_sub(start) {
                None => Some((row, column)),
                Some(index) => retained
                    .get(index)
                    .copied()
                    .flatten()
                    .map(|index| (start + index, column)),
            })
            .collect();
        let remap = |position: &mut Pos| {
            if let Some(index) = position.0.checked_sub(start) {
                position.0 = retained
                    .get(index)
                    .copied()
                    .flatten()
                    .map(|index| start + index)
                    .unwrap_or_else(|| self.row_count().saturating_sub(1));
            }
        };
        let (mut anchor, mut head) = (self.anchor, self.head);
        remap(&mut anchor);
        remap(&mut head);
        self.anchor = anchor;
        self.head = head;
        drafts
    }

    /// Restores a moved token exactly once. Every cell remains unfilled (DEFAULT), rather than
    /// becoming an explicit NULL or a pending INSERT, even when the visible columns changed.
    pub fn restore_blank_insert_drafts(&mut self, drafts: BlankInsertDrafts) {
        let start = self.row_count();
        self.new_rows
            .extend((0..drafts.count).map(|_| vec![None; self.columns.len()]));
        let position = |(row, column): Pos| {
            (
                start + row,
                column.min(self.columns.len().saturating_sub(1)),
            )
        };
        if let Some(anchor) = drafts.anchor {
            self.anchor = position(anchor);
        }
        if let Some(head) = drafts.head {
            self.head = position(head);
        }
    }

    /// Accepts only the writes that were sent, preserving independent, completely blank drafts.
    pub fn accept_committed_preserving_drafts(&mut self) {
        let drafts = self.take_blank_insert_drafts();
        self.accept_committed();
        self.restore_blank_insert_drafts(drafts);
    }

    /// Folds a successful commit into the loaded page when "synchronize after modifying" is off.
    /// Unfilled insert columns become NULL; generated values show up only on the next refresh.
    pub fn accept_committed(&mut self) {
        for (&(r, c), v) in &self.edits {
            if let Some(cell) = self.rows.get_mut(r).and_then(|row| row.get_mut(c)) {
                *cell = v.clone();
            }
        }
        let mut gone: Vec<usize> = self.deleted.iter().copied().collect();
        gone.sort_unstable_by(|a, b| b.cmp(a));
        for r in gone {
            if r < self.rows.len() {
                self.rows.remove(r);
            }
            if r < self.hidden_rows.len() {
                self.hidden_rows.remove(r);
            }
        }
        for row in self.new_rows.drain(..) {
            if row.iter().any(Option::is_some) {
                self.rows
                    .push(row.into_iter().map(|cell| cell.flatten()).collect());
                if !self.hidden_columns.is_empty() {
                    self.hidden_rows.push(vec![None; self.hidden_columns.len()]);
                }
            }
        }
        self.edits.clear();
        self.deleted.clear();
        self.raw.clear();
        if self.row_count() == 0 {
            self.head = (0, 0);
            self.anchor = (0, 0);
        } else if self.head.0 >= self.row_count() {
            let r = self.row_count() - 1;
            self.head.0 = r;
            self.anchor.0 = r;
        }
    }

    /// Whether the grid will not know what a commit stored: an inserted row left a column to the
    /// server (a default, a generated key), or a value was written as an SQL expression. Those rows
    /// show the wrong values and cannot be located again until the page reloads.
    pub fn commit_needs_reload(&self) -> bool {
        let unfilled = self
            .new_rows
            .iter()
            .filter(|row| row.iter().any(Option::is_some))
            .any(|row| {
                (0..self.columns.len())
                    .any(|c| !self.columns[c].read_only && row.get(c).is_none_or(Option::is_none))
            });
        unfilled
            || (!self.hidden_columns.is_empty()
                && self
                    .new_rows
                    .iter()
                    .any(|row| row.iter().any(Option::is_some)))
            || !self.raw.is_empty()
    }

    pub fn has_primary_key(&self) -> bool {
        self.columns
            .iter()
            .chain(&self.hidden_columns)
            .any(|c| c.primary_key)
    }

    fn key(&self, r: usize) -> RowKey {
        let pk = self.has_primary_key();
        let visible = self.columns.iter().zip(&self.rows[r]);
        let hidden = self
            .hidden_columns
            .iter()
            .zip(self.hidden_rows.get(r).into_iter().flatten());
        RowKey(
            visible
                .chain(hidden)
                .filter(|(col, _)| (!pk || col.primary_key) && !col.read_only)
                .map(|(col, v)| (col.name.to_string(), v.as_ref().map(|s| s.to_string())))
                .collect(),
        )
    }

    pub fn change_set(&self) -> ChangeSet {
        let name = |c: usize| self.columns[c].name.to_string();
        let written = |r: usize, c: usize, v: &Cell| {
            (
                name(c),
                Written {
                    value: v.as_ref().map(|s| s.to_string()),
                    raw: self.raw.contains(&(r, c)),
                },
            )
        };
        let mut updates: BTreeMap<usize, Writes> = BTreeMap::new();
        for (&(r, c), v) in &self.edits {
            if !self.deleted.contains(&r) && !self.columns[c].read_only {
                updates.entry(r).or_default().push(written(r, c, v));
            }
        }
        ChangeSet {
            updates: updates
                .into_iter()
                .map(|(r, values)| RowUpdate {
                    key: self.key(r),
                    values,
                })
                .collect(),
            // An inserted row with nothing typed is not inserted
            inserts: self
                .new_rows
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    let r = self.rows.len() + i;
                    row.iter()
                        .enumerate()
                        .filter_map(|(c, v)| {
                            if self.columns.get(c).is_some_and(|col| col.read_only) {
                                return None;
                            }
                            Some(written(r, c, v.as_ref()?))
                        })
                        .collect::<Writes>()
                })
                .filter(|v| !v.is_empty())
                .collect(),
            deletes: self.deleted.iter().map(|&r| self.key(r)).collect(),
        }
    }

    // Selection

    pub fn head(&self) -> Pos {
        self.head
    }

    /// Rows and columns the selection covers (inclusive).
    pub fn selection(&self) -> (RangeInclusive<usize>, RangeInclusive<usize>) {
        let (a, h) = (self.anchor, self.head);
        (a.0.min(h.0)..=a.0.max(h.0), a.1.min(h.1)..=a.1.max(h.1))
    }

    pub fn is_selected(&self, r: usize, c: usize) -> bool {
        let (rows, cols) = self.selection();
        rows.contains(&r) && cols.contains(&c)
    }

    /// Selects one cell; with `extend`, the anchor stays and the selection grows to that cell.
    pub fn select(&mut self, pos: Pos, extend: bool) {
        self.head = pos;
        if !extend {
            self.anchor = pos;
        }
    }

    /// Selects the whole row (a click on the marker column).
    pub fn select_rows(&mut self, r: usize, extend: bool) {
        let last = self.columns.len().saturating_sub(1);
        if !extend {
            self.anchor = (r, 0);
        } else {
            self.anchor.1 = 0;
        }
        self.head = (r, last);
    }

    /// Selects every cell of one column. With `extend`, columns from the anchor to this one.
    pub fn select_column(&mut self, c: usize, extend: bool) {
        let last = self.row_count().saturating_sub(1);
        if !extend {
            self.anchor = (0, c);
        } else {
            self.anchor.0 = 0;
        }
        self.head = (last, c);
    }

    pub fn select_all(&mut self) {
        self.anchor = (0, 0);
        self.head = (
            self.row_count().saturating_sub(1),
            self.columns.len().saturating_sub(1),
        );
    }

    /// The selected rows, each holding only its selected columns.
    pub fn selected_values(&self) -> Vec<Vec<Option<String>>> {
        let (rows, cols) = self.selection();
        rows.filter(|&r| r < self.row_count())
            .map(|r| {
                cols.clone()
                    .map(|c| self.value(r, c).cloned().flatten().map(|s| s.to_string()))
                    .collect()
            })
            .collect()
    }

    pub fn selected_columns(&self) -> Vec<String> {
        self.selection()
            .1
            .filter_map(|c| self.columns.get(c))
            .map(|c| c.name.to_string())
            .collect()
    }

    /// The selection as tab-separated text; NULL is copied as an empty string. `header` adds a name row.
    /// `raw` is the dialect in raw data mode: values are copied as the grid shows them (strings as
    /// quoted literals), so pasting them back in raw data mode, where text is SQL, writes the same values.
    /// Cells containing a tab, a newline or a quote use quoted TSV (RFC 4180 with a tab delimiter),
    /// so a copy/paste round trip keeps one value in one cell.
    pub fn selected_tsv(&self, header: bool, data: bool, raw: Option<Dialect>) -> String {
        let mut lines = Vec::new();
        if header {
            lines.push(
                self.selected_columns()
                    .iter()
                    .map(|name| tsv_field(name))
                    .collect::<Vec<_>>()
                    .join("\t"),
            );
        }
        if data {
            let (rows, cols) = self.selection();
            for r in rows.filter(|&r| r < self.row_count()) {
                let cells: Vec<String> = cols
                    .clone()
                    .map(|c| match raw {
                        Some(dialect) => tsv_field(&self.shown_text(r, c, true, dialect)),
                        None => tsv_field(
                            self.value(r, c)
                                .cloned()
                                .flatten()
                                .unwrap_or_default()
                                .as_ref(),
                        ),
                    })
                    .collect();
                lines.push(cells.join("\t"));
            }
        }
        lines.join("\n")
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

    /// Replaces `query` in one cell. Returns whether the text changed.
    pub fn replace_cell(
        &mut self,
        pos: Pos,
        query: &str,
        replacement: &str,
        case_sensitive: bool,
    ) -> bool {
        if self.locked {
            return false;
        }
        let Some(Some(current)) = self.value(pos.0, pos.1).cloned() else {
            return false;
        };
        let Some(next) = replace_text(&current, query, replacement, case_sensitive) else {
            return false;
        };
        self.set_value(pos.0, pos.1, Some(next.into()));
        true
    }

    /// Replaces `query` in every loaded cell. Returns how many cells changed.
    pub fn replace_all(&mut self, query: &str, replacement: &str, case_sensitive: bool) -> usize {
        let hits: Vec<Pos> = (0..self.row_count())
            .flat_map(|r| (0..self.columns.len()).map(move |c| (r, c)))
            .filter(|&(r, c)| self.cell_matches(r, c, query, case_sensitive))
            .collect();
        hits.into_iter()
            .filter(|&pos| self.replace_cell(pos, query, replacement, case_sensitive))
            .count()
    }

    /// Writes tab-separated text starting at the selection's top left, appending rows past the end.
    /// Quoted TSV is accepted, matching [`selected_tsv`](Self::selected_tsv).
    pub fn paste(&mut self, text: &str, raw: bool) {
        if self.locked {
            return;
        }
        let (rows, cols) = self.selection();
        let (r0, c0) = (*rows.start(), *cols.start());
        for (i, line) in parse_tsv(text).into_iter().enumerate() {
            let r = r0 + i;
            while r >= self.row_count() {
                self.new_rows.push(vec![None; self.columns.len()]);
            }
            for (j, v) in line.into_iter().enumerate() {
                if c0 + j < self.columns.len() {
                    let cell = Some(v.into());
                    if raw {
                        self.set_raw(r, c0 + j, cell);
                    } else {
                        self.set_value(r, c0 + j, cell);
                    }
                }
            }
        }
    }
}

mod layout;
#[cfg(test)]
mod tests;
mod tsv;

pub use layout::ColumnLayout;
use layout::*;
use tsv::*;
