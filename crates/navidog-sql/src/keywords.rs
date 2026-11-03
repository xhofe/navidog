//! Keywords and built-in functions offered by code completion, per dialect (30-query-editor.md §4).

pub const COMMON: &[&str] = &[
    "ADD",
    "ALL",
    "ALTER",
    "AND",
    "ANY",
    "AS",
    "ASC",
    "BEGIN",
    "BETWEEN",
    "BY",
    "CASCADE",
    "CASE",
    "CAST",
    "CHECK",
    "COLUMN",
    "COMMIT",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "DATABASE",
    "DEFAULT",
    "DELETE",
    "DESC",
    "DISTINCT",
    "DROP",
    "ELSE",
    "END",
    "EXCEPT",
    "EXISTS",
    "EXPLAIN",
    "FALSE",
    "FOREIGN",
    "FROM",
    "FULL",
    "GRANT",
    "GROUP",
    "HAVING",
    "IF",
    "IN",
    "INDEX",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "LEFT",
    "LIKE",
    "LIMIT",
    "NATURAL",
    "NOT",
    "NULL",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "PRIMARY",
    "PROCEDURE",
    "REFERENCES",
    "RENAME",
    "REVOKE",
    "RIGHT",
    "ROLLBACK",
    "SAVEPOINT",
    "SELECT",
    "SET",
    "TABLE",
    "THEN",
    "TO",
    "TRANSACTION",
    "TRIGGER",
    "TRUE",
    "TRUNCATE",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USING",
    "VALUES",
    "VIEW",
    "WHEN",
    "WHERE",
    "WITH",
];

pub const COMMON_FUNCTIONS: &[&str] = &[
    "ABS", "AVG", "COALESCE", "COUNT", "LENGTH", "LOWER", "LTRIM", "MAX", "MIN", "NULLIF",
    "REPLACE", "ROUND", "RTRIM", "SUBSTR", "SUM", "TRIM", "UPPER",
];

/// The common list chained with a dialect's own, without building a `Vec` on every keystroke.
/// A named iterator (rather than `impl Iterator`) keeps `Dialect` usable as `dyn Dialect`.
pub struct Words<'a> {
    common: std::slice::Iter<'a, &'a str>,
    extra: std::slice::Iter<'a, &'a str>,
    owned: std::slice::Iter<'a, String>,
}

impl<'a> Words<'a> {
    pub fn owned(common: &'a [&'a str], extra: &'a [String]) -> Self {
        Self {
            common: common.iter(),
            extra: [].iter(),
            owned: extra.iter(),
        }
    }
    pub fn new(common: &'a [&'a str], extra: &'a [&'a str]) -> Self {
        Self {
            common: common.iter(),
            extra: extra.iter(),
            owned: [].iter(),
        }
    }
}

impl<'a> Iterator for Words<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.common
            .next()
            .or_else(|| self.extra.next())
            .copied()
            .or_else(|| self.owned.next().map(String::as_str))
    }
}
