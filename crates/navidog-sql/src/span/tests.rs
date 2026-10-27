use super::*;

fn mysql_lex() -> Lex {
    Lex::MYSQL
}

fn postgres_lex() -> Lex {
    Lex::POSTGRES
}

fn sqlite_lex() -> Lex {
    Lex::SQLITE
}

fn kinds(sql: &str, lex: Lex) -> Vec<(SpanKind, &str)> {
    spans(sql, lex)
        .iter()
        .map(|s| (s.kind, s.text(sql)))
        .collect()
}

fn ends(sql: &str, lex: Lex) -> Vec<&str> {
    spans(sql, lex)
        .iter()
        .filter(|s| s.kind == SpanKind::StatementEnd)
        .map(|s| s.text(sql))
        .collect()
}

fn comments(sql: &str, lex: Lex) -> Vec<&str> {
    spans(sql, lex)
        .iter()
        .filter(|s| s.kind.is_comment())
        .map(|s| s.text(sql))
        .collect()
}

#[test]
fn quotes_and_identifiers_follow_the_dialect() {
    // MySQL: backslash escapes in strings, `"` is also a string, `` ` `` names an identifier
    assert_eq!(
        kinds(r#"SELECT 'it\'s', "a""b", `a b`"#, mysql_lex()),
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
        kinds(r"SELECT 'C:\', E'a\';b'", postgres_lex()),
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
        kinds(r"SELECT [a'b], 'C:\'", sqlite_lex()),
        [
            (SpanKind::Word, "SELECT"),
            (SpanKind::QuotedIdent, "[a'b]"),
            (SpanKind::Other, ","),
            (SpanKind::String, r"'C:\'"),
        ]
    );
    // `[$name]` wins over a bracket identifier in every dialect
    let sql = "SELECT [$x] FROM [t]";
    let found = spans(sql, sqlite_lex());
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
        !spans("[t]", mysql_lex())
            .iter()
            .any(|s| s.kind == SpanKind::QuotedIdent)
    );
}

#[test]
fn dollar_quotes_are_postgres_only_and_never_inside_identifiers() {
    let sql = "SELECT $$ [$x] $$, $tag$body$tag$, foo$tag$, $1";
    let found = spans(sql, postgres_lex());
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
        check(sql, postgres_lex()),
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
        spans("SELECT $$ [$x] $$", mysql_lex())
            .iter()
            .any(|s| s.kind == SpanKind::Param)
    );
}

#[test]
fn comments_come_from_the_dialect() {
    assert_eq!(comments("SELECT 1 # c\n", mysql_lex()), ["# c"]);
    assert!(comments("SELECT 1 # 2", postgres_lex()).is_empty());
    // MySQL does not nest block comments; PostgreSQL does
    assert_eq!(comments("/* a /* b */ c */", mysql_lex()), ["/* a /* b */"]);
    assert_eq!(
        comments("/* a /* b */ c */", postgres_lex()),
        ["/* a /* b */ c */"]
    );
}

#[test]
fn statement_ends_skip_routine_bodies_and_custom_delimiters() {
    let sql =
        "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE a SET x = 1; SELECT 2; END;\nSELECT 3";
    assert_eq!(ends(sql, mysql_lex()), [";"]);

    let sql = "DELIMITER $$\n\
               CREATE PROCEDURE p()\nBEGIN\n  SELECT 1;\n  SELECT 2;\nEND$$\n\
               DELIMITER ;\n\
               CALL p();";
    let found = ends(sql, mysql_lex());
    assert_eq!(found.len(), 2);
    assert_eq!(
        spans(sql, mysql_lex())
            .iter()
            .filter(|s| s.kind == SpanKind::DelimiterLine)
            .count(),
        2
    );
    // `DELIMITER` is a client command only in MySQL
    assert!(ends("DELIMITER //\nSELECT 1//", postgres_lex()).is_empty());
}

#[test]
fn statement_ends_ignore_case_expressions_inside_routines() {
    // `CASE … END` closes the expression, so the following `;` stays inside the routine.
    let sql = "CREATE PROCEDURE p() BEGIN SELECT CASE WHEN 1 THEN 2 END; SELECT 2; END;";
    assert_eq!(
        ends(sql, mysql_lex()).len(),
        1,
        "the routine is one statement"
    );
    // Nested CASE expressions.
    let nested =
        "CREATE PROCEDURE p() BEGIN SELECT CASE WHEN 1 THEN CASE WHEN 2 THEN 3 END END; END;";
    assert_eq!(ends(nested, mysql_lex()).len(), 1);
    // A CASE statement ends with `END CASE`; the trailing CASE is not a new opener.
    let statement = "CREATE PROCEDURE p() BEGIN CASE 1 WHEN 1 THEN SELECT 1; END CASE; END;";
    assert_eq!(ends(statement, mysql_lex()).len(), 1);
    // A CASE branch may hold an IF statement: `END IF` must not close the CASE nesting.
    let branch = "CREATE PROCEDURE p() BEGIN CASE 1 WHEN 1 THEN IF 1 THEN SELECT 1; END IF; \
                  END CASE; SELECT 2; END;";
    assert_eq!(ends(branch, mysql_lex()).len(), 1);
    // Comments between END and CASE do not change the terminator.
    let comment = "CREATE PROCEDURE p() BEGIN CASE 1 WHEN 1 THEN SELECT 1; END /* note */ CASE; \
                   SELECT 2; END;";
    assert_eq!(ends(comment, mysql_lex()).len(), 1);
    // `END IF` still does not close the routine.
    assert_eq!(
        ends(
            "CREATE PROCEDURE p() BEGIN IF 1 THEN SELECT 1; END IF; END;",
            mysql_lex()
        )
        .len(),
        1
    );
    // Outside a routine a CASE expression is plain text and the statement ends at its semicolon.
    assert_eq!(
        ends("SELECT CASE WHEN 1 THEN 2 END; SELECT 2", mysql_lex()).len(),
        1
    );
}

#[test]
fn separators_ignore_quotes_comments_and_groups() {
    let sql = "a, 'x,y', f(1, 2), -- c,\n b /* , */";
    assert_eq!(separators(sql, mysql_lex(), b','), [1, 8, 17]);
    assert_eq!(
        separators("TEXT DEFAULT 'a b' NOT NULL", postgres_lex(), b' '),
        [4, 12, 18, 22]
    );
    assert_eq!(separators("decimal(10, 2)", postgres_lex(), b',').len(), 0);
}

#[test]
fn check_reports_unterminated_and_unbalanced_text() {
    assert_eq!(
        check("SELECT 'x", postgres_lex()),
        Err(LexError::UnclosedQuote)
    );
    assert_eq!(check("SELECT 'x' -- c", postgres_lex()), Ok(()));
    assert_eq!(check("/* x", mysql_lex()), Err(LexError::UnclosedComment));
    assert_eq!(
        check("f(1", postgres_lex()),
        Err(LexError::UnbalancedParentheses)
    );
    assert_eq!(
        check("f(1))", postgres_lex()),
        Err(LexError::UnbalancedParentheses)
    );
    assert_eq!(check("[x", sqlite_lex()), Err(LexError::UnclosedQuote));
    assert_eq!(check("SELECT 1", sqlite_lex()), Ok(()));
}

#[test]
fn depth_tracks_parentheses() {
    let sql = "f(g(1), 2)";
    let found: Vec<_> = spans(sql, sqlite_lex())
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
