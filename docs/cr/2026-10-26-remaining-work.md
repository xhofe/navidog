# 整体审查遗留工作（2026-10-26）

## 范围

- 来源：[2026-10-24 整体审查](2026-10-24-overall-review.md)。
- 基线：`review/overall-fixes` / PR #21，最后包含本轮修复的提交为 `b53a6e7`。
- 本文只记录 **2026-10-24 审查中没有完全做完、或只完成了兼容层** 的事项。已经接受的外部限制（saved query 数据兼容、ad-hoc 更新签名）不列为待办，见文末“已接受的决定”。
- 这些事项不是合并阻塞项；当前审查的结论是剩余项都属于跨 crate/架构迁移，适合按主题另开 PR。每项都应优先做“只搬代码”的提交，再单独做行为变更。

## 总览

| 编号 | 主题 | 当前状态 | 建议 PR | 依赖 | 规模 |
| --- | --- | --- | --- | --- | --- |
| R-04 / D-05 | 统一 SQL 扫描/词法 | 已完成 | SQL lexer | 无 | 大 |
| D-08 | Catalog/OutlineCache 统一缓存 | 已完成 | ScopeCache | 无 | 中 |
| A-01 | `navidog-core` 拆 app-state | 已完成 | app-state split | 无 | 大 |
| A-02 | core 去掉 gpui-kit | 已完成 | 随 A-01 | A-01 | 中 |
| A-03 | `navidog-db` 去掉 table_design | 已完成 | schema crate | R-04 | 大 |
| A-06 | 缓存/选择状态收口 | 已完成 | Cache/selection | D-08 | 中 |
| A-07 | god object/超长文件 | 已完成 | file splits | 无 | 大 |
| A-10 | Connection `profiles + active` | 已完成 | connection model | 无 | 中 |
| S-07 | 共享 i18n 后端（可选） | 已完成（方案 1） | i18n backend | 无 | 中 |

建议合并顺序：**R-04/D-05 → D-08/A-06 → A-01/A-02 → A-03 → A-10 → A-07 → S-07**。A-07 是纯搬家，可以与其他项并行；R-04 先做能减少 A-03 的重复解析。

## 1. R-04 / D-05：统一 SQL 扫描与词法

### 当前做到哪

- `navidog_util::sql_text` 已收拢 `skip_quoted`、`skip_dollar_quoted`、`is_dollar_quote_start`。
- `navidog-db::sql` 已用共享跳过函数，并修正了 `foo$tag$` 和自定义 `DELIMITER` 的 UTF-8 边界。
- 参数扫描、语句切分、筛选解析、格式化和 table_design 解析仍是各自实现。

### 缺口

- 至少 6 个消费者：`db/sql.rs`、`util/sql_text.rs`、`util/sql_format.rs`、`grid/filter.rs`、`db/table_design/parse.rs`、`db/table_design/properties.rs`/`entries.rs`。
- 规则仍不一致：`#` 注释、反斜杠转义、`$tag$`、方括号标识符、嵌套注释、`BEGIN…END` 的边界各写一套。
- 不能只提供“跳过引号”的 helper；调用方需要统一的 `spans`/token 流，才能避免“同一个 SQL 在参数替换和分句器得到不同结论”。

### 目标

在 `navidog-util` 或新的 `navidog-sql` 提供：

```rust
pub struct Span { pub range: Range<usize>, pub kind: SpanKind }
pub fn spans(sql: &str, dialect: Dialect) -> Vec<Span>;
```

至少覆盖字符串、引号标识符、行/块注释、`$tag$`、参数标记、语句分隔符和括号深度。`Dialect` 不能反向依赖 `navidog-db`；把方言枚举放在 `navidog-model` 或 `navidog-util`，db 再 re-export。

### 验收

