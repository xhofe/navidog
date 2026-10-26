//! The dialect-aware SQL scanner.
//!
//! [`spans`] turns a script into an ordered list of non-whitespace spans: words and other
//! punctuation, strings, quoted identifiers, PostgreSQL dollar-quoted bodies, comments,
//! `[$name]` parameters, statement terminators and parentheses (with their nesting depth).
//! Every SQL consumer in the workspace reads this list instead of scanning the text again, so
//! the same SQL cannot be read two different ways.
//!
//! The scanner is lenient by default: an unterminated string, comment or parenthesis runs to the
//! end of the input and the spans still describe it. [`check`] reports those cases for callers
//! that must reject broken SQL (the table designer's definitions).

use std::ops::Range;

use crate::dialect::Dialect;

/// What a span is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind {
    /// An identifier or keyword: ASCII letters, digits and `_`
    Word,
    /// Numbers, operators and other punctuation outside the spans below
    Other,
    /// A string literal: `'…'` (MySQL escapes with a backslash; PostgreSQL `E'…'` too)
    String,
    /// A quoted identifier: `"…"`, `` `…` `` or SQLite `[…]`
    QuotedIdent,
    /// A PostgreSQL dollar-quoted body: `$$…$$` or `$tag$…$tag$`
    DollarQuoted,
    /// `-- …` or, in MySQL, `# …` up to the end of the line
    LineComment,
    /// `/* … */`; PostgreSQL nests them
    BlockComment,
    /// A navidog query parameter marker: `[$name]`
    Param,
    /// A MySQL `DELIMITER` line: it configures the scanner and is never sent to the server
    DelimiterLine,
    /// `(`
    OpenParen,
    /// `)`
    CloseParen,
    /// A statement terminator: `;` outside a routine body, or the current MySQL delimiter
    StatementEnd,
}

impl SpanKind {
    /// Whether the span counts as the statement's content: comments, `DELIMITER` lines and the
    /// terminator do not, so a statement made only of them is dropped.
    pub fn is_code(self) -> bool {
        !matches!(
            self,
            Self::LineComment | Self::BlockComment | Self::DelimiterLine | Self::StatementEnd
        )
    }

    pub fn is_comment(self) -> bool {
        matches!(self, Self::LineComment | Self::BlockComment)
    }

    /// Strings, quoted identifiers and dollar bodies: text the format menu must not touch.
    pub fn is_protected(self) -> bool {
        matches!(self, Self::String | Self::QuotedIdent | Self::DollarQuoted)
    }
}

/// One scanned span and its position in the script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Byte range in the scanned text
    pub range: Range<usize>,
    pub kind: SpanKind,
    /// Parenthesis nesting at the span's first byte; an `OpenParen` opens the next level and a
    /// `CloseParen` sits at the level it closes.
    pub depth: u32,
}

impl Span {
    pub fn text<'a>(&self, sql: &'a str) -> &'a str {
        &sql[self.range.clone()]
    }
}

/// Why [`check`] rejected a script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexError {
    UnclosedQuote,
    UnclosedComment,
    UnbalancedParentheses,
}

/// Every non-whitespace span of `sql`, in order. Never fails: unterminated spans run to the end.
pub fn spans(sql: &str, dialect: Dialect) -> Vec<Span> {
    Scanner::new(sql, dialect).run().0
}

