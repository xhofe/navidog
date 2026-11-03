//! Finite SQL templates use the shared scanner and typed, host-escaped parameters.

use crate::{Dialect, Lex, span};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    Identifier,
    Literal,
    Fragment,
    Integer,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SqlTemplate {
    sql: String,
    parameters: BTreeMap<String, Encoding>,
}
impl SqlTemplate {
    pub fn new(sql: String, parameters: BTreeMap<String, Encoding>) -> Result<Self, String> {
        let template = Self { sql, parameters };
        template.validate()?;
        Ok(template)
    }
    pub fn parameter_names(&self) -> impl Iterator<Item = &str> {
        self.parameters.keys().map(String::as_str)
    }
    pub fn validate(&self) -> Result<(), String> {
        for key in self.parameters.keys() {
            if ![
                "table",
                "name",
                "old_name",
                "column",
                "definition",
                "body",
                "fields",
                "constraint",
                "index",
                "options",
                "comment",
                "position",
                "limit",
                "offset",
                "schema",
                "database",
                "value",
            ]
            .contains(&key.as_str())
                && !key.starts_with("field.")
            {
                return Err(format!("Unknown SQL template parameter: {key}"));
            }
        }
        let mut used = std::collections::BTreeSet::new();
        for part in self.sql.split("${").skip(1) {
            let (key, _) = part
                .split_once('}')
                .ok_or("Unterminated SQL template parameter.")?;
            if !self.parameters.contains_key(key) {
                return Err(format!("Undeclared SQL template parameter: {key}"));
            }
            used.insert(key.to_string());
        }
        if used.len() != self.parameters.len() {
            return Err("SQL template declares an unused parameter.".into());
        }
        Ok(())
    }
    pub fn render(
        &self,
        dialect: &dyn Dialect,
        values: &BTreeMap<String, String>,
    ) -> Result<String, String> {
        self.validate()?;
        let mut encoded = BTreeMap::new();
        for (key, encoding) in &self.parameters {
            let value = values
                .get(key)
                .ok_or_else(|| format!("Missing SQL template parameter: {key}"))?;
            let value = match encoding {
                Encoding::Identifier => dialect.quote_ident(value),
                Encoding::Literal => dialect.string_literal(value),
                Encoding::Integer => value
                    .parse::<u64>()
                    .map_err(|_| "Expected an unsigned integer.")?
                    .to_string(),
                Encoding::Fragment => {
                    fragment(value, dialect.lex())?;
                    value.clone()
                }
            };
            encoded.insert(key.clone(), value);
        }
        let mut sql = String::new();
        let mut rest = self.sql.as_str();
        while let Some((prefix, tail)) = rest.split_once("${") {
            sql.push_str(prefix);
            let (key, tail) = tail
                .split_once('}')
                .ok_or("Unterminated SQL template parameter.")?;
            sql.push_str(encoded.get(key).ok_or("Unknown SQL template parameter.")?);
            rest = tail;
        }
        sql.push_str(rest);
        span::check(&sql, dialect.lex()).map_err(|e| format!("Invalid SQL: {e:?}"))?;
        if !crate::definition::single_statement(&sql, dialect.lex()) {
            return Err("A template must produce exactly one SQL statement.".into());
        }
        Ok(sql)
    }
}
pub fn fragment(sql: &str, lex: Lex) -> Result<(), String> {
    span::check(sql, lex).map_err(|e| format!("Invalid SQL: {e:?}"))?;
    if span::spans(sql, lex).iter().any(|s| {
        matches!(
            s.kind,
            span::SpanKind::StatementEnd
                | span::SpanKind::LineComment
                | span::SpanKind::BlockComment
        )
    }) {
        return Err("SQL fragments cannot contain statement separators or comments.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declared::{DeclaredDialect, DialectDescriptor};
    #[test]
    fn typed_parameters_escape_values_and_reject_unknown_or_injected_fragments() {
        let d = DeclaredDialect::new(DialectDescriptor {
            id: "test".into(),
            ..Default::default()
        })
        .unwrap();
        let template = SqlTemplate::new(
            "ALTER TABLE ${table} ADD COLUMN ${column} ${definition}".into(),
            BTreeMap::from([
                ("table".into(), Encoding::Identifier),
                ("column".into(), Encoding::Identifier),
                ("definition".into(), Encoding::Fragment),
            ]),
        )
        .unwrap();
        let mut values = BTreeMap::from([
            ("table".into(), "t\"x".into()),
            ("column".into(), "id".into()),
            ("definition".into(), "TEXT DEFAULT ';'".into()),
        ]);
        assert_eq!(
            template.render(&d, &values).unwrap(),
            "ALTER TABLE \"t\"\"x\" ADD COLUMN \"id\" TEXT DEFAULT ';'"
        );
        values.insert("definition".into(), "TEXT; DROP TABLE t".into());
        assert!(template.render(&d, &values).is_err());
        assert!(
            SqlTemplate::new(
                "${password}".into(),
                BTreeMap::from([("password".into(), Encoding::Literal)])
            )
            .is_err()
        );
    }
}