- 6 个消费者全部改为消费同一份 span/词法结果，删除各自的跳过函数。
- 回归用例：`SELECT $$ [$x] $$`、`SELECT E'a\\';b'`、`foo$tag$`、`#` 注释（MySQL）、嵌套块注释、`DELIMITER //`、`BEGIN…END`、反斜杠关闭的字符串、方括号标识符。
- 三种方言（MySQL/MariaDB、PostgreSQL、SQLite）的现有测试全过；补一个“同一段 SQL 六个消费者结论一致”的测试。
- 实库测试和 query builder 的 WHERE/ORDER BY 解析通过。

### 主要文件

`crates/navidog-util/src/sql_text.rs`、`crates/navidog-util/src/sql_format.rs`、`crates/navidog-db/src/sql.rs`、`crates/navidog-grid/src/filter.rs`、`crates/navidog-db/src/table_design/parse.rs`、`properties.rs`、`entries.rs`。

### 完成情况（2026-10-26，`refactor/remaining-work`）

- 新建 `navidog-sql`：`dialect`（从 `navidog-db` 移入，`db` 用 `pub use navidog_sql::dialect` 保持旧路径）、`keywords`（同上）和 `span` 词法器。`span::spans(sql, dialect)` 给出字符串、引号标识符、行/块注释（PostgreSQL 嵌套）、`$tag$`、`[$name]` 参数、括号深度、语句分隔符（含 MySQL `DELIMITER` 与 `BEGIN…END` 例外）和 MySQL `#` 注释；`span::check` 报告未闭合引号/注释与括号不平衡；`span::separators` 给出深度 0 的分隔符位置。
- 六个消费者全部改为消费同一份 span 结果，各自的跳过函数已删除：`db/sql.rs`（分句）、`util/sql_text.rs`（参数）、`util/sql_format.rs`（格式化/括号配对）、`grid/filter.rs`（WHERE/ORDER BY 拆分）、`db/table_design/parse.rs`（表体与片段校验）、`properties.rs` 与 `entries.rs`（列属性与索引/约束表单）。table_design 的解析函数增加 `Dialect` 参数，由调用方（designer、plan）传入。
- 回归用例集中在 `navidog-sql/src/span/tests.rs`；跨消费者一致性测试在 `navidog-grid/src/filter/consistency_tests.rs`（同一段 SQL 的字符串在分句、字段列表、参数、筛选、格式化、列属性六处都按文本处理）。MySQL/PostgreSQL/SQLite 三种方言的既有测试全过。

## 2. D-08：Catalog/OutlineCache 统一缓存

### 当前做到哪

- `Load<T>`、`ScopeCache<K,T>`、OutlineCache 已迁移到 `navidog-session::scope_cache`。
- OutlineCache 使用统一的 entries/tasks/失效清理。

### 缺口

`Catalog` 仍有五套 `HashMap`：

- `schemas: (connection,database) -> Vec<String>`
- `objects: (Scope,ObjectKind) -> Vec<ObjectInfo>`
- `ddl: ObjectRef -> String`
- `columns: (Scope,table) -> Vec<ColumnInfo>`
- `children: (Scope,table) -> TableChildren`

以及独立的 `tasks: HashMap<String,Task>`、`generation`/`generation_clock`。这些 key 类型不同，当前 `ScopeCache` 只按 `Scope` 键，无法直接套。

### 目标

- `ScopeCache<K,T>` 泛型化到任意 `K: Eq + Hash + Clone`。
- 五类 key 各自 typed newtype（而不是 `format!("{slot}:{key:?}")` 字符串）。
- `tasks`/generation 也按 typed key 管理；`invalidate_connection` 同时清理 entries、tasks、generation。
- 缓存失效由 shell/`Catalog::require()` 驱动，渲染只读，不在 render 里写状态。

### 验收

- `Catalog` 不再持有五套手写 map 清理逻辑；OutlineCache 和 Catalog 共享同一套加载/失效状态机。
- 关闭连接、切库、刷新、失败重试各有一个测试；延迟 fetch 不能覆盖新代际。
- `Catalog` 的 revision 只由缓存变更递增；状态栏/树不再因为“读缓存”而丢 observer。

### 主要文件

`crates/navidog-session/src/scope_cache.rs`、`crates/navidog-navigator/src/catalog.rs`、`crates/navidog-session/src/outline.rs`。

