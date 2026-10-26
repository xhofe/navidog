use std::collections::HashSet;

use gpui_kit::SharedString;
use navidog_db::dialect::{RowKey, RowUpdate, Written};

use super::*;

fn model(pk: bool) -> GridModel {
    let col = |n: &str, pk| GridColumn {
        name: n.to_string().into(),
        type_name: "".into(),
        primary_key: pk,
        comment: None,
        read_only: false,
    };
    let mut m = GridModel::default();
    let cell = |s: &str| Some(SharedString::from(s.to_string()));
    m.set_data(
        vec![col("id", pk), col("name", false)],
        vec![vec![cell("1"), cell("a")], vec![cell("2"), None]],
    );
    m
}

#[test]
fn change_set_uses_primary_key_or_whole_row() {
    let mut m = model(true);
    m.set_value(1, 1, Some("b".into()));
    m.set_value(0, 1, Some("a".into())); // setting the original value is not a change
    m.select((0, 0), false);
    m.delete_selected_rows();
    let r = m.insert_row();
    m.set_value(r, 1, Some("c".into()));
    m.insert_row(); // an empty inserted row is not inserted
    assert_eq!(m.row_state(0), RowState::Deleted);
    assert_eq!(m.row_state(1), RowState::Edited);
    let cs = m.change_set();
    let v = |c: &str, s: Option<&str>| (c.to_string(), s.map(String::from));
    let w = |c: &str, s: Option<&str>| (c.to_string(), Written::text(s.map(String::from)));
    assert_eq!(
        cs.updates,
        [RowUpdate {
            key: RowKey(vec![v("id", Some("2"))]),
            values: vec![w("name", Some("b"))]
        }]
    );
    assert_eq!(cs.deletes, [RowKey(vec![v("id", Some("1"))])]);
    assert_eq!(cs.inserts, [vec![w("name", Some("c"))]]);

    let mut m = model(false);
    m.set_value(1, 0, Some("3".into()));
    let key = &m.change_set().updates[0].key;
    assert_eq!(key, &RowKey(vec![v("id", Some("2")), v("name", None)]));
}

#[test]
fn past_the_last_row_is_not_an_insert() {
    let m = model(true);
    assert_eq!(m.row_count(), 2);
    assert_eq!(m.row_state(2), RowState::Clean);
    assert!(!m.row_dirty(2));

    let mut empty = GridModel {
        columns: m.columns.clone(),
        ..GridModel::default()
    };
    assert_eq!(empty.row_count(), 0);
    assert!(!empty.row_dirty(0));
    empty.insert_row();
    assert!(empty.row_dirty(0));
    assert!(!empty.row_dirty(1));
}

#[test]
fn accept_committed_keeps_saved_rows_and_drops_empty_inserts() {
    let mut m = model(true);
    m.set_value(1, 1, Some("b".into()));
    m.select((0, 0), false);
    m.delete_selected_rows();
    let r = m.insert_row();
    m.set_value(r, 1, Some("c".into()));
    m.insert_row();
    m.accept_committed();
    assert!(!m.has_changes());
    assert_eq!(m.row_count(), 2);
    let text = |m: &GridModel, r, c| {
        m.value(r, c)
            .and_then(|cell| cell.as_deref())
            .map(|s| s.to_string())
    };
    assert_eq!(text(&m, 0, 0), Some("2".into()));
    assert_eq!(text(&m, 0, 1), Some("b".into()));
    assert_eq!(text(&m, 1, 1), Some("c".into()));
}

