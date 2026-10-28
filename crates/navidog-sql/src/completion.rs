//! Ranked SQL completion shared by the query editor, console, filter text, snippet editor and
//! table designer (30-query-editor.md §4). Callers translate [`Kind`] into their own locale.
//!
//! After `table.` or `alias.` only that table's columns are offered; after `schema.` its tables.
//! ponytail: aliases come from `FROM`/`JOIN` in the current statement only, without sub-query
//! scopes; a syntax tree would handle those.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

use crate::dialect::Dialect;

/// What a completion row is. The UI crate turns this into the menu's detail text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Column,
    Table,
    View,
    Schema,
    Database,
    Function,
    Builtin,
    Keyword,
    Snippet,
    /// A designer type name. Inserted as given, not quoted.
    Type,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub label: String,
    pub insert: String,
    pub kind: Kind,
    /// Column type, shown by the caller after the kind label. Empty otherwise.
    pub type_name: String,
}

impl Hit {
    fn column(label: String, insert: String, type_name: String) -> Self {
        Self {
            label,
            insert,
            kind: Kind::Column,
            type_name,
        }
    }

    fn plain(label: impl Into<String>, insert: impl Into<String>, kind: Kind) -> Self {
        Self {
            label: label.into(),
            insert: insert.into(),
            kind,
            type_name: String::new(),
        }
    }
}

/// One table in a borrowed catalog. The strings live in the caller's outline or draft.
pub struct Table<'a> {
    pub name: &'a str,
    pub view: bool,
    pub columns: &'a [(String, String)],
}

pub struct Catalog<'a> {
    pub schemas: &'a [String],
    pub tables: &'a [Table<'a>],
    pub functions: &'a [String],
}

/// A catalog copied so a completion call can return it from a callback.
#[derive(Clone, Debug, Default)]
pub struct OwnedTable {
    pub name: String,
    pub view: bool,
    pub columns: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct OwnedCatalog {
    pub schemas: Vec<String>,
    pub tables: Vec<OwnedTable>,
    pub functions: Vec<String>,
}

/// Which keywords a field offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keywords {
    /// The dialect's full list, including statement words such as `SELECT`.
    All,
    /// Words used inside a condition or `ORDER BY`. No statement keywords.
    Clause,
    /// No keywords.
    None,
}

/// Shared by the filter and by column expressions. Not split per dialect.
pub const CLAUSE_KEYWORDS: &[&str] = &[
    "AND", "OR", "NOT", "IS", "NULL", "IN", "LIKE", "BETWEEN", "EXISTS", "CASE", "WHEN", "THEN",
    "ELSE", "END", "TRUE", "FALSE", "ASC", "DESC",
];

pub struct Sources<'a> {
    pub dialect: &'a dyn Dialect,
    /// Database (PostgreSQL: schema) the catalog belongs to.
    pub current: Option<&'a str>,
    pub catalog: Option<Catalog<'a>>,
    /// Catalogs of other databases or schemas that are already loaded.
    pub others: &'a dyn Fn(&str) -> Option<OwnedCatalog>,
    /// Snippet name and the code it inserts.
    pub snippets: &'a [(String, String)],
    /// Columns that are in scope even when the text has no `FROM` (the open table, a draft).
    pub in_scope: &'a [(String, String)],
    /// Extra rows merged in before ranking. Type names pass their spelling as `insert`.
    pub extras: &'a [Hit],
    pub keywords: Keywords,
}

pub fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// An identifier character or `.`, the keystrokes that open the menu.
pub fn is_trigger(new_text: &str) -> bool {
    !new_text.is_empty() && new_text.chars().all(|c| is_ident(c) || c == '.')
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    for q in ['`', '"'] {
        if let Some(inner) = t.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner.to_string();
        }
    }
    t.to_string()
}

static ALIAS: LazyLock<Regex> = LazyLock::new(|| {
    let ident = r#"(?:[\w$]+|"[^"]+"|`[^`]+`)"#;
    Regex::new(&format!(
        r"(?i)\b(?:from|join|update|into)\s+({ident}(?:\s*\.\s*{ident})?)(?:\s+(?:as\s+)?({ident}))?"
    ))
    .expect("alias pattern")
});