### 完成情况（2026-10-26，`refactor/remaining-work` 分支）

- `ScopeCache<K, T>` 泛型化到任意 `K: Eq + Hash`，entries/tasks 同一键；`retain` 同时清理两者。
- `Catalog` 的五类缓存全部换成 `ScopeCache` + typed key（`SchemaKey`/`ObjectsKey`/`TableKey`/`ObjectRef`），`tasks`/`generation` 用 `LoadId` 枚举（每类一个变体），`invalidate_connection`/`retain_connections`/`refresh_scope` 同时清理 entries、tasks、generation，不再手写五套 map 清理。
- 面板不再判断“是否要 load”：`Catalog::require(Requirement)` / `require_all` / `reload_all` 是唯一入口，`Catalog::require_selection` 由 Workspace 的 `SelectionChanged` 驱动；objects 面板的 `ensure_loaded` 与渲染期 `stalled_load` 补偿已删除，navigator/信息/DDL 面板只声明 `Requirement`。Session 打开后 Catalog 自己重启当前选择需要的加载。
- 测试：`catalog.rs` 覆盖缓存命中不查询、invalidate 清理 entries/tasks/generation、延迟结果不能覆盖新代际、失败条目只在 reload 时重试；`tests/catalog_lifecycle.rs` 覆盖选择驱动加载、关闭连接清空、重开恢复、删除连接清空；`tests/database_refresh.rs` 覆盖切库刷新和打开中的 DDL 面板。

## 3. A-01：拆开 `navidog-core` 的 app-state

### 当前做到哪

- 纯数据层 `navidog-model` 已抽出：`connection`、`object`、`crypto`、`paths`、`queries`、`json_store`、`column_widths`。
- `navidog-core` 通过 `pub use` 保持旧路径，feature crate 的 import 基本不变。

### 缺口

`navidog-core` 仍有：

- `settings`（`Settings` Global，49 个字段）
- `store`（`ConnectionStore` Entity/Global）
- `workspace`（`Workspace` Global、选择、状态栏文案）
- `favorites`、`snippets`、`vgroups`
- `actions`、`registry`
- `uri`（依赖 `Workspace::Selection`）

这些是 GPUI 应用状态，不适合与纯模型放在同一个 crate；`navidog-core` 仍是 god crate。

### 目标

新建 `navidog-app-state`：

- 拥有 GPUI `Global`/`Entity`、`Workspace`、`ConnectionStore`、`Favorites`、`VirtualGroups`、`Snippets`、`actions`、`Saved query` 路径逻辑。
- `navidog-core` 变成薄的兼容 re-export，或者删除；feature crate 改为直接依赖 app-state/model。
- `Settings` 拆分：纯数据留在 model 或 app-state 的 settings 子模块，`Global` 包装留在 app-state。

### 验收

- `navidog-core`（若保留）不再直接依赖 `gpui-kit`；app-state 的依赖方向是 app-state → model，不反向。
- 所有既有 import 路径有迁移方案；不要求一次改完 feature crate 的公开路径。
- `Settings`/`ConnectionStore`/`Workspace` 的序列化和 headless 测试全过。
- `cargo tree` 能证明 model 不包含 gpui。

### 主要文件

`crates/navidog-core/src/{settings,store,workspace,favorites,snippets,vgroups,actions,registry,uri}.rs`、`crates/navidog-core/Cargo.toml`。

### 完成情况（2026-10-26，`refactor/remaining-work`）

