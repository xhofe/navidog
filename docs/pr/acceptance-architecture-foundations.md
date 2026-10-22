# 架构基础优化第一批验收

日期：2026-10-21。基线：`upstream/main` 的 `4c403de`；分支：`codex/architecture-foundations`。规格先提交为 `4d09157`，边界补充见 `8bc7a6e`、`cf9e7b3`。本分支基于 main 现有 M1–M3，未合入 M4 PR。

## 已完成范围

1. `34702fa`：普通 SQL 每次最多执行一次，传输失败不自动重放；应用元数据通过显式安全读取允许一次恢复，普通 SQL 会话保守保持物理会话依赖。重连更新稳定取消目标，关闭/失效后清除。包含 MySQL、MariaDB、PostgreSQL 主动断连、已提交写入响应丢失、事务丢失与旧取消句柄的实库检查。
2. `5fc7866`：连接配置采用候选保存事务，成功落盘后才更新内存/密钥和发布事件；编辑、复制、删除、分组及批量操作失败回传。删除的关闭会话/清理目录，以及配置文件切换的重连，都在保存成功后执行。测试覆盖激活与非激活配置文件内全部秘密、写入阻断、失败重试与事件次数。
3. `023d3c4`：GridModel 唯一拥有当前结果行与隐藏列，排序/投影不再用旧快照覆盖已提交值；结果来源在运行开始时固定，固定结果切换连接后仍按原来源提交和刷新，匹配会话保留临时对象与事务上下文。包含隐藏/恢复列顺序回归，并移除整份 ResultSet 的多余克隆。
4. `ace88be`、`b1568e0`：复核后补齐空白新增行隐藏/恢复列的默认值状态；跟踪已识别 `USE` / `SET search_path` 后的服务器实际来源，独立刷新恢复完整 PostgreSQL 路径。探测失败保留原 SQL 成功，不重放；来源未知时仅复用同一上下文，拒绝独立会话刷新。实库断言确认刷新后修改覆盖层已清除。
5. `66f9391`：修复全量审计发现的基线证书依赖漏洞，仅更新 `rustls-webpki` 0.103.15 → 0.103.16 与 `webpki-roots` 1.0.9 → 1.0.10；未增加审计忽略。

## 初版与 CI 修复后的本机验证（2026-10-21）

| 检查 | 结果 |
| --- | --- |
| 格式、clippy、裸值和 diff 空白检查 | 通过；依赖保留既有 future-incompatibility 提示 |
| `NAVIDOG_LIVE=1 cargo test --workspace --locked` | 215 项，33 个测试/文档测试套件全部通过，无失败或忽略 |
| DB 实库 | 14 项通过，含原有协议/TLS/SSH/空闲恢复与新增故障注入 |
| 查询测试 | 29 项通过，含双 SQLite 来源隔离、固定结果、临时视图、运行中改目标、三种实库的 SQL 上下文与探测失败 |
| 截图 | 29 张，浅深色逐张检查；新增配置保存失败原图两张随代码归档 |
| 依赖审计 | `cargo deny --all-features check` 四项全部通过；`cargo audit` 在线更新后通过，保留既有 9 条允许的间接依赖 unmaintained 警告。RustSec 数据库为 `e6f94f37`（2026-10-20），已包含 RUSTSEC-2026-0382、RUSTSEC-2026-0381 |

首次审计命中的两项基线漏洞已通过兼容补丁更新修复，并重新在线审计。验证使用仓库固定的 Rust 1.99.0 / gpui-kit 0.8；复现记录保存在本地 `target/architecture-checks/`，构建产物不进 Git。实库容器在验证后关闭，不影响其他本机数据库。

截图和复现说明见 [截图集](../screenshots/architecture/README.md)。本机渲染不代替 Navicat 运行态并排验收。

## CI 稳定性补验

PR #14 的原始提交 `be309f0` 在 Intel macOS 的[一次 CI 运行](https://github.com/xhofe/navidog/actions/runs/39535956784/job/119012529027)中于 `info_tests::query_pages_follow_the_editor` 保存代码段失败，同一提交的另一次 Intel 运行通过。测试未关闭对话框入场动画，`window.click` 在两帧之间取坐标并命中时可能落到移动后的其他位置；打开与输入也挤在同一个窗口更新内。这与 AGENTS 记录的动画和通知时序问题一致。

