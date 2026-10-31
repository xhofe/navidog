//! Security, maintenance, monitor, and find SQL for each product.

#[cfg(test)]
mod tests {
    use navidog_model::object::{ObjectKind, ObjectRef, Scope};
    use navidog_sql::dialect::Dialect;
    use navidog_sql::security::{
        self, AccountDraft, AccountForm, FindRequest, GrantDraft, MatchMode, Probe,
        SQLITE_AUTH_REASON,
    };

    fn scope() -> Scope {
        Scope::new("c", "sakila", Some("public".into()))
    }

    #[test]
    fn security_statements_cover_each_product() {
        let mysql = navidog_mysql::MysqlDialect;
        let postgres = navidog_postgres::PostgresDialect;
        let sqlite = navidog_sqlite::SqliteDialect;
        let mut user = AccountDraft {
            name: "ada".into(),
            password: "secret".into(),
            max_queries: "10".into(),
            ..AccountDraft::default()
        };
        let mysql_sql = security::account_statements(&mysql, mysql.definition_family(), &user)
            .unwrap()
            .join("\n");
        assert!(mysql_sql.contains("CREATE USER"));
        assert!(mysql_sql.contains("IDENTIFIED BY"));
        assert!(mysql_sql.contains("MAX_QUERIES_PER_HOUR 10"));
        user.role = true;
        assert!(
            security::account_statements(&mysql, mysql.definition_family(), &user).unwrap()[0]
                .contains("CREATE ROLE")
        );

        user.role = false;
        user.login = false;
        let group = security::account_statements(&postgres, postgres.definition_family(), &user)
            .unwrap()
            .join("\n");
        assert!(group.contains("CREATE ROLE"));
        assert!(group.contains("NOLOGIN"));
        assert!(!group.contains(" LOGIN"));
        user.login = true;
        user.previous_name = Some("ada".into());
        user.name = "ada2".into();
        let renamed = security::account_statements(&postgres, postgres.definition_family(), &user)
            .unwrap()
            .join("\n");
        assert!(renamed.contains("RENAME TO"));
        assert!(renamed.contains("LOGIN"));

        assert_eq!(
            sqlite.account_form(),
            AccountForm::Unavailable(SQLITE_AUTH_REASON)
        );
        let reason =
            security::account_statements(&sqlite, sqlite.definition_family(), &user).unwrap_err();
        assert!(reason.contains("user-auth"));
        assert!(!reason.contains("CREATE"));
        assert!(sqlite.account_list_sql().is_none());
        assert!(!sqlite.manages_privileges());
        assert!(!sqlite.monitors_server());
        assert!(sqlite.process_sql().is_none());

        let grant = GrantDraft {
            privilege: "select".into(),
            object: "sakila.film".into(),
            grantee: "ada".into(),
            grant_option: true,
            ..GrantDraft::default()
        };
        let granted = security::grant_statements(&mysql, mysql.definition_family(), &grant)
            .unwrap()
            .join("\n");
        assert!(granted.starts_with("GRANT SELECT"));
        assert!(granted.contains("WITH GRANT OPTION"));
        assert!(security::grant_statements(&sqlite, sqlite.definition_family(), &grant).is_err());
        let mut revoke = grant.clone();
        revoke.revoke = true;
        assert!(
            security::grant_statements(&postgres, postgres.definition_family(), &revoke).unwrap()
                [0]
            .starts_with("REVOKE SELECT")
        );

        let jobs = security::maintenance(&mysql, mysql.definition_family(), &scope(), Some("film"));
        assert_eq!(jobs.len(), 4);
        assert!(jobs.iter().any(|job| job.sql.contains("OPTIMIZE TABLE")));
        let sqlite_scope = Scope::new("c", "main", None);
        let vacuum =
            security::maintenance(&sqlite, sqlite.definition_family(), &sqlite_scope, None);
        assert!(vacuum.iter().any(|job| job.sql == "VACUUM"));
        assert!(
            security::maintenance(
                &sqlite,
                sqlite.definition_family(),
                &sqlite_scope,
                Some("film")
            )
            .iter()
            .all(|job| job.sql != "VACUUM")
        );
        let attached = Scope::new("c", "other", None);
        assert!(
            security::maintenance(&sqlite, sqlite.definition_family(), &attached, None)
                .iter()
                .all(|job| job.key != "vacuum")
        );

        assert!(mysql.process_sql().unwrap().contains("PROCESSLIST"));
        assert!(mysql.kill_sql("12").unwrap().unwrap().contains("KILL 12"));
        assert!(mysql.kill_sql("nope").is_err());
        let set =
            security::set_variable_sql(&postgres, postgres.definition_family(), "work_mem", "4MB")
                .unwrap();
        assert!(set[0].contains("ALTER SYSTEM SET work_mem"));
        assert_eq!(set[1], "SELECT pg_reload_conf()");
        assert!(
            security::set_variable_sql(&sqlite, sqlite.definition_family(), "work_mem", "4MB")
                .unwrap()
                .is_empty()
        );

        let plan = security::find_plan(
            &mysql,
            mysql.definition_family(),
            &scope(),
            FindRequest {
                needle: "film",
                mode: MatchMode::Contains,
                case_sensitive: false,
                kinds: &[ObjectKind::Table],
                data: false,
            },
        )
        .unwrap();
        assert!(plan.structure[0].contains("LIKE"));
        assert!(plan.text_columns.is_none());
        let err = security::find_plan(
            &mysql,
            mysql.definition_family(),
            &scope(),
            FindRequest {
                needle: "(unclosed",
                mode: MatchMode::Regex,
                case_sensitive: true,
                kinds: &[ObjectKind::Table],
                data: true,
            },
        )
        .unwrap_err();
        assert!(err.contains("Invalid regular expression"));
        let probe = security::data_probe(
            &postgres,
            postgres.definition_family(),
            &scope(),
            Probe {
                table: "film",
                columns: &["title".into()],
                needle: "dog",
                mode: MatchMode::Contains,
                case_sensitive: false,
            },
        )
        .unwrap();
        assert!(probe.contains("LIMIT 100"));
        assert!(probe.contains("ILIKE"));

        let object = ObjectRef {
            scope: scope(),
            kind: ObjectKind::Table,
            name: "film".into(),
        };
        let used = security::relation_plan(&postgres, postgres.definition_family(), &object, false);
        assert!(used.note.is_none());
        assert!(used.queries.iter().any(|q| q.sql.contains("pg_depend")));
        let members = security::relation_plan(&sqlite, sqlite.definition_family(), &object, true);
        assert_eq!(members.note, Some(SQLITE_AUTH_REASON));
        assert!(members.queries.is_empty());
    }
}
