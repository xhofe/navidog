//! Structured edits replace only the requested SQL clause; unrecognized attributes survive.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnProperty {
    Type,
    Length,
    Scale,
    NotNull,
    Comment,
    Default,
    Collation,
    Charset,
    AutoIncrement,
    Unsigned,
    Generated,
    GeneratedExpression,
    GeneratedStorage,
    PrimaryKey,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ColumnProperties {
    pub data_type: String,
    pub length: String,
    pub scale: String,
    pub not_null: bool,
    pub primary_key: bool,
    pub generated: bool,
    pub generated_expression: String,
    pub generated_storage: String,
    pub comment: String,
    pub default: String,
    pub collation: String,
    pub charset: String,
    pub auto_increment: bool,
    pub unsigned: bool,
}

fn words(s: &str) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut depth, mut quote) = (0, 0, 0);
    let mut start = None;
    while i < b.len() {
        if quote != 0 {
            if b[i] == b'\\' {
                i += 2;
                continue;
            }
            if b[i] == quote {
                if b.get(i + 1) == Some(&quote) {
                    i += 2;
                    continue;
                }
                quote = 0;
            }
        } else {
            match b[i] {
                b'\'' | b'"' | b'`' => quote = b[i],
                b'(' => depth += 1,
                b')' => depth -= 1,
                c if c.is_ascii_whitespace() && depth == 0 => {
                    if let Some(a) = start.take() {
                        out.push((a, i));
                    }
                    i += 1;
                    continue;
                }
                _ => {}
            }
        }
        start.get_or_insert(i);
        i += 1;
    }
    if let Some(a) = start {
        out.push((a, s.len()));
    }
    out
}
fn clause(word: &str) -> bool {
    matches!(
        word.to_ascii_uppercase().as_str(),
        "NOT"
            | "NULL"
            | "DEFAULT"
            | "COMMENT"
            | "COLLATE"
            | "CHARACTER"
            | "AUTO_INCREMENT"
            | "AUTOINCREMENT"
            | "UNSIGNED"
            | "ZEROFILL"
            | "PRIMARY"
            | "UNIQUE"
            | "CHECK"
            | "REFERENCES"
            | "CONSTRAINT"
            | "GENERATED"
            | "AS"
            | "ON"
            | "IDENTITY"
            | "STORAGE"
            | "COLUMN_FORMAT"
            | "INVISIBLE"
            | "VISIBLE"
            | "BINARY"
            | "SRID"
            | "COMPRESSION"
    )
}
fn type_end(s: &str) -> usize {
    words(s)
        .into_iter()
        .skip(1)
        .find(|&(a, b)| clause(&s[a..b]))
        .map(|(a, _)| a)
        .unwrap_or(s.len())
}
fn span(s: &str, key: &str, values: usize) -> Option<(usize, usize)> {
    let w = words(s);
    let keys: Vec<_> = key.split_whitespace().collect();
    for i in 0..w.len() {
        if keys.iter().enumerate().all(|(j, k)| {
            w.get(i + j)
                .is_some_and(|&(a, b)| s[a..b].eq_ignore_ascii_case(k))
        }) {
            if key == "DEFAULT" && values == 1 {
                let first = i + 1;
                if first >= w.len() {
                    return None;
                }
                let end = (first + 1..w.len())
                    .find(|&j| {
                        clause(&s[w[j].0..w[j].1])
                            && !s[w[j].0..w[j].1].eq_ignore_ascii_case("NULL")
                    })
                    .map(|j| w[j].0)
                    .unwrap_or(s.len());
                return Some((w[i].0, s[..end].trim_end().len()));
            }
            let last = i + keys.len() + values - 1;
            if last < w.len() {
                return Some((w[i].0, w[last].1));
            }
        }
    }
    None
}
fn value(s: &str, key: &str) -> String {
    span(s, key, 1)
        .map(|(a, b)| s[a + key.len()..b].trim().to_string())
        .unwrap_or_default()
}
fn literal(s: &str) -> String {
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        s[1..s.len() - 1]
            .replace("''", "'")
            .replace("\\'", "'")
            .replace("\\\\", "\\")
    } else {
        s.to_string()
    }
}
fn generated(s: &str) -> Option<(usize, usize, String, String)> {
    let w = words(s);
    let i = w
        .iter()
        .position(|&(a, b)| s[a..b].eq_ignore_ascii_case("AS"))?;
    let &(a, b) = w.get(i + 1)?;
    let expr = s[a..b].strip_prefix('(')?.strip_suffix(')')?.to_string();
    let start = if i >= 2 && s[w[i - 2].0..w[i - 2].1].eq_ignore_ascii_case("GENERATED") {
        w[i - 2].0
    } else {
        w[i].0
    };
    let (end, storage) = if let Some(&(a, b)) = w.get(i + 2)
        && ["STORED", "VIRTUAL"]
            .iter()
            .any(|v| s[a..b].eq_ignore_ascii_case(v))
    {
        (b, s[a..b].to_string())
    } else {
        (b, String::new())
    };
    Some((start, end, expr, storage))
}
pub fn column_properties(s: &str) -> ColumnProperties {
    let ty = s[..type_end(s)].trim();
    let (data_type, length, scale) = if let Some(a) = ty.find('(') {
        let b = ty.rfind(')').unwrap_or(ty.len());
        let args = &ty[a + 1..b];
        // Enum/set values are a single editable length value, never split at literal commas.
        let split = parse::positions(args, b',').unwrap_or_default();
        let (length, scale) = if split.len() == 1 && !args.contains('\'') {
            (&args[..split[0]], &args[split[0] + 1..])
        } else {
            (args, "")
        };
        (
            format!("{}{}", ty[..a].trim(), ty.get(b + 1..).unwrap_or_default()),
            length.trim().to_string(),
            scale.trim().to_string(),
        )
    } else {
        (ty.to_string(), String::new(), String::new())
    };
    ColumnProperties {
        data_type,
        length,
        scale,
        not_null: span(s, "NOT NULL", 0).is_some(),
        primary_key: span(s, "PRIMARY KEY", 0).is_some(),
        generated: generated(s).is_some(),
        generated_expression: generated(s).map(|g| g.2).unwrap_or_default(),
        generated_storage: generated(s).map(|g| g.3).unwrap_or_default(),
        comment: literal(&value(s, "COMMENT")),
        default: value(s, "DEFAULT"),
        collation: value(s, "COLLATE"),
        charset: value(s, "CHARACTER SET"),
        auto_increment: span(s, "AUTO_INCREMENT", 0).is_some()
            || span(s, "AUTOINCREMENT", 0).is_some(),
        unsigned: span(s, "UNSIGNED", 0).is_some(),
    }
}

