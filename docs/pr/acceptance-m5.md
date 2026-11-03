# M5 插件体系本机验收（2026-11-03）

分支：`m5-plugin-system`。基线：`58cb3cb`（main）。规格先独立提交 `72af47e`；运行时、依赖和界面分段提交。当前完成可运行插件宿主及主要扩展链路，完整 M5 验收仍有下列遗留，不能把 F-PLUG 全部记成完成。

## 已实现和验证

- 官方 Shell 0.8.1 manifest 解析、独立 VM、同步 Policy guard、不渲染服务事件循环。`runtime` headless 测试验证服务零 render 收事件及冻结部分剪贴板授权。
- 默认停用、逐项部分授权、授权指纹/扩大停用、限额配置、重新加载、source 清理。`management` 真实点击测试覆盖部分授权、重载、扩大申请停用、原生组件 Dialog。
- 五处命令、统一 Action、动态快捷键、声明顺序、窗口/目标 token；`app-state` 跨窗口测试覆盖原目标、关闭和重载失效。弹出窗口组合已接同一 Action。
- 请求 ID/generation、有界队列/在途/结果、顺序检查、空/多结果、取消和超时；protocol 单元测试及 HTTP 多结果实库测试验证。大整数沿用字符串，origin/notices 保留。
- Rust 六类显示规则、按列选择、复制与原值不变、对象列配置持久化、停用回退；model 六规则单元测试与 grid formatter UI 测试通过。
- JS Driver/Session 和所有必需目录接口、拥有数据的图标/类型/关键词、事务与 savepoint 门槛、结构化表/视图快照、通用 ALTER 规划。HTTP PostgreSQL 实库测试通过连接、目录、补全、多结果、表 ALTER 预览保存、视图创建/修改。
- 表/对象预览执行精确 SQL，检查草稿/插件代际与数据库 baseline；非事务失败单元测试验证完成步骤和拒绝重放。独立 SQL 操作会话绑定插件取消，日志来源附在驱动已有记录上。
- 作者指南、API v1 类型声明、命令/格式/显示/Dock 示例和 PostgreSQL HTTP 示例服务已提供。服务仅作为本地示例/测试设施。

## 检查结果

| 检查 | 结果 |
| --- | --- |
| fmt / workspace all-targets clippy | 通过 |
| raw-px / i18n / dialect branches / deps | 通过 |
| cargo audit / deny all-features | 通过，未增加漏洞忽略项；JIT Cranelift 的 Apache-2.0 WITH LLVM-exception 纳入允许清单 |
| workspace | 410 项、75 套件通过；最后协议/显示生命周期改动的针对性回归通过 |
| DB/schema/query 实库 | 101 项通过，包含 SSH 隧道、断连/取消、DDL 回归 |
| HTTP PostgreSQL | 端到端通过，约 3～4 秒 |
| 查询解释三数据库 | 通过 |
| 截图 | 76 张；插件空页/详情/授权浅深色已逐张查看，现有主窗口/连接/网格/查询/设计器回归已生成 |
| release 网格 | 100,000×20 加载 161.21ms；逐行 avg 3.17ms/p95 3.87ms，跳跃 avg 4.29ms/p95 5.08ms，0/600 帧超过 8.3ms |
| release 导航树 | avg 0.54/0.72ms，0/600 帧超过 8.3ms |
| release SQL 编辑器 | 50,000 行解析 331.33ms，滚动/输入 avg 约 1.75ms，所有采样 0 帧超过 8.3ms |
| release 应用 | 49,373,488 bytes；同 main 基线 58cb3cb 独立重编译 30,250,960 bytes；增量 19,122,528 bytes（18.24 MiB） |

## 必须继续完成的 M5 验收与限制

1. PostgreSQL HTTP 并发慢查询取消已通过 `runtime_live` 真实 GPUI 执行器验收（504ms）。headless TestDispatcher 串行阻塞 fetch 的实验不用于验收，保留可运行的真实窗口检查。
2. MySQL HTTP 非事务 DDL 部分失败、结构重载与拒绝旧计划重放已通过 `mysql_http_live`，约 3 秒。
3. 列格式化实际显示、停用回退、重新注册恢复与复制原值已通过 grid formatter UI 检查；两个查询编辑器窗口中延迟插入原目标、关闭后 token 失效已通过 plugin_context。显示浅深色专门截图待补。
4. 通用原生设计器已提供 designer descriptor、对象字段的 checkbox/select/text/number 编码契约和表属性 Action 门槛；列移动、表选项/注释、触发器/分区附属操作按模板开放。完整属性/约束/索引及其他对象类别的 UI 验收仍需补齐，缺失操作保留原内容并拒绝修改。
5. 格式接口和导出输入多批事件已补齐，按请求 ID/generation 顺序传入并带背压、行/字节限额，纯协议验收通过；完整向导仍按计划留 M6。
6. Dock 保存布局恢复缺失/停用插件身份已通过 dock UI 测试；已打开面板表有弱实体去重。跨窗口停用/恢复的专门 UI 回归待补。
7. 服务监督入口通过官方 Task.is_done 和 timer 检测任务结束；无限循环触发官方 watchdog 后的运输层停用已通过 failure 检查（约 5 秒）。面板 build_error 同步撤销；已打开草稿保留。
8. JS 堆无公开硬上限，官方 watchdog/栈限制不可配置；M5 不提供进程隔离。
9. gpui-shell 0.8.1 标准库要求 RSA rc.19，当前 russh 0.64.1 固定 rc.18；保留最新版协议实现的本地兼容补丁只改 RSA/PKCS pins 及 PKCS#1 borrowed 类型名。在线 audit 和 SSH live 通过；上游兼容后应移除 vendor patch。
10. 76 张现有/新增截图已逐张查看（浅深色插件页与回归联系表），最终全量与截图测试通过。release 真实执行器 HTTP 取消 541ms。同 main commit 无插件 release 已独立重编译；二十列全启用插件显示规则 avg 3.31/4.46ms，0/600 帧超过 8.3ms，比无规则约增 0.14/0.17ms。

M6 向导、M2/M3 历史目测和既有 M4 遗留不在本次范围。测试截图保留 target/snapshots，不提交 git。
