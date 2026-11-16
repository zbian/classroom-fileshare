//! GUI ↔ 服务 IPC 桥（对接 crates/ipc，T-7/#38）。
//!
//! - 连接：setup 后台 `start_bridge`（服务未起时客户端自旋重试 ~5s 后失败，
//!   不阻塞 GUI 启动）；请求前 `ensure_client` 惰性重连一次兜底
//! - 请求：`svc_request` 命令（payload 为 [`GuiRequest`] 的 serde 外部不可辨名
//!   形态，与 ui/src/shared/ipc/client.ts 约定一致；错误文案原样透传前端）
//! - 通知：`SvcNotify` mpsc → 重打标签为 `{ "t": <变体名>, "payload": … }`
//!   后 `emit("svc://notify")` 广播（前端 store 按 `t` 分发）
//! - OpenItem：服务返回 [`GuiReply::FilePath`]，GUI（用户会话）用系统默认程序
//!   打开（接口修订 #7①，Session 0 服务无法替用户拉起进程）

use crate::state::{config_dir, AppState};
use classroom_ipc::{
    connect_ipc, load_or_create_token, GuiReply, GuiRequest, IpcClient, SvcNotify,
};
use classroom_protocol::Role;
use serde_json::{json, Value};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// 桥状态：已握手的客户端句柄（None = 尚未连上/曾断开，请求前惰性重连）。
/// [`IpcClient`] 为 Clone（共享写端），克隆成本可忽略。
pub struct IpcBridge {
    client: Mutex<Option<IpcClient>>,
}

impl IpcBridge {
    pub fn new() -> Self {
        Self {
            client: Mutex::new(None),
        }
    }
}

impl Default for IpcBridge {
    fn default() -> Self {
        Self::new()
    }
}

fn role_from_config(app: &AppHandle) -> Role {
    app.state::<AppState>()
        .config
        .lock()
        .map(|c| match c.role.as_str() {
            "student" => Role::Student,
            _ => Role::Teacher,
        })
        .unwrap_or(Role::Teacher)
}

async fn connect(
    app: &AppHandle,
) -> anyhow::Result<(IpcClient, tokio::sync::mpsc::Receiver<SvcNotify>)> {
    let role = role_from_config(app);
    let token = load_shared_token()?;
    connect_ipc(role, &token).await
}

/// 读取与后台服务共享的 IPC 令牌（集成契约；实测握手校验出的缺陷修正）：
/// 服务模式令牌在 `%PROGRAMDATA%\ClassShare\data\ipc.token`，console 降级在
/// `%LOCALAPPDATA%\ClassShare\data\ipc.token`（apps/classroom-svc config.rs
/// 的 service_base_dir/data_root）。都不存在时在 PROGRAMDATA 处创建（产品常态；
/// 服务随后 load 同一文件 → 同 token，握手通过）。
fn load_shared_token() -> anyhow::Result<String> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    for var in ["PROGRAMDATA", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            candidates.push(
                std::path::PathBuf::from(base)
                    .join("ClassShare")
                    .join("data"),
            );
        }
    }
    for dir in &candidates {
        let path = dir.join("ipc.token");
        if path.exists() {
            let token = std::fs::read_to_string(&path)?.trim().to_string();
            if !token.is_empty() {
                return Ok(token);
            }
        }
    }
    let fallback = candidates.first().cloned().unwrap_or_else(config_dir);
    load_or_create_token(&fallback)
}

/// setup 后台连接 + 通知转发。连接失败仅记录（`ensure_client` 惰性重连兜底）。
pub fn start_bridge(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match connect(&app).await {
            Ok((client, rx)) => {
                let role = role_from_config(&app);
                eprintln!("[ipc] 服务管道已连接（role={role:?}）");
                *app.state::<IpcBridge>().client.lock().expect("ipc lock") = Some(client);
                forward_loop(&app, rx).await;
            }
            Err(e) => eprintln!("[ipc] 连接失败（服务未起？）：{e:#}"),
        }
    });
}

async fn forward_loop(app: &AppHandle, mut rx: tokio::sync::mpsc::Receiver<SvcNotify>) {
    while let Some(n) = rx.recv().await {
        if let Err(e) = app.emit("svc://notify", retag(&n)) {
            eprintln!("[ipc] 通知转发失败：{e}");
        }
    }
    eprintln!("[ipc] 通知通道关闭（服务退出）");
    // 不可达态通知前端（EX-10/EX-08 文案由前端按角色选择），并清空连接待重连
    let _ = app.emit(
        "svc://notify",
        retag(&SvcNotify::StatusChanged(
            "service=down;room=;online=0".into(),
        )),
    );
    *app.state::<IpcBridge>().client.lock().expect("ipc lock") = None;
}

/// 惰性重连：请求前无客户端时尝试一次；仍失败返回 EX-10 文案。
async fn ensure_client(app: &AppHandle) -> Result<IpcClient, String> {
    if let Some(c) = app
        .state::<IpcBridge>()
        .client
        .lock()
        .expect("ipc lock")
        .clone()
    {
        return Ok(c);
    }
    let (client, rx) = connect(app)
        .await
        .map_err(|_| "后台服务未运行".to_string())?;
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move { forward_loop(&app2, rx).await });
    *app.state::<IpcBridge>().client.lock().expect("ipc lock") = Some(client.clone());
    Ok(client)
}

/// svc_request：前端唯一入口（GuiRequest 外部不可辨名形态 → 反序列化 → 转发）。
/// 服务侧错误文案（SvcResult::Err）原样透传给前端（EX-xx 展示）。
pub async fn svc_request(app: AppHandle, request: GuiRequest) -> Result<GuiReply, String> {
    let client = ensure_client(&app).await?;
    // 双层结果：外层传输错误（anyhow）→ 统一 EX-10 文案；内层服务业务错误（String）原样透传
    let svc = client
        .request(request)
        .await
        .map_err(|e| format!("后台服务未运行（{e:#}）"))?;
    let reply = svc?;
    // 修订 #7①：OpenItem 返回实体绝对路径，由 GUI（用户会话）用默认程序打开
    if let GuiReply::FilePath(path) = &reply {
        tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| format!("打开失败：{e}"))?;
        return Ok(GuiReply::Ok);
    }
    Ok(reply)
}

/// SvcNotify → 前端形态 {"t": 变体名, "payload": …}（见 client.ts 类型镜像）。
fn retag(n: &SvcNotify) -> Value {
    let raw = serde_json::to_value(n).expect("SvcNotify 序列化");
    match raw {
        Value::Object(map) => {
            let (k, v) = map.into_iter().next().expect("外部不可辨名枚举单键");
            let payload = match (k.as_str(), v) {
                ("ItemRemoved", Value::String(id)) => json!({ "id": id }),
                ("Discovered", arr @ Value::Array(_)) => json!({ "beacons": arr }),
                (_, inner) => inner,
            };
            json!({ "t": k, "payload": payload })
        }
        other => other,
    }
}
