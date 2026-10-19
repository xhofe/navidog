# M4 表设计器本机验收

日期：2026-10-18。基线：M3 分支 `codex/m3-completion` 的 `4bb20ea`；开发分支：`codex/m4-table-designer`。规格先提交为 `47fdce5`。本次实现表设计器首版，M4 整体仍进行中。

## 2026-10-18 首版（界面已由下文替换）

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

## 2026-10-18 首版遗留（界面相关项见下文更新）

字段目前为“名称 + 完整 SQL 定义”，约束与索引也使用 SQL，尚未提供 Navicat 的逐属性网格及结构化表单，因此 F-DSG-001/002/003/004/007/009 仍标为部分完成。其他对象设计器不在本次实现内。

SQLite 生成列表重建和附加数据库、PostgreSQL 物理列重排及现有特殊属性/选项/注释修改等明确拒绝。MySQL DDL 无法事务回滚，失败后必须重新加载，执行前结构检查也不能消除并发 DDL 窗口。

本次参考本地 Navicat 帮助 `reference/help-text/mysql_table.txt`、`postgresql_table.txt`、`sqlite_table.txt`、`hot_keys.txt`，复用项目已有尺寸 token；未完成 Navicat 并排视觉校准，不把自动截图当作该项验收。详细边界见 `docs/design/40-table-designer.md` 第 5 节。

## 2026-10-19 CI 修正

PR #3 的 Linux/Windows 界面测试失败于字段名称断言：测试使用 `cmd-a`，没有触发这两个平台的 Input 全选，导致输入追加到默认名称。三个全选操作改为组件支持的 `secondary-a`（macOS Command，其他平台 Control），保留真实键盘输入和草稿断言，并为失败回滚用例增加字段定义精确断言，避免无效 SQL 误代替预期的非空约束失败。

本机设计器两项测试通过；远程跨平台 CI 在推送修正后重新验证。

## 2026-10-19 按 Navicat 重新对齐

撤回首版“字段列表 + SQL 输入框”的主界面。依据本地 Navicat 17.3.12 的 `TableDesignFieldModule_mysql`、`TableDesignFieldsModule_sqlite`、`TableDesignIndexesModule_mysql`、`TableDesignForeignKeysModule_mysql` 导出的视图树，改成字段网格在上、属性区在下，可拖动分隔条并保存位置。表头 23pt、属性控件宽度/行高/间距有 nib 来源；列宽及默认分隔高度仍标为暂定。

字段网格包含名称、类型、长度、小数点、不是 null、虚拟、键、注释，提供单元格输入、类型菜单和复选项。下方提供默认值、排序规则、字符集、自动递增、无符号及生成表达式/存储类型；按字段类型和方言显示。SQLite 自动递增会将单字段 INTEGER 主键转为列内定义，避免产生无效 AUTOINCREMENT。

索引、外键和检查使用独立列表及表单；本表字段、索引方法、删除/更新动作可通过菜单选择。复杂属性/未知定义原样保留在高级页并提示，不通过表单丢弃。SQL 预览、明确执行和失败保留草稿继续沿用。存在无效输入时禁止切换对象或生成旧草稿的预览，修正后恢复。

本次本机验证：

- 全量 209 项测试通过；fmt、全量 clippy、裸值检查及 diff 检查通过。
- `table_design` 8 项通过，包括结构化属性/索引/外键/检查在 MySQL 8、MariaDB 11、PostgreSQL 16 的实库创建与修改；SQLite 实库/UI 验证建表和约束失败回滚。
- 原有数据库实库 11 项、查询 crate 开启实库的 27 项通过。一次中途失败留下的测试表导致元数据断言失败，删除该精确测试表后重跑通过。
- 设计器两个 headless 场景扩展到单元格类型/长度、无效输入阻止预览、默认值、非空、SQLite 自动递增，以及索引/外键/检查表单，保留真实按键、关闭守卫和草稿恢复检查。
- 截图测试输出 42 张，本次设计器 15 张（字段、预览、索引、外键、检查浅深色，以及宽窗和真正的小视口）逐张检查。`TestPlatform::resize` 只改截图尺寸而不发出布局回调，因此小视口改为单独创建窗口，避免把裁剪图当作窄窗布局。
- deny/audit 通过，仍有已允许的间接依赖警告。网格 10 万 × 20：逐行均值 2.88ms、p95 3.43ms；跳跃 3.98ms、p95 4.58ms；导航树均值 0.52/0.75ms；5 万行 SQL 四场景均值 1.52~1.55ms，均 0/300 帧超过 8.3ms。

运行态 Navicat 的辅助功能曾能读到连接表单，随后屏幕采集报错，最终工具明确返回 Mac 已锁定。本次没有宣称完成并排像素校准；用户可直接检查 `target/snapshots/designer-wide-dark.png` 和 `designer-minimum-light.png`。远程 CI 与本机证据分开，按 PR 的对应提交检查。

仍待完成：字段拖放、全部方言专属属性、引用对象元数据级联选择、完整表选项表单，以及 PostgreSQL 已有注释编辑。现有方言迁移限制不变，F-DSG-001/002/003/004/007/009 和 M4 整体继续标为部分/进行中。
