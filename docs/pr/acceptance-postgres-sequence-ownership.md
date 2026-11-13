# PostgreSQL 重建的序列归属验收

基于 `main` 的 `779e7e8`，对应 F-DSG-010。原来按 `nextval(...)` 推断序列归属，会改挂被其他表拥有的序列，也会在重写默认值时丢失表达式的一部分。

加载表时从 `pg_catalog.pg_depend` 读取列拥有的普通序列（自动依赖），保存前比较快照。重建只解除保留的原列的归属，换表后按新列名挂回；保留原序列及完整默认表达式。identity 的内部依赖不走这条路径。旧快照没有归属信息时，重新加载后才能重建。

- `live_postgres_reorder_preserves_sequence_ownership_and_defaults`：PostgreSQL 16 从真实目录读取归属。重排并重命名 `serial` 列后，带点号名称的原序列从 40 继续按步长 3 生成 43；其他表的序列仍归原表列所有，引用列继续取得下一个值。仅改归属后，旧快照无法保存。
- `postgres_rebuild_keeps_constraint_names_and_serial_defaults`：预览保留完整默认表达式、命名约束与触发器，新列对外部序列的引用不产生归属变更。
- `postgres_column_reorder_rebuilds_without_cascade`：未加载序列归属时要求重新加载，重建继续使用不带 `CASCADE` 的 `DROP TABLE`。
