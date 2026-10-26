use std::collections::BTreeMap;

use navidog_db::dialect::{ChangeSet, RowKey, RowUpdate, Writes, Written};
use navidog_util::sql_type::{is_string_type, type_kind};

use super::*;

impl GridModel {
    pub(super) fn cache_columns(&mut self) {
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
