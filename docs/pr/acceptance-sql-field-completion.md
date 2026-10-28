# SQL 输入处自动补全验收

日期：2026-10-28。分支 `feat/sql-field-completion`。规格先提交，补全排序再提交，界面接线与本记录分开提交。

## 范围

查询编辑器已有的补全（关键字、内置函数、表、字段、代码段，前缀 / 包含 / 跨字符三档，菜单里命中字符涂蓝）接到其余在写 SQL 的地方。排序和命中区间在 `navidog-sql`，各界面用自己的文案翻译说明。补全浮层仍没有图标。

- 表查看器文本模式的 WHERE 和 ORDER BY 改为与小输入框同高的单行 SQL 编辑器，文字不溢出、不出滚动条。当前表的字段始终在作用域里；已载入的表和字段、内置函数，以及 `AND`/`OR` 等条件关键字也给。不给 `SELECT` 这类语句关键字，不插入代码段。菜单关着时回车套用筛选且不换行；粘贴的换行（含 `\r\n`）收成一个空格。
- 命令列与查询编辑器同一套候选，代码段按原文插入，不跳占位符。菜单打开时回车不执行语句，方向键不翻历史。
- 代码段编辑框按片段所选驱动补全关键字、内置函数和同驱动片段名。驱动为“全部”时不补全。编辑框在主窗口的遮罩上，补全菜单画在它前面。
- 表设计器的默认值、生成列表达式、ON UPDATE、检查和索引表达式、高级页字段定义改为与小输入框同高的单行 SQL 编辑器。表达式候选是草稿字段名（含未保存的改名）、内置函数和条件关键字。字段定义使用整份关键字并列出类型名。高级页的约束、索引和表选项再带上已载入的表和字段，并使用整份关键字。回车不换行，改动仍写回草稿。只收起用户刚改过的那一格：没碰过的检查表达式，以及没改过的字段定义，保留引号里的换行。类型格和已有选择菜单不改。
- 字段名使用主题的 `property` 色（浅色 `#9A4D00`，深色沿用 `#BCC4E0`）。语法捕获是 `field`，启动时改记到 `property`。确认补全后，除代码段外每个词末尾加一个空格。

## 检查记录

| 检查 | 结果 |
| --- | --- |
| `navidog-sql` completion 测试 | 通过，含当前表字段和额外类型名不必先写 `FROM`；条件关键字保留 `AND`/`OR`，不给 `SELECT`；`\r\n` 收成一个空格；`a_completed_word_gains_a_space_and_a_snippet_keeps_its_text` |
| `a_column_name_uses_the_property_color` | 通过。`SELECT id FROM t` 里的 `id` 使用 `syntax.property` |
| `an_unedited_definition_and_check_keep_their_line_breaks` | 通过。没改过的字段定义和检查表达式保留换行；改过的字段定义仍收成一行 |
| `where_text_collapses_a_pasted_line_break` | 通过。菜单关着时回车套用筛选的判定在 `a_closed_menu_applies_and_an_open_one_confirms_or_dismisses` |
| `a_single_line_sql_editor_does_not_scroll` | 通过。单行框把滚动范围夹到 0；普通代码编辑器仍能滚进末行下的空行 |
| `query_pages_follow_the_editor` | 通过。编辑框改到主窗口遮罩后，测试窗口同样画出这一层 |
| `console_enter_confirms_completion_instead_of_running` | 通过。菜单打开时方向键不翻历史，回车不执行 |
| `an_untyped_snippet_offers_no_names_and_a_typed_one_keeps_its_driver` | 通过 |
| `expression_completion_lists_draft_columns_and_definition_lists_types` | 通过。`edits_fields_previews_sql_and_restores_drafts` 仍从默认值框写回草稿 |
| `scripts/check-i18n.sh`、`scripts/check-deps.sh`、`scripts/check-dialect-branches.sh`、`scripts/check-raw-px.sh` | 通过 |
| `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo test -p navidog --test snapshot` | 通过。筛选窗格浅色 `viewer-filter.png`、深色 `viewer-filter-dark.png` 留在本地 `target/snapshots/`，不提交。WHERE / ORDER BY 单行框与占位文案在浅色和深色下都可见，关上筛选后后续查找栏截图不受影响 |

## 截图

`viewer-filter.png` 和 `viewer-filter-dark.png` 在表查看器打开筛选窗格后截取，随后关上筛选再截后面的查找栏，避免改动原有截图的布局。
