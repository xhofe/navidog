# navidog 整体代码审查（2026-10-24）

审查对象：`review/e120f30`（`e120f30`），工作区干净。范围是全部 11 个 crate 加 `xtask`、`scripts/`、`.github/`，以及 `docs/design/00-overview.md`、`AGENTS.md`、`docs/progress.md` 的工程约定。

本文是跨 crate 的整体审查，不替代既有的两次审查：[2026-10-10 by_grok](2026-10-10-by_grok.md)（CR-01～33）和 [2026-10-17 feat/m3](2026-10-17-feat-m3.md)（CR-01～26）。已经修掉的问题不再重复；旧结论与当前代码不一致时在文中注明（例如 CR-23 记“已修”，但两个十六进制函数仍在）。

## 修复进度

状态只用：`待修`、`进行中`、`已修`、`暂缓`（写明原因）。每批修复后更新本表并提交。

| 编号 | 摘要 | 计划 | 状态 |
| --- | --- | --- | --- |
| R-01 | 手写 UUID/RNG | P0 | 已修 |
| R-02 | 手写日历与 UTC 今天 | P1 | 已修 |
| R-03 | percent/URI 两套与 + 解码 | P0 | 已修 |
| R-04 | 六处 SQL 扫描/解析 | P1 | 进行中 |
| R-05 | 两套 hex 编码 | P1 | 已修 |
| R-06 | 平台目录/家目录展开 | P1 | 已修 |
| R-07 | 原子写不一致 | P1 | 已修 |
| R-08 | 手写 objc_msgSend | P2 | 已修 |
| R-09 | defaults 读系统语言 | P2 | 已修 |
| R-10 | enum key 手写映射 | P2 | 已修 |
| D-01 | 6 个错误文案包装与重复测试 | P1 | 已修 |
| D-02 | 9 处 JSON 持久化 | P1 | 已修 |
| D-03 | MySQL/PG 生命周期复制 | P1 | 已修 |
| D-04 | 网格/查询提交与关闭复制 | P1 | 已修 |
| D-05 | SQL 扫描重复 | P1 | 进行中 |
| D-06 | 对话框 footer 复制 | P2 | 已修 |
| D-07 | driver_name/db_text/kind_label 等 | P1 | 已修 |
| D-08 | Catalog 与 OutlineCache 双缓存 | P2 | 进行中 |
| D-09 | nav.* locale 死块 | P0 | 已修 |
| D-10 | 测试 helper 复制 | P2 | 已修 |
| D-11 | 更新清单 Rust/CI 双份 | P1 | 已修 |
| D-12 | 状态栏重建对象列表 | P1 | 已修 |
| S-01 | 中文注释/文案与 DbError 本地化 | P0 | 已修 |
| S-02 | 未使用依赖 | P0 | 已修 |
| S-03 | 保存失败策略不一致 | P1 | 已修 |
| S-04 | AppState/raw/Settings 多份 | P1 | 已修 |
| S-05 | 命名与弃用 API | P1 | 已修 |
| S-06 | gpui-kit 版本文档漂移 | P0 | 已修 |
| S-07 | 每 crate i18n 后端导致的复制 | P2 | 已修 |
| S-08 | 面板标题错误 | P2 | 已修 |
| S-09 | designer i18n 顺序 | P2 | 已修 |
| A-01 | core god crate | P2 | 暂缓 |
| A-02 | core/db 依赖 gpui-kit | P2 | 暂缓 |
| A-03 | db 职责超出驱动 | P2 | 进行中 |
| A-04 | 驱动元数据散落 | P1 | 已修 |
| A-05 | 提交逻辑在 grid | P1 | 已修 |
| A-06 | 缓存/选择状态复制与倒挂 | P2 | 进行中 |
| A-07 | god object/超长文件 | P2 | 进行中 |
| A-08 | 提前抽象与死代码 | P1 | 已修 |
| A-09 | 测试架构 | P2 | 已修 |
| A-10 | Connection 配置文件交换模型 | P2 | 进行中 |
| B-01 | 连接表单静默丢配置 | P0 | 已修 |
| B-02 | SSH 主机密钥 | P1 | 已修 |
| B-03 | 更新包临时路径/TOCTOU | P1 | 已修 |
| B-04 | profile 路径未校验 id | P0 | 已修 |
| B-05 | $tag$ 参数替换 | P0 | 已修 |
| B-06 | TSV 不转义 | P1 | 已修 |
| B-07 | 排序语义/性能 | P2 | 已修 |
| B-08 | 每格解析与 refit 分配 | P2 | 已修 |
| B-09 | 状态栏每帧构建 | P1 | 已修 |
| B-10 | Catalog 无界增长 | P2 | 已修 |
| B-11 | Windows 非法文件名字符 | P1 | 已修 |
| B-12 | 表单初始焦点 | P2 | 已修 |
| B-13 | UI 路径 panic | P2 | 已修 |
| B-14 | 历史日志轮询 | P2 | 已修 |
| B-15 | write_atomic 无 fsync | P1 | 已修 |
| B-16 | 查找 ASCII-only | P2 | 已修 |

## 暂缓项与原因

这些是架构级重构，不是单点 bug；已在本次分支记录，建议按主题另开 PR，避免把行为迁移和修复混在一起：

- **R-04 / D-05（SQL 扫描统一，进行中）**：引号与 `$tag$` 跳过已收拢到 `navidog_util::sql_text`；`find_params`、分句器、筛选解析的完整词法层仍需语法级重构和三种方言的回归。

- **D-08（Catalog/OutlineCache，进行中）**：`Load<T>` 与 `ScopeCache<T>` 已引入，OutlineCache 已迁移；Catalog 的五类不同 key/失效代际仍需继续迁移。
- **S-07（每 crate i18n 后端）**：明确契约为 crate-local：`scripts/check-i18n.sh` 校验每个字面量 `t!` key 都在本 crate locales 中；跨 crate 文案必须走 `navidog_ui::db_error_text` 这类显式 helper。统一后端仍可后续合并。
- **A-01/A-02（core/db 分层）**：A-03 的 SQL 历史已拆到 `navidog-sql-log`。A-01 建议先抽 `navidog-model`（connection/object/crypto/paths/queries 等纯数据）与 `navidog-app-state`（Global/Entity/actions/Workspace）；A-02 随后把 `DriverRegistry`/`Sessions`/`OutlineCache` 移到 app-state，`navidog-db` 只留驱动与 SQL。两步都要兼容现有 `connections.json` 和所有 feature crate 的导入路径，适合独立 PR。
- **A-06（缓存/选择，进行中）**：navigator 的多选快照改为每帧从 Workspace 派生；`Catalog::refresh_connection` 不再调用 `Sessions::refresh`，由 shell 处理数据库切换。`ScopeCache<T>` 和 `Catalog::require()` 仍是缓存层重构。
- **A-07（god object，进行中）**：`app/update.rs` 的安装/替换逻辑已拆到 `update/install.rs`；viewer/results/editor/form/model 的拆分仍是纯搬家工作。
- **A-10（profile 交换模型，进行中）**：新增 `with_active_profile` 的 Drop 守卫，reencrypt 在 panic 时也会恢复激活配置；`profiles + active` 的 schema 迁移仍未做，需兼容旧 `connections.json`。

## 0. 结论

整体工程质量在个人项目里属于上游：设计文档和规格齐全，283 个测试函数覆盖了真实数据库、headless UI 和截图，`fmt`、`clippy -D warnings`、裸值检查都通过，错误恢复、连接代际、配置原子写这些难点有专门设计。**没有发现新的崩溃级或写错库级缺陷**，但有几处值得优先处理的正确性/安全边界（连接表单静默丢配置、URI 的 `+` 解码、`$tag$` 参数替换、SSH 主机密钥、更新包路径与校验窗口）；更大的问题是随着功能增加而长出来的结构性债务。