#[test]
fn accepting_writes_preserves_only_the_independent_blank_insert_drafts() {
    let mut m = model(true);
    m.set_value(0, 1, Some("updated".into()));
    m.select((1, 0), false);
    m.delete_selected_rows();
    m.insert_row();
    let filled = m.insert_row();
    m.set_value(filled, 0, Some("3".into()));
    m.set_value(filled, 1, Some("inserted".into()));
    let selected = m.insert_row();
    m.select((selected, 1), false);
    let changes = m.change_set();
    assert_eq!(changes.updates.len(), 1);
    assert_eq!(changes.deletes.len(), 1);
    assert_eq!(changes.inserts.len(), 1);

    m.accept_committed_preserving_drafts();
    assert_eq!(m.row_count(), 4);
    assert_eq!(m.value(0, 1), Some(&Some("updated".into())));
    assert_eq!(m.value(1, 1), Some(&Some("inserted".into())));
    assert_eq!(m.head(), (3, 1));
    assert!(m.has_changes());
    let changes = m.change_set();
    assert!(changes.updates.is_empty());
    assert!(changes.deletes.is_empty());
    assert!(changes.inserts.is_empty());
    for row in 2..4 {
        assert_eq!(m.row_state(row), RowState::Inserted);
        assert_eq!(m.value(row, 0), None);
        assert_eq!(m.value(row, 1), None);
    }

    // A successful sync can change the column projection without making defaults NULL.
    for _ in 0..2 {
        let drafts = m.take_blank_insert_drafts();
        m.set_data(
            vec![GridColumn {
                name: "name".into(),
                ..Default::default()
            }],
            vec![vec![Some("from server".into())]],
        );
        m.restore_blank_insert_drafts(drafts);
        assert_eq!(m.row_count(), 3, "repeated sync must not duplicate drafts");
        assert_eq!(m.head(), (2, 0));
        assert_eq!(m.value(1, 0), None);
        assert_eq!(m.value(2, 0), None);
        assert!(m.change_set().inserts.is_empty());
    }
    m.set_value(2, 0, Some("next insert".into()));
    assert_eq!(m.change_set().inserts.len(), 1);
    m.discard();
    assert_eq!(m.row_count(), 1);
    assert!(!m.has_changes());
}

#[test]
fn moving_blank_drafts_keeps_filled_insert_values_and_raw_flags_together() {
    let mut m = model(true);
    m.insert_row();
    let filled = m.insert_row();
    m.set_raw(filled, 1, Some("UPPER('x')".into()));
    m.insert_row();
    m.select((filled, 1), false);
    let drafts = m.take_blank_insert_drafts();
    assert_eq!(m.row_count(), 3);
    assert_eq!(m.head(), (2, 1));
    let changes = m.change_set();
    assert_eq!(changes.inserts.len(), 1);
    assert_eq!(changes.inserts[0].len(), 1);
    assert!(changes.inserts[0][0].1.raw);
    assert_eq!(changes.inserts[0][0].1.value.as_deref(), Some("UPPER('x')"));
    m.restore_blank_insert_drafts(drafts);
    assert_eq!(m.row_count(), 5);
    assert_eq!(m.head(), (2, 1));
    assert_eq!(m.value(3, 1), None);
    assert_eq!(m.value(4, 1), None);
}

#[test]
fn visibility_preserves_blank_insert_drafts_without_projecting_pending_writes() {
    let mut m = model(true);
    let row = m.insert_row();
    m.set_hidden(&["id".into()].into());
    assert_eq!(m.columns.len(), 1);
    assert_eq!(m.columns[0].name, "name");
    assert_eq!(m.row_state(row), RowState::Inserted);
    assert_eq!(m.value(row, 0), None);
    assert!(m.change_set().inserts.is_empty());
    m.set_hidden(&HashSet::new());
    assert_eq!(m.columns.len(), 2);
    assert_eq!(m.row_count(), 3);
    assert!(m.new_rows[row - m.rows.len()].iter().all(Option::is_none));
    m.set_value(row, 0, Some("draft".into()));
    m.set_hidden(&["name".into()].into());
    assert_eq!(m.columns.len(), 2);
    assert_eq!(m.value(row, 0), Some(&Some("draft".into())));
}

