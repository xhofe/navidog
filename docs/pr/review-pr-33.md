# PR #33 审查：M5 插件体系

- 仓库：`xhofe/navidog`
- PR：<https://github.com/xhofe/navidog/pull/33>
- 基准：`main @ 58cb3cb`
- 头部：`m5-plugin-system @ 7535d28`（本地已 check out，本文行号以该提交为准）
- 范围：`git diff main...HEAD`，183 个文件，+40,449 / −1,140；其中 `vendor/` 29,043 行、`Cargo.lock` +2,384/−518、各 crate +8,399/−614、文档 +223
- 审查维度：第三方库复用、重复代码、代码设计与风格一致性、架构、实现合理性、插件编写便利性，以及测试/文档/依赖
- 审查方式：读规格（`03-plugin-api.md`）与 `acceptance-m5.md`、逐文件读 diff、跑 `cargo test -p navidog-plugin -p navidog-sql -p navidog-model`（全部通过）、复现 `cargo fmt --check` 的漏洞、查 `Cargo.lock` 判断候选库是否已在依赖树、对照 gpui-shell 0.8.1 源码确认框架能力边界
- 结论：**建议修改后合并（Request changes）**。方向、规格、权限模型和验收证据都是这个仓库里质量最高的一批；没有发现数据破坏或提权类缺陷。但有一批"每帧重新反序列化/重新编译正则"的性能问题、一处上下文互斥导致的能力失效、若干摆设型声明（类型擦除注册表、手写 allowlist）以及较大的架构外溢（核心 `Dialect`/`Relational` trait 被插件需求改写），建议在合并前处理第 9 节的 P1 项。

---

## 0. 结论摘要

### 0.1 做得好的地方

- **规格先行、提交可分**：`72af47e` 单独提交规格，实现分段提交，`docs/progress.md` 与 `docs/pr/acceptance-m5.md` 的证据链完整，验收项逐条对应到可运行的测试名。
- **权限模型是认真设计的**：默认停用、能力申请指纹、部分授权、扩大申请自动停用（`settings.rs:78-110`）、文件/网络 grant 只从已校验申请里逐项重建（`manifest.rs:258-286`），并且用官方 `PluginManifest::parse` 回灌校验，而不是自己实现一套 capabilities 语法。
- **失效模型一致**：`generation` 贯穿请求、上下文令牌、预览计划；取消/超时/过期响应一律拒绝（`protocol.rs:381-426`），插件停用时按 `Source` 统一撤销注册表条目、快捷键、驱动、会话（`manager.rs:137-194`），并保留连接与草稿。
- **没有 fork 框架**：复用 gpui-shell 的 `Policy`/`HostModule`/`ScriptView`，宿主的 GUI 浮层用官方组件目录，符合 `AGENTS.md` 的"gpui-kit 不 fork"。
- **危险操作有护栏**：非事务 DDL 部分失败会报告"已完成 N/M 步、可能有残留"，并拒绝重放（`preview.rs:74-85`）；预览冻结 SQL、baseline、草稿和插件代际；网格写入要求显式 begin/savepoint 契约（`commit.rs:41-52`）。
- **测试和检查覆盖到位**：`navidog-plugin` 11 个单元测试 + 6 个集成测试文件，包含真实点击的授权流程（`tests/management.rs`）、服务任务失败检测（`tests/failure.rs`）、Dock 布局恢复（`tests/dock.rs`）、HTTP 实库端到端（`tests/http_live.rs`）；`cargo test -p navidog-plugin -p navidog-sql -p navidog-model` 本地全绿。
- **i18n/token 纪律**：新增界面走 `t!`、`palette`、`tokens`，没有裸像素（`scripts/check-raw-px.sh` 通过），文档中英双写。

### 0.2 需要先处理的问题

| 级别 | 问题 | 位置 |
| --- | --- | --- |
| **P1** | 单元格渲染路径每帧 `serde_json::from_value::<Formatter>`，并在 `matches_column`/`apply` 里每单元格重新编译正则（最多 3 次/格/帧） | `crates/navidog-grid/src/grid/delegate.rs:547-556`、`crates/navidog-model/src/cell_formatter.rs:117,125` |
| **P1** | `number` 显示规则经 `f64` 往返，`bigint`/`numeric` 超过 2^53 后显示错误值，与本仓库"大整数用字符串"的既定原则冲突 | `crates/navidog-model/src/cell_formatter.rs:145-152` |
| **P1** | `ContextBridge` 每个窗口只有一个 `Target`，`set_grid` 与 `set_editor` 互相覆盖：任一网格内插件命令执行后，该窗口的 `editor.write` 失效直到重新激活编辑器 | `crates/navidog-app-state/src/plugin.rs:111-140` |
| **P1** | 声明式设计器的"未声明属性不得修改"检查漏三项（`zerofill`、`generated`、`primary_key`），未声明的改动会被放行 | `crates/navidog-schema/src/table_design/declared.rs:105-132` |
| **P1** | 用户可见的失败原因、授权/加载错误全部是英文硬编码，中文界面里直接显示英文；且 `plugin.yml` 未加 `include_str!` 跟踪 | `crates/navidog-plugin/src/manager.rs:227,318`、`panel.rs:105`、`preferences.rs:194-196`、`src/lib.rs:15` |
| **P1** | 加载插件时把生成的 `.navidog-service-v1.js` **写入插件目录**：只读安装、签名分发、插件目录被同步/备份都会出问题 | `crates/navidog-plugin/src/runtime.rs:92-103` |
| **P1** | 服务事件循环靠"`init` 里第一个 `cx.spawn`"这一隐式约定 + `Proxy` 包装 `cx` 来监督，插件作者无法自证、宿主无法给出准确诊断 | `crates/navidog-plugin/src/runtime.rs:73-91` |
| **P1** | `cargo fmt --check` 对本 PR 的若干文件**实际不生效**（超长行导致 rustfmt 放弃整块），已合入明显未格式化的代码 | `crates/navidog-plugin/src/driver.rs:80-86`、`host.rs:68-73`、`format.rs:91-101` |
| **P1** | 能力函数守卫生效范围不一致：`import_rows`/`export_rows` 的异步体没有 `is_active()` 检查，其余六个函数都手写了一遍 | `crates/navidog-plugin/src/host.rs:192-235` 对比 `:33,69,80,96,129,175` |
| P2 | 格式化贡献以 `Value` 存在注册表里，导致 manager 里 `formatter["id"].as_str().unwrap()`、grid 每帧反序列化 | `crates/navidog-app-state/src/plugin.rs:62-70`、`manager.rs:405-418` |
| P2 | 插件列表/详情没有自己的滚动容器；限额错误只在底部一行显示，与规格 §12"错误在对应项附近展示"不符 | `crates/navidog-plugin/src/preferences.rs:214-233`、`limits_view.rs:80-106` |
| P2 | 每个请求 `spawn` 一个 task 只为实现超时；token 表满 4096 时整体清空 | `crates/navidog-plugin/src/protocol.rs:301-311`、`crates/navidog-app-state/src/plugin.rs:147-149` |
| P2 | "是不是插件方言"用 `designer_templates().is_some()` 这种无关能力探测，共 4 处 | `crates/navidog-query/src/explain.rs:53`、`editor/toolbar.rs:319-325`、`crates/navidog-designer/src/object.rs:150,409` |
| P2 | `set_locked` 把"用户锁定"和"驱动不支持写入"压成一个 bool | `crates/navidog-grid/src/grid/mod.rs:340-346` |
| P2 | 启动时在窗口创建闭包里串行加载全部启用插件（每个独立 QuickJS VM） | `crates/navidog/src/lib.rs:82-85`、`crates/navidog-plugin/src/manager.rs:540-553` |
| P2 | `NAVIDOG_LIVE` 未设置时实库测试直接 `return`，CI 绿灯不代表实库链路通过 | `crates/navidog-plugin/tests/http_live.rs:16`、`tests/mysql_http_live.rs` |

### 0.3 合并建议

1. 先修上表 P1：格式化注册表类型化并预编译正则、`number` 精度、`ContextBridge` 双槽位、补齐属性变更检测、错误文案本地化、wrapper 移出插件目录、修复超长行。这些都是局部改动，不涉及协议变更。
2. 把 `ContextBridge` 的 `Target` 拆成 `grid`/`editor` 两个槽位（或按 panel id 存多个 target），这是唯一会让插件作者"昨天能用今天不能用"的功能缺陷。
3. 合并前把 `Dialect`/`Relational` 的公共 trait 变更拆成单独提交并单独说明（见 4.2、4.3），这样后续 M6 出问题时能单独回滚。
4. 长期建议（不必阻塞）：把 `manifest`/`protocol`/`settings`/`driver` 从依赖 GPUI 的 `navidog-plugin` 里拆出一个无 UI 的 `navidog-plugin-core`，让 `navidog-headless` 也能跑协议测试（见 4.4）。

---

## 1. 有成熟库却自己实现

判断标准：仓库里已经有的依赖 > 已在依赖树里的库 > 需要新增一个成熟 crate > 自己写。逐项给结论。