如果只做三件事，按收益排序：

1. **收口三套基础设施**：一个 SQL 词法器、一个 JSON 持久化器、一套错误文案/恢复提示。它们目前各有 6～9 份实现，是后续所有功能都会继续踩的复制源。
2. **拆开 `navidog-core` 和 `navidog-db` 的 GPUI 依赖**：现在“领域层”和“驱动层”都依赖 UI 框架，headless 消费者要编译整个 GPU 栈；这是 M5 插件和 M14 新数据库之前必须解决的边界问题。
3. **把驱动元数据收进 `Driver`**：默认端口、图标、连接表单字段、方言现在散在 8 个文件里；再加一个数据库要改 core、ui、connection、query、designer 五个 crate，与插件 API 的承诺不符。

### 主要问题（按严重度）

| # | 严重度 | 类别 | 问题 | 关键证据 |
| --- | --- | --- | --- | --- |
| 1 | 重要 | 架构 | `navidog-core` 同时是领域层、配置层、全局状态层和跨 feature 动作层；49 个字段的 `Settings` 和 15 个模块使它成为 god crate | `crates/navidog-core/src/settings.rs:159`、`crates/navidog-core/src/actions.rs:1`、`crates/navidog-core/Cargo.toml:11` |
| 2 | 重要 | 架构 | `navidog-core`、`navidog-db` 依赖 `gpui-kit`；驱动栈会带上 GPU/窗口栈，headless 使用和单测都被迫进入 GPUI | `crates/navidog-core/src/store.rs:5`、`crates/navidog-db/src/driver.rs:294`、两份 `Cargo.toml` |
| 3 | 重要 | 重复 | MySQL 与 PostgreSQL 的 `remember/replay/execute/execute_read/ping` 是逐行复制，恢复逻辑已经出现方言分叉 | `crates/navidog-db/src/mysql.rs:180-318`、`crates/navidog-db/src/postgres.rs:272-537` |
| 4 | 重要 | 重复 | 结果网格与查询结果各自实现提交、关闭询问、底栏、重载；两份提交协议已经不同（事务标志 vs 恒为 false） | `crates/navidog-grid/src/viewer/mod.rs:1132`、`crates/navidog-query/src/results.rs:939` |
| 5 | 重要 | 重复 | 9 处 JSON 持久化各写一套 load/save；只有 2 处做坏文件备份，收藏夹/代码段会静默清空 | `crates/navidog-core/src/favorites.rs:43`、`crates/navidog-core/src/snippets.rs:142`、`crates/navidog-core/src/store.rs:218` |
| 6 | 重要 | 重复 | 6 处手写 SQL 扫描/解析，引号、注释、`$tag$` 规则已经不一致；参数替换会改写 `$$...$$` 中的 `[$x]` | `crates/navidog-util/src/sql_text.rs:59`、`crates/navidog-db/src/sql.rs:74`、`crates/navidog-grid/src/filter.rs:912` |
| 7 | 重要 | 正确性 | 连接表单把非法端口、非法多节点行静默丢弃或改成默认值，保存后配置与用户输入不符 | `crates/navidog-connection/src/form.rs:440-446`、`crates/navidog-core/src/connection.rs:227` |
| 8 | 重要 | 规范 | 约 20 条中文注释、15 条中文用户可见文案留在生产代码；`DbError` 把本地化文本烘焙进 `message`，切语言不更新、日志中英混杂 | `crates/navidog-db/src/metadata.rs:15`、`crates/navidog-db/src/sessions.rs:106`、`crates/navidog-core/src/registry.rs:1` |
| 9 | 重要 | 架构 | 网格 crate 持有事务/方言提交逻辑，查询 crate 为了提交反过来依赖网格；层次方向与 AGENTS 的约定相反 | `crates/navidog-grid/src/commit.rs:31`、`crates/navidog-query/src/results.rs:26` |
| 10 | 重要 | 安全 | SSH 主机密钥无条件接受；更新包只校验同一镜像给出的 SHA-256，且下载目录可预测、校验后按路径重新打开 | `crates/navidog-db/src/ssh.rs:19-24`、`crates/navidog/src/update.rs:449-505` |

## 1. 审查范围与验证