- `navidog-core` 整体改名为 `navidog-app-state`：9 个 GPUI 模块（settings/store/workspace/favorites/snippets/vgroups/actions/registry/uri）加 `init` 迁入新 crate，内部 `crate::object` 等纯数据引用改为 `navidog_model::…`，i18n 域从 `core.*` 改为 `app_state.*`（`locales/app_state.yml`）。
- `navidog-core` 已删除，没有兼容 facade：71 个文件里的 322 处 `navidog_core::…` 按模块直接迁到 `navidog_model::…`（connection/crypto/object/paths/queries/json_store/column_widths）或 `navidog_app_state::…`，Cargo 依赖按实际使用拆成 `navidog-model` / `navidog-app-state` 两条。
- A-02：`navidog-db` 只依赖 `navidog-model`，`cargo check -p navidog-db` 不再出现 gpui；新增 `crates/navidog-headless`（只依赖 model + db）证明驱动栈可以脱离 GPUI 编译并运行 SQLite；`scripts/check-deps.sh` 断言 model/db/sql/util/sql-log/headless 的 normal 依赖里没有 `gpui`，并禁止 `crates/navidog-core` 回潮，已接入 `lint.yml`。
- `docs/design/00-overview.md` 的 crate 表、依赖规则和里程碑 crate 列已同步；M7 的 ER 图 crate 暂记作 `navidog-er`，因为 `navidog-model` 已是纯数据层。

## 4. A-02：core 去掉 `gpui-kit`

### 当前做到哪

- `navidog-db` 已不依赖 gpui-kit。
- `navidog-session` 承担 DriverRegistry/Sessions/OutlineCache/scope_cache。

### 缺口

`navidog-core` 仍直接依赖 gpui-kit，且 9 个 app-state 模块尚未搬走。`A-02` 依赖 `A-01` 完成；不能只把依赖写在 app-state 里而继续让 core re-export GPUI 类型。

### 目标

- `navidog-model` 和 `navidog-db` 的 normal dependencies 中不再出现 `gpui-kit`。
- `navidog-core` 只保留纯 re-export 或完全移除；GPUI 只出现在 app-state/session/ui。
- 增加一个 headless 编译检查：只用 `navidog-model` + `navidog-db` 建一个 SQLite 连接/执行 SQL 的测试，不链接 GPUI。

### 验收

- `cargo tree -p navidog-model -e normal` 无 gpui。
- `cargo check -p navidog-db` 在 `default-features` 下无 gpui。
- headless 测试可编译运行；`navidog-session` 仍明确是 GPUI 层。
- CI 增加依赖方向检查，防止 core/db 回退。

### 主要文件

`crates/navidog-core/Cargo.toml`、`crates/navidog-model/Cargo.toml`、`crates/navidog-db/Cargo.toml`、`crates/navidog-session/Cargo.toml`。

## 5. A-03：`navidog-db` 去掉 table_design

### 当前做到哪

- `sessions`、`outline` cache、`log` 已移出 db（`navidog-session` / `navidog-sql-log`）。
- `navidog-db` 仍保留 `table_design/*`（约 4,000 行）、`metadata`、driver、dialect、SQL。

### 缺口

`table_design` 是建表/DDL 规划与解析，不是驱动协议；它还让 db 承担 UI 草稿模型的职责。它依赖 `dialect`、`Relational`、`metadata`，拆出时不能复制这些类型。

### 目标

- 新建 `navidog-schema`（或放回 `navidog-designer` 的非 UI 子模块），只依赖 `navidog-model` + `navidog-db` 的 dialect/driver 接口。
- `table_design` 的纯规划/解析留在 schema；UI 草稿状态留在 designer。
- `navidog-db` 只保留 driver/session trait、dialect、metadata、sql、params、ssh。

### 验收

- `rg 'table_design' crates/navidog-db/src` 为空；designer/query 的 import 更新。
- 现有 table_design 测试全部迁移并通过；实库 DDL 测试通过。
- `navidog-db` 的 public API 不破坏 feature crate；先加 re-export 过渡再删。
- 与 R-04 共用 SQL span/token 层，避免第 7 份解析器。

### 主要文件

`crates/navidog-db/src/table_design/`、`crates/navidog-designer/`、`crates/navidog-db/Cargo.toml`。

### 完成情况（2026-10-26，`refactor/remaining-work`）

