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
        user.locked = true;
        user.expire = "never".into();
        let combined = security::account_statements(&mysql, mysql.definition_family(), &user)
            .unwrap()
            .join("\n");
        let with = combined.find("WITH MAX_").expect(&combined);
        let expire = combined.find("PASSWORD EXPIRE").expect(&combined);
        let lock = combined.find("ACCOUNT LOCK").expect(&combined);
        assert!(with < expire && expire < lock, "{combined}");
        user.locked = false;
        user.expire.clear();
        user.role = true;
        user.previous_name = Some("ada".into());
        user.name = "ada2".into();
        let renamed = security::account_statements(&mysql, mysql.definition_family(), &user)
            .unwrap()
            .join("\n");
        assert!(renamed.contains("RENAME USER"), "{renamed}");
        assert!(!renamed.contains("ALTER ROLE"), "{renamed}");
        user.previous_name = None;
        user.name = "ada".into();
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
        assert!(
            postgres
                .kill_sql("12")
                .unwrap()
                .unwrap()
                .contains("pg_cancel_backend")
        );
        assert!(mysql.kill_sql("nope").is_err());
        let set =
            security::set_variable_sql(&postgres, postgres.definition_family(), "work_mem", "4MB")
                .unwrap_err();
        assert!(set.contains("read-only"), "{set}");
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
        assert!(
            plan.structure[0].contains("ESCAPE '\\\\'"),
            "{}",
            plan.structure[0]
        );
        assert!(plan.text_columns.is_none());
        let glob = security::find_plan(
            &sqlite,
            sqlite.definition_family(),
            &scope(),
            FindRequest {
                needle: "A_b",
                mode: MatchMode::Contains,
                case_sensitive: true,
                kinds: &[ObjectKind::Table],
                data: false,
            },
        )
        .unwrap();
        assert!(glob.structure[0].contains("GLOB"), "{}", glob.structure[0]);
        assert!(glob.structure[0].contains("*A_b*"), "{}", glob.structure[0]);
        let sensitive = security::find_plan(
            &mysql,
            mysql.definition_family(),
            &scope(),
            FindRequest {
                needle: "Film",
                mode: MatchMode::Contains,
                case_sensitive: true,
                kinds: &[ObjectKind::Table],
                data: false,
            },
        )
        .unwrap();
        assert!(
            sensitive.structure[0].contains("LIKE BINARY"),
            "{}",
            sensitive.structure[0]
        );
        assert!(
            !sensitive.structure[0].contains("utf8mb4_bin"),
            "{}",
            sensitive.structure[0]
        );
        let sqlite_regex = security::find_plan(
            &sqlite,
            sqlite.definition_family(),
            &scope(),
            FindRequest {
                needle: "a+",
                mode: MatchMode::Regex,
                case_sensitive: true,
                kinds: &[ObjectKind::Table],
                data: false,
            },
        );
        assert!(sqlite_regex.unwrap_err().contains("REGEXP"));
        let mysql_regex = security::find_plan(
            &mysql,
            mysql.definition_family(),
            &scope(),
            FindRequest {
                needle: "Film",
                mode: MatchMode::Regex,
                case_sensitive: true,
                kinds: &[ObjectKind::Table],
                data: false,
            },
        )
        .unwrap();
        assert!(
            mysql_regex.structure[0].contains("(?-i)Film"),
            "{}",
            mysql_regex.structure[0]
        );
        assert!(
            !mysql_regex.structure[0].contains("BINARY"),
            "{}",
            mysql_regex.structure[0]
        );
        let regex = security::find_plan(
            &postgres,
            postgres.definition_family(),
            &scope(),
            FindRequest {
                needle: r"\S",
                mode: MatchMode::Regex,
                case_sensitive: false,
                kinds: &[ObjectKind::Table],
                data: false,
            },
        )
        .unwrap();
        assert!(regex.structure[0].contains("~*"), "{}", regex.structure[0]);
        assert!(regex.structure[0].contains(r"\S"), "{}", regex.structure[0]);
        assert!(
            !regex.structure[0].contains("LOWER"),
            "{}",
            regex.structure[0]
        );
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
        assert!(probe.contains("ESCAPE E'\\\\'"), "{probe}");

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

    #[test]
    fn live_mysql_case_sensitive_regexp_matches_without_binary() {
        if std::env::var_os("NAVIDOG_LIVE").is_none() {
            return;
        }
        navidog_db::runtime::runtime().block_on(async {
            for (driver, port) in [
                (
                    std::sync::Arc::new(navidog_mysql::MySqlDriver::mysql())
                        as std::sync::Arc<dyn navidog_db::Driver>,
                    13306,
                ),
                (
                    std::sync::Arc::new(navidog_mysql::MySqlDriver::mariadb())
                        as std::sync::Arc<dyn navidog_db::Driver>,
                    13307,
                ),
            ] {
                let mut session = navidog_db::connect(
                    driver,
                    navidog_db::ConnectParams {
                        host: "127.0.0.1".into(),
                        port,
                        database: Some("sakila".into()),
                        user: "root".into(),
                        password: Some("navidog".into()),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
                let r = session.relational().unwrap();
                let dialect = r.dialect();
                let name = format!("navidog_find_{}", std::process::id());
                let table = dialect.qualified(&Scope::new("c", "sakila", None), &name);
                r.execute("SET NAMES latin1").await.unwrap();
                r.execute(&format!("DROP TABLE IF EXISTS {table}"))
                    .await
                    .unwrap();
                r.execute(&format!(
                    "CREATE TABLE {table} (title varchar(20)) CHARACTER SET latin1"
                ))
                .await
                .unwrap();
                r.execute(&format!(
                    "INSERT INTO {table} (title) VALUES ('Film'), ('film')"
                ))
                .await
                .unwrap();
                let sql = security::data_probe(
                    dialect.as_ref(),
                    dialect.definition_family(),
                    &Scope::new("c", "sakila", None),
                    Probe {
                        table: &name,
                        columns: &["title".into()],
                        needle: "Film",
                        mode: MatchMode::Regex,
                        case_sensitive: true,
                    },
                )
                .unwrap();
                assert!(!sql.contains("BINARY"), "{sql}");
                let hit = r.execute(&sql).await.unwrap();
                assert_eq!(hit[0].rows.len(), 1, "{sql} {hit:?}");
                assert_eq!(hit[0].rows[0][0].as_deref(), Some("Film"));
                r.execute(&format!("DROP TABLE {table}")).await.unwrap();
                session.close().await.unwrap();
            }
        });
    }
}