| # | 自实现 | 已有替代 | 结论 |
| --- | --- | --- | --- |
| 1.1 | 文件变更检测：1 秒轮询 + 递归 `read_dir` + SHA-256 | `notify` 8.2 **已在依赖树**（`gpui-component` 直接依赖） | **替换** |
| 1.2 | 列匹配：每次调用 `regex::Regex::new` | `regex`（已有依赖）、`RegexSet`、`OnceLock`/`LazyLock` | **替换**（改为注册时预编译） |
| 1.3 | manifest 闭合集合手写 allowlist + 字符串 `when` 二次匹配 | `serde` 枚举（已有依赖），`strum`（已在 `navidog-model` 依赖） | **替换** |
| 1.4 | `DialectDescriptor` 复制 `DialectFeatures` 的字段，再用属性字符串映射回 features | `DialectFeatures`（本仓库既定位置，`AGENTS.md` 明确规定） | **抽取**（一份表示） |
| 1.5 | `examples/http_server.rs` 手写 HTTP/1.1 | `hyper` 1.12 **已在依赖树**（`gpui-pre-reqwest`） | **保留**（仅 fixture，但见 1.5 的限制） |
| 1.6 | SQL 模板 `${…}` 扫描 | `minijinja`/`handlebars`/`tera` 都不在 lock，且引入会丢掉"渲染后再过唯一 scanner"的性质 | **保留**，抽一个公共 `markers()` |
| 1.7 | 通道：`async-channel` + `futures::oneshot` + `tokio::watch` 三套 | `tokio::sync::{mpsc,oneshot,watch}` 已用于全仓库 | **替换通道**，保留 mutex/watch |
| 1.8 | JS 服务监督（`Proxy` + 1 Hz `is_done`） | gpui-shell 0.8.1 **确实没有**任务监督 API（`Task` 只有 `cancel`/`is_done`，`cancel_policy_tasks` 是 `pub(crate)`） | **保留**，改为 `include_str!` + 解析测试 |
| 1.9 | `group_number` 手写千分位 | lock 里没有 `num-format`/`icu`，且 `navidog-util` 无同类 helper | **保留**（但见 6.2 精度问题） |
| 1.10 | TS 类型声明三份手写（`api-v1.d.ts`、`host.rs` 声明串、`runtime.rs` 测试串） | gpui-shell 已有 `declarations().validate()`；`include_str!` 可复用同一份 | **抽取** |
| 1.11 | 每个请求 `spawn` 一个 task 实现超时 | `tokio::time::timeout` 直接包 future 即可 | **替换** |

（下表只展开需要给判断依据的项；未展开的项在上表的「结论」列已足够。）

### 1.1 文件变更检测（最值得改的一项）

现状：`init` 里起一个 `cx.spawn` 死循环，每秒 `background_executor().timer(1s)`，然后 `audit_runtime_changes`；`source_revision` 递归遍历插件目录（跳过点文件，最多 4096 个路径），对 `path + len + format!("{:?}", modified())` 做 SHA-256（`manager.rs:527-535`、`:630-661`）。

问题不是"自己写了一个 watcher"这么简单：

- **release 构建也在跑**：`audit_runtime_changes` 的前半段用 `cfg!(debug_assertions)` 包着（`:557`），但每秒的唤醒、以及后半段对每个 installation 读 `runtime.failure(cx)` 一直存在。
- **漏事件**：`visit` 在 `paths.len() > 4096` 时直接 `return`（`:632-634`），继续往下遍历的顺序取决于 `read_dir` 顺序（最后才排序），所以文件多的插件会出现"改了某些文件不重载"，而这正是开发模式的唯一手段。
- **会走进 `node_modules`**：gpui-shell 会把插件声明的 git 依赖物化到插件目录下的 `node_modules`，而 walker 只跳过点文件（`:637`），于是依赖目录被纳入哈希，进一步放大 4096 截断的概率。
- **mtime 精度**：`format!("{:?}", metadata.modified())` 依赖平台精度，某些文件系统上 1 秒内两次写入可能同值；对"开发时连续保存"不友好。

已有方案：`notify` 8.2.0 是 `gpui-component` 的无条件依赖，`cargo tree -i notify` 能追到 `navidog-plugin`（经由 `gpui-kit → gpui-shell`），**直接加依赖不会新增任何 crate 或构建时间**。gpui-shell 自己的 `SourceWatcher`（250 ms 轮询、200 ms 防抖、只认 `js/mjs`、跳过 `node_modules`/`target`、最多 4096 文件）可以直接借策略；它的公开句柄 `ShellRuntime::watch` 需要 `Entity<ShellRoot>`，不适合 navidog 的多视图挂载，所以只借策略不借 API。

建议：`notify`（或 `notify-debouncer-mini`）按插件目录注册，防抖 200–300 ms，事件驱动重载，保留现有"重新校验权限再清理重载"的决策；`source_revision` 只留作 `scan()` 的一次性快照（或干脆删掉）。

### 1.2 格式化器的正则

`matches_column` 每次调用 `regex::Regex::new(pattern).is_ok_and(...)`（`:117`），`apply` 里对值再编译一次（`:125`）；而 `delegate.rs:553` 调 `apply` 时 `apply` 内部又调 `matches_column`——同一格最多 3 次编译。仓库里已经有正确写法（`navidog-sql/src/completion.rs:141` 的 `static ALIAS: LazyLock<Regex>`）。

建议：`Formatter` 在校验通过后一次性编译成 `CompiledFormatter { column: Option<Regex>, value: Option<Regex>, display }`，注册进注册表；渲染只做匹配。

### 1.3 manifest 闭合集合

`PERMISSIONS`/`LOCATIONS`（`manifest.rs:11-23`）、`menu`/`when`/`placement` 的 `contains` 检查（`:112-121`、`:145`），加上 `CommandContribution::is_enabled` 里第二次字符串匹配（`app-state/src/plugin.rs:28-49`）。用 `#[serde(rename_all = "snake_case")] enum` 后：解析期就报错、错误信息带字段路径、跨 crate 只有一份定义、UI 可以用 `strum::EnumIter` 生成菜单。

不引入 `jsonschema`/`garde`（都不在 lock，为一个 400 行的 manifest 引入 validator 不划算）；`schemars` 虽在 lock（`gpui-base` 依赖），但它只生成 schema 不校验，可作为作者工具的附属产物。

### 1.5 HTTP fixture 的边界要写清

手写 HTTP（`examples/http_server.rs:172-237`）只解析 `Content-Length`，固定 `HTTP/1.1 200 OK`、`Connection: close`，不处理 chunked/`Expect`/方法。作为"只在 127.0.0.1 上跑、只服务自家测试"的 fixture 可以接受，`hyper` 已在依赖树也不构成理由。但必须在 `docs/plugins/README.md` 里写明"这不是通用网关"，目前只有一句"禁止将它作为生产数据库网关"，建议补上协议限制，避免作者拿它当参考实现。

### 1.8 JS 监督可以保留，但要脱离字符串

`runtime.rs:73-91` 那段 JS 是 `format!` 拼出来的、双重大括号、没有任何测试真正解析它（只有一个跑通的集成测试间接验证）。至少改成 `include_str!("service.js")` + 一条"`load_application` 能解析该文件"的测试；`Proxy` 的 `get` trap 里对函数一律 `bind`，对 getter 会返回错误绑定，这类细节需要注释说明（见 6.x）。

### 1.10 声明与环境

`api_version()` 返回的 `1` 出现在 `runtime.rs:208`、`manifest.rs:88` 和每个示例 `navidog.json:2`；三份类型声明各自维护（`api-v1.d.ts`、`host.rs:236-260` 的声明串、`runtime.rs:271-275` 的测试串）。至少用同一个常量（`pub const API_VERSION: u32 = 1`），并把 `.d.ts` 作为 source of truth 用 `include_str!` 拼进 `declarations`。

---

## 2. 重复代码

按"会造成行为分歧"到"纯机械重复"排序。

### 2.1 三条"守卫式 DDL 执行"管道（行为后果最大）

- `crates/navidog-schema/src/table_design/preview.rs:47-84`（新增：begin → 排他锁 → 重读比对 → 执行 → COMMIT/ROLLBACK + 部分失败文案）
- `crates/navidog-schema/src/table_design/mod.rs:337-520`（既有 `apply`：同样的信封，另加 `prepare_database_sql`、`set_search_path_sql`、SQLite 重建的 pragma/legacy 处理）
- `crates/navidog-schema/src/object_design.rs:81-118`（第三份信封）
- 通用层已有 `crates/navidog-db/src/commit.rs:35-81`

目前没有爆炸性 bug：`PreviewPlan` 只服务声明式方言，而 `DeclaredDialect` 不实现 `prepare_database_sql`/`set_search_path_sql`（`navidog-sql/src/dialect.rs:292-297` 默认 `None`）。但调用点 `crates/navidog-designer/src/panel.rs:1103-1110` 只补了 `prepare_database_sql`、没补 `set_search_path_sql`，一旦声明式方言将来开放这两个钩子（比如支持 schema 的插件驱动），预览路径和保存路径就会在不同会话状态下执行 DDL。

