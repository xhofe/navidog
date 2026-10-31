//! Users, grants, maintenance, server monitor, find, and dependency SQL.
//! Product differences are a [`DefinitionFamily`] match here. Callers ask the [`Dialect`] methods.

use navidog_model::object::{ObjectKind, ObjectRef, Scope};

use crate::definition::DefinitionFamily;
use crate::dialect::Dialect;

/// Shown when SQLite user authentication cannot run. This build does not include the extension.
pub const SQLITE_AUTH_REASON: &str =
    "SQLite user authentication needs the user-auth extension, which this build does not include.";

/// Which account form to show. The panel matches this, not a product id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountForm {
    Mysql,
    Postgres,
    /// No statements. The text is the reason, and nothing is executed.
    Unavailable(&'static str),
}

/// A user, role, or group draft. Unused fields are ignored for the other products.
#[derive(Clone, Debug)]
pub struct AccountDraft {
    pub name: String,
    pub host: String,
    pub previous_name: Option<String>,
    pub previous_host: Option<String>,
    pub password: String,
    /// MySQL role (no host) rather than a user.
    pub role: bool,
    pub locked: bool,
    /// Empty, `never`, or a number of days.
    pub expire: String,
    pub max_queries: String,
    pub max_updates: String,
    pub max_connections: String,
    pub max_user_connections: String,
    /// PostgreSQL. `false` is a group (`NOLOGIN`).
    pub login: bool,
    pub superuser: bool,
    pub createdb: bool,
    pub createrole: bool,
    pub inherit: bool,
    pub replication: bool,
    pub connection_limit: String,
    pub valid_until: String,
}