/// Table names and aliases in the statement around `offset`, alias (or table) → table.
fn aliases(statement: &str, dialect: &dyn Dialect) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for caps in ALIAS.captures_iter(statement) {
        let full = caps[1].to_string();
        let table = unquote(full.rsplit('.').next().unwrap_or(&full));
        out.insert(table.to_lowercase(), table.clone());
        if let Some(a) = caps.get(2) {
            let alias = unquote(a.as_str());
            if !dialect.is_keyword(&alias) {
                out.insert(alias.to_lowercase(), table);
            }
        }
    }
    out
}

fn find_table<'a>(tables: &'a [Table<'a>], name: &str) -> Option<&'a Table<'a>> {
    tables
        .iter()
        .find(|t| t.name == name)
        .or_else(|| tables.iter().find(|t| t.name.eq_ignore_ascii_case(name)))
}

fn column_hit(dialect: &dyn Dialect, name: &str, ty: &str) -> Hit {
    Hit::column(name.to_string(), dialect.typed_name(name), ty.to_string())
}

fn tables_of(dialect: &dyn Dialect, tables: &[Table<'_>]) -> Vec<Hit> {
    tables
        .iter()
        .map(|t| {
            Hit::plain(
                t.name,
                dialect.typed_name(t.name),
                if t.view { Kind::View } else { Kind::Table },
            )
        })
        .collect()
}

fn owned_tables(owned: &OwnedCatalog) -> Vec<Table<'_>> {
    owned
        .tables
        .iter()
        .map(|t| Table {
            name: &t.name,
            view: t.view,
            columns: &t.columns,
        })
        .collect()
}