建议：在 `navidog-schema` 里抽一个 `guarded_ddl(r, d, scope, before, statements)`，`apply`、`PreviewPlan::execute`、`object_design::apply` 全部走它；会话准备抽 `prepare_session(d, &scope)`。

### 2.2 格式化器渲染链两份，其中一份只为测试存在

`delegate.rs:547-556` 与 `grid/mod.rs:430-453` 是同一串四步（取列 → 取注册表 → `from_value` → `apply`），而 `formatted_text` 的调用者只有 `crates/navidog-grid/tests/grid.rs:859,874,876`。也就是说生产路径（复制/导出）仍是原值（符合规格 §14），但仓库里多了一份"复制会走格式化"的近似实现，将来有人把它接到复制上就会违反规格。

建议：抽 `GridDelegate::presentation(row, col, cx) -> Option<Presentation>`，渲染和测试都调它；`formatted_text` 要么删掉，要么就定义为"渲染时实际显示的值"。

### 2.3 `JsSession` 手写全部目录方法，绕开 `relational_catalog!`

`crates/navidog-plugin/src/driver.rs:203-257` 十个方法，每个都是 `self.read("op", json!({"scope":scope,...})).boxed()`；而 `navidog-db/src/lib.rs:110-162` 的宏正是为此存在，三个真实驱动都在用（`navidog-mysql/src/driver.rs:281`、`navidog-postgres/src/driver.rs:490`、`navidog-sqlite/src/driver.rs:217`）。

后果不只是重复：`AGENTS.md` 写的是"漏实现是编译错误"，但本 PR 同时给 `Relational` 加了带默认实现的 `object_snapshot`/`table_snapshot`（`navidog-db/src/driver.rs:314-326`），且这两个方法**不在宏里**——新增目录方法时，插件驱动会静默返回 `Ok(None)` 而不是编译失败。这与仓库的核心不变量直接冲突（见 4.3）。

### 2.4 一条命令注册三遍

`manager.rs:329-344`（快捷键）、`:345-367`（`CommandRegistry`）、`:369-394`（`Contributions.commands`）三个 `for command in &plugin.extensions().commands`，每次重建 `format!("{}/{}", ...)`；同时 `manifest::Command`（6 字段）与 `app_state::plugin::CommandContribution`（6 字段）几乎逐字段相同。加一个命令属性要改三处循环和两个结构体，漏一处就是"菜单里有、工具栏没有"。

### 2.5 限定 ID 的构造/解析共 12 处

构造：`manager.rs:334,347,373,399,409-413,423`、`host.rs:201`、`driver.rs:59`；解析：`manager.rs:453,480`、`panel.rs:85`、`driver.rs:198`；前缀判断：`manager.rs:178` 的 `starts_with(&format!("{id}/"))`。

建议：`navidog_app_state::plugin::qualify(plugin, id) -> SharedString` 与 `unqualify(id) -> Option<(&str, &str)>`，顺便把 ID 字符集断言收到一处。

### 2.6 `plugins.json` 路径在 `manager.rs` 里推导 5 次

`:68`、`:122-126`、`:198-203`、`:229-234`、`:305-310`，全是 `self.root.parent().unwrap_or(&self.root).join("plugins.json")`；数据目录同理（`:244-249`）。读写路径各自推导，一旦配置目录布局变化就会出现"读 A 写 B"且仍然编译通过。

### 2.7 `ResultSet → JSON` 两份 + 三处形状声明

`host.rs:264-266` 的 `result_json`、`examples/http_server.rs:98` 的同一段 `json!`；形状另在 `protocol.rs:17-31,56-69`、`grid/mod.rs:1085-1093`、`api-v1.d.ts:3-5` 各写一遍。建议给 `navidog-db::ResultSet` 一个唯一的线上表示（或 `impl From<&ResultSet> for Value`），并让 `.d.ts` 由同一处生成/校验。

### 2.8 `TableSnapshot` 与 `StructuredTableSnapshot` 各存一份 revision

`model/table_design.rs:51-63` 与 `:82-87` 都带 `revision`/`unknown`/`attachments`，`schema/table_design/mod.rs:190-195` 每次加载都把外层三个字段抄进内层；示例服务器（`http_server.rs:166`）也在两处同时输出。外部变更检查比较的是被填充的那一份（`preview.rs:40`、`table_design/mod.rs:351`），驱动只填外层就绕过检查。

### 2.9 `FieldControl` 校验两份

`navidog-sql/src/declared.rs:275-290` 与 `navidog-schema/src/table_design/declared.rs:40-58`：Number 有限、Checkbox 只认 `true/false`、Select 必须在候选里，三条规定同样，文案不同。`FieldControl` 五个变体里 `Text`/`Sql` 两处在两边都是 `_ => {}`——加新变体时很容易只改一边。

### 2.10 列属性词表有 4 份表示

`navidog-sql/src/declared.rs:114-138`（17 个字面量）、`navidog-designer/src/panel/structured.rs:897-915`（同样的 17 个）、`navidog-schema/src/table_design/declared.rs:105-132`（其中 14 个当作变更检测键）、以及 `navidog-schema/src/table_design/properties.rs` 里的 `ColumnProperty` 枚举。任一列表拼错都会静默改变"未声明改动是否被拒绝"的行为，没有编译错误。

### 2.11 "Plugin generation has ended." 11 处

`host.rs` 6 处（`:33,69,80,96,129,175`）、`protocol.rs` 5 处（`:226,233,235,242,271`）。这段字符串是插件侧分支依据之一，应该收成 `Bridge::generation_error()` / `fn live(&Bridge) -> Result<(), HostError>`。

### 2.12 示例驱动插件几乎是复制粘贴

`examples/plugins/com.example.http-mysql/main.js` 与 `...http-postgres/main.js` 完全相同（31 行），manifest 90% 相同；共享的分批循环硬编码 `offset += 1000`，而宿主默认批量就是 1000（`settings.rs:28`），限额调小后两个示例会一起失败。建议抽一个 `_shared/http-service.js`，批量从事件里带。

### 2.13 其他机械重复（不展开）

- `plugins.json` 保存分支 5 处；`Plugins::installations` 没有 `installation(id)` 取值器，11 处手写 `find`（`manager.rs:138,293,455,481-482,567,574,599,623`，`panel.rs:88-91`，`preferences.rs:37,110`）。
- `String` 错误转换约 32 处（`manager.rs` 12、`manifest.rs` 9、`settings.rs` 4、`HostError::new(e.to_string())` 7）。
- `KeyBinding::load(…, Box::new(action), …, cx).expect(…)` 迁移样板 4 处（`navidog/src/menus.rs:53-70`、`navidog-grid/src/viewer/mod.rs:127-136`、`manager.rs:331-341`）。
- `run_operation_rel` 与新增的 `run_plugin_operation_rel` 函数体相同（`sessions.rs:339-348` vs `:350-362`），`OperationScope::run`/`run_cancellable` 同理。
- `ColumnType` 字面量样板：MySQL 14、PostgreSQL 17、SQLite 16 条 `Cow::Borrowed` + `vec![...]`（见 4.2），可收成 `column_type!` 宏。
- `${…}` 扫描两处：`navidog-sql/src/template.rs:61-72,100-110` 与 `navidog-util/src/sql_text.rs:100-117`（用途不同——SQL 模板 vs 片段占位符，但扫描逻辑一致，建议抽公共 `markers()`）。

---

## 3. 代码设计与风格不统一

### 3.1 同一份数据存两个注册表

`CommandRegistry`（`CommandInfo { title, icon, shortcut, action, locations, enabled }`，`navidog/src/lib.rs:151` 也用它）与 `Contributions.commands`（`CommandContribution { title, title_zh, icon, shortcut, location, when }`）各存一份命令描述，`manager.rs:345-394` 连续填两遍。后果：`title_zh` 只有后者有，`action` 只有前者有；`locations` 是 `Vec`、`location` 是单值；两边的 `enabled` 语义不同（`CommandInfo::enabled` 是闭包，`CommandContribution::when` 是字符串）。这直接违反了 `03-plugin-api.md` §2 的"统一注册表"。

### 3.2 注册表的值类型被擦除成 `Value`

`Contributions { formats: Registry<Value>, formatters: Registry<Value>, panels: Registry<Value> }`（`app-state/src/plugin.rs:62-70`）。而 `Format`/`Panel`/`Formatter` 都有强类型定义（`manifest.rs:45-64`、`model/cell_formatter.rs:41-47`）。`Registry<V>` 没有 trait bound（`registry.rs:19-44`），换成 `Registry<Format>` 是零成本重构，却能删掉：

- `manager.rs:412` 的 `formatter["id"].as_str().unwrap()`；
- `delegate.rs:551`/`mod.rs:447` 的每帧 `from_value`；
- `main_window.rs:1200-1230` 里对 `value["placement"]`/`value["size"]`/`value["title"]` 的手工解析；
- `panel.rs:114-124` 与 `main_window.rs:1214-1220` 对同一份 JSON 的两次不同解析（一次回退成 `"unavailable"`/`"Plugin"`，一次回退成 `"Plugin"`）。