impl Default for AccountDraft {
    fn default() -> Self {
        Self {
            name: String::new(),
            host: String::new(),
            previous_name: None,
            previous_host: None,
            password: String::new(),
            role: false,
            locked: false,
            expire: String::new(),
            max_queries: String::new(),
            max_updates: String::new(),
            max_connections: String::new(),
            max_user_connections: String::new(),
            login: true,
            superuser: false,
            createdb: false,
            createrole: false,
            inherit: true,
            replication: false,
            connection_limit: String::new(),
            valid_until: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GrantDraft {
    pub privilege: String,
    pub object: String,
    pub grantee: String,
    pub host: String,
    pub grant_option: bool,
    pub revoke: bool,
}

impl Default for GrantDraft {
    fn default() -> Self {
        Self {
            privilege: "SELECT".into(),
            object: String::new(),
            grantee: String::new(),
            host: String::new(),
            grant_option: false,
            revoke: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaintenanceJob {
    pub key: &'static str,
    pub sql: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchMode {
    Contains,
    WholeWord,
    Prefix,
    Regex,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindPlan {
    pub structure: Vec<String>,
    /// Columns to probe when data search is on. `None` when it is off.
    pub text_columns: Option<String>,
    pub needle: String,
    pub mode: MatchMode,
    pub case_sensitive: bool,
}

pub struct FindRequest<'a> {
    pub needle: &'a str,
    pub mode: MatchMode,
    pub case_sensitive: bool,
    pub kinds: &'a [ObjectKind],
    pub data: bool,
}

pub struct Probe<'a> {
    pub table: &'a str,
    pub columns: &'a [String],
    pub needle: &'a str,
    pub mode: MatchMode,
    pub case_sensitive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Uses,
    UsedBy,
    Members,
    MemberOf,
    Privileges,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedSql {
    pub section: Section,
    pub sql: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelationPlan {
    /// When set, show this and do not run SQL.
    pub note: Option<&'static str>,
    pub queries: Vec<NamedSql>,
}

pub fn account_form(family: Option<DefinitionFamily>) -> AccountForm {
    match family {
        Some(DefinitionFamily::Mysql) => AccountForm::Mysql,
        Some(DefinitionFamily::Postgres) => AccountForm::Postgres,
        Some(DefinitionFamily::Sqlite) => AccountForm::Unavailable(SQLITE_AUTH_REASON),
        None => AccountForm::Unavailable("This connection cannot manage users."),
    }
}

pub fn account_statements(
    dialect: &dyn Dialect,
    family: Option<DefinitionFamily>,
    draft: &AccountDraft,
) -> Result<Vec<String>, String> {
    match family {
        Some(DefinitionFamily::Mysql) => mysql_account(dialect, draft),
        Some(DefinitionFamily::Postgres) => postgres_account(dialect, draft),
        Some(DefinitionFamily::Sqlite) => Err(SQLITE_AUTH_REASON.into()),
        None => Err("This connection cannot manage users.".into()),
    }
}

pub fn account_list_sql(family: Option<DefinitionFamily>) -> Option<&'static str> {
    match family {
        Some(DefinitionFamily::Mysql) => {
            Some("SELECT User, Host FROM mysql.user ORDER BY User, Host")
        }
        Some(DefinitionFamily::Postgres) => Some(
            "SELECT rolname, CASE WHEN rolcanlogin THEN 'login' ELSE 'group' END FROM pg_roles ORDER BY rolname",
        ),
        _ => None,
    }
}

pub fn grant_statements(
    dialect: &dyn Dialect,
    family: Option<DefinitionFamily>,
    draft: &GrantDraft,
) -> Result<Vec<String>, String> {
    match family {
        Some(DefinitionFamily::Mysql) => mysql_grant(dialect, draft),
        Some(DefinitionFamily::Postgres) => postgres_grant(dialect, draft),
        Some(DefinitionFamily::Sqlite) | None => {
            Err("This connection has no privilege manager.".into())
        }
    }
}

pub fn manages_privileges(family: Option<DefinitionFamily>) -> bool {
    matches!(
        family,
        Some(DefinitionFamily::Mysql | DefinitionFamily::Postgres)
    )
}

pub fn monitors_server(family: Option<DefinitionFamily>) -> bool {
    manages_privileges(family)
}

pub fn maintenance(
    dialect: &dyn Dialect,
    family: Option<DefinitionFamily>,
    scope: &Scope,
    target: Option<&str>,
) -> Vec<MaintenanceJob> {
    let Some(family) = family else {
        return Vec::new();
    };
    let object = target.map(|name| dialect.qualified(scope, name));
    match family {
        DefinitionFamily::Mysql => {
            let Some(object) = object else {
                return Vec::new();
            };
            ["analyze", "check", "optimize", "repair"]
                .into_iter()
                .map(|key| MaintenanceJob {
                    key,
                    sql: format!(
                        "{} TABLE {object}",
                        match key {
                            "analyze" => "ANALYZE",
                            "check" => "CHECK",
                            "optimize" => "OPTIMIZE",
                            _ => "REPAIR",
                        }
                    ),
                })
                .collect()
        }
        DefinitionFamily::Postgres => {
            let target_sql = object.unwrap_or_default();
            let vacuum = if target_sql.is_empty() {
                "VACUUM".into()
            } else {
                format!("VACUUM {target_sql}")
            };
            let analyze = if target_sql.is_empty() {
                "ANALYZE".into()
            } else {
                format!("ANALYZE {target_sql}")
            };
            let reindex = if target_sql.is_empty() {
                format!("REINDEX DATABASE {}", dialect.quote_ident(&scope.database))
            } else {
                format!("REINDEX TABLE {target_sql}")
            };
            vec![
                MaintenanceJob {
                    key: "vacuum",
                    sql: vacuum,
                },
                MaintenanceJob {
                    key: "analyze",
                    sql: analyze,
                },
                MaintenanceJob {
                    key: "reindex",
                    sql: reindex,
                },
            ]
        }
        DefinitionFamily::Sqlite => {
            let mut jobs = Vec::new();
            // VACUUM of an attached database is refused; only the main database.
            if target.is_none() && scope.database.eq_ignore_ascii_case("main") {
                jobs.push(MaintenanceJob {
                    key: "vacuum",
                    sql: "VACUUM".into(),
                });
            }
            let analyze = match &object {
                Some(name) => format!("ANALYZE {name}"),
                None => "ANALYZE".into(),
            };
            let reindex = match &object {
                Some(name) => format!("REINDEX {name}"),
                None => "REINDEX".into(),
            };
            jobs.push(MaintenanceJob {
                key: "analyze",
                sql: analyze,
            });
            jobs.push(MaintenanceJob {
                key: "reindex",
                sql: reindex,
            });
            if target.is_none() {
                jobs.push(MaintenanceJob {
                    key: "integrity",
                    sql: "PRAGMA integrity_check".into(),
                });
            }
            jobs
        }
    }
}

pub fn process_sql(family: Option<DefinitionFamily>) -> Option<&'static str> {
    match family {
        Some(DefinitionFamily::Mysql) => Some("SHOW FULL PROCESSLIST"),
        Some(DefinitionFamily::Postgres) => Some(
            "SELECT pid, usename, datname, state, left(query, 200) FROM pg_stat_activity ORDER BY pid",
        ),
        _ => None,
    }
}

pub fn kill_sql(family: Option<DefinitionFamily>, id: &str) -> Result<Option<String>, String> {
    let Some(family) = family else {
        return Ok(None);
    };
    if !monitors_server(Some(family)) {
        return Ok(None);
    }
    let id = id.trim();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err("Enter a numeric process id.".into());
    }
    let sql = match family {
        DefinitionFamily::Mysql => format!("KILL {id}"),
        DefinitionFamily::Postgres => format!("SELECT pg_terminate_backend({id})"),
        DefinitionFamily::Sqlite => return Ok(None),
    };
    Ok(Some(sql))
}

pub fn variables_sql(family: Option<DefinitionFamily>) -> Option<&'static str> {
    match family {
        Some(DefinitionFamily::Mysql) => Some("SHOW GLOBAL VARIABLES"),
        Some(DefinitionFamily::Postgres) => {
            Some("SELECT name, setting, unit FROM pg_settings ORDER BY name")
        }
        _ => None,
    }
}

pub fn set_variable_sql(
    dialect: &dyn Dialect,
    family: Option<DefinitionFamily>,
    name: &str,
    value: &str,
) -> Result<Vec<String>, String> {
    let Some(family) = family else {
        return Ok(Vec::new());
    };
    if !monitors_server(Some(family)) {
        return Ok(Vec::new());
    }
    let name = name.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        || !name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    {
        return Err("Enter a variable name.".into());
    }
    let value = value.trim();
    if value.is_empty() {
        return Err("Enter a value.".into());
    }
    let literal = if value.chars().all(|c| c.is_ascii_digit()) {
        value.to_string()
    } else {
        dialect.string_literal(value)
    };
    let sql = match family {
        DefinitionFamily::Mysql => vec![format!("SET GLOBAL {name} = {literal}")],
        DefinitionFamily::Postgres => vec![
            format!("ALTER SYSTEM SET {name} = {literal}"),
            "SELECT pg_reload_conf()".into(),
        ],
        DefinitionFamily::Sqlite => Vec::new(),
    };
    Ok(sql)
}

pub fn status_sql(family: Option<DefinitionFamily>) -> Option<&'static str> {
    match family {
        Some(DefinitionFamily::Mysql) => Some("SHOW GLOBAL STATUS"),
        Some(DefinitionFamily::Postgres) => {
            Some("SELECT * FROM pg_stat_database WHERE datname = current_database()")
        }
        _ => None,
    }
}

pub fn find_plan(
    dialect: &dyn Dialect,
    family: Option<DefinitionFamily>,
    scope: &Scope,
    request: FindRequest<'_>,
) -> Result<FindPlan, String> {
    let family = family.ok_or_else(|| "This connection cannot be searched.".to_string())?;
    let needle = request.needle.trim();
    if needle.is_empty() {
        return Err("Enter text to find.".into());
    }
    if request.mode == MatchMode::Regex {
        regex::Regex::new(needle).map_err(|e| format!("Invalid regular expression: {e}"))?;
    }
    let structure = structure_queries(
        dialect,
        family,
        scope,
        needle,
        request.mode,
        request.case_sensitive,
        request.kinds,
    )?;
    let text_columns = if request.data {
        Some(text_columns_sql(dialect, family, scope))
    } else {
        None
    };
    Ok(FindPlan {
        structure,
        text_columns,
        needle: needle.to_string(),
        mode: request.mode,
        case_sensitive: request.case_sensitive,
    })
}

/// `SELECT * FROM table WHERE text columns match LIMIT 100`.
pub fn data_probe(
    dialect: &dyn Dialect,
    family: Option<DefinitionFamily>,
    scope: &Scope,
    probe: Probe<'_>,
) -> Result<String, String> {
    let family = family.ok_or_else(|| "This connection cannot be searched.".to_string())?;
    if probe.columns.is_empty() {
        return Err("No text columns.".into());
    }
    let parts: Result<Vec<_>, _> = probe
        .columns
        .iter()
        .map(|column| {
            predicate(
                dialect,
                family,
                &dialect.quote_ident(column),
                probe.needle,
                probe.mode,
                probe.case_sensitive,
                true,
            )
        })
        .collect();
    Ok(format!(
        "SELECT * FROM {} WHERE {} LIMIT 100",
        dialect.qualified(scope, probe.table),
        parts?.join(" OR ")
    ))
}

pub fn relation_plan(
    dialect: &dyn Dialect,
    family: Option<DefinitionFamily>,
    object: &ObjectRef,
    membership: bool,
) -> RelationPlan {
    let Some(family) = family else {
        return RelationPlan {
            note: Some("This connection has no dependency information."),
            queries: Vec::new(),
        };
    };
    if membership && family == DefinitionFamily::Sqlite {
        return RelationPlan {
            note: Some(SQLITE_AUTH_REASON),
            queries: Vec::new(),
        };
    }
    let name = dialect.string_literal(&object.name);
    let schema = dialect.string_literal(object.scope.schema.as_deref().unwrap_or("public"));
    let database = dialect.string_literal(&object.scope.database);
    let queries = match (family, membership) {
        (DefinitionFamily::Postgres, false) => vec![
            NamedSql {
                section: Section::Uses,
                sql: format!(
                    "SELECT DISTINCT n.nspname, c.relname FROM pg_depend d \
                     JOIN pg_class c ON c.oid = d.refobjid \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE d.objid = (SELECT c2.oid FROM pg_class c2 \
                     JOIN pg_namespace n2 ON n2.oid = c2.relnamespace \
                     WHERE n2.nspname = {schema} AND c2.relname = {name})"
                ),
            },
            NamedSql {
                section: Section::UsedBy,
                sql: format!(
                    "SELECT DISTINCT n.nspname, c.relname FROM pg_depend d \
                     JOIN pg_class c ON c.oid = d.objid \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE d.refobjid = (SELECT c2.oid FROM pg_class c2 \
                     JOIN pg_namespace n2 ON n2.oid = c2.relnamespace \
                     WHERE n2.nspname = {schema} AND c2.relname = {name})"
                ),
            },
        ],
        (DefinitionFamily::Postgres, true) => vec![
            NamedSql {
                section: Section::Members,
                sql: format!(
                    "SELECT r.rolname FROM pg_auth_members m JOIN pg_roles r ON r.oid = m.member \
                     WHERE m.roleid = (SELECT oid FROM pg_roles WHERE rolname = {name})"
                ),
            },
            NamedSql {
                section: Section::MemberOf,
                sql: format!(
                    "SELECT r.rolname FROM pg_auth_members m JOIN pg_roles r ON r.oid = m.roleid \
                     WHERE m.member = (SELECT oid FROM pg_roles WHERE rolname = {name})"
                ),
            },
            NamedSql {
                section: Section::Privileges,
                sql: format!(
                    "SELECT table_schema, table_name, privilege_type \
                     FROM information_schema.role_table_grants WHERE grantee = {name}"
                ),
            },
        ],
        (DefinitionFamily::Mysql, false) => vec![
            NamedSql {
                section: Section::Uses,
                sql: format!(
                    "SELECT REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, CONSTRAINT_NAME \
                     FROM information_schema.KEY_COLUMN_USAGE \
                     WHERE TABLE_SCHEMA = {database} AND TABLE_NAME = {name} \
                     AND REFERENCED_TABLE_NAME IS NOT NULL"
                ),
            },
            NamedSql {
                section: Section::UsedBy,
                sql: format!(
                    "SELECT TABLE_SCHEMA, TABLE_NAME, CONSTRAINT_NAME \
                     FROM information_schema.KEY_COLUMN_USAGE \
                     WHERE REFERENCED_TABLE_SCHEMA = {database} AND REFERENCED_TABLE_NAME = {name}"
                ),
            },
        ],
        (DefinitionFamily::Mysql, true) => {
            let grantee = dialect.string_literal(&format!(
                "'{}'@'{}'",
                object.name,
                object.scope.schema.as_deref().unwrap_or("%")
            ));
            vec![
                NamedSql {
                    section: Section::Members,
                    sql: format!(
                        "SELECT TO_USER, TO_HOST FROM mysql.role_edges WHERE FROM_USER = {name}"
                    ),
                },
                NamedSql {
                    section: Section::MemberOf,
                    sql: format!(
                        "SELECT FROM_USER, FROM_HOST FROM mysql.role_edges WHERE TO_USER = {name}"
                    ),
                },
                NamedSql {
                    section: Section::Privileges,
                    sql: format!(
                        "SELECT TABLE_SCHEMA, TABLE_NAME, PRIVILEGE_TYPE \
                         FROM information_schema.TABLE_PRIVILEGES WHERE GRANTEE = {grantee}"
                    ),
                },
            ]
        }
        (DefinitionFamily::Sqlite, false) => {
            let master = format!(
                "{}.sqlite_master",
                dialect.quote_ident(&object.scope.database)
            );
            let like = dialect.string_literal(&format!("%{}%", object.name.replace('%', "\\%")));
            vec![NamedSql {
                section: Section::UsedBy,
                sql: format!(
                    "SELECT type, name FROM {master} WHERE name != {name} AND sql LIKE {like} ESCAPE '\\'"
                ),
            }]
        }
        (DefinitionFamily::Sqlite, true) => Vec::new(),
    };
    RelationPlan {
        note: None,
        queries,
    }
}

fn mysql_account(dialect: &dyn Dialect, draft: &AccountDraft) -> Result<Vec<String>, String> {
    let name = account_name(&draft.name)?;
    let host = host_or_default(draft.host.trim());
    let previous = draft
        .previous_name
        .as_deref()
        .map(account_name)
        .transpose()?;
    let mut sql = Vec::new();
    if draft.role {
        let ident = dialect.quote_ident(name);
        if let Some(previous) = previous {
            sql.push(format!(
                "ALTER ROLE {} RENAME TO {ident}",
                dialect.quote_ident(previous)
            ));
        } else if draft.previous_name.is_none() {
            sql.push(format!("CREATE ROLE {ident}"));
        }
        return Ok(sql);
    }
    let user = format!(
        "{}@{}",
        dialect.quote_ident(name),
        dialect.quote_ident(host)
    );
    let mut alter = String::new();
    if let Some(password) = optional_password(&draft.password) {
        alter.push_str(" IDENTIFIED BY ");
        alter.push_str(&dialect.string_literal(password));
    }
    if draft.locked {
        alter.push_str(" ACCOUNT LOCK");
    }
    alter.push_str(&mysql_expire(draft.expire.trim())?);
    alter.push_str(&mysql_limits(draft)?);
    if let Some(previous) = previous {
        let previous_host = host_or_default(draft.previous_host.as_deref().unwrap_or("").trim());
        let old = format!(
            "{}@{}",
            dialect.quote_ident(previous),
            dialect.quote_ident(previous_host)
        );
        if previous != name || previous_host != host {
            sql.push(format!("RENAME USER {old} TO {user}"));
        }
        if !alter.trim().is_empty() {
            sql.push(format!("ALTER USER {user}{}", alter.trim_end()));
        }
    } else {
        sql.push(format!("CREATE USER {user}{}", alter.trim_end()));
    }
    Ok(sql)
}

fn postgres_account(dialect: &dyn Dialect, draft: &AccountDraft) -> Result<Vec<String>, String> {
    let name = account_name(&draft.name)?;
    let ident = dialect.quote_ident(name);
    let mut options = vec![if draft.login { "LOGIN" } else { "NOLOGIN" }];
    options.push(if draft.superuser {
        "SUPERUSER"
    } else {
        "NOSUPERUSER"
    });
    options.push(if draft.createdb {
        "CREATEDB"
    } else {
        "NOCREATEDB"
    });
    options.push(if draft.createrole {
        "CREATEROLE"
    } else {
        "NOCREATEROLE"
    });
    options.push(if draft.inherit {
        "INHERIT"
    } else {
        "NOINHERIT"
    });
    options.push(if draft.replication {
        "REPLICATION"
    } else {
        "NOREPLICATION"
    });
    let mut clause = options.join(" ");
    if let Some(password) = optional_password(&draft.password) {
        clause.push_str(" PASSWORD ");
        clause.push_str(&dialect.string_literal(password));
    }
    if let Some(limit) = number("connection limit", &draft.connection_limit)? {
        clause.push_str(" CONNECTION LIMIT ");
        clause.push_str(&limit);
    }
    let until = draft.valid_until.trim();
    if !until.is_empty() {
        clause.push_str(" VALID UNTIL ");
        clause.push_str(&dialect.string_literal(until));
    }
    let mut sql = Vec::new();
    if let Some(previous) = draft
        .previous_name
        .as_deref()
        .map(account_name)
        .transpose()?
    {
        let old = dialect.quote_ident(previous);
        if previous != name {
            sql.push(format!("ALTER ROLE {old} RENAME TO {ident}"));
        }
        sql.push(format!("ALTER ROLE {ident} WITH {clause}"));
    } else {
        sql.push(format!("CREATE ROLE {ident} WITH {clause}"));
    }
    Ok(sql)
}

fn mysql_grant(dialect: &dyn Dialect, draft: &GrantDraft) -> Result<Vec<String>, String> {
    let privilege = privilege(&draft.privilege)?;
    let object = quote_grant_object(dialect, &draft.object, true)?;
    let grantee = account_name(&draft.grantee)?;
    let host = host_or_default(draft.host.trim());
    let user = format!(
        "{}@{}",
        dialect.quote_ident(grantee),
        dialect.quote_ident(host)
    );
    let sql = if draft.revoke {
        format!("REVOKE {privilege} ON {object} FROM {user}")
    } else if draft.grant_option {
        format!("GRANT {privilege} ON {object} TO {user} WITH GRANT OPTION")
    } else {
        format!("GRANT {privilege} ON {object} TO {user}")
    };
    Ok(vec![sql])
}

fn postgres_grant(dialect: &dyn Dialect, draft: &GrantDraft) -> Result<Vec<String>, String> {
    let privilege = privilege(&draft.privilege)?;
    let object = quote_grant_object(dialect, &draft.object, false)?;
    let grantee = dialect.quote_ident(account_name(&draft.grantee)?);
    let sql = if draft.revoke {
        format!("REVOKE {privilege} ON TABLE {object} FROM {grantee}")
    } else if draft.grant_option {
        format!("GRANT {privilege} ON TABLE {object} TO {grantee} WITH GRANT OPTION")
    } else {
        format!("GRANT {privilege} ON TABLE {object} TO {grantee}")
    };
    Ok(vec![sql])
}

fn structure_queries(
    dialect: &dyn Dialect,
    family: DefinitionFamily,
    scope: &Scope,
    needle: &str,
    mode: MatchMode,
    case_sensitive: bool,
    kinds: &[ObjectKind],
) -> Result<Vec<String>, String> {
    let wanted = |kind: ObjectKind| kinds.is_empty() || kinds.contains(&kind);
    match family {
        DefinitionFamily::Mysql => {
            let schema = dialect.string_literal(&scope.database);
            let mut sql = Vec::new();
            let pred = |column: &str| {
                predicate(dialect, family, column, needle, mode, case_sensitive, false)
            };
            if wanted(ObjectKind::Table) {
                sql.push(format!(
                    "SELECT 'table', TABLE_NAME FROM information_schema.TABLES \
                     WHERE TABLE_SCHEMA = {schema} AND TABLE_TYPE = 'BASE TABLE' AND {}",
                    pred("TABLE_NAME")?
                ));
            }
            if wanted(ObjectKind::View) {
                sql.push(format!(
                    "SELECT 'view', TABLE_NAME FROM information_schema.TABLES \
                     WHERE TABLE_SCHEMA = {schema} AND TABLE_TYPE = 'VIEW' AND {}",
                    pred("TABLE_NAME")?
                ));
            }
            if wanted(ObjectKind::Function) {
                sql.push(format!(
                    "SELECT ROUTINE_TYPE, ROUTINE_NAME FROM information_schema.ROUTINES \
                     WHERE ROUTINE_SCHEMA = {schema} AND {}",
                    pred("ROUTINE_NAME")?
                ));
            }
            if wanted(ObjectKind::Event) {
                sql.push(format!(
                    "SELECT 'event', EVENT_NAME FROM information_schema.EVENTS \
                     WHERE EVENT_SCHEMA = {schema} AND {}",
                    pred("EVENT_NAME")?
                ));
            }
            if wanted(ObjectKind::Trigger) {
                sql.push(format!(
                    "SELECT 'trigger', TRIGGER_NAME FROM information_schema.TRIGGERS \
                     WHERE TRIGGER_SCHEMA = {schema} AND {}",
                    pred("TRIGGER_NAME")?
                ));
            }
            Ok(sql)
        }
        DefinitionFamily::Postgres => {
            let schema = dialect.string_literal(scope.schema.as_deref().unwrap_or("public"));
            let pred = predicate(
                dialect,
                family,
                "c.relname",
                needle,
                mode,
                case_sensitive,
                false,
            )?;
            let mut kinds_sql = Vec::new();
            if wanted(ObjectKind::Table) {
                kinds_sql.push("'r'");
            }
            if wanted(ObjectKind::View) {
                kinds_sql.push("'v'");
            }
            if wanted(ObjectKind::MaterializedView) {
                kinds_sql.push("'m'");
            }
            if wanted(ObjectKind::Index) {
                kinds_sql.push("'i'");
            }
            if wanted(ObjectKind::Sequence) {
                kinds_sql.push("'S'");
            }
            let mut sql = Vec::new();
            if !kinds_sql.is_empty() {
                sql.push(format!(
                    "SELECT c.relkind, c.relname FROM pg_class c \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = {schema} AND c.relkind IN ({}) AND {pred}",
                    kinds_sql.join(", ")
                ));
            }
            if wanted(ObjectKind::Function) {
                let p = predicate(
                    dialect,
                    family,
                    "p.proname",
                    needle,
                    mode,
                    case_sensitive,
                    false,
                )?;
                sql.push(format!(
                    "SELECT 'function', p.proname FROM pg_proc p \
                     JOIN pg_namespace n ON n.oid = p.pronamespace \
                     WHERE n.nspname = {schema} AND {p}"
                ));
            }
            Ok(sql)
        }
        DefinitionFamily::Sqlite => {
            let master = format!("{}.sqlite_master", dialect.quote_ident(&scope.database));
            let pred = predicate(dialect, family, "name", needle, mode, case_sensitive, false)?;
            let mut types = Vec::new();
            if wanted(ObjectKind::Table) {
                types.push("'table'");
            }
            if wanted(ObjectKind::View) {
                types.push("'view'");
            }
            if wanted(ObjectKind::Index) {
                types.push("'index'");
            }
            if wanted(ObjectKind::Trigger) {
                types.push("'trigger'");
            }
            if types.is_empty() {
                return Ok(Vec::new());
            }
            Ok(vec![format!(
                "SELECT type, name FROM {master} WHERE type IN ({}) AND {pred}",
                types.join(", ")
            )])
        }
    }
}

fn text_columns_sql(dialect: &dyn Dialect, family: DefinitionFamily, scope: &Scope) -> String {
    match family {
        DefinitionFamily::Mysql => {
            let schema = dialect.string_literal(&scope.database);
            format!(
                "SELECT TABLE_NAME, COLUMN_NAME FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = {schema} AND DATA_TYPE IN \
                 ('char','varchar','tinytext','text','mediumtext','longtext','enum','set','json')"
            )
        }
        DefinitionFamily::Postgres => {
            let schema = dialect.string_literal(scope.schema.as_deref().unwrap_or("public"));
            format!(
                "SELECT table_name, column_name FROM information_schema.columns \
                 WHERE table_schema = {schema} AND data_type IN \
                 ('text','character varying','character','json','jsonb')"
            )
        }
        DefinitionFamily::Sqlite => {
            let master = format!("{}.sqlite_master", dialect.quote_ident(&scope.database));
            let schema = dialect.string_literal(&scope.database);
            format!(
                "SELECT m.name, p.name FROM {master} m \
                 JOIN pragma_table_info(m.name, {schema}) p \
                 WHERE m.type = 'table' AND (p.type = '' OR upper(p.type) LIKE '%CHAR%' \
                 OR upper(p.type) LIKE '%TEXT%' OR upper(p.type) LIKE '%CLOB%')"
            )
        }
    }
}

fn predicate(
    dialect: &dyn Dialect,
    family: DefinitionFamily,
    column: &str,
    needle: &str,
    mode: MatchMode,
    case_sensitive: bool,
    data: bool,
) -> Result<String, String> {
    if mode == MatchMode::Regex {
        let pattern = if case_sensitive {
            needle.to_string()
        } else {
            needle.to_lowercase()
        };
        let column = if case_sensitive {
            column.to_string()
        } else {
            format!("LOWER({column})")
        };
        let op = match family {
            DefinitionFamily::Postgres => "~",
            DefinitionFamily::Mysql | DefinitionFamily::Sqlite => "REGEXP",
        };
        return Ok(format!(
            "{column} {op} {}",
            dialect.string_literal(&pattern)
        ));
    }
    if mode == MatchMode::WholeWord && data {
        let word = escape_like(needle);
        let mid = like_cmp(
            dialect,
            family,
            column,
            &format!("% {word} %"),
            case_sensitive,
        );
        let start = like_cmp(
            dialect,
            family,
            column,
            &format!("{word} %"),
            case_sensitive,
        );
        let end = like_cmp(
            dialect,
            family,
            column,
            &format!("% {word}"),
            case_sensitive,
        );
        let exact = like_cmp(dialect, family, column, &word, case_sensitive);
        return Ok(format!("({mid} OR {start} OR {end} OR {exact})"));
    }
    let pattern = match mode {
        MatchMode::Contains => format!("%{}%", escape_like(needle)),
        MatchMode::Prefix => format!("{}%", escape_like(needle)),
        MatchMode::WholeWord | MatchMode::Regex => escape_like(needle),
    };
    Ok(like_cmp(dialect, family, column, &pattern, case_sensitive))
}

fn like_cmp(
    dialect: &dyn Dialect,
    family: DefinitionFamily,
    column: &str,
    pattern: &str,
    case_sensitive: bool,
) -> String {
    let literal = dialect.string_literal(pattern);
    match (family, case_sensitive) {
        (DefinitionFamily::Postgres, false) => format!("{column} ILIKE {literal} ESCAPE '\\'"),
        (DefinitionFamily::Postgres, true) | (DefinitionFamily::Mysql, false) => {
            format!("{column} LIKE {literal} ESCAPE '\\'")
        }
        (DefinitionFamily::Mysql, true) => {
            format!("{column} LIKE {literal} COLLATE utf8mb4_bin ESCAPE '\\'")
        }
        (DefinitionFamily::Sqlite, false) => format!("{column} LIKE {literal} ESCAPE '\\'"),
        (DefinitionFamily::Sqlite, true) => {
            // SQLite LIKE is case-insensitive for ASCII. GLOB is case-sensitive; the pattern
            // already escaped LIKE metacharacters, and `*` / `?` in the needle stay literal
            // because they were not converted into GLOB wildcards except for our own `%`.
            let glob = pattern.replace('%', "*");
            format!("{column} GLOB {}", dialect.string_literal(&glob))
        }
    }
}

fn escape_like(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn account_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.contains('\0') || name.contains(';') {
        return Err("Enter a name.".into());
    }
    Ok(name)
}

fn optional_password(password: &str) -> Option<&str> {
    let password = password.trim();
    if password.is_empty() {
        None
    } else {
        Some(password)
    }
}

fn host_or_default(host: &str) -> &str {
    if host.is_empty() { "%" } else { host }
}

fn number(label: &str, value: &str) -> Result<Option<String>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if !value.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("{label} must be a number."));
    }
    Ok(Some(value.to_string()))
}

