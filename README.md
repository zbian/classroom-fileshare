# 课堂共享 · 桌面小工具（classroom-fileshare）

一款常驻 Windows 桌面、免账号、仅依赖教室局域网的课堂文本与文件共享小工具。
教师机为局域网内唯一服务端（Host），向全班广播文本 / 文件；学员定向回传给教师；
所有交互收敛为「置顶浮动图标 + 复制 / 拖拽 / 右键」三个本能操作。

## 技术栈（规划）

- Windows 10 / 11 x64，Tauri 2.x（Rust + WebView2）
- 后台 Windows 服务 + GUI 双进程（Session 0 限制拆分）
- UDP 广播发现 + TCP 长连接，SQLite 索引，SHA-256 校验

## 构建与运行

> **O-1 构建口径（M1 验收发现，必读）**：`tauri` 的 dev 模式判定只看是否启用
> `custom-protocol` feature（`tauri-build::is_dev()` 读 `DEP_TAURI_DEV`），**与 cargo
> profile 无关**。因此裸 `cargo build --release` 仍按 dev 模式编译：不内嵌前端资源，
> 产物启动后去连 `tauri.conf.json > build.devUrl`（`http://localhost:1517`），
> 无 dev server 即白窗。本仓库已加构建期告警 + 启动时弹窗明确提示，但
> **验收与分发请使用下表的「生产构建」产物，不要用裸 `cargo build --release`**。

| 场景 | 命令 | 说明 |
| --- | --- | --- |
| 前端依赖 | `pnpm --dir apps/classroom-gui/ui install` | Node 24 + pnpm 11 |
| 开发（前端热更新） | 终端 A：`pnpm --dir apps/classroom-gui/ui dev`<br>终端 B：`cargo build -p classroom-gui`，运行 `target\debug\classroom-gui.exe` | debug 产物连 devUrl 1517 |
| **生产构建（推荐）** | `scripts\build-gui.ps1` | = `pnpm build` + `cargo build --release -p classroom-gui --features custom-protocol` |
| 生产构建（等价手动） | `pnpm --dir apps/classroom-gui/ui build`<br>`cargo build --release -p classroom-gui --features custom-protocol` | 产物约 12 MB，内嵌前端，无需 dev server |
| 安装（服务 + 登录自启） | `scripts\install.ps1 -Mode teacher -Room XK-204` | 非管理员自动降级；详见脚本头注释 |
| 卸载 | `scripts\uninstall.ps1` | 数据默认保留；`-CleanData` 显式删除 |

依赖：Rust stable（cargo 在 `%USERPROFILE%\.cargo\bin`）、Node 24 + pnpm 11、WebView2 Runtime。

## 文档索引

| 文件 | 说明 |
| --- | --- |
| [final_draft.md](final_draft.md) | 产品需求文档 PRD V1.0（研发实现与测试验收的唯一功能依据） |
| [ui-design.html](ui-design.html) | 全量界面设计稿 v1.0（6 组 16 屏，A1–F2，可浏览器打开） |
| [课堂共享_全部界面设计.png](课堂共享_全部界面设计.png) | 设计稿整版渲染图（2400 × 4163 px） |

## 核心产品决策

- 学生提交对其他学生不可见，仅教师可按昵称分组查看
- 教师删除已广播内容时同步删除学员本地副本，离线端上线后延迟补删
- 不做跨网段 / 云端中继；先做 Windows，macOS 以后考虑
- 同名文件直接覆盖并保留 1 份 `.bak` 备份，不做二次确认
- 取消全局热键，文本走复制粘贴；文件走资源管理器右键 / 拖拽

## 优先级口径（PRD 第 12 章）

- **P0**：能上一节课的最小闭环（图标、广播、tips、覆盖+备份、服务自启）
- **P1**：学生回传与分组、删除同步、离线待发队列、完整增删改查
- **P2**：搜索筛选、版本历史、断点续传等，依课堂反馈排期