顺带：`Panel` 的 `title`/`placement`/`size` 在 `manifest.rs:58-64` 已经解析过一次，注册时又 `serde_json::to_value` 回 JSON，再在三个 UI 地方解析回来——绕了一圈。

### 3.3 三套快捷键 API 并存

同一 PR 里同时出现 `KeyBinding::new(...)`（`navidog-ui/src/tabs.rs` 未改、`grid/viewer/mod.rs:124-127` 改为 `load`）、`KeyBinding::load(...).expect(...)`（`menus.rs:53-70`、`viewer/mod.rs:127-136`、`manager.rs:331-341`）。`expect("valid favorite shortcut")` 对 18 个常量键位做运行时断言，属于把编译期常量当作运行期输入。

### 3.4 模块内实现块与测试模块被切成若干段

- `manager.rs`：`impl Plugins` 在 `:63`、`:539`、`:555` 三处（`:539` 与 `:555` 只隔一个 `start_enabled`）。
- `grid/mod.rs`：`impl ResultGrid` 的 `publish_plugin_context` 放在 `mod tests` **之后**（`:1073`），`SetFormatter` action 定义在文件最末（`:1112`）。
- `protocol.rs` 有 3 个 `#[cfg(test)] mod`（`mod tests` `:466`、`mod deadline_tests` `:522`、`mod binary_tests` `:565`）；`format.rs` 的测试函数则被压成一行一条语句。
- `navidog-plugin/src/lib.rs` 的模块声明按 `manifest, protocol, runtime, settings` → 空行 → `host` → 空行 → `manager` + `pub use` → `preferences` → `i18n!` → `driver` → `panel` → `format` → `limits_view` 排列，既非字母序也非分层顺序，全部 `pub mod`（`protocol`/`settings` 更像内部模块）。

对照本仓库其他 crate（`navidog-grid/src/lib.rs`、`navidog-query/src/lib.rs`）：一个 `mod` 块、`pub use` 集中、`init` 在末尾。建议对齐。

### 3.5 缺少 locale 跟踪

其他 5 个有 locale 的 crate 都在 `lib.rs` 里写了 `const _: &str = include_str!("../locales/xxx.yml");`（`app-state`、`connection`、`db`、`query`、`sqlite`），**`navidog-plugin` 没有**。`rust-i18n` 的宏在编译期读文件，缺这一行时只改 `plugin.yml` 不会触发重编译，容易出现"改了文案但界面没变"。

### 3.6 命名与参数风格

`declared.rs` 里大量 `fn plan(d: &dyn Dialect, ...)`、`table_design/mod.rs` 里的 `r`/`d`、`grid/delegate.rs` 的 `d`/`c`、`host.rs` 的 `cx`（可接受）之外，还有 `InstalledPlugin::read` 里 `let mut id = |id: &str|` 用闭包遮蔽同名函数式变量（`manifest.rs:97`）。仓库其他地方的参数是 `dialect`/`relational`/`column`。单字母在 300 行的规划器里显著降低可读性。

### 3.7 `#[non_exhaustive]` 被普遍误用

本 PR 新增了 23 个 `#[non_exhaustive]`，包括 `Command`/`Format`/`Panel`/`Extensions`/`StreamColumn`/`Origin`/`Chunk`/`DriverDescriptor`/`DriverDescriptor` 里的 `DialectDescriptor`/`DesignerDescriptor`/`FieldDescriptor`/`ObjectDesignerDescriptor`、`Limits`/`Approval`/`Formatter`/`Match`/`CommandContribution`，但这些类型要么只在本 crate 内构造（`non_exhaustive` 无意义），要么是**反序列化目标**（`non_exhaustive` 对 `Deserialize` 不产生限制，却会阻止下游用结构体字面量构造）。仓库既有类型（如 `navidog-db` 的 `ResultSet`）并没有这个标注。建议只保留真正面向外部构造的公开数据类型的标注。

### 3.8 用户可见文案是英文硬编码

`manager.rs:227` `"Plugin capabilities changed; review permissions again."`、`:318` `"Plugin needs approval before it can be reloaded."`、`preferences.rs:194-196` 直接把 `plugins.failure()` / `self.error` 渲染出来、`panel.rs:105` 显示 `plugin.failure()`。这些都会以英文出现在中文界面。`AGENTS.md` 的规则是"每个 crate 的 `locales/*.yml` 同时写 `en` 和 `zh-CN`"，错误串也必须走 `t!`（或映射成错误码 + 本地化文案，`DbError::coded` 已有先例）。

---

## 4. 架构设计

### 4.1 PR 范围偏大，且混入了框架补丁升级与 29k 行 vendor

`main` 上是 `gpui-kit = "0.8"`（锁定 0.8.0），本 PR 改为 `=0.8.1` 并引入 `gpui-shell = "=0.8.1"`，于是 0.8.0→0.8.1 的 API 迁移散落在与插件无关的文件里：`ScrollWheelEvent`→`ScrollEvent`（`navidog-ui/src/tabs.rs:427`、`crates/navidog-query/src/explain_visual.rs:388-403`）、`KeyBinding::new`→`KeyBinding::load`（`menus.rs`、`viewer/mod.rs`）、`TestSupportExt` 导入。另外 `[patch.crates-io]` 把整个 russh 0.64.1 复制进 `vendor/`（29,043 行、59 个 `.rs`），只为了 RSA rc.18→rc.19 的 pin。

两件事本身都有理由（gpui-shell 需要 0.8.1；russh 上游未跟上），但放在一起导致：review 无法区分"插件引入的回归"和"框架补丁引入的回归"；`vendor/` 之后每次上游发布都要人工 rebase。建议至少拆成两个 PR 或两个显式提交（`build: pin gpui-kit 0.8.1 + vendor russh` 与 `feat: plugin host`），并在 `docs/` 记录 vendor 的升级/移除流程（`vendor/russh/README.navidog.md` 已有说明，但没写进 `progress.md` 的维护项）。

### 4.2 核心 `Dialect` trait 被插件需求改写，并付出全局代价

为了让"插件方言"拥有自己的字符串，trait 里 24 个方法的返回类型从 `&'static str` / `&'static [T]` 改成 `&str` / `&[T]`（`navidog-sql/src/dialect.rs:216-360`），波及 MySQL/PostgreSQL/SQLite 三个驱动（+88/−85、+104/−102、+95/−93）。代价已经落到生产路径上：

- `ColumnType { name: &'static str, synonyms: &'static [&'static str] }` 改成 `Cow<'static, str> + Vec<String>`，于是 MySQL 的 `const COLUMN_TYPES: &[ColumnType]`（14 条编译期常量）变成 `static COLUMN_TYPES: LazyLock<Vec<ColumnType>>`（`navidog-mysql/src/dialect.rs:203-296`），首次访问时分配 14 个 `Cow` + 14 个 `Vec`；PostgreSQL/SQLite 同样。设计器类型菜单每次展开都读这个表。
- `Relational::object_kinds` 从 `&'static [ObjectKind]` 改成 `Vec<ObjectKind>`（`navidog-db/src/driver.rs:359-361`），每次调用分配一次；导航器/设计器路径会高频调用。
- trait 上新增了 9 个方法，其中 `designer_templates`/`designer_descriptor`/`object_designer`/`plugin_generation`/`grid_writes`/`savepoint_sql`/`rollback_savepoint_sql`/`column_property`/`explain_sql` 里，有 7 个只对插件方言有意义（三个原生方言一律走默认实现）。核心抽象开始承载单一实现者的需求。

更干净的做法（不必现在做，但值得记录）：`Dialect` 保持 `&'static` 与"纯函数式"契约，另立 `trait DeclaredDialectExt`（或让 `DeclaredDialect` 自己提供 `column_types_owned()`，由消费者用 `Cow`/`Vec` 适配）；`plugin_generation` 完全可以放进 `DialectFeatures` 之外的 `DialectOrigin` 枚举，避免"用 Option<u64> 表示我是插件"。

### 4.3 `Relational` 的默认实现破坏了仓库的核心不变量

`AGENTS.md`：驱动用 `relational_catalog!` 补齐目录方法，"漏实现是编译错误"。本 PR 新增的 `object_snapshot`/`table_snapshot` 直接给了默认实现 `Ok(None)`（`navidog-db/src/driver.rs:314-326`），且没有加入宏。于是：

- 任何真实驱动（含未来的内置驱动）漏实现时不会编译失败，而是静默变成"没有结构化快照"；
- `navidog-schema/src/table_design/mod.rs:190-195` 与 `object.rs:409-414` 又依赖 `designer_templates().is_some()` 判断"必须提供快照"，两套判据不一致时错误信息是 `"Plugin driver must provide a structured object snapshot."`——把"没实现"误报成"插件没提供"。

建议：把两个方法加进 `relational_catalog!`（默认实现改为 `unsupported_catalog!` 那种显式 panic/Err），或者引入 `trait StructuredSnapshot` 由需要它的驱动显式实现。