- 新建 `navidog-schema`：`table_design`（parse/properties/entries/options/comments/plan/tests，约 4,000 行）整体迁入，依赖 `navidog-model` + `navidog-db`（`DbError`/`DbResult`/`Relational`/`Dialect`/`sql::split_script`）+ `navidog-sql`，没有复制任何驱动类型。
- `navidog-db` 删除 `pub mod table_design`，`rg 'table_design' crates/navidog-db/src` 为空，只保留 driver/session trait、dialect、metadata、sql、params、ssh、commit。唯一的仓库内消费者（designer、grid 一致性测试）改为 `navidog_schema::table_design`；公开 API 在同一次提交内完成迁移（仓库外没有消费者，因此没有保留过渡 re-export）。
- table_design 的解析改用 R-04 的 `navidog_sql::span`（`parse::positions`/`fragment`、properties `words`、entries `group`），没有第 7 份解析器；实库 DDL 测试随模块迁到 `navidog-schema` 的 `tests` 并保留 `NAVIDOG_LIVE` 开关。

## 6. A-06：缓存/选择状态收口

### 当前做到哪

- `NavigatorPanel.marked` 已删除，多选快照每帧从 `Workspace::selections()` 派生。
- `Catalog::refresh_connection` 不再调用 `Sessions::refresh`，改由 navigator 的 WorkspaceEvent 处理器刷新会话。
- 错误行/字段行的焦点恢复（S-05）已修。

### 缺口

- 四个面板仍各自实现 `ensure_loaded`（navigator、objects、info、ddl），条件不一致；objects 还有渲染期 `stalled_load` 补偿。
- `Catalog` 的缓存失效仍由 feature 自己驱动，缺少统一的 `Catalog::require()`。
- D-08 的 ScopeCache 未迁移完，A-06 无法真正统一“缓存/选择/derived state”。

### 目标

- `Catalog::require(key, fetch)` 统一“读缓存/排队加载/失效代际”。
- 删除四个 `ensure_loaded` 和 `stalled_load` 补偿，加载由统一状态机驱动。
- 选择只保留 `Workspace`；navigator 的 anchor/tree focus 只作为视图状态，不再成为第二权威。
- 与 D-08 同一个 PR 或前后紧邻的 PR，避免两次改 Catalog。

### 验收

- 选择变化、连接打开/关闭、切库、刷新、失败重试、删除连接各一个测试。
- 对象面板/信息面板/DDL 面板不再各自判断“是否要 load”。
- `Catalog` 的 revision/失效测试覆盖延迟 fetch 和连接删除。

### 主要文件

`crates/navidog-navigator/src/{catalog,objects,info,ddl,navigator}.rs`、`crates/navidog-session/src/scope_cache.rs`。

## 7. A-07：god object 与超长文件

### 当前做到哪

- `app/update.rs` 的 install/替换拆到 `update/install.rs`（1246 + 331 行）。
- `grid/viewer/mod.rs` 的 `impl Render` 拆到 `viewer/render.rs`（2882 + 161 行）。

### 缺口（当前行数）

| 文件 | 行数 | 仍混在一起 |
| --- | ---: | --- |
| `grid/viewer/mod.rs` | 2882 | 会话/分页/事务/筛选/单元格编辑/渲染 |
| `query/results.rs` | 1886 | 网格接线、可编辑探测、提交、重载、渲染 |
| `query/editor.rs` | 1742 | 生命周期、会话/上下文、保存/恢复、查找、工具栏 |
| `connection/form.rs` | 1493 | 五页表单、校验、秘密策略、测试连接、路径浏览 |
| `grid/model.rs` | 1738 | 存储、选择、编辑 overlay、投影、排序、查找替换、TSV |
| `app/update.rs` | 1246 | manifest/HTTP/下载校验/UI 仍未拆 |

### 目标

- 只做“搬代码”的提交：`TableViewer` 拆 `TableSession`（连接/分页/事务/提交）和 `TableView`（渲染）；7 个 `*_after_commit` 状态量改成 `enum AfterCommit`。
- `ResultView` 拆 `ResultSession`/`ResultGrid`；`QueryEditor` 拆 toolbar/save-recovery；`ConnectionForm` 拆 pages/secrets；`GridModel` 拆 storage/edit/projection；`update.rs` 拆 `manifest/http/install/ui`。
- 目标线：viewer/results/editor/form/model 各主文件 < 1,000 行；update < 600 行。