/// Like [`spans`], but reports unterminated strings, comments and unbalanced parentheses.
pub fn check(sql: &str, dialect: Dialect) -> Result<(), LexError> {
    match Scanner::new(sql, dialect).run().1 {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Byte offsets where `delimiter` separates top-level text: outside strings, identifiers and
/// comments, and at parenthesis depth 0. `delimiter` must be ASCII.
pub fn separators(sql: &str, dialect: Dialect, delimiter: u8) -> Vec<usize> {
    let mut out = Vec::new();
    let mut depth = 0u32;
    let mut i = 0;
    let mut spans = spans(sql, dialect).into_iter().peekable();
    while i < sql.len() {
        let Some(span) = spans.peek() else {
            push_delimiters(&mut out, &sql[i..], i, delimiter);
            break;
        };
        if span.range.start > i {
            // Whitespace between spans (spans cover every other byte).
            if depth == 0 {
                push_delimiters(&mut out, &sql[i..span.range.start], i, delimiter);
            }
            i = span.range.start;
            continue;
        }
        match span.kind {
            SpanKind::OpenParen => depth = span.depth.saturating_add(1),
            SpanKind::CloseParen => depth = span.depth.saturating_sub(1),
            // Words and punctuation can hold a delimiter byte (`,`, for example).
            SpanKind::Word | SpanKind::Other if depth == 0 => {
                push_delimiters(&mut out, span.text(sql), span.range.start, delimiter);
            }
            _ => {}
        }
        i = span.range.end;
        spans.next();
    }
    out
}

fn push_delimiters(out: &mut Vec<usize>, text: &str, offset: usize, delimiter: u8) {
    out.extend(
        text.bytes()
            .enumerate()
            .filter(|(_, b)| *b == delimiter)
            .map(|(k, _)| offset + k),
    );
}

struct Scanner<'a> {
    sql: &'a str,
    bytes: &'a [u8],
    dialect: Dialect,
    spans: Vec<Span>,
    /// Parenthesis nesting
    depth: u32,
    /// `CREATE TRIGGER / FUNCTION / PROCEDURE / EVENT … BEGIN` nesting: a `;` inside the body
    /// does not end the statement
    block: u32,
    /// `CASE … END` nesting inside a routine body: that `END` closes the expression, not the
    /// routine, so it must not allow a statement boundary
    case: u32,
    /// The `CASE` of an `END CASE` terminator: that word is the terminator's name, not a new
    /// opener, so the next `CASE` is skipped.
    end_case: bool,
    /// Start of the current statement, the prefix `BEGIN` is classified against
    statement_start: usize,
    /// MySQL's current terminator; `None` is `;`
    delimiter: Option<String>,
    /// Whether the current statement has content yet (`DELIMITER` lines only appear before it)
    has_code: bool,
    error: Option<LexError>,
}

impl<'a> Scanner<'a> {
    fn new(sql: &'a str, dialect: Dialect) -> Self {
        Self {
            sql,
            bytes: sql.as_bytes(),
            dialect,
            spans: Vec::new(),
            depth: 0,
            block: 0,
            case: 0,
            end_case: false,
            statement_start: 0,
            delimiter: None,
            has_code: false,
            error: None,
        }
    }

    fn run(mut self) -> (Vec<Span>, Option<LexError>) {
        let mut i = 0;
        while i < self.bytes.len() {
            if self.dialect == Dialect::MySql
                && !self.has_code
                && at_line_start(self.bytes, i)
                && let Some((end, next)) = delimiter_line(self.sql, i)
            {
                self.spans.push(Span {
                    range: i..end,
                    kind: SpanKind::DelimiterLine,
                    depth: self.depth,
                });
                if let Some(next) = next {
                    self.delimiter = (next != ";").then_some(next);
                }
                // The line configures the scanner; a statement after it starts on the next one.
                self.statement_start = end;
                i = end;
                continue;
            }
            if let Some(delimiter) = self.delimiter.as_deref()
                && self.sql[i..].starts_with(delimiter)
            {
                let end = i + delimiter.len();
                self.push(i..end, SpanKind::StatementEnd);
                self.boundary(end);
                i = end;
                continue;
            }
            match self.bytes[i] {
                b'\'' => {
                    let escapes = self.dialect == Dialect::MySql
                        || (self.dialect == Dialect::Postgres && e_prefix(self.bytes, i));
                    i = self.quoted(i, b'\'', escapes, SpanKind::String);
                }
                b'"' => {
                    let (kind, escapes) = if self.dialect == Dialect::MySql {
                        (SpanKind::String, true)
                    } else {
                        (SpanKind::QuotedIdent, false)
                    };
                    i = self.quoted(i, b'"', escapes, kind);
                }
                b'`' => i = self.quoted(i, b'`', false, SpanKind::QuotedIdent),
                b'[' => i = self.bracket(i),
                b'-' if self.bytes.get(i + 1) == Some(&b'-') => i = self.line_comment(i),
                b'#' if self.dialect == Dialect::MySql => i = self.line_comment(i),
                b'/' if self.bytes.get(i + 1) == Some(&b'*') => i = self.block_comment(i),
                b'$' if self.dialect == Dialect::Postgres && is_dollar_quote_start(self.sql, i) => {
                    i = self.dollar_quoted(i);
                }
                b'(' => {
                    self.push(i..i + 1, SpanKind::OpenParen);
                    self.depth = self.depth.saturating_add(1);
                    i += 1;
                }
                b')' => {
                    if self.depth == 0 {
                        self.error(LexError::UnbalancedParentheses);
                    }
                    self.push(i..i + 1, SpanKind::CloseParen);
                    self.depth = self.depth.saturating_sub(1);
                    i += 1;
                }
                b';' if self.delimiter.is_none() && self.block == 0 => {
                    self.push(i..i + 1, SpanKind::StatementEnd);
                    i += 1;
                    self.boundary(i);
                }
                b if b.is_ascii_alphabetic() || b == b'_' => i = self.word(i),
                b if b.is_ascii_whitespace() => i += 1,
                _ => i = self.other(i),
            }
        }
        if self.depth > 0 {
            self.error(LexError::UnbalancedParentheses);
        }
        (self.spans, self.error)
    }

    fn push(&mut self, range: Range<usize>, kind: SpanKind) {
        if kind.is_code() {
            self.has_code = true;
        }
        self.spans.push(Span {
            range,
            kind,
            depth: self.depth,
        });
    }

    /// Resets the per-statement state after a terminator ending at `i`.
    fn boundary(&mut self, i: usize) {
        self.has_code = false;
        self.statement_start = i;
        self.block = 0;
        self.case = 0;
        self.end_case = false;
    }

    fn error(&mut self, error: LexError) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }

    /// A quoted span whose opening quote is at `start`; returns the index after its end.
    fn quoted(&mut self, start: usize, quote: u8, escapes: bool, kind: SpanKind) -> usize {
        let mut i = start + 1;
        while i < self.bytes.len() {
            match self.bytes[i] {
                b'\\' if escapes && quote != b'`' => i += 2,
                b if b == quote => {
                    if self.bytes.get(i + 1) == Some(&quote) {
                        i += 2;
                    } else {
                        i += 1;
                        self.push(start..i, kind);
                        return i;
                    }
                }
                _ => i += 1,
            }
        }
        self.push(start..self.bytes.len(), kind);
        self.error(LexError::UnclosedQuote);
        self.bytes.len()
    }

    /// `[` starts a `[$name]` parameter, a SQLite bracket identifier, or plain punctuation.
    fn bracket(&mut self, start: usize) -> usize {
        if self.bytes.get(start + 1) == Some(&b'$')
            && let Some(end) = param_end(self.sql, start)
        {
            self.push(start..end, SpanKind::Param);
            return end;
        }
        if self.dialect != Dialect::Sqlite {
            return self.other(start);
        }
        let mut i = start + 1;
        while i < self.bytes.len() {
            if self.bytes[i] == b']' {
                if self.bytes.get(i + 1) == Some(&b']') {
                    i += 2;
                    continue;
                }
                i += 1;
                self.push(start..i, SpanKind::QuotedIdent);
                return i;
            }
            i += 1;
        }
        self.push(start..self.bytes.len(), SpanKind::QuotedIdent);
        self.error(LexError::UnclosedQuote);
        self.bytes.len()
    }

    fn line_comment(&mut self, start: usize) -> usize {
        let end = self.sql[start..]
            .find('\n')
            .map_or(self.bytes.len(), |n| start + n);
        self.push(start..end, SpanKind::LineComment);
        end
    }

    fn block_comment(&mut self, start: usize) -> usize {
        let nested = self.dialect == Dialect::Postgres;
        let mut i = start + 2;
        let mut depth = 1usize;
        while i < self.bytes.len() {
            if self.bytes[i] == b'*' && self.bytes.get(i + 1) == Some(&b'/') {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    self.push(start..i, SpanKind::BlockComment);
                    return i;
                }
                continue;
            }
            if nested && self.bytes[i] == b'/' && self.bytes.get(i + 1) == Some(&b'*') {
                depth += 1;
                i += 2;
                continue;
            }
            i += 1;
        }
        self.push(start..self.bytes.len(), SpanKind::BlockComment);
        self.error(LexError::UnclosedComment);
        self.bytes.len()
    }

    fn dollar_quoted(&mut self, start: usize) -> usize {
        match dollar_end(self.sql, start) {
            Some(end) => {
                self.push(start..end, SpanKind::DollarQuoted);
                end
            }
            // `$1` opens nothing: it is punctuation, not an unterminated quote.
            None => self.other(start),
        }
    }

    fn word(&mut self, start: usize) -> usize {
        let mut i = start + 1;
        while i < self.bytes.len()
            && (self.bytes[i].is_ascii_alphanumeric() || self.bytes[i] == b'_')
        {
            i += 1;
        }
        let word = &self.sql[start..i];
        if self.delimiter.is_none() {
            if word.eq_ignore_ascii_case("BEGIN")
                && is_routine(&self.sql[self.statement_start..start])
            {
                self.block += 1;
            } else if word.eq_ignore_ascii_case("CASE") && self.block > 0 {
                // `END CASE` names the terminator of a CASE statement; only a real opener counts.
                if !std::mem::take(&mut self.end_case) {
                    self.case += 1;
                }
            } else if word.eq_ignore_ascii_case("END") {
                match next_word(&self.sql[i..], self.dialect) {
                    // `END IF`, `END LOOP`, … close the inner construct, not the routine.
                    Some(next)
                        if ["IF", "LOOP", "WHILE", "REPEAT"]
                            .iter()
                            .any(|k| next.eq_ignore_ascii_case(k)) => {}
                    // `END CASE` closes a CASE statement; the CASE that follows is its name.
                    Some(next) if next.eq_ignore_ascii_case("CASE") => {
                        self.case = self.case.saturating_sub(1);
                        self.end_case = true;
                    }
                    // A bare END closes a CASE expression when one is open, else the routine.
                    _ if self.case > 0 => self.case -= 1,
                    _ if self.block > 0 => self.block -= 1,
                    _ => {}
                }
            }
        }
        self.push(start..i, SpanKind::Word);
        i
    }

    /// Numbers, operators and punctuation: everything up to the next byte worth its own span.
    fn other(&mut self, start: usize) -> usize {
        let mut i = start;
        while i < self.bytes.len() {
            let b = self.bytes[i];
            let special = b.is_ascii_whitespace()
                || b.is_ascii_alphabetic()
                || b == b'_'
                || matches!(b, b'\'' | b'"' | b'`' | b'[' | b'(' | b')' | b';')
                || (b == b'-' && self.bytes.get(i + 1) == Some(&b'-'))
                || (b == b'/' && self.bytes.get(i + 1) == Some(&b'*'))
                || (b == b'#' && self.dialect == Dialect::MySql)
                || (b == b'$' && self.dialect == Dialect::Postgres);
            if special {
                break;
            }
            if let Some(delimiter) = self.delimiter.as_deref()
                && self.sql.is_char_boundary(i)
                && self.sql[i..].starts_with(delimiter)
            {
                break;
            }
            i += self.sql[i..].chars().next().map_or(1, char::len_utf8);
        }
        // A byte that is only special to the caller above (a stale `;` under a custom
        // delimiter, a PostgreSQL `$` that opens nothing) still needs a span of its own.
        if i == start {
            i += 1;
        }
        self.push(start..i, SpanKind::Other);
        i
    }
}

