use super::*;

fn kinds(sql: &str, dialect: Dialect) -> Vec<(SpanKind, &str)> {
    spans(sql, dialect)
        .iter()
        .map(|s| (s.kind, s.text(sql)))
        .collect()
}

fn ends(sql: &str, dialect: Dialect) -> Vec<&str> {
    spans(sql, dialect)
        .iter()
        .filter(|s| s.kind == SpanKind::StatementEnd)
        .map(|s| s.text(sql))
        .collect()
}

fn comments(sql: &str, dialect: Dialect) -> Vec<&str> {
    spans(sql, dialect)
        .iter()
        .filter(|s| s.kind.is_comment())
        .map(|s| s.text(sql))
        .collect()
}

#[test]
fn quotes_and_identifiers_follow_the_dialect() {
    // MySQL: backslash escapes in strings, `"` is also a string, `` ` `` names an identifier
    assert_eq!(
        kinds(r#"SELECT 'it\'s', "a""b", `a b`"#, Dialect::MySql),
        [
            (SpanKind::Word, "SELECT"),
            (SpanKind::String, r"'it\'s'"),
            (SpanKind::Other, ","),
            (SpanKind::String, r#""a""b""#),
            (SpanKind::Other, ","),
            (SpanKind::QuotedIdent, "`a b`"),
        ]
    );
    // PostgreSQL: a plain string takes the backslash literally, `E'…'` escapes with it
    assert_eq!(
        kinds(r"SELECT 'C:\', E'a\';b'", Dialect::Postgres),
        [
            (SpanKind::Word, "SELECT"),
            (SpanKind::String, r"'C:\'"),
            (SpanKind::Other, ","),
            (SpanKind::Word, "E"),
            (SpanKind::String, r"'a\';b'"),
        ]
    );
    // SQLite: `[x]` is an identifier and a backslash is literal
    assert_eq!(
        kinds(r"SELECT [a'b], 'C:\'", Dialect::Sqlite),
        [
            (SpanKind::Word, "SELECT"),
            (SpanKind::QuotedIdent, "[a'b]"),
            (SpanKind::Other, ","),
            (SpanKind::String, r"'C:\'"),
        ]
    );
    // `[$name]` wins over a bracket identifier in every dialect
    let sql = "SELECT [$x] FROM [t]";
    let found = spans(sql, Dialect::Sqlite);
    assert!(
        found
            .iter()
            .any(|s| s.kind == SpanKind::Param && s.text(sql) == "[$x]")
    );
    assert!(
        found
            .iter()
            .any(|s| s.kind == SpanKind::QuotedIdent && s.text(sql) == "[t]")
    );
    // ... but MySQL has no bracket identifiers
    assert!(
        !spans("[t]", Dialect::MySql)
            .iter()
            .any(|s| s.kind == SpanKind::QuotedIdent)
    );
}

#[test]
fn dollar_quotes_are_postgres_only_and_never_inside_identifiers() {
    let sql = "SELECT $$ [$x] $$, $tag$body$tag$, foo$tag$, $1";
    let found = spans(sql, Dialect::Postgres);
    let dollars: Vec<_> = found
        .iter()
        .filter(|s| s.kind == SpanKind::DollarQuoted)
        .map(|s| s.text(sql))
        .collect();
    assert_eq!(dollars, ["$$ [$x] $$", "$tag$body$tag$"]);
    assert!(
        !found.iter().any(|s| s.kind == SpanKind::Param),
        "a parameter marker inside a dollar body is text"
    );
    assert_eq!(
        check(sql, Dialect::Postgres),
        Ok(()),
        "`$1` is a parameter, not an unterminated quote"
    );
    assert!(is_dollar_quote_start(sql, 7));
    assert!(!is_dollar_quote_start(
        sql,
        sql.find("foo$tag$").unwrap() + 3
    ));
    // MySQL has no dollar quoting, so `[$x]` is a parameter there
    assert!(
        spans("SELECT $$ [$x] $$", Dialect::MySql)
            .iter()
            .any(|s| s.kind == SpanKind::Param)
    );
}

#[test]
fn comments_come_from_the_dialect() {
    assert_eq!(comments("SELECT 1 # c\n", Dialect::MySql), ["# c"]);
    assert!(comments("SELECT 1 # 2", Dialect::Postgres).is_empty());
    // MySQL does not nest block comments; PostgreSQL does
    assert_eq!(
        comments("/* a /* b */ c */", Dialect::MySql),
        ["/* a /* b */"]
    );
    assert_eq!(
        comments("/* a /* b */ c */", Dialect::Postgres),
        ["/* a /* b */ c */"]
    );
}

#[test]
fn statement_ends_skip_routine_bodies_and_custom_delimiters() {
    let sql =
        "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE a SET x = 1; SELECT 2; END;\nSELECT 3";
    assert_eq!(ends(sql, Dialect::MySql), [";"]);

    let sql = "DELIMITER $$\n\
               CREATE PROCEDURE p()\nBEGIN\n  SELECT 1;\n  SELECT 2;\nEND$$\n\
               DELIMITER ;\n\
               CALL p();";
    let found = ends(sql, Dialect::MySql);
    assert_eq!(found.len(), 2);
    assert_eq!(
        spans(sql, Dialect::MySql)
            .iter()
            .filter(|s| s.kind == SpanKind::DelimiterLine)
            .count(),
        2
    );
    // `DELIMITER` is a client command only in MySQL
    assert!(ends("DELIMITER //\nSELECT 1//", Dialect::Postgres).is_empty());
}

#[test]
fn separators_ignore_quotes_comments_and_groups() {
    let sql = "a, 'x,y', f(1, 2), -- c,\n b /* , */";
    assert_eq!(separators(sql, Dialect::MySql, b','), [1, 8, 17]);
    assert_eq!(
        separators("TEXT DEFAULT 'a b' NOT NULL", Dialect::Postgres, b' '),
        [4, 12, 18, 22]
    );
    assert_eq!(
        separators("decimal(10, 2)", Dialect::Postgres, b',').len(),
        0
    );
}

#[test]
fn check_reports_unterminated_and_unbalanced_text() {
    assert_eq!(
        check("SELECT 'x", Dialect::Postgres),
        Err(LexError::UnclosedQuote)
    );
    assert_eq!(check("SELECT 'x' -- c", Dialect::Postgres), Ok(()));
    assert_eq!(
        check("/* x", Dialect::MySql),
        Err(LexError::UnclosedComment)
    );
    assert_eq!(
        check("f(1", Dialect::Postgres),
        Err(LexError::UnbalancedParentheses)
    );
    assert_eq!(
        check("f(1))", Dialect::Postgres),
        Err(LexError::UnbalancedParentheses)
    );
    assert_eq!(check("[x", Dialect::Sqlite), Err(LexError::UnclosedQuote));
    assert_eq!(check("SELECT 1", Dialect::Sqlite), Ok(()));
}

#[test]
fn depth_tracks_parentheses() {
    let sql = "f(g(1), 2)";
    let found: Vec<_> = spans(sql, Dialect::Sqlite)
        .into_iter()
        .filter(|s| matches!(s.kind, SpanKind::OpenParen | SpanKind::CloseParen))
        .map(|s| (s.kind, s.depth))
        .collect();
    assert_eq!(
        found,
        [
            (SpanKind::OpenParen, 0),
            (SpanKind::OpenParen, 1),
            (SpanKind::CloseParen, 2),
            (SpanKind::CloseParen, 1),
        ]
    );
}
