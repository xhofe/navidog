//! An execution plan freezes the exact reviewed SQL and rejects every changed input.
use super::*;

#[derive(Clone)]
pub struct PreviewPlan {
    baseline: Option<TableSnapshot>,
    draft: TableDesign,
    generation: Option<u64>,
    statements: Vec<String>,
}
impl PreviewPlan {
    pub fn new(
        d: &dyn Dialect,
        baseline: Option<TableSnapshot>,
        draft: TableDesign,
        statements: Vec<String>,
    ) -> Self {
        Self {
            baseline,
            draft,
            generation: d.plugin_generation(),
            statements,
        }
    }
    pub fn statements(&self) -> &[String] {
        &self.statements
    }
    pub async fn execute(
        &self,
        r: &mut dyn Relational,
        scope: &Scope,
        draft: &TableDesign,
    ) -> DbResult<TableSnapshot> {
        let d = r.dialect();
        if &self.draft != draft || d.plugin_generation() != self.generation {
            return Err(DbError::msg(
                "Preview expired; review a new plan before saving.",
            ));
        }
        if let Some(old) = &self.baseline
            && load(r, scope, &old.design.name).await? != *old
        {
            return Err(DbError::msg(
                "Database structure changed; reload before planning.",
            ));
        }
        let transaction = d.features().transactional_ddl;
        if transaction {
            r.execute(d.begin_transaction()).await?;
        }
        let mut completed = 0;
        let result = async {
            if let Some(old) = &self.baseline {
                if let Some(lock) = d.exclusive_lock_sql(&d.qualified(scope, &old.design.name)) {
                    r.execute(&lock).await?;
                }
                if load(r, scope, &old.design.name).await? != *old {
                    return Err(DbError::msg(
                        "Database structure changed; reload before planning.",
                    ));
                }
            }
            for sql in &self.statements {
                r.execute(sql).await?;
                completed += 1;
            }
            let snapshot = load(r, scope, &draft.name).await?;
            if transaction {
                r.execute("COMMIT").await?;
            }
            Ok(snapshot)
        }
        .await;
        match result {
            Err(error) if transaction => {
                let _ = r.execute("ROLLBACK").await;
                Err(error)
            }
            Err(error) => Err(DbError::msg(format!(
                "{error}; completed {completed} of {} steps. The database may have partial changes. Reload before planning again.",
                self.statements.len()
            ))),
            Ok(snapshot) => Ok(snapshot),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    struct Partial {
        dialect: Arc<dyn Dialect>,
        steps: Vec<String>,
    }
    impl Relational for Partial {
        fn dialect(&self) -> Arc<dyn Dialect> {
            self.dialect.clone()
        }
        fn execute<'a>(
            &'a mut self,
            sql: &'a str,
        ) -> navidog_db::BoxFuture<'a, DbResult<Vec<navidog_db::ResultSet>>> {
            self.steps.push(sql.into());
            Box::pin(async move {
                if sql == "FAIL" {
                    Err(DbError::msg("Injected DDL failure"))
                } else {
                    Ok(vec![])
                }
            })
        }
        navidog_db::unsupported_catalog!();
    }
    #[test]
    fn nontransactional_failure_reports_completed_steps_and_does_not_replay() {
        let descriptor = serde_json::from_value(serde_json::json!({"id":"fixture"})).unwrap();
        let dialect: Arc<dyn Dialect> = Arc::new(
            navidog_sql::declared::DeclaredDialect::new(descriptor)
                .unwrap()
                .with_generation(3),
        );
        let mut draft = TableDesign::blank(dialect.as_ref());
        draft.name = "t".into();
        let plan = PreviewPlan::new(
            dialect.as_ref(),
            None,
            draft.clone(),
            vec![
                "CREATE TABLE t(id integer)".into(),
                "FAIL".into(),
                "never".into(),
            ],
        );
        let mut rel = Partial {
            dialect,
            steps: vec![],
        };
        let error = navidog_db::runtime::runtime()
            .block_on(plan.execute(&mut rel, &Scope::new("test", "db", None), &draft))
            .unwrap_err();
        assert!(error.message.contains("completed 1 of 3"));
        assert_eq!(rel.steps, vec!["CREATE TABLE t(id integer)", "FAIL"]);
        draft.columns[0].name = "changed".into();
        assert!(
            navidog_db::runtime::runtime()
                .block_on(plan.execute(&mut rel, &Scope::new("test", "db", None), &draft))
                .is_err()
        );
        assert_eq!(rel.steps.len(), 2);
    }
}