#[test]
fn committed_rows_survive_sorting_and_visibility_changes() {
    let mut m = model(true);
    m.set_hidden(&["id".into()].into());
    m.set_value(0, 0, Some("z".into()));
    m.select((1, 0), false);
    m.delete_selected_rows();
    m.accept_committed();
    m.set_hidden(&HashSet::new());
    assert_eq!(m.row_count(), 1);
    let name = m.columns.iter().position(|c| c.name == "name").unwrap();
    let id = m.columns.iter().position(|c| c.name == "id").unwrap();
    let row = m.insert_row();
    m.set_value(row, id, Some("3".into()));
    m.set_value(row, name, Some("c".into()));
    m.accept_committed();
    m.set_hidden(&["id".into()].into());
    m.sort_rows(0, true);
    assert_eq!(m.value(0, 0), Some(&Some("c".into())));
    assert_eq!(m.value(1, 0), Some(&Some("z".into())));
    m.select((0, 0), false);
    m.delete_selected_rows();
    assert_eq!(
        m.change_set().deletes,
        [RowKey(vec![("id".into(), Some("3".into()))])]
    );
    m.discard();
    m.set_hidden(&HashSet::new());
    let name = m.columns.iter().position(|c| c.name == "name").unwrap();
    assert_eq!(m.row_count(), 2);
    assert_eq!(m.value(1, name), Some(&Some("z".into())));
}

#[test]
fn sorting_uses_the_column_type_and_unicode_find_ignores_case() {
    let mut numeric = model(true);
    numeric.columns[0].type_name = "int".into();
    numeric.set_data(
        vec![numeric.columns[0].clone()],
        vec![vec![Some("10".into())], vec![Some("9".into())]],
    );
    numeric.sort_rows(0, true);
    assert_eq!(numeric.value(0, 0), Some(&Some("9".into())));

    let mut text = model(true);
    text.columns[0].type_name = "varchar".into();
    text.set_data(
        vec![text.columns[0].clone()],
        vec![vec![Some("10".into())], vec![Some("9".into())]],
    );
    text.sort_rows(0, true);
    assert_eq!(text.value(0, 0), Some(&Some("10".into())));

    text.set_data(
        vec![text.columns[0].clone()],
        vec![vec![Some("Ärger".into())]],
    );
    assert_eq!(text.find_cell("ärger", (0, 0), false, false), Some((0, 0)));
}

#[test]
fn numeric_sort_is_a_total_order_for_nan_and_bad_values() {
    let mut m = model(true);
    m.columns[0].type_name = "double".into();
    m.set_data(
        vec![m.columns[0].clone()],
        ["NaN", "2", "bad", "1"]
            .into_iter()
            .map(|value| vec![Some(value.into())])
            .collect(),
    );
    m.sort_rows(0, true);
    let values: Vec<_> = (0..m.row_count())
        .map(|row| m.value(row, 0).cloned().flatten().unwrap().to_string())
        .collect();
    assert_eq!(values, ["1", "2", "NaN", "bad"]);
}

#[test]
fn local_sort_puts_null_first_and_numbers_by_value() {
    let mut m = model(false);
    m.columns[0].type_name = "int".into();
    m.set_data(
        vec![m.columns[0].clone()],
        vec![vec![Some("10".into())], vec![None], vec![Some("9".into())]],
    );
    m.sort_rows(0, true);
    assert_eq!(
        m.rows,
        vec![vec![None], vec![Some("9".into())], vec![Some("10".into())]]
    );
    m.sort_rows(0, false);
    assert_eq!(m.value(0, 0), Some(&Some("10".into())));
    m.set_value(0, 0, Some("11".into()));
    m.sort_rows(0, true);
    m.set_hidden(&["id".into()].into());
    assert!(
        m.has_changes(),
        "projection and sorting cannot discard pending edits"
    );
    assert_eq!(m.value(0, 0), Some(&Some("11".into())));
}

#[test]
fn a_locked_grid_refuses_edits() {
    let mut m = model(true);
    m.locked = true;
    m.set_value(0, 1, Some("b".into()));
    m.insert_row();
    m.select((0, 0), false);
    m.delete_selected_rows();
    m.paste("x", false);
    assert!(!m.replace_cell((0, 1), "a", "z", true));
    assert!(!m.has_changes());
    assert_eq!(m.row_count(), 2);
    m.locked = false;
    m.set_value(0, 1, Some("b".into()));
    assert!(m.has_changes());
}