fn mysql_expire(expire: &str) -> Result<String, String> {
    if expire.is_empty() {
        return Ok(String::new());
    }
    if expire.eq_ignore_ascii_case("never") {
        return Ok(" PASSWORD EXPIRE NEVER".into());
    }
    if expire.chars().all(|c| c.is_ascii_digit()) {
        return Ok(format!(" PASSWORD EXPIRE INTERVAL {expire} DAY"));
    }
    Err("Expire must be empty, never, or a number of days.".into())
}

fn mysql_limits(draft: &AccountDraft) -> Result<String, String> {
    let queries = number("max queries", &draft.max_queries)?;
    let updates = number("max updates", &draft.max_updates)?;
    let connections = number("max connections", &draft.max_connections)?;
    let user_connections = number("max user connections", &draft.max_user_connections)?;
    if queries.is_none() && updates.is_none() && connections.is_none() && user_connections.is_none()
    {
        return Ok(String::new());
    }
    Ok(format!(
        " WITH MAX_QUERIES_PER_HOUR {} MAX_UPDATES_PER_HOUR {} MAX_CONNECTIONS_PER_HOUR {} MAX_USER_CONNECTIONS {}",
        queries.unwrap_or_else(|| "0".into()),
        updates.unwrap_or_else(|| "0".into()),
        connections.unwrap_or_else(|| "0".into()),
        user_connections.unwrap_or_else(|| "0".into())
    ))
}

fn privilege(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.eq_ignore_ascii_case("all") || raw.eq_ignore_ascii_case("all privileges") {
        return Ok("ALL PRIVILEGES".into());
    }
    if raw.is_empty()
        || !raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ' || c == ',')
    {
        return Err("Enter a privilege such as SELECT.".into());
    }
    Ok(raw.to_ascii_uppercase())
}

fn quote_grant_object(dialect: &dyn Dialect, raw: &str, mysql: bool) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains(';') || raw.contains('\0') {
        return Err("Enter the object to grant.".into());
    }
    if raw == "*.*" || raw == "*" {
        if mysql {
            return Ok(raw.to_string());
        }
        return Err("Name the table.".into());
    }
    Ok(raw
        .split('.')
        .map(|part| {
            let part = part.trim();
            if part == "*" {
                "*".to_string()
            } else {
                dialect.quote_ident(part)
            }
        })
        .collect::<Vec<_>>()
        .join("."))
}
