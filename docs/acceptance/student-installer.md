# 学生端安装包（本地构建）

## 产物

| 文件 | 说明 |
| --- | --- |
| `ClassShare-Student-Setup.exe` | 单文件安装器（IExpress 自解压，内置 svc + GUI + 安装脚本） |
| `ClassShare-Student-Setup.zip` | 同一内容的 ZIP（离线/兜底部署） |
| `README-zh.txt` | 终端用户安装说明 |

默认输出目录：`%LOCALAPPDATA%\ClassShareDual\dist`

## 构建

```powershell
# 二进制已就绪时
.\scripts\package-student-installer.ps1

# 从源码重建（前端 + svc + gui）后打包
.\scripts\package-student-installer.ps1 -Build
```

依赖：Windows 自带 IExpress（`C:\Windows\System32\iexpress.exe`）；不使用 GitHub Actions。

## 安装行为

1. `setup-student.ps1` 请求管理员权限（UAC）
2. 把 `classroom-svc.exe` / `classroom-gui.exe` 复制到 `%ProgramFiles%\ClassShare`
3. 调用 `scripts/install.ps1 -Mode student`：
   - 管理员：注册 `ClassShareSvc`（Session 0、开机自启）+ 防火墙 UDP 47820 + GUI 登录自启（HKCU Run，`--role=student`）
   - 非管理员：降级为当前用户登录自启（`--console`），功能受限
4. 卸载：`%ProgramFiles%\ClassShare\uninstall-student.cmd`

## 已验证

- IExpress 构建成功，包内 9 个文件齐全（含未上线前的 selftest/inspect 变体验证：解压目录与 svc/gui SHA-256 与源一致）
- 打包脚本可重复执行，产物落 `dist`；`svc`/`gui` 的 sha256 由脚本输出

## 注意

- 安装器未签名，Windows SmartScreen 会提示「未知发布者」→「更多信息 → 仍要运行」
- 学生机与教师机跨网段时必须填教师机地址（自动发现仅限同一广播域）