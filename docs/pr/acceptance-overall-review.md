# 整体审查修复验收（2026-10-24）

- 分支：`review/overall-fixes`
- 审查与进度： [docs/cr/2026-10-24-overall-review.md](../cr/2026-10-24-overall-review.md)
- 范围：11 个 crate + `xtask` + CI/scripts 的跨 crate 审查；48 项已修，6 项进行中，3 项架构级建议暂缓并记录原因。

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

## 暂缓

3 项架构级建议（A-01/A-02/A-03）已在审查文档中记录原因。它们涉及公开行为、live 事务回归或跨 crate 迁移，不适合与本次修复混在一起。

## 风险与注意事项

- 配置/密钥文件现在由 `tempfile` 原子替换，Unix 下临时文件权限为 0600；旧配置文件首次保存后权限会收紧。
- SSH 首次连接会写入 `known_hosts`；已有连接如果服务器密钥变化会被拒绝，需要在表单关闭校验或更新 `known_hosts`。
- 更新包只有在 `codesign --verify` 通过后才会替换；设置 `NAVIDOG_TEAM_ID` 可额外校验 Team ID，未设置时只校验签名完整性。
- TSV 复制会对含制表符/换行/引号的值加引号；粘贴同时接受带引号和不带引号的文本。
