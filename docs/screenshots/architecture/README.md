# 架构基础优化截图

2026-10-21，`cargo test -p navidog --test snapshot --locked` 生成原图。使用临时测试配置，通过阻断原子写入并点击真实表单的保存按钮，验证失败后错误可见、输入仍保留、内存与磁盘原配置不变。未填写秘密字段。

| 场景 | 浅色 | 深色 |
| --- | --- | --- |
| 连接保存失败，保留输入 | [原图](connection-save-failed-light.png) | [原图](connection-save-failed-dark.png) |

截图为本机自动渲染检查，不代替 Navicat 运行态并排验收。