### 4.4 纯逻辑被放进依赖 GPUI 的 crate，headless 无法覆盖

`navidog-plugin` 同时依赖 `gpui-kit`、`gpui-shell`、`navidog-app-state`、`navidog-ui`、`navidog-session` 和纯逻辑层（`model`/`db`/`sql`）。其中 `manifest`/`protocol`/`settings`/`driver`（约 1,600 行）完全不需要 UI，却只能通过 `gpui_kit::test`（`TestAppContext`、窗口、Metal）来测；`navidog-headless` 也无法加载 JS 驱动，所以"插件驱动的目录方法"只能在完整 GUI 测试里验证。

这与仓库的分层规范（`navidog-model`/`db`/`sql`/`schema`/`util` 不依赖 GPUI，`navidog-headless` 证明）方向相反。建议拆 `navidog-plugin-core`（manifest/protocol/settings/driver/dialect 声明，无 GPUI）与 `navidog-plugin`（runtime/manager/panel/preferences/driver 注册，依赖 GPUI），后者只做胶水。

### 4.5 `ContextBridge` 的"每窗口一个 target"模型

`Targets: HashMap<WindowId, Target>`，`set_grid` 与 `set_editor` 都是 `insert`（`app-state/src/plugin.rs:111-140`），因此同一窗口的网格上下文与编辑器上下文**互斥**。`ResultGrid` 在 `on_action(PluginCommand)` 里先 `publish_plugin_context`（`grid/mod.rs:755-757`）、`TableViewer::set_active` 也会发布（`viewer/mod.rs:335-337`）、`QueryEditor::set_active` 发布编辑器上下文（`editor/mod.rs:289-310`）。实操后果：

1. 打开查询结果后在网格里执行一次插件命令 → 该窗口的 editor target 被替换成 grid-only；
2. 之后任何 `editor.write` 能力调用（或 `when: "editor"` 的命令）都会报 `"Context has no query editor."`，直到用户重新点一次查询编辑器标签；
3. `when: "grid"` 的命令在编辑器聚焦时同样被禁用。

这是"用户什么都没做错，能力突然失效"的类型。建议 `Target { alive, grid: Option<GridRead>, editor: Option<EditorWrite>, panel }` 已经具备双槽结构了——把 `set_grid`/`set_editor` 改成就地更新字段并保留另一槽，或按 `panel: EntityId` 存多份 target，由 token 记录更精确的宿主。

### 4.6 `manager.rs`（661 行）里的 `Plugins` 是多职责实体

发现、授权持久化、启停、加载/卸载、贡献注册、驱动注册、快捷键清理、命令派发、文件监听、运行期健康检查、代际管理全在一个 `struct Plugins` 里。可维护性问题已经显现：`enable()`/`reload()` 合计 110 行、`register()` 125 行、`stop()` 58 行、`audit_runtime_changes()` 72 行；测试只能通过 UI 点击间接覆盖（`tests/management.rs`）。

建议按职责拆：`Registry`（贡献注册/撤销）、`Approvals`（持久化与对账）、`RuntimeHost`（加载/代际/健康检查）、`Plugins`（门面与 UI 通知）。

### 4.7 生成代码写入插件目录

`runtime.rs:92-103` 每次加载都把 `.navidog-service-v1.js` 写进插件目录（并检查已有文件不是符号链接）。问题：

- 只读安装（`/Library`、被 `chmod -R a-w`、签名/公证后的 app 资源）直接加载失败，且错误信息是 IO 错误；
- 把"用户提供的目录"变成宿主写目标，备份/同步工具会把它当成插件内容，卸载插件时也不会清理；
- `.gitignore` 里加了一条 `.navidog-service-v1.js`（仓库根），说明这个文件已经漏进过 `examples/plugins/`，属于"生成物污染源码树"的信号。

如果 gpui-shell 的 `load_application` 必须要求入口在插件目录内，那么至少应该：写入 `plugin-data/<id>/` 并用相对导入，或者在文档里明确"插件目录必须可写"，并把它写进安装要求。

### 4.8 `tokio::task_local` 作为隐式日志上下文

`navidog-db/src/log.rs:35-41` 加了 `PLUGIN_SOURCE` task-local 与 `with_plugin_source()`，并且优先级是 task-local > `tag.plugin_source`（`:20-23`）；而 `driver.rs:197-198` 又用"驱动 ID 里带 `/`"反推插件名去填 `tag.plugin_source`。同一个信息有两个来源、两种语义（"谁发起了 SQL" vs "谁提供了驱动"）、覆盖关系靠优先级兜底。

建议：`Tag` 增加 `origin: Option<Origin>` 并显式传递，删掉 task-local；或反过来只留 task-local。无论哪种，都不要"两个都填、谁优先靠 `.or_else`"。

### 4.9 启动路径串行加载

`navidog/src/lib.rs:82-85` 在窗口创建后的 `update` 闭包里调用 `start_enabled`，对每个启用插件同步 `reload`（`manager.rs:540-553`），每个插件要新建独立 `ShellRuntime`（含 QuickJS VM 与组件目录）。20 个插件就是 20 次同步 VM 初始化，期间窗口不可交互。建议：延后到首帧之后、限制并发，或按需懒加载（面板打开/命令触发时）。验收里没有启动耗时数据，建议补一条。

### 4.10 无进程隔离（已知限制，措辞要更清楚）

规格与验收都写了"不提供进程隔离，Shell 无堆上限"。这在 API v1 可以接受，但 `docs/plugins/README.md` 里应出现一句面向用户的"插件代码与宿主同进程、权限是唯一边界"，而不是只在设计文档里。

---

## 5. 代码风格

### 5.1 `cargo fmt --check` 对部分文件失效（最需要修的风格问题）

本 PR 声称 fmt 通过，`cargo fmt --all --check` 在本地也确实返回 0。但代码里存在大量明显未格式化的行，例如：

- `crates/navidog-plugin/src/driver.rs:80-86`：`let request=bridge.request("driver.request","connect",json!({...}),params.connect_timeout)?;` 一行 300+ 字符，随后 `let response=request.await?;`、`let mut session=JsSession {bridge,dialect,...};`
- `crates/navidog-plugin/src/host.rs:68-73`：`module=module.function("list_connections",move |_| {` 与 `let store=ConnectionStore::global(cx); let store=store.read(cx);`
- `crates/navidog-plugin/src/format.rs:91-101`：一个测试函数里每行 2–3 条语句（`let bridge=...;let worker=...;`）
- `crates/navidog-plugin/src/host.rs:237`（597 字符的声明串）、`:265`（348 字符的 `result_json`）、`protocol.rs:282`（226 字符）、`runtime.rs:273`（302 字符的测试声明串）、`driver.rs:84`（267 字符）
- `crates/navidog-designer/src/object.rs:406-414`：`for (key,input) in &this.options {let value=...;input.update(cx,|s,cx|s.set_value(value,window,cx));}`

原因不是配置，而是 **rustfmt 的行为**：当一个语句无论如何都放不进 `max_width`（比如上面那条 300 字符的 `json!`）时，rustfmt 会放弃格式化它所在的整个块，于是块内其余语句也保持原样，而 `--check` 不会报错。本地可复现：

```sh
printf 'fn f(b:P)->R{async move{let r=b.request("a","b",json!({%s}),p)?;let x=r.await?;Ok(())}.boxed()}\n' '"k":"v",...' > /tmp/t.rs
rustfmt --edition 2024 --emit stdout /tmp/t.rs   # 整块原样输出，退出码 0
```

影响：CI 的 fmt 门禁对这些文件形同虚设，review 时也看不出"哪里没格式化"。建议：修复这些行（把 `json!` 拆成多行、`let` 拆句），并考虑在 `scripts/` 加一条 `awk 'length>120'` 的简单检查（仓库已有 `check-raw-px.sh` 这类脚本先例），或在 CI 里对 `rustfmt` 输出做 `error_on_line_overflow` 检查（nightly 选项）。

### 5.2 一行多语句与超长行

`protocol.rs` 6 处、`host.rs` 4 处、`format.rs` 4 处、`driver.rs` 4 处超过 120 字符（不含 5.1 提到的 >300 字符行）。同一文件内风格也不统一：`driver.rs` 的 `connect` 是压扁的，`JsSession` 的其余方法（`:203-257`）是正常多行的。

### 5.3 注释与文档

- 每个模块有一行 `//!`，但没有 crate 级结构说明（`lib.rs` 只有一句）。
- `runtime.rs` 那段 20 行 JS 没有任何行内说明，`Proxy` 的行为、`serviceTask` 的语义、"为什么必须第一个 spawn"只能从代码猜。
- `WireCell::text` 把二进制转成 `\x…`（`protocol.rs:39-52`）没有注释说明为什么是 Postgres 十六进制语法（见 6.9）。
- 反例（正面）：`navidog-sql/src/dialect.rs:214-216` 关于 `DialectFeatures` 与 `check-dialect-branches.sh` 的注释就是好样板。

### 5.4 错误信息风格不统一