#[test]
fn commits_the_grid_cannot_mirror_need_a_reload() {
    let mut m = model(true);
    m.set_value(0, 1, Some("b".into()));
    assert!(!m.commit_needs_reload(), "a plain edit is mirrored");
    // A new row that leaves the key to the server
    let r = m.insert_row();
    m.set_value(r, 1, Some("c".into()));
    assert!(m.commit_needs_reload());
    m.discard();
    // Every column typed: the grid knows the row
    let r = m.insert_row();
    m.set_value(r, 0, Some("9".into()));
    m.set_value(r, 1, Some("c".into()));
    assert!(!m.commit_needs_reload());
    m.discard();
    // An SQL expression stores what the server computes
    m.set_raw(0, 1, Some("UPPER('x')".into()));
    assert!(m.commit_needs_reload());
}

#[test]
fn rectangle_selection_copy_and_paste() {
    let mut m = model(true);
    m.select((1, 1), false);
    m.select((0, 0), true);
    assert_eq!(m.selection(), (0..=1, 0..=1));
    assert_eq!(m.selected_tsv(true, true, None), "id\tname\n1\ta\n2\t");
    m.select_rows(1, false);
    assert_eq!(m.selected_values(), [vec![Some("2".into()), None]]);

    // Pasting two rows at (1, 1): the second is past the end, so it is appended
    m.select((1, 1), false);
    m.paste("x\ny\n", false);
    assert_eq!(m.value(1, 1), Some(&Some("x".into())));
    assert_eq!(m.row_count(), 3);
    assert_eq!(m.value(2, 1), Some(&Some("y".into())));
    assert_eq!(m.value(2, 0), None);
}

#[test]
fn tsv_preserves_a_cr_at_the_end_of_a_quoted_field() {
    let mut m = model(true);
    m.set_value(0, 1, Some("abc\r".into()));
    m.select((0, 1), false);
    let text = m.selected_tsv(false, true, None);
    assert_eq!(text, "\"abc\r\"");
    let mut out = model(true);
    out.select((0, 1), false);
    out.paste(&text, false);
    assert_eq!(out.value(0, 1), Some(&Some("abc\r".into())));
}

#[test]
fn a_single_quoted_empty_field_replaces_the_cell() {
    let mut m = model(true);
    m.select((0, 1), false);
    m.paste("\"\"", false);
    assert_eq!(m.value(0, 1).cloned().flatten(), Some("".into()));
}

#[test]
fn tsv_quotes_round_trip_tabs_newlines_and_quotes() {
    let mut m = model(true);
    m.set_value(0, 1, Some("a\tb\nc\"d".into()));
    m.select((0, 1), false);
    let text = m.selected_tsv(false, true, None);
    assert_eq!(text, "\"a\tb\nc\"\"d\"");
    let mut out = model(true);
    out.select((0, 1), false);
    out.paste(&text, false);
    assert_eq!(out.value(0, 1), Some(&Some("a\tb\nc\"d".into())));
}

#[test]
fn leaving_a_dirty_row_is_detectable_and_a_column_selects() {
    let mut m = model(true);
    assert!(!m.row_dirty(0));
    m.set_value(0, 1, Some("z".into()));
    assert!(m.row_dirty(0));
    assert!(!m.row_dirty(1));
    m.select_column(1, false);
    assert_eq!(m.selection(), (0..=1, 1..=1));
    m.select_column(0, true);
    assert_eq!(m.selection(), (0..=1, 0..=1));
}

#[test]
fn projection_drops_hidden_columns() {
    let m = model(true);
    let rows = vec![vec![Some("1".into()), Some("a".into())]];
    let (columns, rows) = GridModel::project(&m.columns, &rows, &["id".into()].into());
    assert_eq!(columns[0].name.as_ref(), "name");
    assert_eq!(rows, [vec![Some("a".into())]]);
    // Hiding every column keeps them: the grid would have nothing left to unhide
    let (columns, _) = GridModel::project(&m.columns, &[], &["id".into(), "name".into()].into());
    assert_eq!(columns.len(), 2);
}

