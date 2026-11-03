# navidog 插件作者指南（API v1）

插件放在 navidog 配置目录的 `plugins/<反向域名 id>/`，包含官方 `gpui-shell.json`、`navidog.json` 和 JS 入口。偏好设置 → 插件 → 重新扫描，首次启用逐项授权。扫描不运行代码，新安装默认停用；取消任一能力后对应函数不导出，作者应使用 `import * as host from "navidog"` 并检查需要的函数，避免部分授权导致静态 import 失败。

基于 gpui-shell / gpui-kit **0.8.1**。完整宿主类型见 [`api-v1.d.ts`](../../crates/navidog-plugin/api-v1.d.ts)，协议规格见 [`03-plugin-api.md`](../design/03-plugin-api.md)。Shell manifest 的未知字段拒绝；navidog 扩展写在独立 manifest。Shell `shell-version` 是最低语义版本，当前宿主固定 0.8.1。

服务 `View.init` 中启动 `cx.spawn` 事件循环，`render` 返回空元素。每个插件一个独立 QuickJS VM；Dock 面板是同一 VM 的独立入口。HostModule 只能纯数据，宿主函数不接受回调，不回调引擎。不要在事件循环中串行等待长请求阻塞 cancel，驱动操作在子任务中处理。

原始 Shell `window.open_dialog`、`window.open_sheet`、toast 依赖独立 ShellRoot，navidog 不支持。用官方 `gpui-component` 目录中的 Dialog、Sheet、Tooltip、Popover，通知使用 `host.notify(context, level, message)`。现有窗口 Base Root 提供组件浮层。

运行限额见插件高级限额：默认 30 秒、256 事件、64 在途请求、1,000 行/1 MiB 每批、100,000 行/64 MiB 每请求。参数正值，最多 1,024 在途请求。驱动优先连接/读取超时。超时、取消、无效响应会结束请求；runtime 失败需主动重载。JS 堆无公开硬上限，watchdog 和栈限制不可配置；无进程隔离。

`respond` 和 `respond_chunk` 同时带 request id 和 generation。结果编号、批号均从 0 开始，首批必须有 columns，最后一批 end=true，最终 reply 只在所有结果集结束后发送。空结果也发 columns/end。rows 使用字符串或 null；大整数、affected_rows 用十进制字符串，二进制值沿用数据库文本编码，columns 保留 type_name 和 origin。取消后不再回复；重复/乱序/过期回应会被拒绝。

所有贡献全局 ID 是 `<plugin>/<local-id>`，本地 ID 不能重复。五个命令位置为 tools.menu、main.toolbar、objects.context、grid.context、editor.context。命令上下文 token 绑定触发时窗口和面板，异步结果不改为当前焦点；关闭目标或插件重载后失败。SQL 只作用于已打开连接，独立操作会话执行一次后关闭，日志附插件来源，不参与编辑器事务。

格式化器由 Rust 执行：unix_timestamp、json_pretty、enum_map、number、template、color；只影响显示。列标题右键选择，表查看器按对象/列保存，查询结果只保存到关闭。停用后显示原值，选择保留。

JS Driver 声明 relational family、连接字段及 dialect。必需目录操作见 HTTP 示例服务源码。connect 只向该驱动传凭据，返回 session_token、version、可选十进制 session_id。方言拥有类型/关键词等字符串，随 VM generation 释放。网格写入必须声明 begin/savepoint/rollback_savepoint；缺失时生成写入拒绝。

SQL 模板声明 sql 与 parameters，编码为 identifier、literal、fragment、integer。参数只允许规格中的有限名单；字段参数使用 field.<name>。Rust 转义标识符和字面量，fragment 用唯一 SQL scanner 校验，不允许额外语句和注释。未知操作或参数加载时失败。通用表设计器使用结构化快照和 ALTER 模板，缺少操作保持原内容并拒绝修改，不进入内置数据库重建路径。预览冻结精确 SQL、草稿、baseline 和插件 generation，执行前重新读结构；失败不重放 SQL。

可安装示例：[`com.example.commands`](../../examples/plugins/com.example.commands) 覆盖五处命令、格式、显示规则与 Dock；[`com.example.http-postgres`](../../examples/plugins/com.example.http-postgres) 演示 JS HTTP 驱动。启动测试数据库后运行：

```sh
docker compose up -d --wait
cargo run -p navidog-plugin --example http_server
```

HTTP 服务仅绑定 127.0.0.1:18123，是示例/测试设施；禁止将它作为生产数据库网关。连接表单填写 127.0.0.1:15432、postgres、navidog、pagila。验证结束 `docker compose down`。debug 源文件变化先重新校验授权再清理重载；扩大授权停用等待确认，运行失败不自动重启。