同一类失败有 4 种句式：`"Unknown or cancelled plugin request."`（句号结尾）、`"Plugin event queue is full."`、`"Plugin request timed out."`、`"Limits must be positive and cumulative limits must cover one batch."`；大小写与简洁度不一。

---

## 6. 实现合理性

### 6.1 性能：渲染路径上的重复工作

| 位置 | 每帧成本 |
| --- | --- |
| `delegate.rs:551` | 每可见单元格一次 `serde_json::from_value::<Formatter>`（含 `BTreeMap`/`Vec`/`String` 分配） |
| `cell_formatter.rs:117` | 每单元格一次 `Regex::new`（列名） |
| `cell_formatter.rs:125` | 每单元格一次 `Regex::new`（值），且 `apply` 内部又调 `matches_column` |
| `delegate.rs:469-476` | 每次右键菜单构建再 `from_value` 一遍全部 formatter |

PR 自己的数据显示 20 列全开时每帧 +0.14–0.17 ms（该示例规则只按类型匹配，没有正则）。当规则用列名/值正则匹配时，按 30 个可见单元格估算约 90 次正则编译/帧——只是还没到卡顿阈值。数据量更大、列更多或规则更多时这条曲线是线性的，而修复成本很低（注册表存 `Arc<CompiledFormatter>`）。

### 6.2 `number` 规则丢大整数

`apply` 走 `value.parse::<f64>()` 再 `format!("{n:.2}")`（`cell_formatter.rs:146-152`）。对于 `bigint` 值 `9007199254740993`，显示成 `9,007,199,254,740,992`——错一位，且用户无从察觉。这与本仓库"大整数用十进制字符串"的原则（`03-plugin-api.md` §13、`docs/plugins/manifest.md` 的"大整数和 affected_rows 为十进制字符串"）自相矛盾。建议：整数值先按十进制字符串做千分位（不进浮点），仅在有小数位需求时才用 `f64`，并在文档里写明精度边界。

### 6.3 结果集限额在数据全部到达后才检查

`host.rs:178-187`：先把整个 `Vec<ResultSet>` 收齐、再 `serde_json::to_vec` 算字节、再判断 `result_rows`/`result_bytes`。一个 `SELECT * FROM huge_table` 会先在内存里物化完整结果（外加一份 JSON 副本）才被拒绝。`navidog-db` 的结果 API 是"一次性返回整集"，所以彻底修复需要流式接口；但至少可以在 `run_plugin_operation_rel` 的 `rel.execute` 上套一层行数上限，或先判 `result_rows` 再序列化（现在两个判断都在序列化之后）。

### 6.4 授权键的字符串拼接有歧义

`manifest.rs:304`：`format!("shell.{}={value}", path.join("."))`。路径用 `.` 连接，于是 `{"a": {"b": true}}` 与 `{"a.b": true}` 生成同一个键 `shell.a.b=true`；而重建时用的是 `BTreeMap` 里最后写入的那条 path（`insert_grant`，`:318-329`）。虽然重建后官方 parser 会因为未知字段拒绝，用户最多看到"能力申请无效"，但"授权项 ↔ 实际能力"的映射不是双射，属于安全边界的实现瑕疵。建议键里带转义（或直接用 `serde_json` 的规范路径/数组下标）。

### 6.5 panic 点

- `protocol.rs:244`：`event["generation"] = …` 对非对象 `Value` 会 panic（`serde_json::Value` 的 `IndexMut` 对非对象是 panic）。
- `manager.rs:412`：`formatter["id"].as_str().unwrap()`（manifest 已验证，但注册路径应自己兜住）。
- `manager.rs:143`：`expect("plugin generation exhausted")`。实际需要 2^53 次停用才会到达，但 panic 出现在用户可触发的函数里，改成饱和加法或返回错误更稳妥。
- `format.rs:91` 等测试里的密集 `unwrap` 属正常，但在生产函数（如 `manager.rs:575` 的 `.unwrap()`）里应改成 `ok_or`。
- `runtime.rs:73-91` 是一段 `format!` 出来的 JS，只在运行期由 QuickJS 语法检查；没有编译期检查、没有单元测试解析它（见 1.8）。

### 6.6 错误被吞掉

`navidog-sql/src/declared.rs:402-409,432-441,490-494`：`exclusive_lock_sql`、`rename_object`、`explain_sql` 全部 `.ok().flatten()`。模板渲染失败（参数缺失、fragment 非法）会静默变成"这个能力不可用"，插件作者拿不到任何线索；`select_page`（`:513-526`）更是在模板失败后回退到内置的 `SELECT * … LIMIT …` 生成，悄悄绕过了插件声明的 SQL。

同类：`navidog-model/src/formatter_choices.rs:26-31,35-36` 的 `load_optional().unwrap_or_default()`——配置文件损坏时静默清空所有用户的列格式选择，随后 `store` 再覆盖写回。

### 6.7 声明式设计器的属性检查漏项

`navidog-schema/src/table_design/declared.rs:105-132` 的 `checks` 数组覆盖 14 个属性，而 `DialectDescriptor::validate` 允许 17 个（多出 `zerofill`、`generated`、`primary_key`）。漏检意味着：插件只声明了 `type/not_null/default`，用户在原生设计器里改了 `zerofill`，规划器**不会**报"未声明属性被修改"，而是直接按 `alter_column` 模板执行。这与规格 §15"未声明部分保留原文并禁止修改"矛盾，属于 fail-open。

### 6.8 `table` 参数已经限定名+引用，却要求以 `fragment` 编码

`declared.rs:265`：`("table", self.qualified(scope, &draft.name))`——`qualified` 内部已经 `quote_ident` 过；但示例 `examples/plugins/com.example.http-postgres/navidog.json` 里 `create_table.table` 写的是 `"fragment"`，同一个 `add_column.table` 也是 `"fragment"`，只有 `column` 用 `"identifier"`。若作者把 `table` 写成 `identifier`，会被二次引用成 `"""db"".""t"""`。

也就是说：`table` 在语义上是"已转义的限定名"，`column` 是"裸名"，两者对外都叫"参数"，编码却必须不同，而文档（`docs/plugins/manifest.md`）只写"Rust 转义标识符和字面量"。建议：把 `table` 的编码固定为 `fragment` 并在加载期校验（写 `identifier` 直接报错），或改为传裸名 + 独立的 `qualified: bool`。

### 6.9 二进制用 Postgres 的 `\x` 语法表示

`protocol.rs:39-52` 把 `{"base64": …}` 解码后格式化成 `\x#{hex}`。仓库里没有任何其他驱动用这种表示；`navidog-util::format::encode_hex` 用的是"空格分隔的十六进制对"。`\x…` 是 Postgres 的 `bytea` 输出格式，让 MySQL/SQLite 插件的 BLOB 也长成 PG 的样子，且与 `Column::type_name` 无关地丢失了"这是二进制"的信息。文档 `docs/plugins/manifest.md` 只说"二进制值沿用数据库文本编码"。建议：要么保留 `\x` 但在文档里写死为"API v1 二进制编码"，要么沿用 `navidog-util` 的既有格式。

### 6.10 每个请求一个 task 只为超时

`protocol.rs:301-311`：`request_with_id` 把 `tokio::time::timeout(timeout, receive)` 放进 `navidog_db::runtime::spawn`。这只是为了让超时发生在后台任务里，代价是每个请求一次调度 + 一次 future 装箱；直接用 `timeout(...).await` 包在返回的 future 里即可（`request_with_id` 本来就返回 `BoxFuture`）。

### 6.11 `explain` 的能力探测

`navidog-query/src/explain.rs:53`：`if dialect.designer_templates().is_some()` 就走声明式 `explain` 模板。这是用"有没有设计器模板"判断"是不是插件方言"。同类判断在 `editor/toolbar.rs:319-325`、`designer/object.rs:150,409` 各有一份。建议在 `Dialect` 上提供 `fn declared(&self) -> Option<&DialectDeclaration>`（`DeclaredDialect` 返回 `Some(self)`，其他返回 `None`），一处判断。

### 6.12 `set_locked` 混用两种语义

`grid/mod.rs:340-346`：`locked = locked || !target.dialect.grid_writes()`。于是"驱动不支持写入"和"用户手动锁定"共用一个布尔，后续任何 `set_locked(false)` 都无法解锁（`||` 会重新置真），且错误信息来自 `commit.rs:41-52` 而不是网格本身。建议拆成 `user_locked` 与 `writes_unavailable`，或让 `set_locked` 只表达用户意图、写入时再判能力。

### 6.13 列格式化选择以列名为键

`delegate.rs:547`、`mod.rs:447`、`formatter_choices` 全部以 `column.name` 为键。`SELECT a.id, b.id FROM a JOIN b` 会得到两个同名 `id` 列，两列共享一个格式化选择；列改名后选择丢失（对表查看器是合理的，对查询结果本来就不持久化）。建议键用 `(列名, 出现序号)` 或列索引。

### 6.14 其他观察

