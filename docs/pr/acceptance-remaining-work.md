# 整体审查遗留工作验收（2026-10-26）

## 范围

- 来源：[2026-10-26 遗留清单](../cr/2026-10-26-remaining-work.md)。
- 分支：`refactor/remaining-work`，从 `main`（`fa5faa9`）创建。
- 清单原本建议按主题拆成 8 个 PR（纯迁移优先）。按任务要求，本次全部工作放在同一个分支和一个 PR 里；每个事项仍然按“先搬代码、再单独行为”的顺序提交，提交按主题分开，便于逐项 review。

## 逐项结果

| 编号 | 结果 | 提交主题 |
| --- | --- | --- |
| R-04 / D-05 | 统一 SQL 词法 | `refactor(sql)` |
| D-08 / A-06 | 缓存/选择状态收口 | `refactor(navigator)` |
| A-01 / A-02 | app-state 拆分、依赖方向 | `refactor(state)` |
| A-03 | `table_design` 拆到 `navidog-schema` | `refactor(schema)` |
| A-07 | 超长文件拆分 | `refactor(grid)`/`refactor(query)`/`refactor(connection)`/`refactor(update)` |
| A-10 | `Connection` 的 `profiles + active` | `refactor(connection)` |
| S-07 | i18n 契约与动态 key 检查 | `docs(i18n)` |

### R-04 / D-05：统一 SQL 扫描

- 新建 `navidog-sql`：`Dialect`（从 `navidog-db` 迁入并由 db re-export）、关键字表、`span` 词法器。
- `span::spans(sql, dialect)` 覆盖字符串、引号标识符、行/块注释（PostgreSQL 嵌套）、`$tag$`、`[$name]` 参数、括号深度、语句分隔符（MySQL `DELIMITER` 与 `BEGIN…END` 例外）。`span::check` 报告未闭合引号/注释和括号不平衡，`span::separators` 给出深度 0 的分隔符位置。
- 六个消费者改为消费同一份 span 结果，各自的跳过函数删除：`navidog-db::sql`、`navidog-util::sql_text`、`navidog-util::sql_format`、`navidog-grid::filter`、`navidog-schema::table_design::{parse,properties,entries}`。
- 回归：`navidog-sql/src/span/tests.rs` 覆盖清单里的 `SELECT $$ [$x] $$`、`E'a\';b'`、`foo$tag$`、`#` 注释、嵌套注释、`DELIMITER //`、`BEGIN…END`、反斜杠关闭的字符串和方括号标识符；`navidog-grid/src/filter/consistency_tests.rs` 用同一段 SQL 检查分句、字段列表、参数、筛选、格式化、列属性六处结论一致。

### D-08 / A-06：Catalog/OutlineCache 统一缓存

- `ScopeCache<K, T>` 泛型化到任意键；`retain` 同时清理 entries 和 tasks。
- `Catalog` 的五类缓存换成 `ScopeCache` + typed key（`SchemaKey`/`ObjectsKey`/`TableKey`/`ObjectRef`），在途加载用 `LoadId` 枚举，`invalidate_connection`/`retain_connections`/`refresh_scope` 同时清理 entries、tasks 和 generation。
- `Catalog::require/require_all/reload_all` 是唯一加载入口，`require_selection` 由 Workspace 的 `SelectionChanged` 驱动；objects 面板的 `ensure_loaded` 和渲染期 `stalled_load` 补偿删除，navigator/信息/DDL 面板只声明 `Requirement`，Session 打开后 Catalog 自己重启当前选择需要的加载。
- 测试：`catalog.rs` 覆盖缓存命中不发起查询、invalidate 清理 entries/tasks/generation、延迟结果不覆盖新代际、失败条目只在 reload 时重试；`tests/catalog_lifecycle.rs` 覆盖选择驱动加载、关闭连接清空、重开恢复、删除连接清空；`tests/database_refresh.rs` 覆盖切库刷新与打开中的 DDL 面板。

### A-01 / A-02：app-state 拆分与依赖方向

- `navidog-core` 整体改名为 `navidog-app-state`（settings/store/workspace/favorites/snippets/vgroups/actions/registry/uri + `init`），纯数据继续留在 `navidog-model`。72 个文件的 `navidog_core::…` 按模块迁到 `navidog_model::…` 或 `navidog_app_state::…`；`navidog-core` 删除，没有兼容 facade。
- i18n 域从 `core.*` 改为 `app_state.*`（`locales/app_state.yml`）。
- `navidog-db` 只依赖 `navidog-model`，驱动栈不再链接 GPUI；新增 `crates/navidog-headless`（只依赖 model + db）编译并运行 SQLite。
- 新增 `scripts/check-deps.sh`：`navidog-model`/`navidog-db`/`navidog-sql`/`navidog-schema`/`navidog-util`/`navidog-sql-log`/`navidog-headless` 的 normal 依赖不得出现 `gpui`，`crates/navidog-core` 不得回潮；已接入 `lint.yml`。
- `docs/design/00-overview.md` 的 crate 表、依赖规则和里程碑 crate 列同步；M7 的 ER 图 crate 暂记作 `navidog-er`，因为 `navidog-model` 已是纯数据层。