| 项 | 数值 |
| --- | --- |
| crate | 11 个（`navidog`、`navidog-core`、`navidog-ui`、`navidog-util`、`navidog-db`、`navidog-plugin`、`navidog-connection`、`navidog-navigator`、`navidog-grid`、`navidog-query`、`navidog-designer`）+ `xtask` |
| 生产代码 | 135 个 `src/*.rs`，54,555 行 |
| 测试代码 | 25 个测试文件（含 `src/*_tests.rs`、`src/tests.rs`），11,050 行；283 个 `#[test]` / `#[gpui_kit::test]`（另有 `xtask` 1 个，共 284 项） |
| 示例 | 3 个，500 行 |
| 最大生产文件 | `navidog-grid/src/viewer/mod.rs` 3,064 行；另有 6 个生产文件超过 1,000 行（见附录 A） |

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo test --workspace --no-fail-fast` | 通过（284 项，含 `xtask`；未设置 `NAVIDOG_LIVE`，实库测试按设计跳过） |
| `scripts/check-raw-px.sh` | 通过 |
| 实库测试、截图测试 | 未运行（需要 Docker / Metal） |
| `cargo deny`、`cargo audit` | 未运行（需要网络/凭据） |

**方法**：逐 crate 阅读生产代码；用脚本做函数体归一化哈希找完全相同函数、8-token shingle + Jaccard 找近似函数（阈值 0.62，人工过滤）；扫描依赖实际使用位置、`t!` 键与 locale 文件的对应关系、跨 crate 依赖边；对每条严重/重要结论回到源码核对。子代理报告里未通过复核的结论已丢弃（例如“`status_text` 从未被调用”“连接表单 7 个标签自带冒号”在当前代码上是错的），文中的行号均以 `e120f30` 为准。

**严重度**：`重要`＝会持续放大成本、影响正确性/安全或阻塞后续里程碑；`一般`＝局部可维护性问题；`建议`＝风格与一致性。本文没有把“暂定 token、未做并排目测”这类 `progress.md` 已登记的偏差重复计入问题，只在它们与结构方案冲突时引用。

## 2. 成熟库/组件已经存在却自行实现

以下 crate 都已在 `Cargo.lock` 中（多为传递依赖），引入不需要新增第三方树；表格里的“影响”是如果不换会付出的实际代价，不是“应该用库”的泛泛建议。

| 编号 | 位置 | 自研内容 | 可用库/组件 | 影响 | 建议 |
| --- | --- | --- | --- | --- | --- |
| R-01 | `crates/navidog-query/src/editor.rs:1372` | `uuid_v4()` 用两个 `rand_u64()`，随机源是 `RandomState` + 时间 | `uuid`（已在 workspace） | 32 位十六进制串不是 RFC 4122 UUID，无版本/变体位；该值同时用于自动恢复文件名 `recovery.rs:42`，碰撞会覆盖草稿 | `Uuid::new_v4().simple()` |
| R-02 | `crates/navidog-grid/src/date.rs:104-158` | `days_in_month`、`is_leap`、`weekday_sunday`（Sakamoto）、`month_cells`、`civil_from_days`、`utc_today` | `chrono`（workspace）；`gpui-component 0.8` 已有 `date_picker::DatePicker` / `calendar`（`gpui_kit::component` 下可访问） | 55 行日历数学；`utc_today` 用 UTC，东八区 0:00–8:00 点“今天”写入昨天（`progress.md` §4 已登记） | 用 `chrono::Local` 和 `NaiveDate`；日历控件评估 `Calendar`，保留时区尾串的定制逻辑 |
| R-03 | `crates/navidog-core/src/uri.rs:168-201`、`crates/navidog-core/src/queries.rs:13-41` | 两套 percent 编解码 | `percent-encoding`、`url`（均在锁） | `uri.rs:190` 把 `+` 解成空格；`mysql://u:p+q@h/db` 的密码变成 `p q`，真实密码含 `+` 时永远连不上；两套编码器对保留字符的集合不同 | 统一用 `percent-encoding`，按 path/query 组件选择字符集；删除 `+`→空格 |
| R-04 | 见下 | 6 处 SQL 扫描/解析 | `tree-sitter-sequel`（已在锁，供高亮）；`sqlparser` 不在锁中，需单独评估 | 引号、注释、美元引号规则已分叉；`$tag$` 参数替换和 PG 下的 Builder 解析都是实际 bug | 在 `navidog-util` 提供唯一的 `spans(sql, dialect)`，上层全部消费它；DDL 解析再评估 `sqlparser` |
| R-05 | `crates/navidog-util/src/format.rs:4`、`crates/navidog-util/src/update.rs:61`、`crates/navidog-ui/src/theme.rs:137` | 两套十六进制编码 + 一套颜色十六进制 | `hex`（已在锁） | `format.rs` 每字节 `format!` 再 join；命名上 `encode_hex` 与 `hex_encode` 不区分输出格式，CR-23 说已修但两个函数仍在 | `hex::encode`；带空格的展示格式单独命名 |
| R-06 | `crates/navidog-core/src/paths.rs:4-34`、`crates/navidog-db/src/params.rs:70` | 手写 macOS/Windows/Linux 配置目录和 `~` 展开 | `dirs`、`shellexpand`（均在锁） | `HOME` 未设时 `unwrap_or_default()` 得到空路径，配置会写到进程 CWD；`expand_home` 只认 `~/`；三个 OS 的分支都编进每个二进制 | `dirs::config_dir()` / `home_dir()`，`shellexpand::tilde`，`config_dir` 返回 `Option` |
| R-07 | `crates/navidog-core/src/paths.rs:42`、`crates/navidog-core/src/crypto.rs:237` | 两套“临时文件 + rename”原子写 | `tempfile`（已在锁） | 固定 `.tmp` 文件名、无 `sync_all`、进程间不互斥；断电后可能留下空文件，与函数注释的承诺不符。密钥文件另写一套 0600 + sync 的实现 | 一个 `NamedTempFile` + `persist` 的 helper，秘密文件带权限参数 |
| R-08 | `crates/navidog/src/lib.rs:248-286` | 手写 `objc_msgSend` 的 `transmute` 调用设置 Dock 图标 | `objc2`、`objc2-app-kit`（均在锁） | ABI 脆弱；代码注释自己记录了 arm64 变参 ABI 导致 AppKit 抛异常的事故；`unsafe` 范围大 | 用 `objc2` 的 `NSImage`/`NSApplication`；或只在 bundle 里用 `.icns`，删掉这段 |
| R-09 | `crates/navidog-core/src/settings.rs:137` | 用 `defaults read -g AppleLanguages` 子进程判断系统语言 | `sys-locale`（已在锁） | 平台分支、解析脆弱、每次读设置都起子进程 | `sys_locale::get_locale()` |
| R-10 | `crates/navidog-core/src/object.rs:25,39`、`crates/navidog-core/src/connection.rs:441`、`crates/navidog-grid/src/filter.rs:41` | 手写 enum 的 `key` / `from_key` / `as_str` | `strum`（已在锁） | 新增一个 `ObjectKind` 要同步 key、from_key、图标、locale、`Dialect::object_kinds` 五处；漏一处就是静默 `None`/空图标 | `EnumString`/`Display` derive；图标和 locale 仍按类型保留 |

### R-04 的六处 SQL 扫描

| 文件 | 职责 | 已观察到的规则差异 |
| --- | --- | --- |
| `crates/navidog-db/src/sql.rs:74` | 分句、`DELIMITER`、`BEGIN…END` | 认识 `$tag$`、E 字符串；`#` 注释只对 MySQL |
| `crates/navidog-util/src/sql_text.rs:59` | `[$name]` 参数 | 不认识 `$tag$`；不处理 `#` 注释 |
| `crates/navidog-util/src/sql_format.rs:42` | 美化/简化时的 token 切分 | 认识 `$tag$`；`#` 注释由调用方开关 |
| `crates/navidog-grid/src/filter.rs:912,935` | WHERE/ORDER BY 解析 | `skip_string_body` 只对 MySQL 认反斜杠，`paren_end` 对所有方言认；同一段文本两个函数结论不同 |
| `crates/navidog-db/src/table_design/parse.rs:7` | DDL 顶层逗号/括号 | 引号集多一个 `[`；错误信息是英文，但与 `sql.rs` 的 `$tag$` 规则不同 |
| `crates/navidog-db/src/table_design/properties.rs:47`、`entries.rs:158` | 列定义、约束/索引条目 | 只做词切分，不共享上面的引号规则 |

**实际后果**（不是理论风险）：`SELECT $$ [$x] $$` 在参数替换时会被改成 `SELECT $$ '值' $$`，语句含义被静默改变；PostgreSQL 的 `standard_conforming_strings=on` 下，`paren_end` 仍把反斜杠当转义，而 `skip_string_body` 只在 MySQL 如此；同一段文本的括号深度因此可能不同，`parse_where` 直接返回 `None`，用户点“Builder”没有任何反应。

## 3. 重复代码