- `color` 规则的 7 个 token 名在 `cell_formatter.rs:86-101` 校验、在 `delegate.rs:556-564` 再映射一次，而 `navidog-ui` 的 palette 是唯一权威；palette 改名时插件规则会静默退化成默认色。
- `Presentation`/`Display` 用 `String` 承载 `unit`（`s`/`ms`），又用 `contains` 校验，和 `FieldControl` 的枚举风格不一致。
- `1_048_576`（`cell_formatter.rs:120`）是魔数，与 `Limits::batch_bytes` 无关，建议命名常量。

---

## 7. 编写插件的便利性

### 7.1 "第一个 `cx.spawn` 必须是事件循环"是隐式契约

`runtime.rs:79-85` 用 `Proxy` 截获 `cx.spawn`，把第一次调用记为 `serviceTask`；没有就抛 `Plugin service init must start its event loop with cx.spawn`；之后每秒检查 `is_done()` 并 `runtime_failed`。作者需要知道：

- `init` 里第一个 `spawn` 必须是死循环；
- `render` 不能抛（`tests/runtime.rs` 里 `render(){throw new Error('Service must not render');}` 是故意反例）；
- 若有任何前置的初始化 `spawn`（比如先加载配置），会被误判为事件循环，随后 `is_done()` 为真 → 插件 1 秒内被停用，界面只显示"Plugin service task ended. Reload to restart."。

文档只写了"服务 `View.init` 必须首先用 `cx.spawn` 启动持久事件循环"。建议：给一个显式 API（`host.start_service(async () => {...})`）由宿主登记任务，或允许 `init` 返回任务；这样宿主可以给出"你在 init 里先 spawn 了别的任务"这种可诊断的错误。

### 7.2 `generation` 要手工回传

`respond(id, generation, reply)`、`respond_chunk(id, generation, chunk)` 的 `generation` 来自事件对象（`event.generation`）。作者必须每次原样回传；忘了就会得到 `"Stale plugin generation."`（只有重载后才发生，开发时很难触发，线上易犯）。建议提供 `host.reply(event, {ok:true})` 这类封装，或者让宿主在请求对象上带回一个不可伪造的 handle。

### 7.3 事件类型不是判别联合

`api-v1.d.ts:7`：`Event { kind: ...; request_id?: string; op?: string; arguments?: Record<string, unknown> }`——所有字段可选，`arguments` 是 `unknown`。作者写 `event.arguments.input` 没有任何类型保护，而 `import`/`export`/`driver.request` 的 `arguments` 形状完全不同（`driver.request` 的 `op` 有 9 种，参数各不相同）。建议：

```ts
type Event =
  | { kind: "command"; command_id: string; context: string; generation: number }
  | { kind: "driver.request"; request_id: string; op: DriverOp; arguments: DriverArgs }
  | { kind: "import"; request_id: string; arguments: { input: string; context: string } }
  | { kind: "export"; request_id: string; arguments: { columns: Column[] } }
  | { kind: "export.chunk"; request_id: string; chunk: Chunk }
  | { kind: "cancel"; request_id: string; reason: string; generation: number };
```

配套的 `DriverOp`/`DriverArgs` 建议按 op 做判别联合。这是纯 `.d.ts` 工作，收益最大的一项作者体验改进。

### 7.4 声明与实现已经漂移

| 项 | 声明 | 实现 |
| --- | --- | --- |
| `runtime_failed` | 不在 `api-v1.d.ts` | `host.rs:25-28` 注册，并在 `:237` 的声明串里导出 |
| `grid_selection` 返回值 | `d.ts:12` 返回 `ResultSet` | 设计文档 §5 写 `ResultSet \| null`；实现 `host.rs:79-91` 恒返回对象，但 `ContextBridge::grid` 在无目标时是错误而非 null |
| `list_connections().color` | `unknown` | `host.rs:72` 是 `c.color`（字符串） |
| `Reply.code` | `code?: string` | `respond` 只读 `ok`/`data`/`error`，`code` 被忽略 |
| 限额项数 | 文档 §12 写"六项限额" | `Limits` 有 7 个字段（`settings.rs:12-20`） |
| 宿主 API 签名 | 文档 §5（`03-plugin-api.md:110-132`）还是 M5 之前的草案：`respond(request_id, result)`、`notify(level, message)`、`execute(connection_id, database, sql)`、`grid_selection()`、`insert_text(text)`，都没有 `generation`/`context` | 实现（`runtime.rs:223-241`、`host.rs:32-38,79-83,95-100,128-139`）要求 `respond(id, generation, reply)`，且每个能力函数第一个参数都是 context |
| 面板身份 | 文档 §8 写 `plugin:<插件 id>/<面板 id>` | 实际是 `<插件 id>/<面板 id>`（无 `plugin:` 前缀，`manager.rs:423`、`panel.rs:114-126`）；重启后老布局能否匹配取决于 `PanelInfo` 里的这个字符串 |
| 示例数量 | 文档 §16 / 验收记录写"五类可安装示例" | `examples/plugins/` 只有 3 个包；format/cell_formatter/panel 都挤在 `com.example.commands` 里，没有独立的 importer/exporter 示例 |

### 7.5 文档把作者指回 Rust 源码

`docs/plugins/manifest.md`："表 templates 操作名见 `DialectDescriptor::validate`"。作者要装一个 Rust 仓库才能知道 23 个合法操作名、17 个合法参数名、17 个合法列属性名。建议在 `manifest.md` 里直接列出这三张表（它们本来就已经在 `template.rs:35-58` 和 `declared.rs:114-189` 硬编码，抄一份的成本很低，还能顺便用脚本检查同步）。

### 7.6 错误信息对作者不友好

`"Unknown permission: …"`、`"Invalid contribution id: …"` 只给字段值不给文件/位置；`Extensions::parse` 的 serde 错误是 `serde_json` 原样字符串（没有字段路径包装）。建议在错误前缀里带上 `navidog.json` 与贡献类型（`commands[2].menu: …`），或改用 `serde_path_to_error`（是一个新依赖，但很小）。

### 7.7 缺少调试通道

gpui-shell 0.8.1 的 `console.log` 是空实现（`host_modules.rs:1207`：`function("log", |_| Ok(HostValue::Null))`），navidog 也没有把插件输出转发到 `tracing`，所以插件作者**没有任何日志手段**：`console.log` 静默丢弃，`navidog.notify` 只弹一条用户可见通知，`runtime_failed` 之后也没有"最近一次错误"回显。建议提供 `host.log(level, message)` 落到 `tracing`（或把 `console.*` 接到 `tracing`），并在插件详情里显示最近 N 条诊断。

### 7.8 示例可读性

`examples/plugins/com.example.commands/main.js` 把 5 种事件分支压进 25 行，`export.chunk` 的分批校验写成 `if (!state || event.chunk.sequence !== state.sequence++)`；`panel.js` 里 `new Dialog("plugin-about", "About plugin…", () => {})` 的第三个参数没有说明。作为唯一的分批协议示例，建议拆成带注释的 `main.js`（每个分支 3–5 行 + 注释），并补一个"只做 formatter"的最小示例。

### 7.9 规格第 5 章仍在教 M5 之前的 API

`docs/design/03-plugin-api.md:110-132` 是"HostModule `navidog`"章节，也是新作者读到的第一段代码，但它列出的签名是 M5 之前的草案：没有 `context`、没有 `generation`。照抄那段示例的结果是：

- `respond(event.request_id, await handle(event))` → `argument 2 must be a number, got an object`（`host_modules.rs` 的参数检查）；
- `notify("info", "saved")` → `argument 3 is missing; 2 were passed`（level 被当成 context）；
- `grid_selection()` → `argument 1 is missing; 0 were passed`；
- `execute(connId, db, sql)` → 连接 ID 被当成 context token，报 `Plugin context has expired.`。

这些错误都指向"参数不对"，不会告诉作者"你在用已经废弃的 API"。文档在 §11 写了"与前述概念示例冲突时以本节为准"，但 §5 本身没有任何"草稿/已废弃"标记。建议：直接按 v1 改写 §5（或明确标注"§11-16 才是 v1"），并加一条 doc-smoke（照 `runtime.rs:271-275` 那样把声明串 `validate()` 一遍，或对示例做参数级调用）。

### 7.10 作者拿不到任何类型检查

`crates/navidog-plugin/api-v1.d.ts` 只存在于仓库源码里：`docs/plugins/README.md` 与 `03-plugin-api.md` §16 用文字指向它，但没有任何步骤把它（连同 `declare module "navidog"`）装进插件目录；三个示例插件也没有 `jsconfig.json`/`tsconfig.json`。`03-plugin-api.md` §3 声称 `declarations` 会让 gpui-shell 生成的 `gpui-kit.d.ts` 里出现 `declare module "navidog"`，但本 PR 没有调用任何类型声明写出接口，插件目录里也不会有这份文件。结果是编辑器对 `import ... from "gpui-kit"` / `"navidog"` 只能报 `Cannot find module`：没有补全、没有 hover、也没法开 `// @ts-check`。

这是作者体验里投入产出比最高的一项：在插件详情/授权通过时（debug 构建）为插件目录写一份 `navidog.d.ts`（内容就是 `host.rs:236-260` 已经拼好的声明串，按已授权项裁剪），再调 gpui-shell 的类型声明写出接口生成 `gpui-kit.d.ts` + `jsconfig.json`，作者就能用 `// @ts-check` 或 `tsserver` 直接检查。