/// `sql[start]` begins a valid PostgreSQL dollar quote rather than a `$tag$` inside an
/// identifier such as `foo$tag$`.
pub fn is_dollar_quote_start(sql: &str, start: usize) -> bool {
    sql.as_bytes().get(start) == Some(&b'$') && !after_identifier(sql, start)
}

fn after_identifier(sql: &str, start: usize) -> bool {
    start > 0
        && matches!(
            sql.as_bytes()[start - 1],
            b if b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
        )
}

/// The end of a dollar-quoted string starting at `start`, or `None` when `start` is not a valid
/// opening tag (for example `$1`).
pub fn skip_dollar_quoted(sql: &str, start: usize) -> Option<usize> {
    dollar_end(sql, start)
}

fn dollar_end(sql: &str, start: usize) -> Option<usize> {
    let rest = &sql[start + 1..];
    let tag_len = rest.find('$')?;
    let tag = &rest[..tag_len];
    if !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || tag.starts_with(|c: char| c.is_ascii_digit())
    {
        return None;
    }
    let delim = &sql[start..start + tag_len + 2];
    let body = start + tag_len + 2;
    Some(
        sql[body..]
            .find(delim)
            .map_or(sql.len(), |n| body + n + delim.len()),
    )
}

/// The end of the `[$name]` marker at `start`, when it is one.
fn param_end(sql: &str, start: usize) -> Option<usize> {
    let body = &sql[start + 2..];
    let len = body.find(']')?;
    if len == 0 || body[..len].contains(['\n', '[']) {
        return None;
    }
    Some(start + 2 + len + 1)
}

