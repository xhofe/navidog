# API v1 manifest/schema

Shell manifest 由 [gpui-shell 官方 schema](https://gpui-kit.com/zh-CN/shell) 校验，navidog 不扩展该文件。`navidog.json` 根字段 api 必须为 1，其余列表默认为空，未知字段拒绝。安装示例见 examples/plugins。

| 字段 | 类型 | 校验 |
| --- | --- | --- |
| permissions | string[] | connections.read/query.execute/grid.read/editor.write；未知项拒绝 |
| commands | Command[] | id/title/menu 必须；title_zh/icon/shortcut/when 可选；menu 只能五处固定位置，when 只能 connection.open/object.selected/grid/editor |
| formats | Format[] | id/name/extensions/import/export；至少支持一个方向 |
| cell_formatters | Formatter[] | id/name/match/display；match.type/column/value 为类型和正则交集；六类规则在作者指南 |
| drivers | Driver[] | id/name/family=relational/default_port/fields/icon/dialect；fields 只能现有连接字段 |
| panels | Panel[] | id/title/entry/placement/size；entry 相对插件目录，placement left/right/bottom/center，size 有限正数 |

所有贡献 id 在插件内唯一，允许字母、数字、点、连字符、下划线。全局加插件前缀。图标必须是插件自己提供的相对路径，禁止路径越界。

Dialect 必须有 id 和 quote（双引号/反引号/方括号）。可声明 namespace、object_kinds、system_databases/system_schemas、keywords/functions/types 和完整 extra_lex。types 为 name/synonyms/args（none/length/length_and_scale）。begin/savepoint/rollback_savepoint 必须同时有或同时无，transactional_ddl=true 必须声明 begin。

Designer 为 properties（有限列属性名）、constraints、index_methods、options。options 为字段名到 Field 的对象。Field 包含 label、control、default、encoding：control 是 text/sql/checkbox/number，select 写成 `{"select":["a","b"]}`。encoding 是 identifier/literal/fragment/integer；模板中的 field 参数必须与字段声明一致。

Objects 按 DefinitionKind key 索引，值为 fields/create/update。未声明更新操作拒绝编辑保存。create/update 是 SqlTemplate（sql、parameters）。表 templates 操作名见 `DialectDescriptor::validate`，未知操作拒绝。模板参数仅允许 table/name/old_name/column/definition/body/fields/constraint/index/options/comment/position/limit/offset/schema/database/value，以及已声明的 field.<name>。

StructuredTableSnapshot 是 snapshot（TableDesign/ddl/triggers）+ revision/unknown/attachments。TableDesign 保存 columns/constraints/indexes/options/option_fields/comment/owner/unlogged/triggers/partition/partition_children；列有稳定 id、original、name、definition、comment。ObjectSnapshot 保存 name/body/fields/ddl/revision/unknown。revision 必须随数据库结构变化稳定改变；未知片段和附属对象必须完整保留。

请求回应携带相同 request_id/generation。StreamColumn 是 name/type_name/origin；WireCell 为 string/null 或 `{"base64":"…"}`。大整数和 affected_rows 为十进制字符串。结果 Chunk 首批 columns，后续可省略，每个结果独立 result_set/sequence/end；最终回应只能在最后结果结束后发送。导出输入使用 export.chunk 并受同样批量限额约束。