### A-03：`table_design` 拆到 `navidog-schema`

- 新 `navidog-schema` 持有表草稿/解析/表单/迁移计划（约 4,000 行），依赖 `navidog-model` + `navidog-db` 的接口 + `navidog-sql`，没有复制驱动类型。
- `navidog-db` 删除 `pub mod table_design`，`rg 'table_design' crates/navidog-db/src` 为空；designer 和 grid 的一致性测试改为 `navidog_schema::table_design`。仓库外没有消费者，为此没有保留过渡 re-export。
- 实库 DDL 测试随模块迁到 `navidog-schema`，CI 的 `integration.yml` 同步改了命令。

### A-07：超长文件拆分

纯搬家，断言没有改动；主文件行数：

| 文件 | 拆分前 | 现在 |
| --- | ---: | ---: |
| `grid/viewer/mod.rs` | 2882 | 768 |
| `query/results/mod.rs` | 1886 | 348 |
| `query/editor/mod.rs` | 1742 | 922 |
| `connection/form/mod.rs` | 1493 | 534 |
| `grid/model/mod.rs` | 1738 | 139 |
| `app/update/mod.rs` | 1353 | 27 |

模块划分见[遗留清单的完成记录](../cr/2026-10-26-remaining-work.md)。结构级拆分同时完成：`TableSession`/`TableView`、`ResultSession`/`ResultGrid`、`GridModel` 的 `storage`/`edit`/`projection`，四个 `*_after_commit` 字段合并成 `enum AfterCommit`；仍是纯搬家，断言未改。

### A-10：`Connection` 的 `profiles + active`

- 内存模型：`profiles: Vec<Profile>` + `active: usize`（激活项在数组里）。`active()`/`active_mut()`/`active_profile_name()` 访问激活配置，`Deref`/`DerefMut` 到 `Profile` 让 `connection.general` 等既有字段访问不改。切换/添加/删除只移动索引，`with_active_profile` 用 Drop 守卫恢复。
- `ConnectionsFile::reencrypt` 按索引遍历全部配置文件；`duplicate` 随克隆带走全部配置。
- **兼容窗口**：`connections.json` 的写格式保持历史布局（激活设置在顶层、`profile` 是激活名、`profiles` 只放其余配置），所以**旧版本仍能读新版本写的文件，不需要迁移**；读格式同时接受 `profiles` 含全部配置 + `active` 索引的新布局，将来切换写格式时不用再做迁移。两代 fixture 都有测试。
- 边界测试：7 个以上配置文件、重名、空名字、旧/新两种 JSON、panic 后激活项恢复（`navidog-model`）；8 个配置文件的 rekey/duplicate 秘密字段（`navidog-app-state`）。

### S-07：i18n 契约

- 采用方案 1（保持 crate-local 后端）：跨 crate 文案走显式 helper，不合并 backend。
- `scripts/check-i18n.sh` 新增动态 key 检查：`t!` 第一个参数不是字面量时必须登记在 `scripts/i18n-dynamic-keys.txt`（当前 19 条），否则失败。
- 契约写入 `docs/design/00-overview.md` 的 i18n 段，`AGENTS.md` 的“文案”条目同步；`docs` 记录了为什么不合并 backend。

## 验证

全部在本机（macOS）执行：

| 检查 | 命令 | 结果 |
| --- | --- | --- |
| 格式 | `cargo fmt --all --check` | 通过 |
| 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| 单元/headless 测试 | `cargo test --workspace --exclude navidog -j 1`、`cargo test -p navidog -j 1` | 通过 |
| 裸值检查 | `scripts/check-raw-px.sh` | 通过 |
| i18n 检查 | `scripts/check-i18n.sh` | 通过 |
| 依赖方向 | `scripts/check-deps.sh` | 通过 |
| headless 依赖检查 | `cargo test -p navidog-headless` | 通过（SQLite 连接/执行 SQL，模型序列化） |
| 实库测试 | `docker compose up -d --wait && NAVIDOG_LIVE=1 cargo test -p navidog-db -p navidog-schema -p navidog-query --lib --test live --locked -j 1` | 通过：`navidog-db` 的 `tests/live.rs` 15 项、`navidog-schema` 的 lib（含 3 项实库 DDL/表选项/注释重命名回归）22 项、`navidog-query` 的 lib 33 项；MySQL 8.4、MariaDB 11、PostgreSQL 16 容器均 Healthy |
| 截图 | `cargo test -p navidog --test snapshot` | 未跑：外观未变（见下） |

截图：本次没有改外观（拆分为纯搬家、缓存/连接模型的行为与界面无关），按遗留清单的 A-07 验收“截图测试只在外观受影响时补跑”未补跑；如需要，可在 PR 批准后补一轮浅深色截图。