/// `E'…'`: a PostgreSQL escape string, where a backslash escapes the next character.
fn e_prefix(bytes: &[u8], quote: usize) -> bool {
    quote > 0
        && bytes[quote - 1].eq_ignore_ascii_case(&b'e')
        && (quote < 2 || !(bytes[quote - 2].is_ascii_alphanumeric() || bytes[quote - 2] == b'_'))
}

fn is_routine(prefix: &str) -> bool {
    let p = prefix.trim_start().to_ascii_uppercase();
    p.starts_with("CREATE")
        && ["TRIGGER", "FUNCTION", "PROCEDURE", "EVENT"]
            .iter()
            .any(|k| p.contains(k))
}

/// The first word after `rest`, skipping whitespace and comments: the word that decides whether
/// an `END` closes an `IF`, names a `CASE` terminator or closes the routine itself.
fn next_word(rest: &str, dialect: Dialect) -> Option<&str> {
    let mut s = rest;
    loop {
        s = s.trim_start();
        if let Some(after) = s.strip_prefix("--") {
            s = after.find('\n').map_or("", |n| &after[n + 1..]);
        } else if dialect == Dialect::MySql
            && let Some(after) = s.strip_prefix('#')
        {
            s = after.find('\n').map_or("", |n| &after[n + 1..]);
        } else if let Some(after) = s.strip_prefix("/*") {
            s = after.find("*/").map_or("", |n| &after[n + 2..]);
        } else {
            break;
        }
    }
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    (end > 0).then(|| &s[..end])
}

