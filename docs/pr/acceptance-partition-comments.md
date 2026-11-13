# 表注释与分区回读验收

分支 `codex/m4-partition-comments`，基于 `main` 的 `fea6170`。对应 F-DSG-008 / F-DSG-009。

表设计器原先按子串切分 `PARTITION BY`，会把表注释拆开。现在复用 SQL 扫描器识别顶层关键字，并读出 MySQL `SHOW CREATE TABLE` 在末尾用版本注释包装的分区子句。原始 DDL 仍用于外部变更检查。

- `partition_clause_ignores_table_comment_text` 覆盖无分区、普通分区子句和 MySQL 版本注释三种回读形态；修复前能复现注释被截断。
- `live_table_options_and_dialect_attributes` 用 MySQL 8.4 和 MariaDB 11 回读含 `PARTITION BY` 字样的注释，覆盖普通表和哈希分区表；分区表只改注释时，预览只含一条 `ALTER TABLE … COMMENT`，保存并重新读取后分区定义保持一致。
- 在新建的 Compose 项目上，实库相关套件 100 项和 `live_plans` 均通过。原 Compose 容器有历史测试表，保留原数据并恢复为停止状态。
- 全工作区 389 项测试通过；fmt、clippy、裸值、i18n、方言分支、依赖方向检查、macOS 截图命令和三项性能检查均完成。

依赖锁定文件未改动。依赖审计报告基线中的 RUSTSEC-2026-0459（`libsqlite3-sys`）与 RUSTSEC-2026-0495（`russh`），留待独立依赖更新处理。
