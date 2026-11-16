//! 课堂共享后台服务入口：
//! - 默认：经服务控制管理器（SCM）以 Session 0 服务运行（无 UI，PRD 3.2）
//! - `--console`：前台降级模式（开发调试，或无管理员权限时经 HKCU Run
//!   以登录自启方式运行，RK-06/EX-10）
//! - `--config <path>`：覆盖 svc.json 路径
//!
//! SCM failure 自动重启策略（崩溃 5s/5s/30s 重启）由 install.ps1 配置。

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use tokio::sync::Notify;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

use classroom_svc::config::{self, SvcConfig, SERVICE_NAME};
use classroom_svc::logging;
use classroom_svc::runtime::run_core;

/// 停机信号（SCM Stop/Shutdown 与 Ctrl+C 共用同一实例）。
static SHUTDOWN: OnceLock<Arc<Notify>> = OnceLock::new();

fn shutdown_handle() -> Arc<Notify> {
    SHUTDOWN.get_or_init(|| Arc::new(Notify::new())).clone()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--console") {
        run_console(&args);
        return;
    }
    if let Err(e) = service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
        eprintln!("无法经服务控制管理器启动（{e}）。调试或无管理员环境请使用 --console");
        std::process::exit(1);
    }
}

/// 前台降级模式：配置与数据根位于 `%LOCALAPPDATA%\ClassShare`，
/// Ctrl+C 触发与 SCM 停机相同的优雅退出路径。
fn run_console(args: &[String]) {
    let Ok((cfg, data_root)) = bootstrap(true, config_override(args)) else {
        // 配置损坏：驻留等待人工修复（fail-fast，评审随包项 2）
        eprintln!("svc.json 配置损坏，无法启动。请修复后重启进程。");
        std::process::exit(1);
    };
    if let Err(e) = logging::init(&data_root.join("logs")) {
        eprintln!("日志初始化失败（继续运行）: {e}");
    }
    tracing::info!(
        mode = ?cfg.mode,
        data_root = %data_root.display(),
        "前台降级模式启动（--console）"
    );
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("创建 tokio runtime");
    let shutdown = shutdown_handle();
    let ctrl_shutdown = shutdown.clone();
    rt.block_on(async move {
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("收到 Ctrl+C，退出");
            ctrl_shutdown.notify_one();
        });
        if let Err(e) = run_core(&cfg, &data_root, shutdown).await {
            tracing::error!(error = %e, "服务核心异常退出");
        }
    });
}

fn config_override(args: &[String]) -> Option<PathBuf> {
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == "--config" {
            return it.next().map(PathBuf::from);
        }
    }
    None
}

/// 读取配置并确定数据根。配置损坏即 fail-fast（评审随包项 2）：回退默认
/// 会让学员机静默变成教师广播幽灵房间（EX-11 语义被破坏）——报错后由
/// 调用方转 Stopped / 驻留等待人工修复 svc.json。
fn bootstrap(console: bool, config_path: Option<PathBuf>) -> anyhow::Result<(SvcConfig, PathBuf)> {
    let base = config::service_base_dir(console);
    let path = config_path.unwrap_or_else(|| base.join("svc.json"));
    let cfg = config::load_config(&path)
        .map_err(|e| anyhow::anyhow!("读取配置 {} 失败（{e}）", path.display()))?;
    Ok((cfg, config::data_root(console)))
}

define_windows_service!(ffi_service_main, my_service_main);

/// 服务主函数（SCM 在后台线程调用）：注册控制回调 → StartPending →
/// 核心运行 → Stopped。SCM failure 策略（install.ps1 配置）在异常退出时
/// 自动重启本进程。
fn my_service_main(_arguments: Vec<OsString>) {
    let shutdown = shutdown_handle();
    let ctrl_shutdown = shutdown.clone();
    let event_handler = move |control: ServiceControl| -> ServiceControlHandlerResult {
        match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                // 评审随包项 1：notify_one（单许可保存）——Stop 在核心进入
                // select 前到达也不丢，杜绝 StartPending 期挂起（与 net 侧 C 项对齐）
                ctrl_shutdown.notify_one();
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };
    let status_handle = match service_control_handler::register(SERVICE_NAME, event_handler) {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("注册服务控制处理器失败: {e}");
            return;
        }
    };
    let set_status = |state: ServiceState| {
        let _ = status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
            exit_code: ServiceExitCode::NO_ERROR,
            checkpoint: 0,
            wait_hint: std::time::Duration::from_secs(5),
            process_id: None,
        });
    };
    set_status(ServiceState::StartPending);

    // 配置损坏：fail-fast 转 Stopped（SCM failure 策略会按计划重启尝试；
    // 不回退默认配置，杜绝学员机静默变教师幽灵房间——EX-11）
    let (cfg, data_root) = match bootstrap(false, None) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}；svc.json 配置损坏，服务转 Stopped，请修复配置后重启服务。");
            set_status(ServiceState::Stopped);
            return;
        }
    };
    if let Err(e) = logging::init(&data_root.join("logs")) {
        eprintln!("日志初始化失败（继续运行）: {e}");
    }
    tracing::info!(service = SERVICE_NAME, "服务启动（Session 0）");

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!(error = %e, "tokio runtime 创建失败");
            set_status(ServiceState::Stopped);
            return;
        }
    };
    set_status(ServiceState::Running);
    if let Err(e) = rt.block_on(run_core(&cfg, &data_root, shutdown)) {
        tracing::error!(error = %e, "服务核心异常退出");
    }
    set_status(ServiceState::Stopped);
    tracing::info!("服务已停止");
}