pub fn edit_column(
    s: &str,
    property: ColumnProperty,
    input: &str,
    dialect: Dialect,
) -> DbResult<String> {
    if property != ColumnProperty::Comment {
        parse::fragment(input)?;
    }
    let mut p = column_properties(s);
    if matches!(
        property,
        ColumnProperty::Type | ColumnProperty::Length | ColumnProperty::Scale
    ) {
        match property {
            ColumnProperty::Type => {
                p.data_type = input.trim().into();
                if [
                    "TEXT",
                    "BLOB",
                    "BYTEA",
                    "JSON",
                    "JSONB",
                    "BOOLEAN",
                    "DATE",
                    "INTEGER",
                    "BIGINT",
                    "SMALLINT",
                    "REAL",
                    "DOUBLE PRECISION",
                ]
                .iter()
                .any(|t| p.data_type.eq_ignore_ascii_case(t))
                {
                    p.length.clear();
                    p.scale.clear();
                }
            }
            ColumnProperty::Length => p.length = input.trim().into(),
            _ => p.scale = input.trim().into(),
        }
        if p.data_type.is_empty() {
            return Err(DbError::msg("A field needs a type."));
        }
        let args = if p.length.is_empty() {
            String::new()
        } else if p.scale.is_empty() {
            format!("({})", p.length)
        } else {
            format!("({},{})", p.length, p.scale)
        };
        let original_type = s[..type_end(s)].trim();
        let ty = if property != ColumnProperty::Type
            && let Some(a) = original_type.find('(')
            && let Some(b) = original_type.rfind(')')
        {
            // A length edit changes only the arguments, preserving array and timezone suffixes.
            format!("{}{args}{}", &original_type[..a], &original_type[b + 1..])
        } else {
            let upper = p.data_type.to_ascii_uppercase();
            let at = [
                upper.find(" WITH TIME ZONE"),
                upper.find(" WITHOUT TIME ZONE"),
                upper.find('['),
            ]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(p.data_type.len());
            format!("{}{args}{}", &p.data_type[..at], &p.data_type[at..])
        };
        return Ok(format!("{ty} {}", s[type_end(s)..].trim())
            .trim()
            .to_string());
    }
    if matches!(
        property,
        ColumnProperty::Generated
            | ColumnProperty::GeneratedExpression
            | ColumnProperty::GeneratedStorage
    ) {
        let old = generated(s);
        if old.is_none() && span(s, "GENERATED", 0).is_some() {
            return Err(DbError::msg(
                "Identity attributes cannot be changed as a generated expression.",
            ));
        }
        let remove = property == ColumnProperty::Generated && input == "false";
        let expression = if property == ColumnProperty::GeneratedExpression {
            input.to_string()
        } else {
            old.as_ref()
                .map(|g| g.2.clone())
                .unwrap_or_else(|| "0".into())
        };
        let storage = if property == ColumnProperty::GeneratedStorage {
            input.to_ascii_uppercase()
        } else {
            old.as_ref()
                .map(|g| g.3.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    if dialect == Dialect::Postgres {
                        "STORED"
                    } else {
                        "VIRTUAL"
                    }
                    .into()
                })
        };
        if !["STORED", "VIRTUAL"].contains(&storage.as_str()) {
            return Err(DbError::msg("Generated storage must be STORED or VIRTUAL."));
        }
        let replacement = if remove {
            String::new()
        } else {
            format!("GENERATED ALWAYS AS ({expression}) {storage}")
        };
        let mut result = s.to_string();
        if let Some((a, b, _, _)) = old {
            result.replace_range(a..b, &replacement);
        } else if !remove {
            result.push(' ');
            result.push_str(&replacement);
        }
        return Ok(result.trim().to_string());
    }
    let (key, count, replacement) = match property {
        ColumnProperty::PrimaryKey => (
            "PRIMARY KEY",
            0,
            if input == "true" {
                "PRIMARY KEY".into()
            } else {
                String::new()
            },
        ),
        ColumnProperty::NotNull => (
            "NOT NULL",
            0,
            if input == "true" {
                "NOT NULL".into()
            } else {
                String::new()
            },
        ),
        ColumnProperty::Comment => {
            if dialect != Dialect::MySql {
                return Err(DbError::msg(
                    "Column comments are edited through the database comment definition.",
                ));
            }
            (
                "COMMENT",
                1,
                if input.is_empty() {
                    String::new()
                } else {
                    format!(
                        "COMMENT '{}'",
                        input.replace('\\', "\\\\").replace('\'', "''")
                    )
                },
            )
        }
        ColumnProperty::Default => (
            "DEFAULT",
            1,
            if input.trim().is_empty() {
                String::new()
            } else {
                format!("DEFAULT {input}")
            },
        ),
        ColumnProperty::Collation => (
            "COLLATE",
            1,
            if input.trim().is_empty() {
                String::new()
            } else {
                format!("COLLATE {input}")
            },
        ),
        ColumnProperty::Charset => (
            "CHARACTER SET",
            1,
            if input.trim().is_empty() {
                String::new()
            } else {
                format!("CHARACTER SET {input}")
            },
        ),
        ColumnProperty::AutoIncrement => {
            let key = if dialect == Dialect::Sqlite {
                "AUTOINCREMENT"
            } else {
                "AUTO_INCREMENT"
            };
            (
                key,
                0,
                if input == "true" {
                    key.into()
                } else {
                    String::new()
                },
            )
        }
        ColumnProperty::Unsigned => (
            "UNSIGNED",
            0,
            if input == "true" {
                "UNSIGNED".into()
            } else {
                String::new()
            },
        ),
        _ => unreachable!(),
    };
    let mut out = s.to_string();
    if let Some((a, b)) = span(s, key, count) {
        out.replace_range(a..b, &replacement);
    } else if !replacement.is_empty() {
        if property == ColumnProperty::NotNull {
            let end = type_end(s);
            let tail = s[end..].trim();
            let tail = if tail.eq_ignore_ascii_case("NULL") {
                ""
            } else if tail
                .get(..5)
                .is_some_and(|v| v.eq_ignore_ascii_case("NULL "))
            {
                &tail[5..]
            } else {
                tail
            };
            out = format!("{} {replacement} {}", s[..end].trim(), tail);
        } else {
            out.push(' ');
            out.push_str(&replacement);
        }
    }
    Ok(out.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn postgres_default_cast_and_generated_clauses_are_not_truncated() {
        assert_eq!(
            column_properties("timestamp(6) with time zone NOT NULL").data_type,
            "timestamp with time zone"
        );
        assert_eq!(
            edit_column(
                "timestamp(6) with time zone NOT NULL",
                ColumnProperty::Length,
                "3",
                Dialect::Postgres
            )
            .unwrap(),
            "timestamp(3) with time zone NOT NULL"
        );
        assert_eq!(
            edit_column(
                "character varying(80)[] DEFAULT '{}'::character varying[]",
                ColumnProperty::Length,
                "100",
                Dialect::Postgres
            )
            .unwrap(),
            "character varying(100)[] DEFAULT '{}'::character varying[]"
        );
        let s = "character varying(80) NOT NULL DEFAULT 'before'::character varying";
        let edited = edit_column(s, ColumnProperty::Default, "'after'", Dialect::Postgres).unwrap();
        assert_eq!(edited, "character varying(80) NOT NULL DEFAULT 'after'");
        assert_eq!(
            edit_column(
                "TEXT DEFAULT 'a'",
                ColumnProperty::NotNull,
                "true",
                Dialect::Postgres
            )
            .unwrap(),
            "TEXT NOT NULL DEFAULT 'a'"
        );
        let s = "INTEGER GENERATED ALWAYS AS (length('a b')) STORED NOT NULL";
        let p = column_properties(s);
        assert!(p.generated);
        assert_eq!(p.generated_expression, "length('a b')");
        assert_eq!(p.generated_storage, "STORED");
        let edited = edit_column(
            s,
            ColumnProperty::GeneratedExpression,
            "id + 1",
            Dialect::Sqlite,
        )
        .unwrap();
        assert_eq!(
            edited,
            "INTEGER GENERATED ALWAYS AS (id + 1) STORED NOT NULL"
        );
    }
    #[test]
    fn structured_edits_preserve_unrelated_attributes_and_literals() {
        let s = "DECIMAL(10,2) UNSIGNED NOT NULL DEFAULT (round(1.5, 2)) COMMENT 'NOT NULL DEFAULT x' INVISIBLE";
        let p = column_properties(s);
        assert_eq!(p.length, "10");
        assert_eq!(p.scale, "2");
        assert_eq!(p.default, "(round(1.5, 2))");
        let edited = edit_column(s, ColumnProperty::Length, "20", Dialect::MySql).unwrap();
        assert!(edited.starts_with("DECIMAL(20,2)"));
        assert!(edited.ends_with("INVISIBLE"));
        let edited =
            edit_column(&edited, ColumnProperty::NotNull, "false", Dialect::MySql).unwrap();
        assert!(!column_properties(&edited).not_null);
        assert!(edited.contains("'NOT NULL DEFAULT x'"));
        let edited =
            edit_column(&edited, ColumnProperty::Comment, "O'Brien", Dialect::MySql).unwrap();
        assert_eq!(column_properties(&edited).comment, "O'Brien");
        assert!(
            edit_column(
                s,
                ColumnProperty::Default,
                "0; DROP TABLE x",
                Dialect::MySql
            )
            .is_err()
        );
        assert_eq!(
            column_properties("ENUM('a,b','c') NOT NULL").length,
            "'a,b','c'"
        );
    }
}
