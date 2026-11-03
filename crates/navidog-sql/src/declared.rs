//! Plugin dialects own their declarations; nothing is leaked across reloads.

use crate::{
    ContextSelection, Dialect, DialectFeatures, ExplainStyle, Lex, TableOptionStyle,
    template::SqlTemplate,
};
use navidog_model::object::{ObjectKind, Scope};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct DialectDescriptor {
    pub id: String,
    pub quote: char,
    pub has_schemas: bool,
    pub database_prefix: bool,
    pub object_kinds: Vec<ObjectKind>,
    pub keywords: Vec<String>,
    pub functions: Vec<String>,
    pub system_databases: Vec<String>,
    pub system_schemas: Vec<String>,
    pub backslash_strings: bool,
    pub dollar_quotes: bool,
    pub nested_comments: bool,
    pub begin: Option<String>,
    pub savepoint: Option<String>,
    pub rollback_savepoint: Option<String>,
    pub transactional_ddl: bool,
    pub types: Vec<crate::ColumnType>,
    pub templates: BTreeMap<String, SqlTemplate>,
}
impl Default for DialectDescriptor {
    fn default() -> Self {
        Self {
            id: String::new(),
            quote: '"',
            has_schemas: false,
            database_prefix: false,
            object_kinds: vec![ObjectKind::Table, ObjectKind::View],
            keywords: Vec::new(),
            functions: Vec::new(),
            system_databases: Vec::new(),
            system_schemas: Vec::new(),
            backslash_strings: false,
            dollar_quotes: false,
            nested_comments: false,
            begin: None,
            savepoint: None,
            rollback_savepoint: None,
            transactional_ddl: false,
            types: Vec::new(),
            templates: BTreeMap::new(),
        }
    }
}
impl DialectDescriptor {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() || !['"', '`', '['].contains(&self.quote) {
            return Err("Dialect needs an id and a supported identifier quote.".into());
        }
        if self.begin.is_some() != self.savepoint.is_some()
            || self.begin.is_some() != self.rollback_savepoint.is_some()
        {
            return Err(
                "Grid writes need begin, savepoint and rollback-savepoint contracts.".into(),
            );
        }
        const OPERATIONS: &[&str] = &[
            "create_table",
            "add_column",
            "drop_column",
            "alter_column",
            "add_constraint",
            "drop_constraint",
            "create_index",
            "drop_index",
            "move_column",
            "lock_table",
            "explain",
            "select_page",
            "count",
            "rename_table",
            "rename_view",
            "create_view",
            "update_view",
        ];
        if self.transactional_ddl && self.begin.is_none() {
            return Err("Transactional DDL needs a declared begin operation.".into());
        }
        for statement in [&self.begin, &self.savepoint, &self.rollback_savepoint]
            .into_iter()
            .flatten()
        {
            crate::span::check(statement, crate::Lex::GENERIC)
                .map_err(|e| format!("Invalid transaction statement: {e:?}"))?;
            if statement.trim().is_empty()
                || !crate::definition::single_statement(statement, crate::Lex::GENERIC)
            {
                return Err("Transaction operations must be single non-empty statements.".into());
            }
        }
        for (operation, template) in &self.templates {
            if !OPERATIONS.contains(&operation.as_str()) {
                return Err(format!("Unknown SQL operation template: {operation}"));
            }
            template.validate()?;
        }
        Ok(())
    }
}
#[derive(Debug)]
pub struct DeclaredDialect {
    descriptor: DialectDescriptor,
    generation: u64,
}
impl DeclaredDialect {
    pub fn new(descriptor: DialectDescriptor) -> Result<Self, String> {
        descriptor.validate()?;
        Ok(Self {
            descriptor,
            generation: 0,
        })
    }
    pub fn with_generation(mut self, generation: u64) -> Self {
        self.generation = generation;
        self
    }
    pub fn descriptor(&self) -> &DialectDescriptor {
        &self.descriptor
    }
    pub fn template(
        &self,
        name: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<Option<String>, String> {
        self.descriptor
            .templates
            .get(name)
            .map(|t| t.render(self, values))
            .transpose()
    }
}
impl Dialect for DeclaredDialect {
    fn definition_statements(
        &self,
        scope: &Scope,
        draft: &crate::definition::DefinitionDraft,
    ) -> Result<Vec<String>, String> {
        if draft.raw {
            return Err("Plugin designers require structured fields.".into());
        }
        let operation = format!(
            "{}_{}",
            if draft.previous.is_some() {
                "update"
            } else {
                "create"
            },
            draft.kind.key()
        );
        let mut values = BTreeMap::from([
            ("table".into(), self.qualified(scope, &draft.name)),
            ("name".into(), draft.name.clone()),
            (
                "old_name".into(),
                draft.previous.clone().unwrap_or_default(),
            ),
            ("body".into(), draft.body.clone()),
        ]);
        values.extend(
            draft
                .options
                .iter()
                .map(|(key, value)| (format!("field.{key}"), value.clone())),
        );
        self.template(&operation, &values)?
            .map(|sql| vec![sql])
            .ok_or_else(|| format!("Driver has not declared {operation}."))
    }
    fn plugin_generation(&self) -> Option<u64> {
        Some(self.generation)
    }
    fn grid_writes(&self) -> bool {
        self.descriptor.begin.is_some()
            && self.descriptor.savepoint.is_some()
            && self.descriptor.rollback_savepoint.is_some()
    }
    fn savepoint_sql(&self) -> &str {
        self.descriptor.savepoint.as_deref().unwrap_or("")
    }
    fn rollback_savepoint_sql(&self) -> &str {
        self.descriptor.rollback_savepoint.as_deref().unwrap_or("")
    }
    fn designer_templates(&self) -> Option<&BTreeMap<String, SqlTemplate>> {
        Some(&self.descriptor.templates)
    }
    fn id(&self) -> &str {
        &self.descriptor.id
    }
    fn lex(&self) -> Lex {
        Lex {
            backtick_idents: self.descriptor.quote == '`',
            hash_comments: false,
            dollar_quotes: self.descriptor.dollar_quotes,
            bracket_idents: self.descriptor.quote == '[',
            backslash_strings: self.descriptor.backslash_strings,
            escape_string_prefix: false,
            double_quote_strings: false,
            nested_block_comments: self.descriptor.nested_comments,
            delimiter_command: false,
        }
    }
    fn quote_ident(&self, name: &str) -> String {
        let open = self.descriptor.quote;
        let close = if open == '[' { ']' } else { open };
        format!(
            "{open}{}{close}",
            name.replace(close, &format!("{close}{close}"))
        )
    }
    fn string_literal(&self, value: &str) -> String {
        let value = if self.descriptor.backslash_strings {
            value.replace('\\', "\\\\")
        } else {
            value.into()
        };
        format!("'{}'", value.replace('\'', "''"))
    }
    fn qualify_prefix<'a>(&self, scope: &'a Scope) -> Option<&'a str> {
        if self.descriptor.has_schemas {
            scope.schema.as_deref()
        } else if self.descriptor.database_prefix {
            Some(&scope.database)
        } else {
            None
        }
    }
    fn object_kinds(&self) -> &[ObjectKind] {
        &self.descriptor.object_kinds
    }
    fn begin_transaction(&self) -> &str {
        self.descriptor.begin.as_deref().unwrap_or("")
    }
    fn is_system_database(&self, name: &str) -> bool {
        self.descriptor.system_databases.iter().any(|s| s == name)
    }
    fn is_system_schema(&self, name: &str) -> bool {
        self.descriptor.system_schemas.iter().any(|s| s == name)
    }
    fn exclusive_lock_sql(&self, qualified: &str) -> Option<String> {
        self.template(
            "lock_table",
            &BTreeMap::from([("table".into(), qualified.into())]),
        )
        .ok()
        .flatten()
    }
    fn extra_keywords(&self) -> &[&str] {
        &[]
    }
    fn extra_functions(&self) -> &[&str] {
        &[]
    }
    fn keywords(&self) -> crate::keywords::Words<'_> {
        crate::keywords::Words::owned(crate::keywords::COMMON, &self.descriptor.keywords)
    }
    fn functions(&self) -> crate::keywords::Words<'_> {
        crate::keywords::Words::owned(
            crate::keywords::COMMON_FUNCTIONS,
            &self.descriptor.functions,
        )
    }
    fn rename_object(
        &self,
        scope: &Scope,
        kind: ObjectKind,
        old: &str,
        new: &str,
    ) -> Option<String> {
        self.template(
            &format!("rename_{}", kind.key()),
            &BTreeMap::from([
                ("table".into(), self.qualified(scope, old)),
                ("old_name".into(), old.into()),
                ("name".into(), new.into()),
            ]),
        )
        .ok()
        .flatten()
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
    fn console_prompt(&self, database: &str, _: bool, continuation: bool) -> String {
        if continuation {
            "...>".into()
        } else {
            format!("{database}>")
        }
    }
    fn context_selection(&self) -> ContextSelection {
        if self.descriptor.has_schemas {
            ContextSelection::Schema
        } else {
            ContextSelection::Database
        }
    }
    fn features(&self) -> DialectFeatures {
        DialectFeatures {
            has_schemas: self.descriptor.has_schemas,
            transactional_ddl: self.descriptor.transactional_ddl,
            supports_column_comments: false,
            supports_virtual_generated: false,
            supports_index_method: false,
            supports_index_expression: false,
            fk_reference_schema: false,
            ..Default::default()
        }
    }
    fn column_types(&self) -> &[crate::ColumnType] {
        &self.descriptor.types
    }
    fn table_option_style(&self) -> TableOptionStyle {
        TableOptionStyle::Clauses
    }
    fn explain_style(&self) -> ExplainStyle {
        ExplainStyle::QueryPlanRows
    }
}
