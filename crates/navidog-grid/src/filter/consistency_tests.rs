//! The six SQL consumers must read one script the same way (R-04/D-05): a string, quoted
//! identifier, comment or dollar body is text for every one of them, whether it holds a `;`, a
//! comma, `AND`, a `#` or a parameter marker.

use super::parse_where;
use navidog_db::dialect::Dialect;
use navidog_db::sql::split_statements;
use navidog_db::table_design::{column_properties, split_field_list};
use navidog_util::sql_format::{Token, tokens};
use navidog_util::sql_text::find_params;

/// Every special character a consumer might mistake for syntax, inside one string literal.
const QUOTED: &str = "'x;y -- # /* $$ [$p] , AND (z)'";

fn dialects() -> [Dialect; 3] {
    [Dialect::MySql, Dialect::Postgres, Dialect::Sqlite]
}

#[test]
fn a_semicolon_inside_a_string_does_not_split_a_statement() {
    for dialect in dialects() {
        let sql = format!("SELECT {QUOTED}; SELECT 2");
        assert_eq!(
            split_statements(&sql, dialect),
            [format!("SELECT {QUOTED}"), "SELECT 2".to_string()],
            "{dialect:?}"
        );
    }
}

#[test]
fn a_comma_inside_a_string_does_not_split_a_field_list() {
    for dialect in dialects() {
        let list = format!("{QUOTED}, second");
        assert_eq!(
            split_field_list(&list, dialect).unwrap(),
            [QUOTED.trim(), "second"],
            "{dialect:?}"
        );
    }
}

#[test]
fn a_parameter_inside_a_string_is_not_a_parameter() {
    for dialect in dialects() {
        assert!(
            find_params(&format!("SELECT {QUOTED}"), dialect).is_empty(),
            "{dialect:?}"
        );
        assert_eq!(
            find_params("SELECT '[$a]', [$b]", dialect).len(),
            1,
            "{dialect:?}"
        );
    }
}

#[test]
fn a_join_inside_a_string_does_not_split_a_filter() {
    for dialect in dialects() {
        let filter = parse_where(&format!("name = {QUOTED} AND id = 1"), dialect).unwrap();
        assert_eq!(filter.rules.len(), 2, "{dialect:?}");
        assert_eq!(filter.rules[0].value, "x;y -- # /* $$ [$p] , AND (z)");
    }
}

#[test]
fn the_format_menu_keeps_a_string_whole() {
    for dialect in dialects() {
        let mysql = dialect == Dialect::MySql;
        // The editor's format menu only knows the MySQL flag; PostgreSQL and SQLite share the
        // generic rules, which also protect dollar bodies.
        let found = tokens(&format!("SELECT {QUOTED}"), mysql);
        let quoted = found
            .iter()
            .filter_map(|t| match t {
                Token::Quoted(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            quoted.iter().any(|text| text.contains("AND")),
            "{dialect:?}: {found:?}"
        );
    }
}

#[test]
fn a_definition_parser_sees_the_same_string() {
    for dialect in dialects() {
        let p = column_properties(&format!("TEXT COMMENT {QUOTED}"), dialect);
        assert_eq!(p.comment, "x;y -- # /* $$ [$p] , AND (z)", "{dialect:?}");
    }
}