| 编号 | 重复内容 | 证据 | 风险 | 建议 |
| --- | --- | --- | --- | --- |
| D-01 | 会话丢失/结果未知的本地化包装，6 个包装 + 1 个基函数，每个配一份近 40 行的测试 | `navidog-ui/src/lib.rs:22`、`navidog-grid/src/lib.rs:29`、`navidog-grid/src/commit.rs:76`、`navidog-query/src/errors.rs:6`、`navidog-connection/src/lib.rs:357`、`navidog-connection/src/reload.rs:91`、`navidog-navigator/src/errors.rs:6` | 改一句恢复文案要改 6 处；测试里的 “Reopen”/“重新打开” 断言也是 5 份 | `db_error_text` 接收一个 `Recovery` 枚举（SessionLost/OutcomeUnknown）+ 调用方的两条文案；测试只保留基函数和 1 个 feature 代表 |
| D-02 | 9 处 JSON 文件读写 | `core/store.rs:218,486`、`settings.rs:321,333`、`favorites.rs:43,55`、`vgroups.rs:194,230`、`column_widths.rs:36,58`、`snippets.rs:142,150`、`query/recovery.rs:50`、`db/outline.rs:218,222`、`app/state.rs:75,82` | 只有 store/settings 在解析失败时备份；favorites/snippets 的 `.ok()…unwrap_or_default()` 会让一次截断的写在下一次保存时把整个文件清空；保存失败大多只 `tracing::error!`，UI 无提示 | 一个 `JsonStore<T>`：`load_or_backup` + 原子 `save` + `Result`；按“用户数据/可重建缓存”分两档策略 |
| D-03 | MySQL/PostgreSQL 会话生命周期 | `mysql.rs:180-318` ↔ `postgres.rs:272-537`：`remember`、`replay`、`execute`、`execute_read`、重连 `ping` | 唯一有意的差异是 `e.disconnected` vs `self.lost(&e)`；恢复策略的未来修复必须做两遍，且已经出现分叉 | 在 `Relational` 上给 `execute`/`execute_read` 默认实现，只要求驱动提供 `attempt`/`reconnect`/`remember`/`lost`；或 `RecoveringSession` 包装器 |
| D-04 | 网格与查询结果的提交、关闭、底栏、重载 | `viewer/mod.rs:410-494,953-1242,1703` ↔ `query/editor.rs:836-898`、`query/results.rs:939-1015,1181` | 两份提交协议已不同：表查看器把 `in_tx/end_transaction` 传给 `apply`（`viewer/mod.rs:1167`），查询结果恒传 `false,false`（`results.rs:970`）；关闭询问的按钮和标题也各写一套 | 提取 `ResultGrid` 之上的 `GridSession`（提交/重载）与 `close_prompt`（UI）两层；查询结果复用同一流程 |
| D-05 | SQL 扫描/解析 6 处 | 见 §2 R-04 | 规则分叉、参数替换 bug | 同 R-04 |
| D-06 | 对话框 OK/Cancel 行 | `connection/secrets.rs:223` 有一个 `footer`，仅 connection crate 内复用；`app/opening.rs:26-46` 又定义 `Ok`/`confirm_on_enter`/`footer`；`form.rs:1381`、`preferences.rs:674`、`snippets.rs:239`、`ops.rs:64/184/320` 手写同样结构 | 同一组元素 id（`"cancel"`/`"ok"`）在多处重复；`show_cancel`、`cancel_text`、图标在各副本间不一致 | `navidog_ui::skin::dialog_footer(ok_label, on_ok)`，所有对话框统一用 |
| D-07 | 小工具函数 | `driver_name`：`connection/lib.rs:339` ↔ `navigator/lib.rs:164`；`db_text`：`connection/lib.rs:357` ↔ `grid/lib.rs:29`；`kind_label`：`navigator/mod.rs:472` ↔ `ops.rs:127`，`info.rs:387` 又内联一次；拖拽预览：`navigator/mod.rs:307` ↔ `:360` | 改一处忘另一处；`kind_label` 的第三份是内联，搜索不到 | 放到 `navidog-ui` 或 `navidog-core` 的公共 helper；拖拽预览用同一组件 + 不同数据 |
| D-08 | 元数据缓存 | `navigator/catalog.rs:24-37`（5 个 map + tasks/generation/revision）与 `db/outline.rs:250-280`（outline 内存缓存 + 文件缓存） | 两套几乎相同的“按 scope 缓存 + 代际丢弃 + 失效”逻辑；一个在 navigator，一个在 db | 一个带类型的 `ScopeCache<T>`；outline 只是其中一种值；缓存失效策略由 shell 统一驱动 |
| D-09 | `nav.*` locale 块 | `connection/locales/connection.yml:378-461`（26 个键）与 `navigator/locales/navigator.yml:2-69` 重复；`nav.filter_starred` 的值不同（`Show starred` vs `Show only starred connections`） | 每个 crate 的 `i18n!` 是独立后端，所以 connection 的这 26 个键实际无人使用，是死数据；一旦有人复用，加载顺序决定文案，且两个值已经不一致 | 删除 connection.yml 的 `nav` 块；`nav.*` 只归 navigator；加一个跨文件重复键检查 |
| D-10 | 测试 helper | `wait`/`sql` 在 `query/editor_tests.rs:36`、`grid/tests/viewer.rs:41`、`navigator/tests/keys.rs:42`、`navigator/src/info_tests.rs:32`、`app/tests/query_refresh.rs:24` 各一份 | 20ms 真实 sleep + 10s deadline，和 AGENTS 的确定性指引相反；机器慢时偶发 | 新增 dev-only `navidog-test-support`，集中 `wait`/`open_sqlite`/`seed`；等待改用 `advance_clock` |
| D-11 | 更新清单 schema | Rust：`app/update.rs:151-190`；CI：`.github/workflows/publish.yml:564-575` 用 `printf` 手写同一份 JSON，平台作业用 6 字段行 + `awk` 位置解析 | 改一个 Rust 字段，CI 不会编译失败，发布时才暴露 | `cargo xtask manifest` 复用 Rust 结构体序列化；CI 只上传 |
| D-12 | `ObjectsPanel::content` 的双重构建 | `navigator/lib.rs:171` 的 `status_text` 与 `objects.rs:799` 的 render 各调用一次 | 每个对象行、属性都克隆一遍；见 B-09 | 状态栏只读缓存计数，不重建行 |

## 4. 设计与风格

| 编号 | 严重度 | 问题 | 证据 | 建议 |
| --- | --- | --- | --- | --- |
| S-01 | 重要 | 生产代码里仍有中文注释和用户可见中文；`DbError` 把本地化文案烘焙进 `message` | 注释：`core/registry.rs:1-111`（整文件）、`core/paths.rs:3,36,41`、`db/runtime.rs:1-2`、`db/ssh.rs:1,18,27`、`plugin/lib.rs:1-8`；文案：`db/metadata.rs:15,96`、`db/sqlite.rs:95,99`、`db/sessions.rs:106,168`、`db/mysql.rs:122,168,211`、`db/postgres.rs:77,188`、`db/ssh.rs:72`、`db/runtime.rs:16`、`app/lib.rs:178`、`app/state.rs:83,85` | 注释按 AGENTS 改英文；文案全部进 `locales/*.yml`。`DbError` 增加 `kind`（AppClosed/NotRelational/…），只在 UI 边界用 `t!` 渲染；服务器消息保持原文 |
| S-02 | 一般 | 未使用的依赖 | `anyhow` 在 6 个 manifest 中声明，源码只有 `query/completion.rs:13` 一处；`navidog-connection` 的 `tokio` 无使用、`serde_json` 仅测试 | 删除 5 个未使用的 `anyhow` 和 connection 的 `tokio`；测试专用依赖移到 `dev-dependencies`；把 `udeps` 从每周改到 PR/每次推送 |
| S-03 | 一般 | 配置保存失败的处理不一致 | `store.rs:486` 返回 `Result` 并弹通知；`settings.rs:333`、`favorites.rs:55`、`snippets.rs:150`、`column_widths.rs:58`、`state.rs:82` 只记日志；`favorites`/`snippets`/`state` 解析失败不备份 | 统一为 `JsonStore::save -> Result`；调用方弹一次“配置无法保存”；用户数据（收藏/代码段）保留 `.bak` |
| S-04 | 一般 | 同一状态有多份 | `AppState` 全局 + `MainWindow.state`（`app/lib.rs:71,107`、`main_window.rs:41,474`）；raw 标志三份（`viewer/mod.rs:211`、`grid/mod.rs:170`、`delegate.rs:20`）；`Settings` 启动读两次（`app/lib.rs:33`、`core/settings.rs:381`） | 一份为唯一所有者；`AppState` 只留全局或只留 window，另一个用借用；raw 只留 `ResultGrid` 并向 delegate 同步 |
| S-05 | 建议 | 命名/生命周期约定不一致 | `Tab_`（`connection/form.rs:82`，为避开 `Tab` 导入）；`run_once` 已弃用但仍公开（`db/sessions.rs:348`），只被 `tests/error_api.rs:16` 用；`store_error_message` 弃用转发（`connection/lib.rs:319`）；`StoreError::localized_message` 与自由函数 `error_message` 重复（`core/store.rs:15,195`）；`hex_encode` vs `encode_hex` | 统一命名；删除兼容入口前先改测试；一个错误一个本地化入口 |
| S-06 | 重要 | 文档与代码版本漂移 | `AGENTS.md:3,26` 与 `docs/design/00-overview.md:67,109` 写 gpui-kit 0.7/0.7.3；`Cargo.toml:27` 是 0.8；`deny.toml:7` 的注释也写 0.7.3 | 更新文档版本与“已知的坑”；这一步不做，后续按 AGENTS 排查 0.8 问题会走错方向 |
| S-07 | 一般 | 每个 crate 一个 i18n 后端，导致跨 crate 键不可复用，只能复制文案和测试 | `crates/*/src/lib.rs` 各自 `rust_i18n::i18n!`；`nav.database_session_lost`/`query.session_lost`/`viewer.session_lost` 三套 | 共享的恢复文案放 `navidog-ui`，以函数形式暴露；feature 只传自己的两条语境文案；测试只测基函数 |
| S-08 | 建议 | 面板标题与内容不符 | `navigator/mod.rs:464` 返回 `t!("nav.refresh")`（导航器标题显示 “Refresh”）；`info.rs:92` 永远返回 “General”，但信息窗格会切 DDL/Fields/Snippets | 加 `nav.title`/`info.title`，或按当前页返回；`title_bar(false)` 只是隐藏，不代表可以写错 |
| S-09 | 建议 | `navidog-designer` 的 i18n 声明顺序与其他 crate 不同，且没有 `include_str!` 跟踪 locale 变更 | `crates/navidog-designer/src/lib.rs:7` 在 `mod`/`init` 之后才 `i18n!`；其他 crate 在文件头并 `include_str!` | 统一到同一模板，避免只改 yml 时 Cargo 不重编译 |

