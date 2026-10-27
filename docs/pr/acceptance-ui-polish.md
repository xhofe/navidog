# 界面对齐与默认值验收

日期：2026-10-28。分支 `fix/ui-polish`。规格先提交，代码按主题分提交。

## 行为

- 表设计器字段编辑使用与数据网格相同的小号输入框，字号拉回正文 13pt。
- 关闭其他、关闭右侧、关闭全部时，每个未保存选项卡按顺序询问。保存或不保存后继续下一个；取消清空队列。
- 测试连接的结果在按钮行上方：测试中可取消，成功写出服务器版本，失败写出驱动原文。
- 非 Navicat 主题的选中色与组件的 `list_active` 相同。导航树里点字段时只高亮这一行，父表不再另画一层。
- 对象列表默认详细信息。列按目录属性固定，空注释仍占一列；数据长度按 1024 显示为 `16 KB`。
- 偏好设置「显示系统对象」缺省打开，查看菜单「显示隐藏项目」读写同一项。SQLite 的 `sqlite_` 对象在关掉时滤掉。
- 文件菜单的新建连接在偏好设置或对话框挡住主窗口时仍可点。
- 新的 `settings.json` 缺省主题是 Default，行底纹是无。已经写上的值不改。
- 关于页和安装包描述改为 “An open-source alternative to Navicat, written in Rust”。
- 编辑、保存、复制和修改主密码在存储锁定时弹出同一个解锁对话框，不再要求先打开某个已保存密码的连接。
- 数据库图标改为 Tabler（MySQL）和 Devicon（MariaDB、PostgreSQL、SQLite），总览里的来源说明已改，不提取 Navicat 图标。

## 检查记录

| 检查 | 结果 |
| --- | --- |
| `cargo test -p navidog-util --lib byte_sizes` | 通过 |
| `cargo test -p navidog-app-state --lib settings` | 6 项通过，含缺省主题、行底纹和系统对象 |
| `cargo test -p navidog-ui --lib switching_family` | 通过；Default 主题的选中色等于 `list_active` |
| `cargo test -p navidog-navigator --lib profile_joins` | 通过 |
| `cargo test -p navidog-connection --lib a_locked_save_asks_for_the_master_password` | 通过 |
| `cargo test -p navidog-connection --lib failed_password_save_still_uses_credentials_once` | 通过 |
| `cargo test -p navidog --test close_prompt` | 通过；同一进程里覆盖单表提交，以及两个脏查询的关闭全部、放弃后继续、取消后停止 |
| `cargo clippy`（改动的 crate，`--all-targets -- -D warnings`） | 通过 |
| `scripts/check-i18n.sh`、`scripts/check-dialect-branches.sh`、`scripts/check-raw-px.sh` | 通过 |
| `cargo fmt`（改动的 crate） | 已格式化 |

全工作区 `cargo test --workspace`、截图和实库测试这次没有重跑。对象列表和测试连接底栏的并排目测仍按规格清单留着。