fn at_line_start(bytes: &[u8], i: usize) -> bool {
    bytes[..i]
        .iter()
        .rev()
        .take_while(|b| **b != b'\n')
        .all(|b| b.is_ascii_whitespace())
}

/// A `DELIMITER <symbol>` line at `i`: where the line ends, and the new terminator (`None` when
/// the line names none).
fn delimiter_line(sql: &str, i: usize) -> Option<(usize, Option<String>)> {
    const KEYWORD: &str = "DELIMITER";
    let rest = &sql[i..];
    if !starts_with_ignore_ascii_case(rest, KEYWORD) {
        return None;
    }
    let after = &rest[KEYWORD.len()..];
    if !after.is_empty() && !after.starts_with([' ', '\t']) {
        return None;
    }
    let end = rest.find('\n').map_or(sql.len(), |n| i + n);
    let next = sql[i + KEYWORD.len()..end]
        .split_whitespace()
        .next()
        .map(String::from);
    Some((end, next))
}

/// `s` starts with `prefix`, ignoring ASCII case (byte-wise, so a multi-byte character at the cut
/// point cannot panic).
fn starts_with_ignore_ascii_case(s: &str, prefix: &str) -> bool {
    s.as_bytes()
        .get(..prefix.len())
        .is_some_and(|b| b.eq_ignore_ascii_case(prefix.as_bytes()))
}

#[cfg(test)]
mod tests;
