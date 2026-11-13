use super::*;
use navidog_sql::span::{self, LexError, SpanKind};

/// Top-level separators, ignoring quoted text, SQL comments and nested expressions.
pub(super) fn positions(text: &str, delimiter: u8, dialect: &dyn Dialect) -> DbResult<Vec<usize>> {
    validate(text, dialect)?;
    Ok(span::separators(text, dialect.lex(), delimiter))
}

/// Rejects unterminated strings and comments and unbalanced parentheses.
fn validate(text: &str, dialect: &dyn Dialect) -> DbResult<()> {
    span::check(text, dialect.lex()).map_err(|error| {
        DbError::msg(match error {
            LexError::UnclosedQuote => "Unclosed SQL quote.",
            LexError::UnclosedComment => "Unclosed SQL comment.",
            LexError::UnbalancedParentheses => "Unbalanced SQL parentheses.",
        })
    })
}

/// One SQL definition: no extra statements, no comments and no dollar quotes.
pub(super) fn fragment(text: &str, dialect: &dyn Dialect) -> DbResult<()> {
    if text.contains('\0') {
        return Err(DbError::msg(
            "A SQL definition cannot contain extra statements.",
        ));
    }
    validate(text, dialect)?;
    let spans = span::spans(text, dialect.lex());
    if spans.iter().any(|s| s.kind == SpanKind::StatementEnd) {
        return Err(DbError::msg(
            "A SQL definition cannot contain extra statements.",
        ));
    }
    if spans.iter().any(|s| s.kind.is_comment()) {
        return Err(DbError::msg(
            "SQL comments are not supported in definitions.",
        ));
    }
    if spans.iter().any(|s| s.kind == SpanKind::DollarQuoted) {
        return Err(DbError::msg(
            "Dollar quotes are not supported in definitions.",
        ));
    }
    Ok(())
}

pub(super) fn ident(input: &str) -> DbResult<(String, &str)> {
    let input = input.trim_start();
    let Some(first) = input.chars().next() else {
        return Err(DbError::msg("Missing SQL identifier."));
    };
    if matches!(first, '`' | '"' | '[') {
        let end = if first == '[' { ']' } else { first };
        let mut name = String::new();
        let mut chars = input.char_indices().skip(1).peekable();
        while let Some((i, c)) = chars.next() {
            if c == end {
                if chars.peek().is_some_and(|(_, n)| *n == end) {
                    chars.next();
                    name.push(end);
                } else {
                    return Ok((name, input[i + c.len_utf8()..].trim_start()));
                }
            } else {
                name.push(c);
            }
        }
        return Err(DbError::msg("Unclosed identifier."));
    }
    let len = input
        .find(|c: char| c.is_whitespace() || c == '(')
        .unwrap_or(input.len());
    if len == 0 {
        return Err(DbError::msg("Missing SQL identifier."));
    }
    Ok((input[..len].into(), input[len..].trim_start()))
}

fn partition_clause(sql: &str, spans: &[span::Span]) -> Option<(usize, String)> {
    let mut tokens = spans
        .iter()
        .filter(|token| {
            // Executable MySQL comments are not whitespace. Handle the server's outer wrapper
            // separately instead of silently removing one between the keywords.
            let text = token.text(sql);
            !token.kind.is_comment() || text.starts_with("/*!") || text.starts_with("/*M!")
        })
        .peekable();
    while let Some(partition) = tokens.next() {
        if partition.depth == 0
            && partition.kind == SpanKind::Word
            && partition.text(sql).eq_ignore_ascii_case("PARTITION")
            && let Some(by) = tokens.peek()
            && by.depth == 0
            && by.kind == SpanKind::Word
            && by.text(sql).eq_ignore_ascii_case("BY")
        {
            let start = partition.range.start;
            let between = &sql[partition.range.end..by.range.start];
            let clause = if between.trim().is_empty() {
                sql[start..].to_owned()
            } else {
                // The designer accepts no SQL comments in a saved partition draft.
                format!("PARTITION {}", &sql[by.range.start..])
            };
            return Some((start, clause));
        }
    }
    None
}

