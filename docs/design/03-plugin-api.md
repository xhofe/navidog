# 插件 API

navidog 的插件是 [gpui-shell](https://gpui-kit.com/zh-CN/shell) 应用：一个目录，里面有 JavaScript 源码、`gpui-shell.json` 和 navidog 自己的 `navidog.json`。本文定义注册表、插件目录结构、授权、HostModule `navidog` 和 JS 驱动协议。M5 实现按本文第 11～16 节的 API v1 契约执行；注册表在 M1 就建立（决策见 [00-overview.md](00-overview.md#3-决策记录)）。

## 1. 设计约束（来自 gpui-shell）

| 约束 | 后果 |
| --- | --- |
| 脚本默认没有任何能力，文件、网络、剪贴板都要由宿主授予；授予内容在 View 加载时冻结。 | 授权必须在加载插件之前完成，修改授权后要重新加载插件。 |
| `gpui-shell.json` 中出现未知字段时，整个 manifest 判定为无效。 | navidog 的扩展声明不能写进 `gpui-shell.json`，单独放在 `navidog.json`。 |
| HostModule 边界上只能传纯数据（null、布尔、数字、字符串、数组、对象），宿主拿不到脚本函数的句柄。 | 插件不能把回调函数注册给宿主。 |
| host function 禁止回调进脚本引擎。 | 宿主要通知插件（命令被触发、驱动请求等），只能由插件拉取：`await next_event()`。 |
| 每个插件有独立的 `Policy`，包含自己的 capabilities 和 HostModule 注册表（`Policy::with_host_module`）。 | navidog 自有的能力（读取连接列表、执行 SQL 等）按插件分别注册：没有授权的函数不会出现在该插件的 `navidog` 模块里。 |
| 原始 Shell 浮层依赖独立 `ShellRoot`；官方组件浮层使用窗口 Base Root。 | 保留 navidog 的窗口 Root，使用官方 `gpui-component` 的 Dialog、Sheet、Tooltip、Popover；原始 Shell 浮层不支持，通知走 `navidog.notify`。 |

## 2. 注册表（`navidog-app-state`，M1）

内置功能和插件功能都通过同一套注册表注册。每个条目都带有 `source` 字段，取值为 `Builtin` 或 `Plugin(PluginId)`；停用插件时，按 source 批量移除它注册的条目。

```rust
pub enum Source {
    Builtin,
    Plugin(PluginId),
}

pub struct Registry<K, V> { /* 按 K 索引，保持注册顺序 */ }

impl<K: Eq + Hash + Clone, V> Registry<K, V> {
    pub fn register(&mut self, key: K, source: Source, value: V) -> Result<(), RegistryError>;
    pub fn unregister_source(&mut self, source: &Source);
    pub fn get(&self, key: &K) -> Option<&V>;
    pub fn iter(&self) -> impl Iterator<Item = (&K, &Source, &V)>;
}
```

| 注册表 | 键 | 值 | 内置条目示例 |
| --- | --- | --- | --- |
| `CommandRegistry` | `CommandId`（如 `connection.new`） | 标题（i18n 键）、图标、默认快捷键、可用条件、菜单与工具栏位置 | 新建连接、新建查询、刷新 |
| `DriverRegistry` | `DriverId`（如 `mysql`） | 驱动元数据（名称、类别、图标、默认端口、表单字段集合）和 `Arc<dyn Driver>` | mysql、mariadb、postgres、sqlite |
| `FormatRegistry` | `FormatId`（如 `csv`） | 导入导出格式元数据和实现 | csv、json、sql |
| `CellFormatterRegistry` | `FormatterId` | 声明式格式化规则 | 时间戳显示为日期 |
| `PanelRegistry` | `PanelId` | 面板元数据（标题、默认停靠位置、尺寸）和构造函数 | 导航窗格、信息窗格 |

注册表是 `navidog-app-state` 中的 GPUI `Global`；修改时调用 `cx.refresh_windows()`，菜单栏通过 `cx.set_menus` 重建。

## 3. 插件目录

```
~/Library/Application Support/navidog/plugins/
  com.example.clickhouse/
    gpui-shell.json      # gpui-shell 的 manifest：id、name、entry、capabilities
    navidog.json         # navidog 的扩展声明
    main.js              # 服务入口：init 中启动事件循环
    panels/explorer.js   # 每个面板一个入口，default 导出 View
```

### 3.1 `gpui-shell.json`

格式完全由 gpui-shell 定义：`id`（反向域名）、`name`、`version`、`shell-version`、`entry`、`dependencies`、`capabilities`。navidog 不添加任何字段。

### 3.2 `navidog.json`

```json
{
  "api": 1,
  "permissions": ["connections.read", "query.execute"],
  "commands": [
    { "id": "copy-as-markdown", "title": "Copy as Markdown", "title_zh": "复制为 Markdown",
      "menu": "grid.context", "shortcut": null }
  ],
  "formats": [
    { "id": "parquet", "name": "Parquet", "extensions": ["parquet"], "import": false, "export": true }
  ],
  "cell_formatters": [
    { "id": "unix-ts", "match": { "type": ["int", "bigint"], "column": "(?i)_at$" },
      "display": { "kind": "unix_timestamp", "unit": "s", "format": "%Y-%m-%d %H:%M:%S" } }
  ],
  "drivers": [
    { "id": "clickhouse", "name": "ClickHouse", "family": "relational", "icon": "icons/ch.svg",
      "default_port": 8123, "fields": ["host", "port", "user", "password", "database"] }
  ],
  "panels": [
    { "id": "explorer", "title": "Explorer", "entry": "panels/explorer.js",
      "placement": "right", "size": 280 }
  ]
}
```

- `api`：插件 API 的主版本。宿主不支持该版本时拒绝加载，并说明原因。
- `permissions`：navidog 自有能力的申请（见第 4 节）。文件、网络等系统能力仍然写在 `gpui-shell.json` 的 `capabilities` 中。
- 所有 `id` 都在插件内唯一；注册到全局时加上插件前缀，例如 `com.example.clickhouse/copy-as-markdown`。

## 4. 授权

首次启用插件时，弹出授权对话框，列出两类申请：

1. **系统能力**（`gpui-shell.json` 的 `capabilities`）：文件读写目录、网络主机、HTTP 规则、剪贴板、存储。原样展示，`${pluginDir}`、`${dataDir}` 替换为实际路径。
2. **navidog 能力**（`navidog.json` 的 `permissions`）：

| 能力 | 允许插件做什么 |
| --- | --- |
| `connections.read` | 读取连接列表的元数据（名称、类型、主机、颜色），不包含密码 |
| `query.execute` | 在用户已打开的连接上执行 SQL；每次执行写入历史日志并标注插件 ID |
| `grid.read` | 读取当前结果网格的选中数据 |
| `editor.write` | 向当前查询编辑器插入文本 |

用户可以逐项取消勾选。授权结果保存在 `plugins.json`，可以在偏好设置的插件页撤销。授权变化后插件会重新加载。

JS 驱动需要的网络能力也来自 `gpui-shell.json`，例如 `network.hosts` 或 `network.http`。驱动只能拿到使用该驱动的连接的凭据，并且只在 `driver.connect` 请求中拿到。

## 5. HostModule `navidog`

宿主按插件分别构造 `HostModule::new("navidog")`，只注册已授权的函数，并通过 `declarations` 提供类型，让 gpui-shell 生成的 `gpui-kit.d.ts` 中包含 `declare module "navidog"`。

```ts
// 始终可用
export function api_version(): number;
export function locale(): string;                       // "zh-CN" | "en"
export function next_event(): Promise<NavidogEvent>;   // 拉取下一个宿主事件
export function respond(request_id: string, result: Reply): void;
export function respond_chunk(request_id: string, chunk: ResultChunk): void;
export function notify(level: "info" | "success" | "warning" | "error", message: string): void;

// connections.read
export function list_connections(): ConnectionInfo[];

// query.execute
export function execute(connection_id: string, database: string | null, sql: string): Promise<ResultSet[]>;

// grid.read
export function grid_selection(): ResultSet | null;

// editor.write
export function insert_text(text: string): void;
```

### 5.1 事件循环

插件入口的 `init` 中启动一个循环，在循环里拉取事件。宿主为每个插件维护一个事件队列；插件停用时，未完成的 `next_event` 以错误结束，循环随之退出。

```js
import { next_event, respond } from "navidog";

export default class Service extends View {
  init(_props, cx) {
    cx.spawn(async () => {
      for (;;) {
        const event = await next_event();
        switch (event.kind) {
          case "command":
            /* event.command_id, event.context */
            break;
          case "driver.request":
            respond(event.request_id, await handleDriver(event));
            break;
          case "export":
            respond(event.request_id, await exportRows(event));
            break;
        }
      }
    });
  }
  render() { return div(); }
}
```

| `event.kind` | 触发时机 | 需要回复 |
| --- | --- | --- |
| `command` | 用户触发了插件注册的命令 | 否 |
| `driver.request` | JS 驱动收到宿主请求（见第 6 节） | 是 |
| `export` | 用户选择插件格式导出，事件中带有按批次提供的行数据 | 是 |
| `import` | 用户选择插件格式导入，插件以 `respond_chunk` 回传行数据 | 是 |
| `cancel` | 宿主取消一个未完成的请求 | 否 |

宿主为每个请求设置超时（默认 30 秒，驱动请求使用连接设置中的超时），超时后向调用方报错，并给插件发送 `cancel` 事件。

## 6. JS 驱动协议

宿主侧的 `JsDriver` 实现 `navidog-db` 的 `Driver` 和 `Relational` trait，把每次调用转换成 `driver.request` 事件。

| `op` | 参数 | 回复 |
| --- | --- | --- |
| `connect` | `connection`：表单字段值，包括凭据 | `{ session_id }` |
| `disconnect` | `session_id` | `{}` |
| `ping` | `session_id` | `{}` |
| `server_version` | `session_id` | `{ version }` |
| `list_databases` | `session_id` | `{ databases: string[] }` |
| `list_schemas` | `session_id, database` | `{ schemas: string[] }` |
| `list_objects` | `session_id, database, schema, kind` | `{ objects: ObjectInfo[] }` |
| `execute` | `session_id, sql` | 先用若干个 `respond_chunk` 回传结果，最后用 `respond` 收尾 |
| `cancel` | `session_id` | `{}` |

结果数据的格式：

```ts
type ResultChunk = {
  columns?: { name: string; type: string; nullable?: boolean }[]; // 仅第一个 chunk 携带
  rows: (null | boolean | number | string)[][];                    // 二进制以 base64 字符串传输
};
type Reply = { ok: true; data?: unknown } | { ok: false; error: string; code?: string };
```

数值超出 JS 安全整数范围时，必须以字符串传输。宿主按列类型解析成网格值。

## 7. 单元格格式化器

渲染期间不能调用 JS，所以格式化器是声明式的：`navidog.json` 中的 `cell_formatters` 由 Rust 解析并执行。

- `match`：按列类型列表、列名正则、值正则匹配，条件之间是“且”的关系。
- `display.kind`：取值为 `unix_timestamp`、`json_pretty`、`enum_map`（值到标签的映射）、`number`（小数位、千分位）、`template`（`{value}` 占位符）、`color`（按值着色，颜色只能用 token 名）。

如果需要更复杂的转换，插件可以注册一个命令，由命令生成派生列，而不是使用格式化器。

## 8. 插件面板

- `navidog.json` 中的每个面板，宿主用公开 `runtime.load_application(plugin_dir, entry)` 和 `mount_application` 创建 `ScriptView`，放进实现了 `gpui-component` `Panel` 的 `PluginPanel` 中，然后按 `placement`（left、right、bottom、center）和 `size` 加入 `DockArea`。
- 面板标识为 `plugin:<插件 id>/<面板 id>`，用于 Dock 布局的持久化。插件未加载时，布局中对应的位置显示为占位面板。
- 宿主状态（连接列表、主题、语言）发生变化时，宿主调用 `view.refresh(cx)`，让脚本重新执行 `render`。

## 9. 生命周期

1. **发现**：启动时扫描插件目录，读取两份 manifest，不执行任何代码。
2. **校验**：检查 `api` 版本、id 格式、`shell-version`，以及 `navidog.json` 的 schema。
3. **授权**：首次启用时弹出授权对话框，之后读取 `plugins.json`。
4. **加载**：为插件构造 `Policy`（capabilities 加 `navidog` HostModule），加载服务入口，注册 manifest 中声明的命令、格式、格式化器、驱动和面板。
5. **停用或卸载**：从所有注册表移除该插件的条目（`unregister_source`），撤销贡献并把已打开面板原位替换为占位，结束事件队列、取消请求和断开插件会话；保留连接配置与草稿。
6. **开发模式**：debug 构建中监听插件目录，变化后重新校验、检查授权、清理并重新加载；不得直接绕过授权替换运行代码。

## 10. 待 M5 验证的风险

- 插件面板内包一层 `ShellRoot` 能否与宿主的 `Root` 共存，以及对话框是否只覆盖在面板范围内。
- 服务入口的 View 需要一个活的 `Window` 才能实例化：确认在主窗口中实例化但不渲染时，它的事件循环是否正常运行。
- QuickJS 运行时会让二进制增加约 13.5 MiB：确认 release 构建体积可以接受。


## 11. M5 API v1 与发布源码契约

第 11～16 节是 M5 的完整契约；与前述概念示例冲突时以本节为准。框架依据 `gpui-shell` / `gpui-kit` **0.8.1 发布源码**，不是最新开发分支。`gpui-kit` 升至 0.8.1；直接依赖 `gpui-shell` 0.8.1，关闭默认特性，只启用 `quickjs`、`components`、`gpui-fast`，不覆盖 `gpui-pre-*`。

每个启用插件一个 `ShellRuntime::new_isolated_with_components`。服务入口挂载后保留实体但不渲染；每个面板入口是该 VM 中的独立应用，使用相同授权和宿主服务。只用官方 `PluginManifest::read` / `parse` 校验 Shell manifest，发现阶段不读执行源码、不解析依赖、不写类型声明。Shell `shell-version` 是最低版本字符串，官方解析器检查兼容性；未知字段拒绝。面板入口必须位于插件目录内。

加载与挂载期间用同步 guard 安装 `Policy`，包括 `with_application`、`with_capabilities`、`with_data_dir`、按授权注册的 HostModule。guard 禁止重入、不 await、不推进渲染帧，正常返回或失败均恢复空 Policy。每个 View 冻结自己的 grant；代码中不得在宿主函数里回调 VM。

JS 堆没有公开硬上限，watchdog 与栈限制不可配置。M5 用独立 VM 和宿主请求限额，不增加进程隔离。运行时终止后必须由用户主动重新加载。原始 Shell `window.open_dialog` / `open_sheet` / toast 不支持；作者用官方组件目录的 Dialog、Sheet、Tooltip、Popover，通知用 `navidog.notify`，错误提示给出替代接口。

## 12. 授权、限额和管理界面（F-PLUG-001、002、008）

`plugins.json` 是版本化配置，保存插件 ID、启用状态、已确认能力申请指纹、实际逐项授权、运行限额。代码更新沿用授权；任何新增或扩大能力申请先停用，原授权作为重新确认的预选项。能力减少可继续运行但不能保留已删除 grant。未见过的插件默认停用。文件与网络 grant 只从已校验的申请中逐项选取，不接受插件提交的任意授权对象。持久化成功后再修改运行状态。

| 限额 | 默认 | 校验 |
| --- | ---: | --- |
| 通用请求超时 | 30 秒 | 有限正数 |
| 事件队列 | 256 项 | 正整数 |
| 在途请求 | 64 项 | 1～1,024；0.8.1 官方 runtime 任务上限为 1,024 |
| 单批数据 | 1,000 行 / 1 MiB | 正整数 |
| 单请求累计结果 | 100,000 行 / 64 MiB | 正整数，不能小于单批限额 |

connect 优先使用连接超时；执行和目录请求优先使用读取超时，缺省用插件超时。超时、取消、超限失败都发送 cancel，之后拒绝任何结果。停止、撤权、重新加载、运行失败：先推进 generation，再撤销所有 source 贡献，失败全部在途请求、结束事件拉取、断开会话。保存的连接与未保存草稿不删除。Dock 面板原位显示“插件已停用”或错误原因，重新启用后恢复同一面板身份。

偏好设置新增“插件”页，沿用现有窗口尺寸、页边距、控件和行高 token。页内左侧列表、右侧详情使用 `min_w_0`；名称、版本、路径单行截断并悬停全文。列表呈现名称、版本和状态；详情展示失败原因与启用/停用、授权…、重新加载。授权对话框独立复选每项系统/navidog 能力，支持部分授权；Escape 关闭且恢复焦点。高级区展开后编辑六项限额，错误在对应项附近展示，“保存并重新加载”提交。无插件时说明安装目录并提供重新扫描。debug 监听同一生命周期，不自动重试运行时终止。

## 13. 贡献、上下文和宿主接口（F-PLUG-003、004、007）

统一注册表保留声明顺序，每项 `Source::Plugin(id)`；键是 `<plugin>/<local-id>`。补齐格式、格式化器和面板注册表，驱动注册表仍在 session。注册失败不能留下部分贡献。

命令位置仅允许 `tools.menu`、`main.toolbar`、`objects.context`、`grid.context`、`editor.context`。一个携带命令 ID 的 `PluginCommand` Action 供所有位置和快捷键共用；标题、图标、快捷键、可用条件共享描述。可用条件只允许已声明的宿主状态，如打开连接、选中对象、网格、编辑器，未知位置或条件拒绝。动态图标路径由拥有数据的接口返回，不泄漏内存。

上下文 token 不透明，绑定触发时窗口、面板、target generation、插件 generation。异步请求不得改为“当前焦点”；关闭目标、移动失效目标、插件重载后返回过期上下文错误。跨边界仅纯数据。API v1：

```ts
api_version(): number;
locale(): string;
next_event(): Promise<Event>;
respond(request_id: string, generation: number, reply: Reply): void;
respond_chunk(request_id: string, generation: number, chunk: ResultChunk): void;
notify(context: string, level: "info" | "success" | "warning" | "error", message: string): void;
list_connections(): ConnectionInfo[]; // connections.read；永不含密码
execute(context: string, connection_id: string, database: string | null, sql: string): Promise<ResultSet[]>;
grid_selection(context: string): ResultSet | null;
insert_text(context: string, text: string): void;
import_rows(context: string, format_id: string, input: string): Promise<ResultSet[]>;
export_rows(context: string, format_id: string, rows: ResultSet): Promise<string>;
```

缺权函数不导出，显式失败。SQL 仅针对已打开连接，经 `Sessions::run_operation_rel` 的独立操作会话执行一次后关闭，不进入查询编辑器事务。历史记录可选 `plugin_source`，旧记录兼容，每次 SQL 只记录一次。

请求拥有唯一 ID、plugin generation、操作、不可变上下文及可选会话 token。chunk 包含 result_set（从 0）、sequence（从 0）、columns（首批）、rows、end、affected_rows、notices；最终 reply 收尾。空结果仍显式传 columns/end。下一结果集只能在上一结果 end 后开始；拒绝重复、过期、乱序、重复列、列数不符、未结束结果的最终响应和无请求响应。数值超安全整数用字符串，二进制用显式 base64 类型，保留 origin 和 notices，affected rows 用十进制字符串。取消后不可回传结果。格式扩展通过相同请求/分批协议调用，M6 再提供完整向导。

## 14. 格式化器（F-PLUG-005）

Rust 执行六类声明式显示规则：时间戳（秒/毫秒）、JSON、枚举映射、数字（小数与分组）、模板（只允许 `{value}`）、token 颜色。match 按类型、列名正则、值正则取交集；无匹配不改显示。拒绝无效正则、未知颜色 token、无效时间格式和小数位。用户在列菜单选择，候选按列匹配过滤；不修改原数据、复制、写入与导出值。表查看器选择按连接/对象/列保存；查询结果只活到选项卡关闭。插件停用显示原值，重新可用时恢复原选择。

## 15. JS 驱动与原生设计器（F-PLUG-006）

`JsDriver` / `JsSession` 实现 Driver、Session、Relational 所有必需目录方法，包含 databases、schemas、objects、DDL、columns、children、foreign keys、outline。connect 返回 opaque session token、缓存版本、可选数据库会话 ID；凭据只进入该驱动的 connect 请求。取消可跨线程发出事件，卸载失败所有会话请求。

声明式 dialect 包含词法、引用、字面量、命名空间、对象类别、系统对象、关键词/函数、分页、事务与 SQL 操作模板。所有动态列表拥有数据或借用 self；不使用内存泄漏。新能力门槛同时在 UI、Action、网格提交、schema 执行检查。网格写入要求显式 transaction 与 savepoint 契约；缺少时禁用生成写入，普通 SQL 保留。解释计划走声明模板与通用结果显示。

表草稿/快照纯数据放 model，schema re-export。Relational 可选结构化 table/object snapshot 包含完整结构、未知片段、附属对象和稳定 revision。JS 读取，Rust 规划。SQL crate 持有 designer descriptor，schema 通用模板规划器，designer 复用原生表单。类型/属性/约束/索引/选项/注释/触发器/分区按声明开放；未声明部分保留原文并禁止修改。

通用迁移仅用 ALTER 与显式模板，覆盖 create、列增删改、约束、索引及声明的附属操作。未声明移动模板不允许重排，不使用 PostgreSQL/SQLite 的重建路径。对象类别声明字段、控件、默认值、编码与 create/update 模板；缺少类别或更新能力则禁用。

模板是有限宿主参数：identifier / literal 由 Rust 转义；fragment 用 navidog-sql 唯一扫描器校验；未知参数加载时拒绝。预览 plan 冻结 baseline revision、草稿版本、插件 generation 和精确 SQL。执行前再次读结构并比 revision，草稿/插件变化使计划失效；执行预览语句一次。原子 DDL 用声明事务/锁且失败回滚；非事务 DDL 部分失败展示已完成步骤和可能变化，保留草稿，重新加载后再规划；任意 SQL 不自动重放。

## 16. 交付与验收

交付 API v1 `.d.ts`、manifest/schema、安装/调试/兼容说明及五类可安装示例。HTTP PostgreSQL 示例连接本地测试服务后访问真实数据库；MySQL 夹具覆盖非事务 DDL 部分失败和重新加载恢复。服务仅用于示例与测试。

- [x] 官方 manifest、加载、独立 VM 与同步 Policy guard；服务不渲染仍收事件
- [x] 默认停用、部分授权、授权扩大停用、限额保存与重载、debug 同一流程
- [ ] 五处命令与快捷键、跨窗口上下文、关闭目标/重载后失效
- [x] 完整结果协议、空/多结果、来源/二进制/大整数、取消/超时/限额/过期响应
- [x] 独立 SQL 会话及插件日志来源；撤销全部贡献/请求/会话，保留配置和草稿
- [x] 格式 import/export 接口及端到端测试（向导留 M6）
- [x] 六类格式化、列选择/持久化/停用回退恢复
- [ ] 全部驱动目录、补全、取消、网格写入及能力门槛
- [ ] 通用原生表/视图设计、模板校验、预览版本、结构冲突、事务回滚/部分失败保护
- [x] 官方组件浮层与 Dock 占位恢复、管理页 headless/浅深色截图
- [ ] 示例、文档、类型声明；progress §5 全部检查及 release 体积/前台性能

验收证据见 [M5 验收记录](../pr/acceptance-m5.md)。未经运行的检查不得勾选或记为完成。M6 向导、M2/M3 目测与既有 M4 遗留不在 M5 范围内。


### M5 最终协议细化

服务入口 `init` 的首个 `cx.spawn` 必须是长期事件循环。宿主生成保留的 `.navidog-service-v1.js`，通过官方 Task.is_done 与 timer 监督事件循环；结束/错误/watchdog 后 `runtime_failed` 只终止运输层，Rust 在后续更新中撤销 source 与延迟释放 VM。服务仍不渲染。该文件禁止插件覆盖或使用符号链接。

导出输入协议先发送 export（columns、context、stream），随后 export.chunk（同 request_id/generation、sequence、rows、end）。宿主按单批行/字节拆分并使用队列背压，超时从请求提交开始。插件在 end 后回应文本；取消后停止收集。API 的 columns 在后续结果批次可省略。

声明式方言新增 designer descriptor（properties、constraints、index_methods、options）和 objects 描述（字段 label/control/default/encoding，create/update 模板）。控件支持 text、SQL、checkbox、number、select；字段参数与模板编码必须一致。表操作按模板开放 create/add/drop/alter/move column、constraint/index、table options/comment、trigger、partition/child；缺失操作禁止提交，原内容保留。完整 Lex 可声明，仍仅由唯一 scanner 读取。