## 5. 架构

### A-01 `navidog-core` 是 god crate，领域层与应用状态层混在一起

16 个源文件、4,478 行，包含：连接/对象模型、加密、路径、连接存储、`Settings`（49 个字段）、`Workspace`（选择、状态栏、query context）、`Favorites`、`VirtualGroups`、`Snippets`、`Saved queries`、`Column widths`、`Registry`、全部跨 feature 的 `actions`。`Settings` 同时保存外观、网格、编辑器、补全、自动恢复、更新、日志的设置；任何 feature 加一个偏好都要改 core。

**影响**：`navidog-core` 无法在不引入 gpui-kit 的情况下作为纯领域模型复用；M5 插件、M14 新数据库、未来的 CLI/同步都会先碰到这层；`Settings` 的 49 个字段让序列化兼容和测试都变重。

**建议**：拆成 `navidog-model`（纯数据：`connection`、`object`、`crypto`、`paths`、`queries`、`settings` 的数据结构）和 `navidog-app-state`（GPUI `Global`/`Entity`、`Workspace`、`Favorites`、`VirtualGroups`、`actions`、`ConnectionStore`）。先做“只搬代码”的提交，再逐个把 feature 设置收进各自的 `Settings` 子结构。

### A-02 领域层和驱动层依赖 `gpui-kit`

`navidog-core/Cargo.toml:11`、`navidog-db/Cargo.toml:13` 都引入 `gpui-kit`；`core/store.rs:5` 使用 `Entity/Global/AppContext`，`settings.rs:307` 实现 `Global`，`db/driver.rs:294` 实现 `Global`。依赖链变成 `navidog-db -> navidog-core -> gpui-kit -> gpui-pre/wgpu`。headless 使用驱动时要编译 GPU 栈；`navidog-db` 的单测需要 `gpui_kit::test`；驱动层与窗口框架的生命周期绑死。

**建议**：`navidog-model` 不依赖 gpui；`DriverRegistry` 由 shell 持有，`Sessions` 接收 `&Registry` 或泛型 `D: Driver`；`navidog-db` 只保留 `Driver/Session/Relational` 与纯 SQL；把 `sessions`、`log`、`outline`、`table_design` 里的应用状态移到 `navidog-app-state` 或新 crate `navidog-session`。

### A-03 `navidog-db` 的职责超出“驱动”

24 个文件、8,970 行，除 MySQL/PostgreSQL/SQLite/SSH 外，还包含：

- `sessions.rs`（436 行）与 `sessions/operation.rs`：连接代际、取消、drain、per-db 会话；
- `log.rs`：SQL 历史环形缓冲 + 写文件线程；
- `outline.rs`：补全元数据缓存 + 用户目录写文件；
- `table_design/*`（1,600+ 行）：表设计的 DDL 解析/生成/迁移。

这些都是“应用会话/缓存/功能规划”，不是数据库协议。它们让 `navidog-db` 成为最重的两个 crate 之一，也让 feature crate 为了元数据缓存反向依赖 db 的全局实体。

**建议**：`navidog-db` 保留 driver、dialect、metadata、sql、params、ssh；`sessions` 移到 `navidog-app-state`；`outline` 与 catalog 合并；`table_design` 的规划部分可以留在 db（它确实是方言），但把 UI 草稿模型移回 designer。

### A-04 驱动元数据散落，插件驱动的承诺无法兑现

同一件事在多个地方各写一遍：

| 事实 | 位置 |
| --- | --- |
| 默认端口 5432/3306 | `core/connection.rs:127-130`、`db/params.rs:84-88`、`db/mysql.rs:40`、`db/postgres.rs:38` |
| driver id 字符串分支 | `core/connection.rs`、`core/uri.rs:124`、`db/params.rs:84`、`connection/form.rs:390,682,831,1100`、`ui/assets.rs:77`、`query/editor.rs:647`、`designer/panel.rs:271`、`navigator/snippets.rs:65` |
| 图标 | `ui/assets.rs:77` 的 `driver_icon` |
| 连接表单字段 | `connection/form.rs` 内按 `mysql/pg` 分支 |
| 方言 | `Sessions::dialect`，但 restore 时 `designer/panel.rs:266` 和 `query/editor.rs:647` 各自 fallback 到字符串匹配 |

`docs/design/03-plugin-api.md` 说 `DriverRegistry` 的值包含“名称、类别、图标、默认端口、表单字段集合”，但 `DriverInfo`（`db/driver.rs:85`）只有名称/类别/端口/SSL/SSH。M5 的 JS 驱动无法声明图标和表单字段，只能改宿主代码。

**建议**：`DriverInfo` 增加 `icon`、`dialect`、`form`（字段集合/默认值）；`DriverRegistry::dialect(id)` 统一 restore fallback；`ui::driver_icon` 读 `DriverInfo.icon`；`Connection::new`/`ConnectParams` 用 `DriverInfo.default_port`，不硬编码。

### A-05 事务/提交逻辑在 feature crate，层次反向

`navidog-grid/src/commit.rs:31` 的 `apply(rel: &mut dyn Relational, ...)` 发 `BEGIN/SAVEPOINT/ROLLBACK/COMMIT`；`matched_a_row`（`commit.rs:63`）按 SQL 前缀判断 DELETE/UPDATE，是纯方言策略。`navidog-query` 为了复用提交，`results.rs:26` 依赖 `navidog-grid`；AGENTS 只允许这一条跨 feature 依赖，但根本原因是提交逻辑放错了层。

**建议**：把 `commit.rs` 移到 `navidog-db`（如 `dialect::commit` 或 `sessions::tx`），grid 只保留 UI 和 `ChangeSet`；query 不再需要为提交依赖 grid。`ChangeSet` 生成 SQL 时同时携带 `enum StatementKind { Insert, Update, Delete }`，不要用字符串前缀反推。

### A-06 缓存/选择状态多处复制，且出现层次倒挂

