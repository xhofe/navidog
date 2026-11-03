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