#[test]
fn hidden_columns_still_locate_rows() {
    let v = |c: &str, s: &str| (c.to_string(), Some(s.to_string()));
    let full = model(true);
    let rows = vec![
        vec![Some("1".into()), Some("a".into())],
        vec![Some("2".into()), Some("a".into())],
    ];
    let hide = |pk: bool, rows: &[Vec<Cell>]| {
        let mut columns = full.columns.clone();
        columns[0].primary_key = pk;
        let hidden: HashSet<String> = ["id".into()].into();
        let (shown, shown_rows) = GridModel::project(&columns, rows, &hidden);
        let mut m = GridModel::default();
        m.set_data(shown, shown_rows);
        m.set_hidden_columns(&columns, rows, &hidden);
        m
    };

    // Hidden primary key: still the key, and the table still counts as keyed
    let mut m = hide(true, &rows);
    assert!(m.has_primary_key());
    m.set_value(1, 0, Some("b".into()));
    assert_eq!(m.change_set().updates[0].key, RowKey(vec![v("id", "2")]));

    // No primary key: the hidden column stays in the whole-row key
    let mut m = hide(false, &rows);
    m.set_value(1, 0, Some("b".into()));
    assert_eq!(
        m.change_set().updates[0].key,
        RowKey(vec![v("name", "a"), v("id", "2")])
    );

    // Deleting a row keeps the hidden values aligned with the remaining rows
    let mut m = hide(true, &rows);
    m.select((0, 0), false);
    m.delete_selected_rows();
    m.accept_committed();
    m.select((0, 0), false);
    m.delete_selected_rows();
    assert_eq!(m.change_set().deletes, [RowKey(vec![v("id", "2")])]);
}

#[test]
fn raw_edits_stay_unquoted_and_reverting_clears_the_flag() {
    let mut m = model(true);
    m.set_raw(0, 1, Some("UPPER('a')".into()));
    let cs = m.change_set();
    assert_eq!(cs.updates[0].values[0].1, Written::expr("UPPER('a')"));
    m.set_value(0, 1, Some("a".into()));
    assert!(m.change_set().updates.is_empty());
    let r = m.insert_row();
    m.set_raw(r, 1, Some("NOW()".into()));
    assert_eq!(m.change_set().inserts[0][0].1, Written::expr("NOW()"));
}

#[test]
fn raw_mode_quotes_string_values_only() {
    let mut m = model(true);
    m.set_column_type(1, "varchar(45)");
    assert_eq!(
        m.raw_text(0, 1, true, &navidog_sqlite::SqliteDialect),
        "'a'"
    );
    assert_eq!(m.raw_text(0, 0, true, &navidog_sqlite::SqliteDialect), "1");
    assert_eq!(m.raw_text(1, 1, true, &navidog_mysql::MysqlDialect), "");
    m.set_value(0, 1, Some("O'Brien".into()));
    assert_eq!(
        m.raw_text(0, 1, true, &navidog_mysql::MysqlDialect),
        "'O''Brien'"
    );
    assert_eq!(
        m.raw_text(0, 1, false, &navidog_mysql::MysqlDialect),
        "O'Brien"
    );
    // Copy in raw mode is what the grid shows, so pasting it back as SQL writes the same value
    m.select((0, 0), false);
    m.select((0, 1), true);
    assert_eq!(
        m.selected_tsv(
            false,
            true,
            Some(std::sync::Arc::new(navidog_mysql::MysqlDialect))
        ),
        "1\t'O''Brien'"
    );
    assert_eq!(m.selected_tsv(false, true, None), "1\tO'Brien");
    m.discard();
    m.select((0, 1), false);
    m.paste("'O''Brien'", true);
    let cs = m.change_set();
    assert_eq!(cs.updates[0].values[0].1, Written::expr("'O''Brien'"));
}