- `Catalog`（navigator）持有 schemas/objects/ddl/columns/children + tasks + generation + revision；`OutlineCache`（db）又有一套 scope → outline 的内存/文件缓存。
- `Catalog::refresh_connection`（`catalog.rs:260`）在缓存失效时调用 `Sessions::refresh`，即“元数据缓存”反过来驱动“连接会话”，与 feature → db 的方向相反。
- `Workspace::selections` 是权威选择，但 `NavigatorPanel` 还保存 `marked`、树的焦点行和 `anchor`（`navigator/mod.rs:411-413`），`sync_selection` 手工对齐。
- 四个面板各自实现 `ensure_loaded`（navigator、objects、info、ddl），条件不一致，于是 `objects.rs:234` 出现渲染期 `stalled_load` 补偿。

**建议**：一个带类型的 `ScopeCache<T>` + 一个 `Catalog::require()`；由 shell 决定“SQL 完成后刷新哪些缓存”，cache 不碰 sessions；选择只留 `Workspace`，navigator 的 `marked` 从 `Workspace` 派生。

### A-07 god object 与超长文件

| 单元 | 行数/方法数 | 混在一起的职责 |
| --- | --- | --- |
| `grid/viewer/mod.rs:507-1954` `impl TableViewer` | 1,448 行 / 41 方法 | 会话打开/重开、SQL 构建、分页/count、事务、筛选器、单元格编辑器、表单视图、约 600 行渲染；7 个 `*_after_commit`/`*_pending`/`*_stale` 状态量（`viewer/mod.rs:198-222`，其中 `hidden_after_commit` 是 `Option<HashSet<_>>`）编码一个状态机 |
| `query/results.rs:588-1281` `impl ResultView` | 694 行 | 网格接线、可编辑探测、单表识别、提交、重载、渲染 |
| `query/editor.rs:351-1296` `impl QueryEditor` | 946 行 | 面板生命周期、会话/上下文、保存/恢复、查找、缩放、工具栏 |
| `connection/form.rs:196-1286` `impl ConnectionForm` | 1,091 行 | 5 页表单、校验、秘密策略、测试连接、保存、路径浏览 |
| `app/update.rs` | 1,415 行 | manifest 模型、HTTP、下载校验、macOS/Windows/Linux 安装、UI 对话框 |
| `grid/model.rs:66-804` `impl GridModel` | 739 行 | 存储、选择、编辑 overlay、投影、排序、查找替换、TSV、变更集 |

**建议**：先做“只搬代码”的拆分，再改行为。`TableViewer` 拆出 `TableSession`（连接/分页/事务/提交）和 `TableView`（渲染）；用 `enum AfterCommit { Nothing, AddRow, Reload{count}, Close, HideColumn(name), Transaction }` 替代这 7 个状态量；`update.rs` 拆 `manifest/http/install/ui` 四个模块。

### A-08 提前抽象与死代码

- `navidog-plugin` 只有 13 行 `pub fn init(_cx: &mut App) {}`，却在 workspace 成员、`navidog` 依赖和发布清单里。
- `registry.rs` 的 `PanelRegistry`、`FormatRegistry`、`CellFormatterRegistry`、`FormatInfo`、`CellFormatterInfo` 无任何使用；`PanelInfo` 与 gpui 的 `PanelInfo` 同名但不同物。
- `Document`/`KeyValue` trait、`Family::Document/KeyValue` 没有实现，M14 之前是纯占位。
- `Sessions::run_once` 已弃用但仍公开，仅一个兼容测试使用；`navidog_connection::store_error_message` 同样。

提前定义接口本身不算错，但这些代码现在既没有测试也没有调用方，会随主分支演进而腐烂（例如 `PanelRegistry` 的 `PanelPlacement` 与真实 dock 注册已经脱节）。**建议**：M5 开工前把 plugin crate 从 workspace 移除或标 `publish = false` 并加 `#[doc(hidden)]`；registry 里未实现的类型先删，等实现时再加。

### A-09 测试架构

283 个测试是优点，但组织方式有债务：

- 没有共享的 test-support crate，`wait`/`sql`/`open_viewer` 在 5 个文件里复制（D-10）。
- `NAVIDOG_HOME` 是进程全局变量，各 crate 用互不相同的锁；`grid/viewer/mod.rs:2205` 的单个测试 859 行，`unsafe { std::env::set_var }`，失败时只报一个函数名，无法定位是哪一段场景。
- 交互测试用 `("designer-flag", 12usize)` 这样的 magic 索引（`designer/tests.rs:67`），加一列就会悄悄点到别的控件。
- 测试文件有的放 `tests/`，有的放 `src/*_tests.rs`，还有 `src/tests.rs`，没有统一规则。

**建议**：新增 dev-only `navidog-test-support`；把 859 行的测试拆成命名子流程；元素 id 改成 `(row, col)` 复合；统一测试位置规则并写进 AGENTS。

### A-10 `Connection` 用“交换字段”表示配置文件

`connection.rs:59-64` 把激活配置放在 `Connection::{general,advanced,ssl,ssh}`，非激活配置在 `profiles: Vec<Profile>`；`swap_profile`（`:81-88`）一次交换 5 个字段。后果：

- `ConnectionsFile::reencrypt`（`store.rs:103-128`）必须先 swap 进、处理、再 swap 出；任何 `return`/panic 都会把激活配置留在错误状态。
- “当前配置文件”不是显式字段，序列化依赖“恰好交换过”的不变量。
- 新增一个配置块（例如代理）要同时改 `Profile`、`swap_profile`、`reencrypt` 三处。

**建议**：`profiles: Vec<Profile>` + `active: usize`，用 `active()`/`active_mut()` 访问；或者在读写边界构造 `ActiveProfile` 新类型。迁移时要兼容现有 `connections.json`（用 `#[serde(flatten)]` 过渡）。

## 6. 健壮性、安全、性能