### 验收

- 纯移动提交，`git diff --color-moved` 可读；行为测试不改断言。
- 每个拆分 PR 至少包含一个现有测试的迁移；不新增行为。
- 拆分后 `cargo test`、`clippy`、headless UI 测试全过；截图测试只在外观受影响时补跑。

### 主要文件

`crates/navidog-grid/src/viewer/`、`crates/navidog-query/src/{results,editor}.rs`、`crates/navidog-connection/src/form.rs`、`crates/navidog-grid/src/model.rs`、`crates/navidog/src/update.rs`。

### 完成情况（2026-10-26，`refactor/remaining-work`）

纯搬家，行为测试的断言没有改动：

| 文件 | 拆分前 | 主文件现在 | 拆出的模块 |
| --- | ---: | ---: | --- |
| `grid/viewer/mod.rs` | 2882 | 768 | `TableSession`（连接/分页/事务/提交状态）、`TableView`（渲染状态）、`enum AfterCommit`、`session.rs`（连接/加载/分页）、`editing.rs`（提交/draft 状态）、`actions.rs`（工具栏命令与菜单）、`render.rs`（工具栏/底栏/主区域渲染）、`tests.rs` |
| `query/results.rs` | 1886 | 348 | `ResultSession`（运行/提交/刷新状态）、`tabs.rs`（运行状态与选项卡）、`editing.rs`（可编辑探测/提交/重载）、`render.rs`、`page.rs`、`tests.rs` |
| `query/editor.rs` | 1742 | 922 | `session.rs`（生命周期/会话/上下文）、`editing.rs`（文本/偏好/补全焦点）、`toolbar.rs`、`render.rs`；测试原本在 `editor_tests.rs` |
| `connection/form.rs` | 1493 | 534 | `pages.rs`（五页表单）、`validation.rs`、`secrets.rs`、`test_connection.rs`、`browse.rs`、`tests.rs` |
| `grid/model.rs` | 1738 | 139 | `storage.rs`（行/单元格存储与选择）、`edit.rs`（编辑覆盖层/草稿/提交接受）、`projection.rs`（列布局/排序/隐藏，原 `layout.rs`）、`tsv.rs`（TSV 读写）、`tests.rs` |
| `app/update.rs` | 1353 | 27 | `manifest.rs`、`http.rs`、`download.rs`、`ui.rs`、`tests.rs`（`install.rs` 保持原样） |

- 跨模块调用的方法改成 `pub(crate)`，同父模块内共享的自由函数改成 `pub(super)`；模块 `mod.rs` 保留结构体、trait impl 和构造器，并 `pub use` 原来的公开 API，外部路径不变。
- `TableViewer` 的会话/分页/事务/提交状态抽成 `TableSession`，`TableViewer` 通过 `Deref` 保持 `viewer.page` 这类字段访问；渲染状态放 `TableView`。四个 `*_after_commit` 字段合成 `enum AfterCommit`（`None` / `Close` / `Hidden { hidden, reload }` / `Reload` / `AddRow`），加载与提交的消费路径按枚举分支；设置状态时要求空闲，组合行为与拆分前一致。
- `ResultView` 的运行/提交/刷新状态抽成 `ResultSession`（访问写作 `result.session.status`），渲染半边仍是 `ResultView`，网格是现成的 `ResultGrid`。
- `GridModel` 拆成 `storage.rs`、`edit.rs`、`projection.rs`（原 `layout.rs` 改名），`tsv.rs` 不变；`find_cell`/`find_column_name`/`cell_matches` 留在 `mod.rs`。
- 拆分仍是纯搬家：方法体、断言没有改；`cargo test`（含 headless UI 测试）、`clippy`、`fmt` 全过。
- 每项都跑了 `cargo test -p <crate>`、`clippy`、`fmt`；没有新增行为，没有改断言。

## 8. A-10：`Connection` 的 `profiles + active`

### 当前做到哪