fn version_comment_body<'a>(options: &'a str, token: &span::Span) -> Option<&'a str> {
    if token.kind != SpanKind::BlockComment || token.depth != 0 {
        return None;
    }
    let body = token
        .text(options)
        .strip_prefix("/*!")?
        .strip_suffix("*/")?;
    Some(body.trim_start_matches(|c: char| c.is_ascii_digit()).trim())
}

fn split_partition<'a>(options: &'a str, d: &dyn Dialect) -> (&'a str, String) {
    if !d.features().supports_partitions {
        return (options, String::new());
    }
    let spans = span::spans(options, d.lex());
    if let Some((offset, clause)) = partition_clause(options, &spans) {
        return (options[..offset].trim_end(), clause);
    }
    // MySQL SHOW CREATE TABLE wraps the clause in one or more trailing version comments.
    if d.table_option_style() == navidog_sql::TableOptionStyle::Clauses {
        let mut parts: Vec<&str> = Vec::new();
        for token in spans.iter().rev() {
            let Some(body) = version_comment_body(options, token) else {
                break;
            };
            if let Some((0, mut clause)) = partition_clause(body, &span::spans(body, d.lex())) {
                for part in parts.into_iter().rev() {
                    clause.push(' ');
                    clause.push_str(part);
                }
                return (options[..token.range.start].trim_end(), clause);
            }
            parts.push(body);
        }
    }
    (options, String::new())
}

pub(super) fn create(ddl: &str, name: &str, d: &dyn Dialect) -> DbResult<TableDesign> {
    let statements = navidog_db::sql::split_script(ddl, d.lex());
    let sql = statements
        .first()
        .ok_or_else(|| DbError::msg("No CREATE TABLE definition returned."))?
        .text
        .trim();
    let upper = sql.to_uppercase();
    let unlogged = upper.starts_with("CREATE UNLOGGED TABLE ");
    if !upper.starts_with("CREATE TABLE ") && !unlogged {
        return Err(DbError::msg("Unsupported table definition."));
    }
    let rest = sql[if unlogged { 22 } else { 13 }..].trim_start();
    // Find the body while skipping quoted table/schema names.
    let (_, mut rest) = ident(rest)?;
    if rest.starts_with('.') {
        (_, rest) = ident(&rest[1..])?;
    }
    let start = rest
        .find('(')
        .ok_or_else(|| DbError::msg("Missing table body."))?;
    let body_and_options = &rest[start + 1..];
    // Try closing parentheses from the end; the body must be balanced.
    let end = body_and_options
        .char_indices()
        .filter(|(_, c)| *c == ')')
        .map(|(i, _)| i)
        .find(|i| positions(&body_and_options[..*i], b',', d).is_ok())
        .ok_or_else(|| DbError::msg("Invalid table body."))?;
    let body = &body_and_options[..end];
    let (options, partition) = split_partition(body_and_options[end + 1..].trim(), d);
    let mut design = TableDesign {
        name: name.into(),
        unlogged,
        options: options.into(),
        partition,
        ..Default::default()
    };
    let mut begin = 0;
    for finish in positions(body, b',', d)?
        .into_iter()
        .chain(std::iter::once(body.len()))
    {
        let part = body[begin..finish].trim();
        begin = finish + 1;
        let (head, rest) = ident(part)?;
        let constraint = !part.starts_with(['`', '"', '['])
            && matches!(
                head.to_uppercase().as_str(),
                "CONSTRAINT"
                    | "PRIMARY"
                    | "UNIQUE"
                    | "FOREIGN"
                    | "CHECK"
                    | "KEY"
                    | "INDEX"
                    | "FULLTEXT"
                    | "SPATIAL"
                    | "EXCLUDE"
            );
        if constraint {
            design.constraints.push(part.into());
        } else {
            let mut c = DesignColumn::new(design.columns.len() as u64 + 1, head.clone(), rest);
            c.original = Some(head);
            design.columns.push(c);
        }
    }
    for s in statements.iter().skip(1) {
        design.indexes.push(s.text.clone());
    }
    Ok(design)
}
