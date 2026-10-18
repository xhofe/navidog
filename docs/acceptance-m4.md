# M4 表设计器首版本机验收

日期：2026-10-18。基线：M3 分支 `codex/m3-completion` 的 `4bb20ea`；开发分支：`codex/m4-table-designer`。规格先提交为 `47fdce5`。本次实现表设计器首版，M4 整体仍进行中。

## 已实现

- 新增独立 `navidog-designer`，从对象工具栏和导航菜单新建/设计表，表默认双击仍查看数据。
- 字段增加、删除、复制、重排、重命名与完整 SQL 定义；约束、索引、选项编辑；保存先预览，再明确执行。
- 自有数据库会话、执行期间禁用编辑、失败保留草稿、关闭守卫、重新加载确认、布局草稿恢复，成功后刷新目录和补全缓存。
- MySQL/MariaDB 差异 ALTER；PostgreSQL 事务 ALTER；SQLite 事务重建或原生重命名/索引修改。保存前检查外部结构变化，不支持的迁移明确拒绝。
- SQLite 回归覆盖已有行、索引、触发器、入站外键、视图、隐藏 rowid、自增序列高水位、生成列纯索引修改，以及约束失败、外键失败、依赖字段删除和外部结构变化。

## 验证结果

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all --check`、`scripts/check-raw-px.sh`、`git diff --check` | 通过 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 通过；`block`、`proc-macro-error2` 有依赖 future-incompatibility 提示 |
| `cargo test --workspace --locked` | 206 项通过，35 个测试/文档测试套件 |
| `NAVIDOG_LIVE=1 cargo test -p navidog-db table_design --locked` | 5 项通过，含 MySQL 8、MariaDB 11、PostgreSQL 16 创建、修改、重命名、索引、检查约束和外键的实库验证 |
| `NAVIDOG_LIVE=1 cargo test -p navidog-db --test live --locked` | 11 项通过 |
| `NAVIDOG_LIVE=1 cargo test -p navidog-query --locked` | 27 项通过 |
| 设计器 headless UI | 2 项通过：真实按键与点击、草稿恢复、预览不执行、执行建表、失败回滚保留草稿 |
| 选项卡测试 | 通过，补充设计器未保存关闭的取消/放弃分支 |
| `cargo deny --all-features check` | advisories、bans、licenses、sources 通过，保留重复依赖等已有警告 |
| `cargo audit` | 通过，9 条已允许的间接依赖 unmaintained 警告 |
| `cargo test -p navidog --test snapshot --locked` | 通过，生成 31 张；新增字段和 SQL 预览的浅深色 4 张已逐张查看 |

截图在本地 `target/snapshots/designer.png`、`designer-dark.png`、`designer-preview.png`、`designer-preview-dark.png`，构建产物不进 Git。

性能程序实际执行：10 万行 × 20 列网格逐行平均 3.09ms、p95 3.92ms，跳跃平均 3.87ms、p95 4.63ms；均 0/300 帧超过 8.3ms。导航树逐行/跳跃平均 0.53/0.71ms，均 0/300 帧超限。5 万行 SQL 首次解析约 340ms，四个场景平均 1.54~1.60ms，均 0/300 帧超限。这些是本机测量，不代表任意硬件的保证。

测试使用项目 Docker Compose 栈，结束后关闭；SQLite/UI 配置使用独立临时目录。

## 明确保留的事项

字段目前为“名称 + 完整 SQL 定义”，约束与索引也使用 SQL，尚未提供 Navicat 的逐属性网格及结构化表单，因此 F-DSG-001/002/003/004/007/009 仍标为部分完成。其他对象设计器不在本次实现内。

SQLite 生成列表重建和附加数据库、PostgreSQL 物理列重排及现有特殊属性/选项/注释修改等明确拒绝。MySQL DDL 无法事务回滚，失败后必须重新加载，执行前结构检查也不能消除并发 DDL 窗口。

本次参考本地 Navicat 帮助 `reference/help-text/mysql_table.txt`、`postgresql_table.txt`、`sqlite_table.txt`、`hot_keys.txt`，复用项目已有尺寸 token；未完成 Navicat 并排视觉校准，不把自动截图当作该项验收。详细边界见 `docs/design/40-table-designer.md` 第 5 节。
