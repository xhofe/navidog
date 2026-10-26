# 整体审查修复验收（2026-10-24）

- 分支：`review/overall-fixes`
- 审查与进度： [docs/cr/2026-10-24-overall-review.md](../cr/2026-10-24-overall-review.md)
- 范围：11 个 crate + `xtask` + CI/scripts 的跨 crate 审查；48 项已修，9 项进行中，0 项暂缓。

## 验证

在本机 macOS（Rust 1.99.0）执行：

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo test --workspace --no-fail-fast -j 1` | 全部通过 |
| `scripts/check-raw-px.sh` | 通过 |
| `scripts/check-i18n.sh` | 通过 |
| `cargo check -p xtask` + `cargo run -p xtask -- manifest ...` | 通过（手工校验 `latest.json`） |

说明：本机同时运行多个 GPUI headless 测试二进制时出现过挂起，因此全量测试用 `-j 1`；单独按 crate 并行也通过。实库测试需要 `NAVIDOG_LIVE=1` 和 Docker，本次未跑；截图测试需要 Metal，本次未跑。

## 主要修复

- **正确性**：UUID/RNG、URI `+` 解码、ISO-8601/百分号、`$tag$` 参数、profile 路径穿越、Windows 文件名字符、表单静默丢配置。
- **数据安全**：统一的原子 JSON 写入与 `.bak` 备份、发布包 0700 临时目录、下载后 `codesign` 校验、macOS 签名 Team ID 可选校验。
- **SSH**：默认按 `~/.ssh/known_hosts` 做 TOFU 校验，首次信任写入，密钥变化拒绝；连接表单可关闭。
- **国际化**：生产代码中文注释/文案清理；`navidog.*` 稳定错误码由 UI 统一本地化；新增 `scripts/check-i18n.sh` 防回归。
- **网格/查询**：带引号 TSV 往返、按列类型排序、缓存列类型避免每格解析、`refit_auto` 避免每格 `String`、历史日志改为广播而非 250ms 轮询。
- **一致性与清理**：`JsonStore` 统一持久化策略、`DriverInfo` 收拢 icon/dialect/default port、`driver_name`/`kind_label`/拖拽预览/对话框 footer 去重、删除空 plugin crate/未用 registry/弃用 API。
- **测试/CI**：共享 headless `wait` helper；`xtask manifest` 生成 `latest.json`；`deny.toml`/`AGENTS.md`/`00-overview.md` 版本同步。

## PR review follow-ups (2026-10-26)

- **TSV**：保留引号字段末尾的 `\r`；`""` 可粘贴为一个空单元格；CRLF 才消费行尾 CR。
- **历史日志**：先订阅再取初始快照；收到 `Lagged` 后重新拉取当前 ring，不再永久停止更新。
- **navigator 状态栏**：行数缓存绑定 selection，selection 变化后回退实时计算。
- **SQL 参数**：`foo$tag$` 这类标识符中的 `$tag$` 不再被当成 dollar quote。
- **事务**：自身开启的事务在 `COMMIT` 失败后执行 `ROLLBACK`；用户事务不主动回滚。
- **SSH**：已有任何 known_hosts key 时拒绝不匹配的新 key，只有真正新主机才 TOFU。
- **SQLite 诊断**：`navidog.database_missing` 保留文件路径。
- **连接表单**：多节点/禁用 SSH 时不再校验隐藏端口。
- **i18n 脚本**：每个 key 必须同时有 `en` 和 `zh-CN`；`t!` 扫描支持空格/换行。
- **Windows 更新**：`.msi` 直接交给 `msiexec /i`，`.exe` 直接启动，不再拼 `cmd /C start`。
- **写入耐久性**：Unix 原子替换后 sync 父目录。
- **失败写入测试**：helper 先探测权限是否真的阻断写入；只跳过失败保存断言，Manage Connections 检查继续执行。
- **写入耐久性**：父目录同步改为 best-effort；rename 已完成后目录 fsync 失败不再报告保存失败。
- **navigator detail view**：行数缓存改为 detail 过滤后的实际显示行数。
- **i18n 注释**：`t!` 扫描先剥离 Rust 行/块注释，注释里的示例 key 不再误报。
- **i18n 动态 key**：脚本只校验字面量 `t!("key")`；`t!(format!(...))` 仍靠调用点测试，作为已知限制记录。

已确认无需再改：

- `navidog-test-support` 的 `TestAppContext` 导入已启用 `test-support` feature。
- `replace_bundle` 已有独立的 macOS `cfg`。

按维护者决定保留：

- **不做旧版 saved query 文件名/目录的数据兼容**，继续使用当前编码。
- **保留 ad-hoc 签名更新的自动安装**：未设置 `NAVIDOG_TEAM_ID` 时仍只校验签名完整性，接受第三方镜像可替换的风险，换取免费包自动更新。

## PR #21 second CR (2026-10-26)

- **CR-01**：状态栏行数改为在对象面板 observer 中重算并延迟通知，不再在 `render` 里写缓存；主窗口同时观察 Catalog。
- **CR-03**：`ROLLBACK` / `ROLLBACK TO` 失败写 error 日志，不静默丢弃。
- **S-01/S-02**：SQL 切分在自定义 DELIMITER 下按字符边界推进；`foo$tag$` 不再被当作 dollar quote。
- **S-03**：下载文件名拒绝含冒号/控制字符的 Windows 盘符形式。
- **S-04**：AppImage tar 先检查只有一个顶层常规文件，解压后拒绝符号链接/非普通文件。
- **S-05**：树重建前记录焦点行，重建后恢复字段行而不是映回表行。
- **S-06**：数字排序用 `total_cmp`，NaN 和解析失败值有确定的全序。
- **CR-02**：按维护者决定只记录限制，不实现通配/`@revoked`/`@cert-authority` 匹配。
- **CR-04**：脚本仍只覆盖字面量 `t!` key，动态 `format!` key 作为已知限制。

## 暂缓

A-01/A-02/A-03 等剩余分层工作仍在进行已在审查文档中记录原因。它们涉及公开行为、live 事务回归或跨 crate 迁移，不适合与本次修复混在一起。

## 风险与注意事项

- 配置/密钥文件现在由 `tempfile` 原子替换，Unix 下临时文件权限为 0600；旧配置文件首次保存后权限会收紧。
- SSH 首次连接会写入 `known_hosts`；已有连接如果服务器密钥变化会被拒绝，需要在表单关闭校验或更新 `known_hosts`。当前只匹配精确主机名和 OpenSSH `|1|` 哈希行；`*.example.com`、`!host`、`@revoked`、`@cert-authority` 不参与判断，相关主机仍可能被首次 TOFU。
- 更新包只有在 `codesign --verify` 通过后才会替换；设置 `NAVIDOG_TEAM_ID` 可额外校验 Team ID，未设置时只校验签名完整性；ad-hoc 签名仍可自动安装，这是明确接受的风险。
- TSV 复制会对含制表符/换行/引号的值加引号；粘贴同时接受带引号和不带引号的文本。