#[test]
fn find_wraps_and_replace_keeps_the_change_set() {
    let mut m = model(true);
    assert_eq!(m.find_cell("A", (0, 0), false, false), Some((0, 1)));
    assert_eq!(m.find_cell("nope", (0, 0), false, false), None);
    assert_eq!(m.find_cell("a", (0, 1), true, true), Some((0, 1)));
    assert_eq!(m.find_cell("2", (0, 1), true, true), Some((1, 0)));
    // Find Next from a hit in the last cell goes back to the first hit, not the same cell
    let mut last = model(true);
    last.set_value(1, 1, Some("1".into()));
    assert_eq!(last.find_cell("1", (1, 1), true, true), Some((0, 0)));
    assert_eq!(m.find_column_name("na", 0, false, false), Some(1));
    assert!(m.replace_cell((0, 1), "a", "z", true));
    assert_eq!(m.value(0, 1), Some(&Some("z".into())));
    assert_eq!(m.replace_all("z", "zz", false), 1);
    assert!(m.has_changes());
}

#[test]
fn read_only_columns_are_not_written() {
    let mut m = model(true);
    m.columns[0].read_only = true;
    m.set_value(0, 0, Some("9".into()));
    assert!(m.change_set().updates.is_empty());
    assert_eq!(m.value(0, 0), Some(&Some("1".into())));
}

#[test]
fn move_column_moves_cells_edits_and_selection() {
    let mut m = model(true);
    m.set_value(0, 1, Some("z".into()));
    m.set_raw(1, 1, Some("NOW()".into()));
    m.select((1, 1), false);
    m.move_column(1, 0);
    assert_eq!(m.columns[0].name.as_ref(), "name");
    assert_eq!(m.columns[1].name.as_ref(), "id");
    assert_eq!(m.value(0, 0).and_then(|c| c.as_deref()), Some("z"));
    assert_eq!(m.value(0, 1).and_then(|c| c.as_deref()), Some("1"));
    assert!(m.raw.contains(&(1, 0)));
    assert_eq!(m.head(), (1, 0));
}

fn names(list: &[&str]) -> Vec<SharedString> {
    list.iter()
        .map(|s| SharedString::from(s.to_string()))
        .collect()
}

#[test]
fn freeze_column_moves_to_the_end_of_the_frozen_prefix_and_unfreeze_restores_home() {
    let present = names(&["a", "b", "c", "d"]);
    let mut layout = ColumnLayout::default();
    layout.sync(&present);
    assert!(layout.freeze_column(&present, &present[2]));
    assert_eq!(layout.display_names(&present), names(&["c", "a", "b", "d"]));
    assert!(layout.freeze_column(&present, &present[0]));
    assert_eq!(layout.display_names(&present), names(&["c", "a", "b", "d"]));
    assert!(!layout.freeze_column(&present, &present[2]));
    layout.unfreeze_all();
    assert_eq!(layout.display_names(&present), present);
}

#[test]
fn freeze_through_does_not_move_columns() {
    let present = names(&["a", "b", "c", "d"]);
    let mut layout = ColumnLayout::default();
    layout.sync(&present);
    layout.freeze_through(&present, 1);
    assert_eq!(layout.display_names(&present), present);
    assert!(layout.is_frozen("a") && layout.is_frozen("b"));
    assert!(!layout.is_frozen("c"));
    layout.unfreeze_all();
    assert_eq!(layout.display_names(&present), present);
    assert!(!layout.any_frozen());
}

#[test]
fn restore_server_undoes_drags_and_freezes() {
    let present = names(&["a", "b", "c", "d"]);
    let mut layout = ColumnLayout::default();
    layout.remember_server(&present);
    layout.sync(&present);
    layout.drag(&present, 0, 3);
    assert!(layout.freeze_column(&present, &present[1]));
    assert_ne!(layout.display_names(&present), present);
    layout.restore_server();
    assert_eq!(layout.display_names(&present), present);
    assert!(layout.is_server_order(&present));
}