## Review 跟进（2026-10-26，PR #22）

CodeRabbit 提了 7 条可操作评论，逐条对照代码核实后全部成立并已修复：

| # | 位置 | 结论 | 处理 |
| --- | --- | --- | --- |
| 1 | `navigator/navigator/menus.rs` | “删除配置文件”用了 `profiles.is_empty()`，新模型下永远启用但点了没反应 | 改成 `profiles.len() <= 1` |
| 2 | `grid/viewer/render.rs` | “限制记录数”复选框把 `0` 当成有效值，和输入框回车的 `n > 0` 规则不一致 | 解析后加 `.filter(\|n\| *n > 0)` |
| 3 | `navigator/catalog.rs` | `refresh_scope` 只丢掉 DDL 缓存、没有重载，打开中的 DDL 视图会一直显示旧文本 | 收集该 scope 的 DDL key，删缓存后按 `Requirement::Ddl` 强制重载；`catalog_lifecycle` 补回归 |
| 4 | `navigator/ops.rs` | `refresh` 直接按对象生成 requirement，同一批里多个同类对象会重复触发 `list_objects` | 用 `HashSet` 按 `(scope, kind)` 去重 |
| 5 | `schema/table_design/options.rs` | `pg_clause` 和 SQLite 分支用 MySQL 规则扫描选项 | PostgreSQL/SQLite 各自用自己的方言；补 SQLite 选项测试（`#` 不是注释） |
| 6 | `sql/span.rs` | 例程体里的 `CASE … END` 会被当成例程的 `END`，导致语句提前切分 | 词法器单独跟踪 `CASE` 嵌套，区分 `END CASE`/`END IF`/例程 `END`；补嵌套 CASE、`END CASE`、`END IF` 回归 |
| 7 | `scripts/check-deps.sh` | `cargo tree \| grep -q` 在 `pipefail` 下会被 SIGPIPE 短路，可能漏报 | 先把完整输出写到临时文件，再 grep，并检查 `cargo tree` 自身退出码 |

未处理：CodeRabbit 的 Docstring coverage 门禁（本仓库不要求每个函数都有 docstring，补 224 条会制造噪音）；Sourcery 因 diff 超过 20000 行无法 review；安全架构审查无 finding。

## 本地 review 跟进（2026-10-27）

[2026-10-26 PR #22 审查](../cr/2026-10-26-pr-22.md) 的结论与处理：

- **Standards（违反唯一扫描器）**：`schema/table_design/options.rs` 的 `tokens` 改为消费 `navidog_sql::span::spans`，`decoded` 接收方言（反斜杠只在 MySQL 下当转义）；补 PostgreSQL 带反斜杠标识符和 SQLite `#` 的回归。
- **Catalog 状态机**：完成加载时调用 `ScopeCache::finish_task`，`task_pending` 不再残留；`retain_connections`/`invalidate_connection`/`refresh_scope` 只在真正删除 entries/tasks/generation 时递增 revision 并通知；新增回归测试。generation token 保留，因为强制刷新会替换 task，只有 token 能阻止被替换的加载写回。
- **A-10**：`reencrypt` 直接遍历 `profiles`，不再临时改 `active`；`SecretField` 的辅助函数改成接收 `&Profile`。
- **A-07 结构拆分**（`TableSession`/`TableView`、`ResultSession`、`GridModel` 的 storage/edit/projection、`AfterCommit`）：已补齐，见下面“结构级拆分”一节。
- **A-07 复选框拒绝 0**：是对齐输入框回车规则的缺陷修复，放在单独的 `fix:` 提交里，不算搬家。

### 结构级拆分（2026-10-27）

- `grid/viewer`：会话/分页/事务/提交状态抽成 `TableSession`（`TableViewer` 实现 `Deref`/`DerefMut`，字段访问保持不变），渲染状态放 `TableView`；四个 `*_after_commit` 字段合成 `enum AfterCommit`（`None` / `Close` / `Hidden { hidden, reload }` / `Reload` / `AddRow`），加载/提交路径按枚举消费，行为不变。
- `query/results`：运行/提交/刷新状态抽成 `ResultSession`（`ResultView` 显式写 `result.session.*`），网格继续用现成的 `ResultGrid`。
- `grid/model`：`storage.rs`（行/单元格存储与选择）、`edit.rs`（编辑覆盖层/草稿/提交接受）、`projection.rs`（列布局/排序/隐藏，原 `layout.rs` 改名）。
- 方法体与测试断言未改；`cargo test`、`clippy -D warnings`、`fmt --check` 全过。

## 遗留

- `TableViewer` 的 `*_after_commit` 状态量合并成 `enum AfterCommit` 没做（理由见上）。
- `cargo tree` 之外的依赖方向保护只有 `scripts/check-deps.sh`；后续新增 crate 时记得把它加进脚本的 crate 列表。
- 统一 i18n backend 没有采用，改走方案 1；改路线的条件写在 00-overview。
