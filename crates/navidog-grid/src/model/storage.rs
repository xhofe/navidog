use std::ops::RangeInclusive;

use gpui_kit::SharedString;
use navidog_db::dialect::Dialect;
use navidog_util::sql_type::TypeKind;

use super::*;

impl GridModel {
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

    pub fn value(&self, r: usize, c: usize) -> Option<&Cell> {
        match r.checked_sub(self.rows.len()) {
            None => self.edits.get(&(r, c)).or_else(|| self.rows[r].get(c)),
            Some(n) => self.new_rows.get(n)?.get(c)?.as_ref(),
        }
    }

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
}