- `Connection` 顶层的 `general/advanced/ssl/ssh` 仍是激活配置；非激活配置在 `profiles`。
- `swap_profile` 交换 5 组字段；`with_active_profile` 的 Drop 守卫保证 reencrypt panic 时恢复。
- `profile: String` 表示激活 profile 名。

### 缺口

- “当前配置文件”仍是隐式不变量；任何 return/panic 之外的逻辑错误都可能留下错误激活配置。
- `reencrypt`、`duplicate`、`add_profile`、`switch_profile` 都要手动处理交换。
- 新增配置块（代理、认证）要同时改 `Profile`、`swap_profile`、`reencrypt`。

### 目标

内部模型改为：

```rust
pub struct Connection {
    pub profiles: Vec<Profile>,
    pub active: usize,
    // 非 profile 字段：id/name/group/...
}
```

- `active()` / `active_mut()` 访问激活配置；`profiles` 包含激活 profile。
- 序列化兼容：自定义 `Deserialize` 读取旧 JSON（顶层配置 + `profile` + 非激活 profiles），`Serialize` 可先写旧格式一个版本，再切新格式；测试两代 fixture。
- `reencrypt`/`duplicate`/`add_profile`/`switch_profile` 只操作 `profiles[active]`，不再 swap 字段。

### 验收

- 旧 `connections.json` fixture 能读；新写入的格式被记录并在验收文档说明兼容窗口。
- 所有 profile 的 secrets 在 reencrypt/duplicate 后仍正确；panic/early-return 不再破坏激活配置。
- 7 个以上 profile、重名、空 profile 名的边界测试。

### 主要文件

`crates/navidog-model/src/connection.rs`、`crates/navidog-core/src/store.rs`（或 app-state 迁移后的 store）。

### 完成情况（2026-10-26，`refactor/remaining-work`）

- `Connection` 的内部模型改成 `profiles: Vec<Profile>` + `active: usize`（激活项也在 `profiles` 里），删掉顶层 `general/advanced/ssl/ssh` 和 `profile` 字段；`active()`/`active_mut()`/`active_profile_name()` 访问激活配置，`Deref`/`DerefMut` 到 `Profile` 让 `connection.general` 这类既有字段访问不用改。`switch_profile`/`add_profile`/`remove_active_profile`/`with_active_profile` 只改 `active` 和 `profiles`，不再 `mem::swap` 五组字段；`with_active_profile` 仍用 Drop 守卫恢复激活项。
- 序列化：仍写旧布局（顶层是激活设置、`profile` 是激活名、`profiles` 只放其余配置），旧版本能读新版本写的文件；自定义 `Deserialize` 同时接受旧布局和 `profiles` 含全部配置 + `active` 索引的新布局（两代 fixture 都有测试）。`ConnectionsFile::reencrypt` 改成按索引遍历所有配置文件，`duplicate` 随克隆带走全部配置。
- 边界测试：7 个以上配置文件切换、重名、空名字（`navidog-model` 的 `many_profiles_keep_their_own_settings`）、旧/新两种 JSON 布局、panic 后激活项恢复；store 的 rekey/duplicate 覆盖两个配置文件的秘密字段。
## 9. S-07：共享 i18n 后端（可选）

### 当前做到哪

- 契约已收敛为 crate-local：每个 crate 的 `rust_i18n::i18n!` 只加载自己的 locale。
- `scripts/check-i18n.sh` 强制字面量 `t!` key 在本 crate 定义，并要求 en/zh-CN 双值。
- 跨 crate 文案走 `navidog_ui::db_error_text` 等显式 helper。

### 缺口

- 每个 crate 仍是独立后端，共享文案仍需函数封装或复制。
- CR-04 的已知限制：`t!(format!("..."))` 动态 key 不在静态检查内。
- 如果要跨 crate 复用 key，需要构建期合并 locale 或统一 backend，当前没有做。

### 建议先定策略