| 编号 | 严重度 | 问题 | 证据 | 建议 |
| --- | --- | --- | --- | --- |
| B-01 | 重要 | 连接表单静默丢配置 | `form.rs:440` `g.port = value.parse().ok()`；`:446` 多节点 `filter_map(Member::parse)`，非法行直接消失；`:483` SSH 端口 `unwrap_or(22)`；`core/connection.rs:227` 端口解析失败返回 `None` | `collect` 对每个数值字段做显式校验，非法时返回 `Err(t!("conn.bad_port", …))`；多节点保留原始行并标错，不要过滤 |
| B-02 | 重要 | SSH 主机密钥无条件接受 | `db/ssh.rs:19-24` `Ok(true)`；`progress.md` §4 已登记“不校验服务器主机密钥” | 默认 `known_hosts` 校验 + 首次信任提示；设置里允许关闭；至少记录服务器指纹 |
| B-03 | 重要 | 更新包的临时路径与校验/执行窗口 | `app/update.rs:449-470` 固定 `temp_dir()/navidog-update`，`File::create` 跟随 symlink；`:501-505` 校验后返回路径，`install` 在 `:567` 再按路径打开；`:597/:732` 交给 `hdiutil`/`open` 执行 | 用 `tempfile::Builder` 建 0700 私有目录；校验后使用同一文件句柄/`persist`，避免 TOCTOU；正式签名后校验 `codesign`/Team ID 与 Windows Authenticode；清单与包同源，SHA-256 不能防镜像被篡改 |
| B-04 | 一般 | 连接 id 未校验就拼进 profile 路径 | `column_widths.rs:21`、`vgroups.rs:190`、`db/outline.rs:201` 都 `profiles.join(connection)`；只有 database/schema 走 `encode_segment` | `paths::profile_dir` 校验 id 格式（`c-` + hex）或编码；所有 profile 文件走同一个 helper |
| B-05 | 重要 | 参数替换改写 PostgreSQL 美元引号字符串 | `util/sql_text.rs:59` 的 `find_params` 跳过 `'`/`"`/`` ` ``/`--`/`/*`，不跳过 `$tag$`；`db/sql.rs:126` 的分句器有 `$tag$` | 参数扫描复用 R-04 的统一词法器；补 `SELECT $$ [$x] $$` 的测试 |
| B-06 | 一般 | TSV 复制/粘贴不转义制表符和换行 | `grid/model.rs:640-642` 注释自己承认；`paste` 在 `:786`/`:792` 按 `\t`/`\n` 拆；`progress.md` §4 已登记 | 带引号 TSV（RFC 4180 风格）或只在值含分隔符时加引号；补往返测试。数据库客户端里的静默错列比“暂不支持”更糟 |
| B-07 | 一般 | 查询结果内存排序与表查看器服务器排序语义不同 | `grid/model.rs:119` 每个比较都 `parse::<f64>()`，非数字回退字符串比较；`results.rs:929` 走内存排序，`viewer/mod.rs:590` 发 ORDER BY 给服务器 | 按 `GridColumn.type_name` 选择比较器；或查询结果也走服务器排序；至少把“排序是本地近似”写进 UI |
| B-08 | 一般 | 每格每帧类型解析与 `refit_auto` 分配 | `grid/delegate.rs:511` `type_kind(&col.type_name)` 每格；`model.rs:213` `is_string_type`（raw 模式每格）；`grid/mod.rs:908-934` 每列最多 64 个 `String`，200 列约 13k 次分配/每次 set_data | `GridColumn` 预计算 `TypeKind`/`is_text`；`refit_auto` 复用缓冲或只对可见列做 |
| B-09 | 一般 | 状态栏每帧重建整个对象列表 | `navigator/lib.rs:171` `ObjectsPanel::content(cx)` 只为取 `rows.len()`；`content`（`objects.rs:262`）遍历连接/组/虚拟组/对象并 `properties.clone()`；`main_window.rs:660` 每帧调用 | `ObjectsPanel` 缓存 `(revision, count)`；状态栏读缓存 |
| B-10 | 一般 | `Catalog` 的 `tasks`/`generation` 无界 | `catalog.rs:36-37` 用 `format!("{slot}:{key:?}")` 作键，只增不删；`invalidate_connection` 清 5 个数据 map，不清这两个 | 关闭连接时 prune；用 typed key 替代 `Debug` 字符串 |
| B-11 | 一般 | 文件名编码不覆盖 Windows 非法字符 | `core/queries.rs:13-23` 只编码 `/ \ : %` 和首位 `.`；`*`、`?`、`"`、`<`、`>`、竖线和控制字符在 Windows 上仍非法；服务器库名/查询名可能包含它们 | 统一文件名编码（percent-encode 非 `[A-Za-z0-9._-]`），或使用 `sanitize-filename` 的规则；补 Windows 目标测试 |
| B-12 | 建议 | 连接表单的初始焦点在 `open_dialog` 之前设置，通常不生效 | `form.rs:236` 在 `ConnectionForm::new` 里 `focus`，`open_dialog` 在 `:73` 才调用；同文件 `secrets.rs:260` 的注释已说明必须先 defer | 与 `secrets.rs` 一样用 `window.defer` 在对话框打开后聚焦 |
| B-13 | 建议 | UI 路径仍有 panic 点 | `app/lib.rs:178` `expect("无法打开主窗口")`；`db/runtime.rs:16`；`state.rs:83`；`results.rs:1144` `drafts.unwrap()`；`run.rs:394/749` | 主窗口打不开时显示系统对话框并退出；其余把不变量改成 `debug_assert` + 降级路径 |
| B-14 | 建议 | 历史日志 250ms 轮询 | `query/history.rs:139`；`progress.md` §4 已登记 | 用 `cx.subscribe`/channel 从 `db/log.rs` 推送，轮询只作兜底 |
| B-15 | 建议 | `write_atomic` 无 `fsync`、固定临时名 | `core/paths.rs:42-49`；`crypto.rs:237` 另有一套 | 同 R-07；至少 `sync_all` + 唯一临时名 |
| B-16 | 建议 | 查找的 ASCII-only 折叠 | `ui/skin.rs:56-71` 只做 ASCII 大小写；`queries.rs:83` 用 Unicode `to_lowercase`，两处行为不一致 | 统一用 Unicode 折叠并按字节映射回原文；或明确“仅 ASCII”并写进规格 |

## 7. 建议整改路线

### P0：一周内可完成，直接消除正确性/安全问题

1. **i18n 收口**：15 条中文用户可见文案进 locale；`DbError` 增加 `kind`，UI 边界本地化；删除 `connection.yml` 的 26 个 `nav.*` 死键；加 CI lint（`.rs` 字符串里出现 CJK 即失败，`tests/`/`examples/` 除外）。
2. **输入校验**：连接表单端口、多节点、SSH 端口、auth 选择全部显式报错；补 UI 测试（当前只有 happy path）。
3. **URI/参数**：`percent` 解码去掉 `+`→空格；`find_params` 跳过 `$tag$`；补 `mysql://u:p+q@h/db` 和 `SELECT $$ [$x] $$` 回归。
4. **UUID**：`query/editor.rs` 改用 `uuid::Uuid::new_v4().simple()`；旧自动恢复文件的 id 不变，只影响新文件。
5. **路径安全**：`paths::profile_dir` 校验/编码连接 id；三个 profile 文件 helper 统一。
6. **依赖与文档**：删除 5 个未使用的 `anyhow`、connection 的 `tokio`；把 `udeps` 改到 PR；把 AGENTS/00-overview/deny.toml 的 gpui-kit 版本改成 0.8。

### P1：两到四周，建立可复用的基础设施

1. **`navidog-util::sql::spans`**：带方言的引号/注释/`$tag$` 扫描；迁移 `sql.rs`、`sql_text.rs`、`sql_format.rs`、`filter.rs`、`table_design/parse.rs`、`properties.rs`；每迁移一处保留现有测试，补一致性测试（同一段 SQL 六个消费者结论一致）。
2. **`JsonStore<T>`**：`load_or_backup`、原子 `save -> Result`、坏文件策略；迁移 9 处；收藏/代码段至少保留 `.bak`。
3. **恢复文案**：`db_error_text(Recovery)` 一个入口，删除 6 个包装和 5 份重复测试。
4. **提交逻辑下移**：`grid/commit.rs` → `navidog-db`；`ChangeSet` 带 `StatementKind`；统一表查看器与查询结果的提交流程，消除 `false,false` 分叉。
5. **MySQL/PG 恢复默认实现**：`Relational` 默认 `execute`/`execute_read`，驱动只实现 `attempt`/`reconnect`/`remember`/`lost`；补一个共享的恢复测试套件。
6. **驱动元数据**：`DriverInfo` 加 `icon`/`dialect`/`form`；删除 `ui::driver_icon`、`params.rs` 的端口分支、`connection/form.rs` 的 `mysql/pg` 字符串判断；M5 前完成。
7. **UI 样板**：`dialog_footer`、`close_prompt`、`GridSession` 三个 helper；`TableViewer`/`QueryEditor` 改用。

### P2：与 M5/M6 并行，做结构性拆分

1. **拆 core**：`navidog-model`（纯）+ `navidog-app-state`（GPUI 全局/动作/workspace）；先搬代码，后改 API。
2. **拆 db**：driver 与 session/log/outline 分离；`navidog-db` 不再依赖 gpui-kit；`Sessions` 由 shell 注入注册表。
3. **拆 god object**：`TableViewer`（1,448 行 impl）、`ConnectionForm`（1,091 行）、`update.rs`（1,415 行）、`ResultView`（694 行）。
4. **缓存/选择统一**：`ScopeCache<T>`、`Catalog::require`、选择只留 `Workspace`。
5. **连接配置文件模型**：`profiles + active`，迁移 `reencrypt`/`swap_profile` 调用点。
6. **测试基础设施**：`navidog-test-support`；拆 859 行测试；元素 id 复合化。
7. **死代码清理**：plugin crate 暂退 workspace/发布清单；未使用的 registry 类型、`Document`/`KeyValue`、`run_once`/`store_error_message` 删除或标 `doc(hidden)`。

### 建议新增的 CI 门禁

| 门禁 | 目的 | 现状 |
| --- | --- | --- |
| `cargo udeps` on push/PR | 抓未使用依赖 | 现在只在每周一跑 |
| i18n lint | `.rs` 字符串禁止 CJK；跨 locale 文件重复键；`t!` 键必须存在于本 crate | 无 |
| `cargo deny` 已有；`cargo machete` 可选 | 依赖与许可证 | deny 已有；`udeps` 未进门禁 |
| “只搬代码” diff 检查 | 结构拆分时禁止行为变更 | 无；可要求拆分 PR 带 `--color-moved` 说明 |
| test-support 使用检查 | 新测试不得再复制 `wait`/`sql` | 无 |

## 8. 附录

### A. 各 crate 规模

| crate | 源文件 | 生产行数 | 测试文件 | 测试行数 | 测试数 | 最大生产文件 |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| navidog | 13 | 4,712 | 5 | 1,910 | 18 | `src/update.rs` 1,415 |
| navidog-connection | 7 | 3,042 | 5 | 670 | 8 | `src/form.rs` 1,396 |
| navidog-core | 16 | 4,478 | 1 | 68 | 35 | `src/store.rs` 979 |
| navidog-db | 24 | 8,970 | 5 | 2,709 | 76 | `src/table_design/properties.rs` 731 |
| navidog-designer | 7 | 2,986 | 1 | 1,005 | 8 | `src/panel.rs` 965 |
| navidog-grid | 18 | 10,069 | 2 | 1,049 | 45 | `src/viewer/mod.rs` 3,064 |
| navidog-navigator | 17 | 6,915 | 4 | 1,013 | 18 | `src/navigator/mod.rs` 1,104 |
| navidog-plugin | 1 | 13 | 0 | 0 | 0 | `src/lib.rs` 13 |
| navidog-query | 17 | 9,350 | 1 | 2,518 | 34 | `src/results.rs` 1,884 |
| navidog-ui | 7 | 2,042 | 1 | 108 | 14 | `src/tokens.rs` 559 |
| navidog-util | 8 | 1,978 | 0 | 0 | 27 | `src/explain.rs` 571 |
| **合计** | **135** | **54,555** | **25** | **11,050** | **283**（另 `xtask` 1） | |

### B. 跨 crate 依赖边（`Cargo.toml`）

```
navidog            -> ui, core, db, plugin, connection, navigator, grid, query, designer, util
navidog-connection -> ui, core, db
navidog-core       -> (无 navidog 依赖；依赖 gpui-kit)
navidog-db         -> core, util (依赖 gpui-kit)
navidog-designer   -> ui, util, core, db
navidog-grid       -> ui, core, util, db
navidog-navigator  -> ui, core, db, util
navidog-plugin     -> (无)
navidog-query      -> ui, core, util, db, grid   # 唯一允许的 feature 间依赖
navidog-ui         -> (无 navidog 依赖)
navidog-util       -> (无 navidog 依赖)
```

**垂直方向的泄漏**是主要问题：`core` 和 `db` 都依赖 `gpui-kit`，所以“纯领域/纯驱动”的分层只存在于文档。水平方向干净：除 `query -> grid` 外没有 feature 互依赖。

### C. `progress.md` 已登记的偏差（本文不重复计为新问题）

- 日期选择器的周一为首日、`今天` 用 UTC（M2，§4）。
- TSV 复制不转义制表符/换行（M2，§4）。
- 历史日志每 250ms 轮询（M3，§4）。
- SSH 不校验服务器主机密钥（M1，§4）。
- 关闭有未提交修改的选项卡依赖 `Panel::closable` + 关闭守卫，而不是 gpui-kit 钩子（M2，§4）。
- MySQL 的 UPDATE 不检查匹配行（M2，§4）；MySQL 写入超时只保存（M3，§4）。
- 代码补全别名不支持子查询作用域（M3，§4）。
- 弹出窗口的查询编辑器没有代码段/标识符页（M3，§4）。

这些偏差本身有理由；本文只在它们与结构方案冲突时引用（例如 UTC 日期用 chrono 可以顺手消除，TSV 应在“网格会话”抽象里一次修好）。

### D. 审查方法说明与已知假阳性

- 相同函数体：归一化后 SHA-1 分组得到 9 组；多数是测试 helper 或被误切的嵌套 `fn` 声明，真正有价值的只有 `driver_name`、`close_now` 等少数几组，已分别归入 D-04/D-07。
- 近似函数：8-token shingle，Jaccard ≥ 0.62，人工筛掉“同名不同逻辑”和测试；保留的 D-01～D-04 都逐行核对。
- 子代理报告中未采纳的结论：
  - “`status_text` 从未被调用”：实际在 `query/editor_tests.rs:2484` 使用，只是测试专用；已改为“测试专用访问器”。
  - “连接表单 7 个标签自带冒号”：当前 `connection.yml` 已无尾冒号；`form.rs:845` 的 `trim_end_matches(':')` 现在是冗余代码，属于 P3 清理项，不是 bug。
  - “`navidog-designer/src/lib.rs` 没有模块头”：实际有 `//!`，只是 `i18n!` 在文件末尾。
- 未运行实库/截图；文中的性能问题是静态分析（每帧调用、每格解析、分配次数），不是本次实测的帧时间。既有验收记录的性能数字（`docs/pr/acceptance-m3.md`、`acceptance-m4.md`）只用于判断基线。

### E. 快速索引（最重要的 15 条）

| 编号 | 位置 |
| --- | --- |
| R-01 | `crates/navidog-query/src/editor.rs:1372` |
| R-02 | `crates/navidog-grid/src/date.rs:104-158` |
| R-03 | `crates/navidog-core/src/uri.rs:168-201` |
| R-04 | `crates/navidog-util/src/sql_text.rs:59`、`crates/navidog-db/src/sql.rs:74`、`crates/navidog-grid/src/filter.rs:912` |
| D-01 | `crates/navidog-ui/src/lib.rs:22`、`crates/navidog-query/src/errors.rs:6` |
| D-02 | `crates/navidog-core/src/favorites.rs:43`、`crates/navidog-core/src/snippets.rs:142` |
| D-03 | `crates/navidog-db/src/mysql.rs:180-318`、`crates/navidog-db/src/postgres.rs:272-537` |
| D-04 | `crates/navidog-grid/src/viewer/mod.rs:1132`、`crates/navidog-query/src/results.rs:939` |
| S-01 | `crates/navidog-db/src/metadata.rs:15`、`crates/navidog-core/src/registry.rs:1` |
| A-01 | `crates/navidog-core/src/settings.rs:159`、`crates/navidog-core/src/actions.rs:1` |
| A-02 | `crates/navidog-core/src/store.rs:5`、`crates/navidog-db/src/driver.rs:294` |
| A-05 | `crates/navidog-grid/src/commit.rs:31` |
| B-01 | `crates/navidog-connection/src/form.rs:440-446` |
| B-02 | `crates/navidog-db/src/ssh.rs:19-24` |
| B-03 | `crates/navidog/src/update.rs:449-505` |