/// The word being typed and what to offer for it.
pub fn complete(text: &str, offset: usize, src: &Sources) -> (Range<usize>, Vec<Hit>) {
    let before = &text[..offset];
    let word_len: usize = before
        .chars()
        .rev()
        .take_while(|c| is_ident(*c))
        .map(char::len_utf8)
        .sum();
    let start = offset - word_len;
    let word = &text[start..offset];
    let stmt_start = text[..start].rfind(';').map_or(0, |i| i + 1);
    let stmt_end = text[offset..].find(';').map_or(text.len(), |i| offset + i);
    let statement = &text[stmt_start..stmt_end];
    let known = aliases(statement, src.dialect);
    let mut out: Vec<Hit> = Vec::new();
    let qualifier = before[..start].strip_suffix('.').map(|q| {
        let len: usize = q
            .chars()
            .rev()
            .take_while(|c| is_ident(*c) || *c == '`' || *c == '"')
            .map(char::len_utf8)
            .sum();
        unquote(&q[q.len() - len..])
    });
    if let Some(q) = qualifier {
        let table = known.get(&q.to_lowercase()).cloned().unwrap_or(q.clone());
        if let Some(t) = src
            .catalog
            .as_ref()
            .and_then(|c| find_table(c.tables, &table))
        {
            out.extend(
                t.columns
                    .iter()
                    .map(|(n, ty)| column_hit(src.dialect, n, ty)),
            );
        } else if src.current.is_some_and(|c| c.eq_ignore_ascii_case(&q)) {
            out.extend(
                src.catalog
                    .as_ref()
                    .map(|c| tables_of(src.dialect, c.tables))
                    .unwrap_or_default(),
            );
        } else if let Some(owned) = (src.others)(&q) {
            let tables = owned_tables(&owned);
            out.extend(tables_of(src.dialect, &tables));
        }
    } else {
        if let Some(catalog) = src.catalog.as_ref() {
            for table in known.values() {
                if let Some(t) = find_table(catalog.tables, table) {
                    out.extend(
                        t.columns
                            .iter()
                            .map(|(n, ty)| column_hit(src.dialect, n, ty)),
                    );
                }
            }
            out.extend(tables_of(src.dialect, catalog.tables));
            let schema = src.dialect.features().has_schemas;
            out.extend(catalog.schemas.iter().map(|s| {
                Hit::plain(
                    s.clone(),
                    src.dialect.typed_name(s),
                    if schema { Kind::Schema } else { Kind::Database },
                )
            }));
            out.extend(
                catalog
                    .functions
                    .iter()
                    .map(|f| Hit::plain(f.clone(), src.dialect.typed_name(f), Kind::Function)),
            );
        }
        out.extend(
            src.in_scope
                .iter()
                .map(|(n, ty)| column_hit(src.dialect, n, ty)),
        );
        out.extend(src.extras.iter().cloned());
        out.extend(
            src.snippets
                .iter()
                .map(|(name, code)| Hit::plain(name.clone(), code.clone(), Kind::Snippet)),
        );
        out.extend(
            src.dialect
                .functions()
                .map(|f| Hit::plain(f, f, Kind::Builtin)),
        );
        match src.keywords {
            Keywords::All => out.extend(
                src.dialect
                    .keywords()
                    .map(|k| Hit::plain(k, k, Kind::Keyword)),
            ),
            Keywords::Clause => out.extend(
                CLAUSE_KEYWORDS
                    .iter()
                    .copied()
                    .map(|k| Hit::plain(k, k, Kind::Keyword)),
            ),
            Keywords::None => {}
        }
    }
    let needle = word.to_lowercase();
    let mut seen = std::collections::HashSet::new();
    let mut ranked: Vec<(u8, u32, Hit)> = out
        .into_iter()
        .filter(|c| seen.insert((c.label.clone(), c.kind, c.type_name.clone())))
        .filter_map(|c| {
            let label = c.label.to_lowercase();
            match_key(&label, &needle).map(|(tier, gaps)| (tier, gaps, c))
        })
        .collect();
    ranked.sort_by(|(ta, ga, a), (tb, gb, b)| {
        ta.cmp(tb)
            .then(ga.cmp(gb))
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
    (
        start..offset,
        ranked.into_iter().map(|(_, _, c)| c).collect(),
    )
}

/// Prefix is 0, a consecutive substring is 1, a subsequence is 2.
/// `gaps` is characters skipped between the first and last hit; only
/// subsequences use it, so a tighter match sorts first.
fn match_key(label: &str, needle: &str) -> Option<(u8, u32)> {
    if needle.is_empty() || label.starts_with(needle) {
        return Some((0, 0));
    }
    if label.contains(needle) {
        return Some((1, 0));
    }
    subsequence_gaps(label, needle).map(|gaps| (2, gaps))
}

/// Characters between the first and last matched character, minus the needle.
/// `None` when `needle` is not a subsequence of `label`.
fn subsequence_gaps(label: &str, needle: &str) -> Option<u32> {
    let mut rest = label.chars().enumerate();
    let mut first = None;
    let mut last = 0;
    for n in needle.chars() {
        let (i, _) = rest.find(|(_, c)| *c == n)?;
        first.get_or_insert(i);
        last = i;
    }
    let span = last - first? + 1;
    Some((span - needle.chars().count()) as u32)
}

fn chars_eq_ignore_case(left: char, right: char) -> bool {
    left == right
        || left.eq_ignore_ascii_case(&right)
        || left.to_lowercase().eq(right.to_lowercase())
}

/// Byte ranges the completion menu paints. A contiguous hit (prefix or
/// contains) wins, so `info` in `in_info` paints that span rather than the
/// looser characters around it. Otherwise each matched character is painted,
/// and adjacent hits merge into one range.
pub fn highlight(query: &str, label: &str) -> Vec<Range<usize>> {
    let contiguous = highlight_contains(query, label);
    if contiguous.is_empty() {
        highlight_characters(query, label)
    } else {
        contiguous
    }
}

fn highlight_contains(query: &str, label: &str) -> Vec<Range<usize>> {
    let query: Vec<char> = query.chars().collect();
    if query.is_empty() {
        return Vec::new();
    }
    let label: Vec<(usize, char)> = label.char_indices().collect();
    let Some(last_start) = label.len().checked_sub(query.len()) else {
        return Vec::new();
    };
    for start in 0..=last_start {
        let matched = query
            .iter()
            .zip(label[start..].iter())
            .all(|(query_char, (_, label_char))| chars_eq_ignore_case(*query_char, *label_char));
        if !matched {
            continue;
        }
        let byte_start = label[start].0;
        let (last_index, last_char) = label[start + query.len() - 1];
        let end = last_index + last_char.len_utf8();
        // One matched span. `vec![start..end]` is the range itself, not a vec of indexes.
        #[allow(clippy::single_range_in_vec_init)]
        return vec![byte_start..end];
    }
    Vec::new()
}

fn highlight_characters(query: &str, label: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut rest = label;
    let mut base = 0usize;
    for query_char in query.chars() {
        let Some((index, matched)) = rest
            .char_indices()
            .find(|(_, label_char)| chars_eq_ignore_case(query_char, *label_char))
        else {
            return Vec::new();
        };
        let start = base + index;
        let end = start + matched.len_utf8();
        let next = index + matched.len_utf8();
        base += next;
        rest = &rest[next..];
        if let Some(last) = ranges.last_mut()
            && last.end == start
        {
            last.end = end;
            continue;
        }
        ranges.push(start..end);
    }
    ranges
}

/// Completion reads at most this much text before the caret.
pub const WINDOW: usize = 64 * 1024;

/// `(byte offset where the window starts, text of the window)`.
pub fn window(text: &str, offset: usize) -> (usize, String) {
    let from = offset.saturating_sub(WINDOW);
    let from = (from..=offset)
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(offset);
    let to = (offset + 4096).min(text.len());
    let to = (to..=text.len())
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(text.len());
    (from, text[from..to].to_string())
}

/// Collapse pasted line breaks so a single-line SQL field stays one line.
pub fn one_line(text: &str) -> String {
    // A Windows break is one line, so it becomes one space rather than two.
    text.replace("\r\n", " ").replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::{ContextSelection, DialectFeatures, ExplainStyle, TableOptionStyle};
    use crate::span::Lex;
    use navidog_model::object::{ObjectKind, Scope};

    #[derive(Debug)]
    struct Plain;

    impl Dialect for Plain {
        fn id(&self) -> &'static str {
            "plain"
        }
        fn lex(&self) -> Lex {
            Lex::SQLITE
        }
        fn quote_ident(&self, name: &str) -> String {
            format!("\"{name}\"")
        }
        fn string_literal(&self, value: &str) -> String {
            format!("'{value}'")
        }
        fn qualify_prefix<'a>(&self, _: &'a Scope) -> Option<&'a str> {
            None
        }
        fn object_kinds(&self) -> &'static [ObjectKind] {
            &[]
        }
        fn begin_transaction(&self) -> &'static str {
            "BEGIN"
        }
        fn is_system_database(&self, _: &str) -> bool {
            false
        }
        fn is_system_schema(&self, _: &str) -> bool {
            false
        }
        fn extra_keywords(&self) -> &'static [&'static str] {
            &[]
        }
        fn extra_functions(&self) -> &'static [&'static str] {
            &[]
        }
        fn rename_object(&self, _: &Scope, _: ObjectKind, _: &str, _: &str) -> Option<String> {
            None
        }
        fn duplicate_table(
            &self,
            _: &Scope,
            _: &str,
            _: &str,
            _: bool,
            _: Option<&str>,
        ) -> Option<Vec<String>> {
            None
        }
        fn console_prompt(&self, _: &str, _: bool, _: bool) -> String {
            String::new()
        }
        fn context_selection(&self) -> ContextSelection {
            ContextSelection::Database
        }
        fn features(&self) -> DialectFeatures {
            DialectFeatures::default()
        }
        fn table_option_style(&self) -> TableOptionStyle {
            TableOptionStyle::TableFlags
        }
        fn explain_style(&self) -> ExplainStyle {
            ExplainStyle::QueryPlanRows
        }
    }

    struct Held {
        schemas: Vec<String>,
        columns: Vec<Vec<(String, String)>>,
        functions: Vec<String>,
    }

    fn hold() -> (Held, Vec<(String, bool)>) {
        (
            Held {
                schemas: vec!["sakila".into(), "world".into()],
                columns: vec![
                    vec![
                        ("actor_id".into(), "smallint".into()),
                        ("first_name".into(), "varchar(45)".into()),
                    ],
                    vec![("actor_id".into(), "smallint".into())],
                ],
                functions: vec!["get_customer_balance".into()],
            },
            vec![("actor".into(), false), ("actor_info".into(), true)],
        )
    }

    fn labels(text: &str, in_scope: &[(String, String)], extras: &[Hit]) -> Vec<String> {
        let (held, names) = hold();
        let tables: Vec<Table> = names
            .iter()
            .zip(held.columns.iter())
            .map(|((name, view), columns)| Table {
                name,
                view: *view,
                columns,
            })
            .collect();
        let none = |_: &str| None;
        run_with(&held, &tables, text, &none, in_scope, extras)
    }

    fn run_with(
        held: &Held,
        tables: &[Table<'_>],
        text: &str,
        others: &dyn Fn(&str) -> Option<OwnedCatalog>,
        in_scope: &[(String, String)],
        extras: &[Hit],
    ) -> Vec<String> {
        let snippets = vec![("my snippet".to_string(), "SELECT ${x}".to_string())];
        let offset = text.find('|').unwrap();
        let text = text.replace('|', "");
        complete(
            &text,
            offset,
            &Sources {
                dialect: &Plain,
                current: Some("sakila"),
                catalog: Some(Catalog {
                    schemas: &held.schemas,
                    tables,
                    functions: &held.functions,
                }),
                others,
                snippets: &snippets,
                in_scope,
                extras,
                keywords: Keywords::All,
            },
        )
        .1
        .into_iter()
        .map(|c| c.label)
        .collect()
    }

    #[test]
    fn dot_after_database_lists_tables_and_after_alias_lists_columns() {
        let none = |_: &str| None;
        let (held, names) = hold();
        let tables: Vec<Table> = names
            .iter()
            .zip(held.columns.iter())
            .map(|((name, view), columns)| Table {
                name,
                view: *view,
                columns,
            })
            .collect();
        let run = |text: &str, others: &dyn Fn(&str) -> Option<OwnedCatalog>| {
            run_with(&held, &tables, text, others, &[], &[])
        };
        assert_eq!(
            run("SELECT * FROM sakila.|", &none),
            ["actor", "actor_info"]
        );
        assert_eq!(
            run("SELECT a.| FROM actor a WHERE 1", &none),
            ["actor_id", "first_name"]
        );
        assert_eq!(run("SELECT x.f| FROM actor AS x", &none), ["first_name"]);
        assert_eq!(
            run("SELECT actor.| FROM actor", &none),
            ["actor_id", "first_name"]
        );
        let world = |name: &str| {
            (name == "world").then(|| OwnedCatalog {
                tables: vec![OwnedTable {
                    name: "city".into(),
                    ..OwnedTable::default()
                }],
                ..OwnedCatalog::default()
            })
        };
        assert_eq!(run("SELECT * FROM world.|", &world), ["city"]);
    }

    #[test]
    fn prefix_matches_come_before_contains_and_snippets_show_up() {
        let got = labels("SELECT * FROM act|", &[], &[]);
        assert_eq!(&got[..2], ["actor", "actor_info"]);
        assert!(labels("SELECT * FROM t WHERE zzz|", &[], &[]).is_empty());
        assert!(labels("SELECT first| FROM actor", &[], &[]).contains(&"first_name".to_string()));
        assert!(labels("my|", &[], &[]).contains(&"my snippet".to_string()));
        assert!(labels("sel|", &[], &[]).contains(&"SELECT".to_string()));
        let got = labels("info|", &[], &[]);
        assert!(got.iter().any(|l| l == "actor_info"));
    }

    #[test]
    fn subsequence_matches_rank_after_tighter_hits() {
        let names = [
            ("mocfg".to_string(), false),
            ("x_mocfg_y".to_string(), false),
            ("mount_cfgs".to_string(), false),
            ("m_x_o_x_c_f_g".to_string(), false),
        ];
        let held = Held {
            schemas: Vec::new(),
            columns: vec![Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            functions: Vec::new(),
        };
        let tables: Vec<Table> = names
            .iter()
            .zip(held.columns.iter())
            .map(|((name, view), cols)| Table {
                name,
                view: *view,
                columns: cols,
            })
            .collect();
        let none = |_: &str| None;
        let labels = |text: &str| run_with(&held, &tables, text, &none, &[], &[]);
        let got = labels("mocfg|");
        let pos = |name: &str| got.iter().position(|l| l == name).unwrap();
        assert!(pos("mocfg") < pos("x_mocfg_y"));
        assert!(pos("x_mocfg_y") < pos("mount_cfgs"));
        assert!(pos("mount_cfgs") < pos("m_x_o_x_c_f_g"));
        assert!(labels("MOCFG|").contains(&"mount_cfgs".to_string()));
        assert!(labels("munt|").contains(&"mount_cfgs".to_string()));
        assert!(!labels("moxnt|").iter().any(|l| l == "mount_cfgs"));
    }

    #[test]
    fn in_scope_columns_and_extra_type_names_show_without_a_from() {
        let scope = [("first_name".into(), "varchar(45)".into())];
        let extras = [Hit::plain("varchar", "varchar", Kind::Type)];
        let got = labels("fir|", &scope, &extras);
        assert!(got.contains(&"first_name".to_string()));
        assert!(labels("var|", &[], &extras).contains(&"varchar".to_string()));
    }

    #[test]
    fn highlight_paints_the_matched_characters() {
        assert_eq!(highlight("act", "actor"), vec![0..3]);
        assert_eq!(highlight("info", "actor_info"), vec![6..10]);
        assert_eq!(highlight("info", "in_info"), vec![3..7]);
        assert_eq!(highlight("mocfg", "mount_cfgs"), vec![0..2, 6..9]);
        assert_eq!(highlight("MOCFG", "mount_cfgs"), vec![0..2, 6..9]);
        assert!(highlight("moxnt", "mount_cfgs").is_empty());
    }

    #[test]
    fn one_line_turns_breaks_into_spaces() {
        assert_eq!(one_line("a\nb\rc"), "a b c");
        assert_eq!(one_line("a\r\nb"), "a b");
        assert_eq!(one_line("ab"), "ab");
    }

    #[test]
    fn clause_keywords_keep_and_or_and_drop_statement_words() {
        let (held, names) = hold();
        let tables: Vec<Table> = names
            .iter()
            .zip(held.columns.iter())
            .map(|((name, view), columns)| Table {
                name,
                view: *view,
                columns,
            })
            .collect();
        let none = |_: &str| None;
        let labels = |text: &str, keywords: Keywords| {
            let offset = text.find('|').unwrap();
            let text = text.replace('|', "");
            complete(
                &text,
                offset,
                &Sources {
                    dialect: &Plain,
                    current: Some("sakila"),
                    catalog: Some(Catalog {
                        schemas: &held.schemas,
                        tables: &tables,
                        functions: &held.functions,
                    }),
                    others: &none,
                    snippets: &[],
                    in_scope: &[],
                    extras: &[],
                    keywords,
                },
            )
            .1
            .into_iter()
            .map(|c| c.label)
            .collect::<Vec<_>>()
        };
        let clause = labels("AN|", Keywords::Clause);
        assert!(clause.iter().any(|l| l == "AND"));
        assert!(labels("OR|", Keywords::Clause).iter().any(|l| l == "OR"));
        assert!(labels("AS|", Keywords::Clause).iter().any(|l| l == "ASC"));
        assert!(
            !labels("SEL|", Keywords::Clause)
                .iter()
                .any(|l| l == "SELECT")
        );
        assert!(labels("SEL|", Keywords::All).iter().any(|l| l == "SELECT"));
        assert!(!labels("AN|", Keywords::None).iter().any(|l| l == "AND"));
    }
}