---

## 8. 其他

### 8.1 测试

- 好的部分：权限/生命周期/协议/失败/面板/实库都有对应用例，且 `tests/management.rs` 是真的点 UI（`window.click("plugin-toggle")`）。
- 问题一：**live 测试静默通过**。`tests/http_live.rs:16` 与 `tests/mysql_http_live.rs` 在 `NAVIDOG_LIVE` 未设置时直接 `return`，`cargo test` 会把它们记为 passed。CI 里除非显式设置，否则这条链路永远是绿的。
- 问题二：**测试隔离依赖进程级环境变量**。`tests/management.rs:12-15`、`tests/dock.rs:25-28` 用 `std::env::set_var("NAVIDOG_HOME", …)` 并注释"这个集成测试二进制只有一个测试"。只要有人加第二个 `#[gpui::test]`，两个测试就会抢同一个 HOME。建议改成 `Plugins::new_with_root(path)`（代码里已经有 `new(root)`，只是没有从测试注入的入口）。
- 问题三：`PluginPreferences`、`LimitsView`、`ApprovalView` 没有独立测试，只有一条端到端点击路径；面板渲染的回退文案（`panel.rs:103-106`）没有断言。
- 问题四：单元测试集中在 `protocol.rs`（数据流）与 `manifest.rs`（校验），`manager.rs` 的 `stop()`（清空/重绑快捷键）与 `source_revision` 没有单测（`Approval::reconcile` 在 `settings.rs` 已有覆盖）。

### 8.2 文档

除了 7.4/7.5 的漂移，还有：

- `docs/plugins/README.md` 与 `docs/design/03-plugin-api.md` 有 60% 内容重复（事件循环、限额、二进制、格式器、驱动门槛各讲两遍），两处口径已经开始不一致（限额"六项"）。
- `acceptance-m5.md` 的"76 张截图已逐张查看"无法复核（截图不入库，符合约定），但可以补几个关键截图的文件名/路径，方便下一位维护者重跑。
- `progress.md` 把 F-PLUG-001..008 全部标"完成"，其中 F-PLUG-008 的"错误在对应项附近展示"未实现（`limits_view.rs` 只有底部一行）、F-PLUG-004 的"向导留 M6"已注明，状态本身可以接受，但建议在"验证"列标注"限额错误定位未覆盖"。

### 8.3 体积与特性

`navidog-ui/Cargo.toml:11` 把**正式依赖**的 `gpui-kit` 改成 `features = ["tree-sitter-sql"]`（`:21` 的 dev-dependencies 早就开了）。库 crate 开 feature 会让整个 workspace 统一编译该特性（Cargo 特性取并集），这与 `AGENTS.md` 的 gpui-kit 坑位说明"测试主题的 crate 要开 `tree-sitter-sql`"初衷不同：现在生产构建也带上了 tree-sitter-sql 及其语法依赖。如果不是插件面板高亮所必需，建议挪回测试 crate 或在外壳 crate 里开。

### 8.4 vendor

`vendor/russh`（29,043 行）占用本 PR 72% 的 diff。`README.navidog.md` 写清了理由与移除条件，但没有：

- 版本升级步骤（下次上游发版后如何 rebase、要重跑哪些检查）；
- `deny.toml`/`cargo audit` 如何在 vendor 目录上生效的说明（`cargo audit` 默认不扫 path 依赖里的 CVE）；
- 在 `docs/progress.md` 的"维护事项"里登记。

### 8.5 提交粒度

规格单独提交（`72af47e`）、验收文档单独提交（`2cc9ab1`、`7535d28`）都很好；但 `3c4421e build: integrate Shell 0.8.1 with compatible SSH dependencies` 把"升 gpui-kit 0.8.1 + vendor russh + 引入 gpui-shell"合成一个提交，导致 `tabs.rs`/`explain_visual.rs`/`viewer/mod.rs` 的 API 迁移在 diff 里看不出归属。

---

## 9. 整改清单

按"合并前 / 合并后第一个迭代 / 可延后"分档。每一项都能独立提交。

**合并前（P1）**

- [ ] 格式化贡献改为强类型（`Registry<Arc<CompiledFormatter>>`），在注册时编译正则，删除渲染路径上的 `from_value` 与 `Regex::new`（6.1、3.2）
- [ ] `number` 规则改用整数/字符串路径，补 `9007199254740993` 的回归测试（6.2）
- [ ] `ContextBridge` 支持同一窗口同时存在 grid 与 editor 目标；补"网格命令后仍可 `insert_text`"的测试（4.5）
- [ ] 补齐 `zerofill`/`generated`/`primary_key` 的变更检测，或反过来把 `designer.properties` 收敛成 `ColumnProperty` 枚举（6.7、2.10）
- [ ] 用户可见错误走 `t!`（或错误码 + 本地化），并给 `navidog-plugin` 补 `include_str!("../locales/plugin.yml")`（3.8、3.5）
- [ ] 生成的服务 wrapper 移出插件目录（或文档明确要求插件目录可写），并改用 `include_str!` + 解析测试（4.7、1.8）
- [ ] 事件循环改为显式登记（或在 `init` 返回值上监督），删掉 `Proxy`（7.1）
- [ ] 修复 5.1 列出的未格式化行，并在 CI 增加超长行检查（5.1）
- [ ] 统一 host 函数的 `is_active()` 守卫（抽 helper），补 `import_rows`/`export_rows` 的守卫与测试（2.11、P1 表）
- [ ] `object_snapshot`/`table_snapshot` 加入 `relational_catalog!`（或显式 trait），恢复"漏实现=编译错误"（4.3、2.3）
- [ ] 声明式 DDL 的执行信封统一到一处，预览路径复用（2.1）

**合并后第一个迭代（P2）**

- [ ] `notify` 替换 1 秒轮询 + 目录哈希（1.1）
- [ ] manifest 的 `menu`/`when`/`placement`/permission 改为 serde 枚举，`when` 判断只留一处（1.3、2.5）
- [ ] `DialectDescriptor` 与 `DialectFeatures` 合并表示（1.4、4.2）
- [ ] `format`/`panel`/`formatter` 贡献全部强类型，删掉 `main_window.rs`/`panel.rs` 里的 JSON 解析（3.2、2.4）
- [ ] 抽 `qualify`/`unqualify`、`installation(id)`、`config_file()` 等取值器（2.5、2.6、2.13）
- [ ] `DeclaredDialect` 的错误不再 `.ok().flatten()`；`formatter_choices` 损坏时不要静默清空（6.6）
- [ ] `set_locked` 拆成两个语义；`explain`/设计器改用显式的声明式方言探测（6.11、6.12）
- [ ] `Plugins` 拆分职责，`enable`/`register`/`stop` 各拆成可单测的小函数（4.6）
- [ ] 启动加载异步化 + 补启动耗时数据（4.9）
- [ ] live 测试在未设置环境变量时显式 `eprintln!("skipped")` 或改用 `#[ignore]`（8.1）
- [ ] 示例：抽共享 `http-service.js`、批量不硬编码 1000、补最小 formatter 示例（2.12、7.8）
- [ ] 改写 `03-plugin-api.md` §5 的 v1 签名并加 doc-smoke；为插件目录生成 `navidog.d.ts`/`jsconfig.json`（7.9、7.10、1.10）

**可延后**

- [ ] 拆 `navidog-plugin-core`（无 GPUI），让 headless 覆盖协议/驱动（4.4）
- [ ] `Dialect`/`Relational` 的 `&'static` 退化回滚或换设计（4.2）
- [ ] `api-v1.d.ts` 改成判别联合并作为唯一声明源（7.3、7.4、1.10）
- [ ] `vendor/russh` 的升级/审计流程文档化（8.4）
- [ ] 授权键改为无歧义编码（6.4）
- [ ] 二进制线上表示统一（6.9）

---

## 10. 附：本次审查的执行记录

```sh
# 范围与规模
git diff main...HEAD --stat                      # 183 files, +40449/−1140
git diff main...HEAD --numstat -- vendor | awk …  # vendor 29043 行（russh）

# 验证 PR 声称的测试
cargo test -p navidog-plugin -p navidog-sql -p navidog-model   # 全部通过
cargo test -p navidog-plugin                                   # 6 个集成测试文件 + lib 单测，均通过
                                                               # 注意：http_live/mysql_http_live 在无 NAVIDOG_LIVE 时直接 return

# 复现 fmt 漏洞
cargo fmt --all --check                          # exit 0
rustfmt --edition 2024 --emit stdout crates/navidog-plugin/src/driver.rs   # 80-86 行原样保留

# 判断候选库是否已在依赖树
for c in notify schemars jsonschema garde validator axum hyper minijinja num-format; do
    grep "^name = \"$c\"" Cargo.lock || echo "$c absent"
done
cargo tree -i notify | head -3
cargo tree -i schemars | head -3
cargo tree -i hyper | head -3
```