修复仅调整测试：初始化启用 reduced motion，打开对话框后先完成一轮调度，再执行真实点击和输入；保存前断言名称为 `films` 且名称框拥有焦点，保留原有保存结果和数据库类型断言。未增加重试点击、等待保存或跳过测试。该用例本机连续 20 次独立运行通过；其余复验和远端平台检查见 [PR #14](https://github.com/xhofe/navidog/pull/14/checks)。

修复后本机重跑全工作区实库模式仍为 215 项、33 个套件全部通过；fmt、裸值检查、clippy、deny、在线 audit、29 张截图及三项 release 性能检查均通过。归档的浅深色失败截图与本次生成结果逐字节一致。实库容器已关闭，记录保存在 `target/architecture-checks/ci-fix-*.log`。

## PR #14 review 修复（2026-10-22）

本轮按作者的八条评论补充实现与回归，规格先提交为 `a3bdf52`。推送前合入作者新增的 `9c69bd1`、`0c9f525`，保留其新增回归、公开接口、分组缺失 ID 拒绝，以及独立操作会话中的 MySQL 数据库上下文准备；合并结果重新全量验证。下表记录当前行为及可运行的检查入口；上面的 215 项测试、29 张截图和后面的性能数字属于上一轮验证，不代表本轮最终结果。

| 评论 | 修复后的行为 | 回归入口 |
| --- | --- | --- |
| [4332969362：未提交时排序箭头与行不一致](https://github.com/xhofe/navidog/pull/14#discussion_r4332969362) | 查询结果先结算正在输入的单元格；有待提交修改或空白新增草稿时拒绝排序，显示本地化提示，保留行与上次接受的排序箭头。表头内部记录的排序也恢复，下一次点击仍按原顺序切换。 | `navidog-query` 的 `editor_tests::accepted_results_keep_their_rows_and_original_source`：点击真实表头，分别覆盖脏值、空白草稿及拒绝后的后续排序。 |
| [4332969373：完成写入污染元数据会话恢复](https://github.com/xhofe/navidog/pull/14#discussion_r4332969373) | 查询结果提交、导航器 DDL/复制表和连接 FLUSH 通过 `Sessions::run_operation_rel` 使用独立短期会话；闭包只执行一次，结束后关闭。关闭失败只记日志，保留原操作成功或失败；共享元数据会话保持安全读恢复能力。表查看器仍使用自己持有的会话。 | `navidog-db` 的 `sessions::tests::operation_cleanup_preserves_success_and_failure_without_replaying`；实库 `sessions::tests::completed_operation_does_not_pin_or_replay_on_metadata_recovery`（MySQL/MariaDB/PostgreSQL，需 `NAVIDOG_LIVE=1`）；查询编辑器原来源回归另保留一个元数据事务，确认结果提交不会进入该事务。 |
| [4332969383：数据库层写死英文恢复引导](https://github.com/xhofe/navidog/pull/14#discussion_r4332969383) | `DbError` 保留驱动原始错误码和消息，单独提供会话丢失、执行结果未知状态。网格、查询、导航器和连接 FLUSH 在各自界面边界使用 en/zh-CN 文案；连接建立失败不误报写入结果未知。 | DB 的 `execution::tests::session_loss_keeps_the_original_database_error_and_structures_its_outcome`、现有 `uncertain_writes_are_never_replayed` / `transaction_loss_cannot_reconnect_through_metadata_or_ping`；导航器 `errors::tests::recovery_guidance_is_localized_without_changing_the_driver_diagnostic`；连接 `reload::tests::flush_recovery_guidance_keeps_diagnostics_and_uses_the_interface_language`。 |
| [4332969392：缺失 ID 静默成功](https://github.com/xhofe/navidog/pull/14#discussion_r4332969392) | 单个或批量更新遇到任意缺失 ID，先返回 `StoreError::NotFound`，不运行任何修改回调、不保存、不发布 Changed；空批量成功但不执行回调或保存。 | `navidog-core` 的 `store::tests::missing_update_targets_are_rejected_before_any_callback`：覆盖缺失 ID 在批量前、中、后的情况，核对回调、事件、内存与原磁盘内容。 |
| [4332969398：保存密码失败阻断本次连接](https://github.com/xhofe/navidog/pull/14#discussion_r4332969398) | 保存失败后仍以本次输入的内存凭据继续连接，关闭密码对话框并仅调用一次连接回调；通知明确密码没有保存，原持久配置保持不变。 | `navidog-connection` 的 `secrets::tests::failed_password_save_still_uses_credentials_once`：制造写入失败，核对一次回调、收到的凭据、对话框关闭和持久状态。 |
| [4332969404：配置错误混用中英文且 feature 无法复用](https://github.com/xhofe/navidog/pull/14#discussion_r4332969404) | StoreError 的本地化映射移到 `navidog-core`，连接与导航器使用同一入口；日志 Display 和界面文案分开，不增加 feature crate 互相依赖。 | `navidog-core/tests/store_errors.rs` 的 `store_errors_have_english_and_chinese_messages`，检查错误变体在两种语言下的文案及原诊断保留。 |
| [4332969407：隐藏列拒绝结果被丢弃](https://github.com/xhofe/navidog/pull/14#discussion_r4332969407) | `ResultGrid::set_hidden_columns` 返回成功或拒绝，查看器只在成功后更新隐藏列菜单状态。拒绝时保留列、活动编辑器与待提交值；空白新增草稿仍可投影。 | `navidog-grid/tests/grid.rs` 的 `refused_column_visibility_keeps_pending_values_and_the_editor`；查看器 `viewer::ownership_tests::visibility_uses_accepted_updates_inserts_and_deletes`。 |
| [4332969410：刷新静默丢失修改](https://github.com/xhofe/navidog/pull/14#discussion_r4332969410) | 查询结果手动刷新先结算活动单元格，再拒绝待提交修改和空白草稿；拒绝不发送 SELECT。提交后同步走单独的重载路径，先接受已完成的写入；后续读取失败不保留可重复提交的旧修改。 | 查询编辑器原来源回归检查活动输入、空白草稿、SQL 日志及触发器计数；同步读取故意失败后，再次提交不会产生第二次写入。 |

此外，导航器右键菜单新增“重新打开连接”，断连操作错误也能引导到原连接的同一动作。确认文案说明事务、锁、临时对象无法恢复，已执行语句的结果可能未知；取消不关闭会话。确认后复用 `SessionsEvent::Reopening`，保留编辑器选项卡、查询文本及本地行草稿；再次提交前需要核对服务器状态，不能把重新打开理解为恢复旧事务或安全重试。回归入口为 `navidog-connection/tests/reopen.rs` 的 `reopening_requires_confirmation_and_preserves_the_editor_lifecycle`，以及导航器 `ops::recovery_tests::session_loss_error_offers_reopen_for_the_original_connection_after_dismissal`。查看器原来源回归实际替换会话，确认草稿不丢失、不自动提交，用户明确 Apply 后才写入；查询编辑器原来源回归通过真实文本输入产生未保存文档，确认重开前后文档、固定结果及空白新增草稿保留。

### 本轮最终验证

- 最终代码以 `NAVIDOG_LIVE=1 cargo test --workspace --locked` 通过 232 项、37 个测试/文档测试套件，0 失败/忽略；其中 DB 为 27 项 lib、15 项 live、2 项 SQLite，query 31 项。新增操作会话回归位于 lib 内，实测 MySQL/MariaDB/PostgreSQL 写入后元数据恢复；CI integration 同步执行 `cargo test -p navidog-db --lib --test live --locked`。显式重开的查看器及查询草稿保留断言也包含在最终全量结果中。
- fmt、全工作区全目标 clippy（拒绝警告）、裸值与 diff 空白检查通过。在线 `cargo audit` 使用 RustSec `4b587f00`（2026-10-20，1348 条通告），无未允许的漏洞；`cargo deny --all-features check` 的 advisories/bans/licenses/sources 全部通过。保留既有 9 条允许的间接依赖 unmaintained 警告和原有 future-incompatibility 提示，未新增忽略。
- 37 张新生成的浅深色截图已逐张检查；新增重开确认两张原图随代码提交，见 [截图集](../screenshots/architecture/README.md)。既有保存失败两张原图与本次生成结果逐字节相同。截图不代替 Navicat 运行态并排验收。
- 三项 release 性能复验通过：10 万行 × 20 列网格逐行/跳跃平均 3.35/4.80ms；导航树 0.54/0.74ms；5 万行 SQL 的滚动、装饰及输入平均 1.63～1.67ms，首次解析 340ms；各场景均 0/300 帧超过 8.3ms。以上仅为本机测量。
- 本轮复现日志保存在 `target/pr14-review-checks/`；实库容器验证后关闭。PR #14 已合并，本轮补充从最新 main 的 `9f3a926` 整理到 `codex/pr14-review-followup`；保留已合入的 PR #15 底部 SQL 修复。远端 CI 以本轮后续 PR 的最新提交检查为准；上一轮已通过的 CI 不作为本轮结果。

## 保留边界

- 任意普通 SQL 执行过后，会话不自动重连；传输失败必须明确重新打开，避免丢失事务、锁、临时对象或未知会话设置。
- 仍为全量内存结果，不引入流式协议；刷新跨来源使用新会话，只保证原连接/数据库/模式，不复制旧会话临时对象。
- SQL 实际来源跟踪覆盖既有识别的 `USE` / `SET search_path` 形式；不推断任意函数、`SET LOCAL` 或其他未识别命令内部的上下文变化。
- 编辑器跨窗口宿主协议、完整配置版本迁移、元数据缓存统一与 M5 注册入口不在本批范围。
- 独立操作会话不继承查询编辑器中手动开启的事务、锁或临时对象；查询结果提交仍是完整独立事务。来源匹配的结果刷新继续复用原编辑器会话。
- 显式重新打开只建立新会话，不恢复旧事务、锁、临时对象，也不重放结果未知的操作；用户应先核对服务器上的实际结果。

## 初版性能复验（2026-10-21）

三项 release 程序均在初版及 CI 修复后的代码上复验完成：10 万行 × 20 列网格逐行/跳跃均值 3.15/4.46ms，导航树约 0.54/0.73ms，5 万行 SQL 编辑器滚动/装饰/输入各场景均值约 1.59～1.81ms，首次解析约 347ms；各场景均 0/300 帧超过 8.3ms。这是本机测量，不代表任意硬件保证。