1. **保持 crate-local 契约**（推荐，成本低）：把“跨 crate 只能通过 helper”写进 AGENTS；给动态 key 加 allowlist/运行时测试，或至少记录限制。
2. **统一 backend**：构建期合并所有 `locales/*.yml`，验证加载顺序和 fallback；跨 crate 的 key 才可复用。需要修改每个 crate 的 `i18n!` 初始化和测试。

### 验收

- 方案 1：动态 key 有明确约束/检查；脚本注释说明只扫字面量；`docs` 记录。
- 方案 2：一个二进制只初始化一次 backend；跨 crate key 解析测试；所有现有 locale 双语言检查通过。
- 无论哪种，S-07 不再以“已修”掩盖统一 backend 未做的事实。

### 完成情况（2026-10-26，`refactor/remaining-work`，选择方案 1）

- 明确采用方案 1（保持 crate-local 契约）：不合并 backend，跨 crate 文案继续走显式 helper。
- `scripts/check-i18n.sh` 增加动态 key 检查：`t!` 的第一个参数不是字面量时，必须逐条出现在 `scripts/i18n-dynamic-keys.txt`（`<path> TAB <第一个参数>`），当前共 19 条（`navidog-ui` 的跨 crate helper、designer 的表头/选项、navigator 的 `col.*`/`kind.*`/`nav.color_*`、外壳的菜单 label）。脚本注释说明只做静态字面量检查，动态 key 的清单就是约束。
- `docs/design/00-overview.md` 的 i18n 段写入契约（crate-local、跨 crate helper、动态 key 登记、为什么不合并 backend），`AGENTS.md` 的“文案”条目同步；没有把“统一 backend 未做”继续记成已修。

## 建议的 PR 切分

| PR | 内容 | 前置 | 备注 |
| --- | --- | --- | --- |
| 1 | R-04/D-05 SQL lexer | 无 | 先统一 span，再迁 6 个消费者 |
| 2 | D-08/A-06 ScopeCache + Catalog::require | 无 | 一起做，避免两次改 Catalog |
| 3 | A-01 app-state 拆分 | 无 | 纯搬代码；feature import 可保留 re-export |
| 4 | A-02 core 去 gpui | PR 3 | 加 headless 编译测试 |
| 5 | A-03 table_design 拆出 | PR 1 | 复用 lexer，避免新解析器 |
| 6 | A-10 profiles + active | 无 | 先定兼容窗口，再动 store |
| 7 | A-07 文件拆分 | 无 | 可并行；每 PR 只搬一个文件 |
| 8 | S-07 统一 backend 或契约收尾 | 无 | 可选；先定策略 |

## 与其他遗留项的关系

- 2026-10-26 的 PR #21 第二轮 CR 另有两项“按决定保留”的限制：
  - **CR-02**：`known_hosts` 只支持精确主机名和 `|1|` 哈希；通配、`!`、`@revoked`、`@cert-authority` 不参与判断。见 [2026-10-26-pr-21.md](2026-10-26-pr-21.md)。
  - **CR-04**：i18n 脚本只检查字面量 key；动态 `format!` key 是已知限制，已并入本文 S-07。
- **不做 saved query 数据兼容**和**保留 ad-hoc 更新签名自动安装**是已接受的决定，不列入本清单；详情见 [验收记录](../pr/acceptance-overall-review.md)。
- A-01/A-02 与 A-03 都是公开 API/依赖方向迁移；不要与 bug 修复混在同一个 PR，避免 review 无法区分行为变化。

## 完成定义与通用验证

每个 PR 至少：

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p <affected crates> -j 1
scripts/check-raw-px.sh
scripts/check-i18n.sh
```

涉及 SQL/table_design/A-01/store 的改动还要跑：

- 实库测试（MySQL/MariaDB/PostgreSQL，`docker compose`）；
- `navidog-query` 的 Builder/参数/筛选测试；
- 旧 `connections.json` fixture（A-10）；
- 依赖方向检查（A-02）；
- 需要时跑 Metal 截图测试（A-07 的 UI 拆分）。

每完成一项，更新 [2026-10-24 整体审查](2026-10-24-overall-review.md) 的状态，并在本文件删除或标记对应条目；不要把“部分完成”留成永久状态。
