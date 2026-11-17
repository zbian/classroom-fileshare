//! GUI ↔ 服务 IPC 桥（对接 crates/ipc，T-7/#38）。
//!
//! - 连接：setup 后台 `start_bridge`（服务未起时客户端自旋重试 ~5s 后失败，
//!   不阻塞 GUI 启动）；请求前 `ensure_client` 惰性重连一次兜底
//! - 请求：`svc_request` 命令（payload 为 [`GuiRequest`] 的 serde 外部不可辨名
//!   形态，与 ui/src/shared/ipc/client.ts 约定一致；错误文案原样透传前端）
//! - 通知：`SvcNotify` mpsc → 重打标签为 `{ "t": <变体名>, "payload": … }`
//!   后 `emit("svc://notify")` 广播（前端 store 按 `t` 分发）
//! - tips 接线（P0 #79）：`ShareReceived` 在此映射为 [`TipDto`] 并调用
//!   [`crate::tips::push_tip`]（映射放数据源侧，tips 模块只管展示）
//! - OpenItem：服务返回 [`GuiReply::FilePath`]，GUI（用户会话）用系统默认程序
//!   打开（接口修订 #7①，Session 0 服务无法替用户拉起进程）

use crate::state::{config_dir, AppState};
use crate::tips::{self, TipDto};
use classroom_ipc::{
    connect_ipc, load_or_create_token, GuiReply, GuiRequest, IpcClient, SvcNotify,
};
use classroom_protocol::{ItemKind, Role, ShareItem};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// 桥状态：已握手的客户端句柄（None = 尚未连上/曾断开，请求前惰性重连）。
/// [`IpcClient`] 为 Clone（共享写端），克隆成本可忽略。
pub struct IpcBridge {
    client: Mutex<Option<IpcClient>>,
    /// tips 去重表：item id → 最近一次已推送的 version。
    /// 服务端对同一 upsert 会同时下发 `ShareReceived` 与 `ItemUpdated(overwritten)`，
    /// 两者都触发提示时按此表去重，保证「一条共享只弹一个气泡」。
    last_tip_version: Mutex<HashMap<String, u64>>,
}

impl IpcBridge {
    pub fn new() -> Self {
        Self {
            client: Mutex::new(None),
            last_tip_version: Mutex::new(HashMap::new()),
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

/// 首连/重连退避：1s → 2s → 5s 封顶（E12 ②：常驻图标场景下服务可能晚于 GUI
/// 启动或重启，必须自动重试，不能只在有 UI 请求时才连）。
fn backoff_delay(attempt: u32) -> std::time::Duration {
    match attempt {
        0 => std::time::Duration::from_secs(1),
        1 => std::time::Duration::from_secs(2),
        _ => std::time::Duration::from_secs(5),
    }
}

/// 带退避的无限重连，直到 `connect` 成功返回（E12 ②）。
///
/// `connect` 由调用方注入（真实实现 = [`connect`]；单测用假实现验证重试次数
/// 与退避节奏），避免「mock≠真实」的最常见形态：这里测的是重试控制流本身。
async fn retry_until_connected<T, E, F, Fut>(mut connect: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempt: u32 = 0;
    loop {
        match connect().await {
            Ok(value) => return value,
            Err(e) => {
                let delay = backoff_delay(attempt);
                eprintln!(
                    "[ipc] 连接失败（第 {} 次）：{e}；{}s 后重试",
                    attempt + 1,
                    delay.as_secs()
                );
                tokio::time::sleep(delay).await;
                attempt = attempt.saturating_add(1);
            }
        }
    }
}

/// setup 后台连接 + 通知转发。**连接由本函数独占负责**：
/// 首连失败自动退避重试；连接断开（服务退出/重启）后回到重试循环，
/// 因此常驻图标即使没人打开主界面也能自动恢复（E12 ②）。
pub fn start_bridge(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let (client, rx) = {
                let app_for_connect = app.clone();
                retry_until_connected(|| {
                    let app = app_for_connect.clone();
                    async move { connect(&app).await }
                })
                .await
            };
            let role = role_from_config(&app);
            eprintln!("[ipc] 服务管道已连接（role={role:?}）");
            *app.state::<IpcBridge>().client.lock().expect("ipc lock") = Some(client);
            // 连接成功即通知前端重拉结构化状态（主窗已开时自愈 EX-10）
            let _ = app.emit(
                "svc://notify",
                retag(&SvcNotify::StatusChanged(String::new())),
            );
            // 阻塞直到通道关闭（服务退出）；之后回到重连
            forward_loop(&app, rx).await;
        }
    });
}

async fn forward_loop(app: &AppHandle, mut rx: tokio::sync::mpsc::Receiver<SvcNotify>) {
    // 本机角色用于「不给自己弹提醒」判定（见 notify_to_tip）。
    let self_role = role_from_config(app);
    while let Some(n) = rx.recv().await {
        // P0 #79：tips 接线。服务端每次 upsert 会同时下发 ShareReceived 与
        // ItemUpdated（runtime::map_notify 的 upsert 闭包），由 tips_to_push
        // 按 (item id, version) 去重，保证同一条共享只弹一个气泡；覆盖由
        // ItemUpdated(overwritten=true) 兜底。
        let pending = {
            let bridge = app.state::<IpcBridge>();
            let mut last = bridge.last_tip_version.lock().expect("tips dedupe lock");
            tips_to_push(&n, &self_role, &mut last)
        };
        if let Some(tip) = pending {
            tips::push_tip(app, tip);
        }
        if let Err(e) = app.emit("svc://notify", retag(&n)) {
            eprintln!("[ipc] 通知转发失败：{e}");
        }
    }
    eprintln!("[ipc] 通知通道关闭（服务退出）");
    // 不可达态：通知前端（文本不携带语义，前端收到后重拉结构化 status 判定）
    let _ = app.emit(
        "svc://notify",
        retag(&SvcNotify::StatusChanged(String::new())),
    );
    *app.state::<IpcBridge>().client.lock().expect("ipc lock") = None;
}

/// 通知 → tips 载荷（纯函数，便于单测）。返回 `(TipDto, version)`，version 供
/// 调用方去重。
///
/// 取舍（reviewer 清单第 1 项）：
/// - `ShareReceived`：新共享到达（含同名覆盖后的新版本）→ 推
/// - `ItemUpdated{overwritten: true}`：覆盖是用户可见事件（PRD 4.2 / 8.3 覆盖提示）→ 推；
///   与 ShareReceived 同源时由 `mark_tip_once` 按 (id, version) 去重
/// - `ItemUpdated{overwritten: false}`：与 ShareReceived 完全同源，单独推会双气泡 → 不推
/// - `ItemRemoved`：删除不是「新共享」，不推（删除提示走主窗/E4 语义）→ 不推
/// - 本人产生的条目：服务端在本地发送时也会回 `HostEvent::ReceivedUpsert`
///   （crates/host/src/hub.rs 广播路径），按 owner_role 排除，避免「自己发、自己弹」
fn notify_to_tip(n: &SvcNotify, self_role: &Role) -> Option<(TipDto, u64)> {
    let item = match n {
        SvcNotify::ShareReceived(item) => item,
        SvcNotify::ItemUpdated(payload) if payload.overwritten => &payload.item,
        _ => return None,
    };
    if item.owner_role == *self_role {
        return None;
    }
    Some((tip_from_share_item(item), item.version))
}

/// tips 去重判定（纯函数）：同一条目只在 version 增长时推送。
fn should_push_tip(previous_version: u64, version: u64) -> bool {
    version > previous_version
}

/// 通知 → 待推送 tips（纯函数，含 (id, version) 去重）。
///
/// 把「映射 + 去重」收在一个纯函数里，便于单测精确覆盖服务端的双通知行为
/// （reviewer 清单第 2/4 项：一个事件不弹两次，且用测试锁死该行为）。
fn tips_to_push(
    n: &SvcNotify,
    self_role: &Role,
    last_tip_version: &mut HashMap<String, u64>,
) -> Option<TipDto> {
    let (tip, version) = notify_to_tip(n, self_role)?;
    let previous = last_tip_version.get(&tip.id).copied().unwrap_or(0);
    if !should_push_tip(previous, version) {
        return None;
    }
    // 仅需短窗口去重；超过上限整体清空，避免长时间运行无界增长
    if last_tip_version.len() > 512 {
        last_tip_version.clear();
    }
    last_tip_version.insert(tip.id.clone(), version);
    Some(tip)
}

/// `ShareItem` → tips 展示字段（reviewer 建议：映射放数据源侧，tips 模块只展示）。
fn tip_from_share_item(item: &ShareItem) -> TipDto {
    let from = match item.owner_role {
        Role::Teacher => "教师".to_string(),
        Role::Student if !item.owner_id.is_empty() => item.owner_id.clone(),
        Role::Student => "学员".to_string(),
    };
    let (kind, name, size_text) = match item.kind {
        ItemKind::File => {
            // 文件 title 可为空（PRD 3.5），回退到 rel_path 文件名，再回退占位
            let fallback = item
                .rel_path
                .as_deref()
                .and_then(|p| p.rsplit(['/', '\\']).next())
                .filter(|s| !s.is_empty());
            let name = if item.title.trim().is_empty() {
                fallback.unwrap_or("（未命名文件）").to_string()
            } else {
                item.title.clone()
            };
            ("file", name, human_size(item.size))
        }
        // 文本的 size 为字符数（PRD 3.5）
        ItemKind::Text => (
            "text",
            if item.title.trim().is_empty() {
                "（无标题文本）".to_string()
            } else {
                item.title.clone()
            },
            format!("{} 字", item.size),
        ),
    };
    TipDto {
        id: item.id.clone(),
        kind: kind.to_string(),
        name,
        from,
        size_text,
    }
}

/// 字节数 → 人类可读（tips 副标题用，与界面一致：B / KB / MB）。
fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < MB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{:.1} MB", b / MB)
    }
}

/// 惰性重连：请求前无客户端时尝试一次；仍失败返回 EX-10 文案。
async fn ensure_client(app: &AppHandle) -> Result<IpcClient, String> {
    // 连接所有权归 start_bridge（含退避重连）：这里只等待其就绪，避免出现
    // 第二条连接与两个 forward_loop（双通知/双 tips 的经典竞态）。
    for _ in 0..=12 {
        if let Some(c) = app
            .state::<IpcBridge>()
            .client
            .lock()
            .expect("ipc lock")
            .clone()
        {
            return Ok(c);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Err("后台服务未运行".to_string())
}

/// svc_request：前端唯一入口（GuiRequest 外部不可辨名形态 → 反序列化 → 转发）。
/// 服务侧错误文案（SvcResult::Err）原样透传给前端（EX-xx 展示）。
pub async fn svc_request(app: AppHandle, request: GuiRequest) -> Result<Value, String> {
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
        return Ok(json!({ "t": "Ok" }));
    }
    // 与 notify 同型：GuiReply 走外部不可辨名序列化（{"Items":[…]}），而前端
    // client.ts 按 {"t":"Items","items":[…]} 解包——不做对称 retag 时真实模式
    // List 恒空 / Status 恒 down（reviewer 实锤的第三次同型逃逸，此处修复）。
    Ok(retag_reply(&reply))
}

/// GuiReply → 前端契约形态 {"t": 变体名, <契约键>: 载荷}：
/// Ok → {"t":"Ok"}；Items → items；Status → snapshot；FilePath → path
/// （FilePath 已被桥内 opener 拦截，正常不外泄；兜底保持契约）。
fn retag_reply(reply: &GuiReply) -> Value {
    let raw = serde_json::to_value(reply).expect("GuiReply 序列化");
    match raw {
        Value::String(k) => json!({ "t": k }), // 单元变体（Ok 等）序列化为裸串
        Value::Object(map) => {
            let (k, v) = map.into_iter().next().expect("外部不可辨名枚举单键");
            let key = match k.as_str() {
                "Items" => "items",
                "Status" => "snapshot",
                "FilePath" => "path",
                other => other,
            };
            json!({ "t": k, key: v })
        }
        other => other,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 前端契约：unit 变体经 {"Status": null} 形态反序列化（client.ts 的 request 形态）
    #[test]
    fn gui_request_deserializes_null_map_form() {
        let v: GuiRequest = serde_json::from_value(json!({ "Status": null })).expect("map 形态");
        assert!(matches!(v, GuiRequest::Status));
        let v2: GuiRequest = serde_json::from_value(json!("Status")).expect("裸串形态");
        assert!(matches!(v2, GuiRequest::Status));
    }

    /// 阻断修复回归：reply 对称 retag 后为前端契约形态（{"t":…} + 契约键）
    #[test]
    fn retag_reply_matches_frontend_contract() {
        let ok = retag_reply(&GuiReply::Ok);
        assert_eq!(ok, json!({ "t": "Ok" }));

        let items = retag_reply(&GuiReply::Items(Vec::new()));
        assert_eq!(items, json!({ "t": "Items", "items": [] }));

        let snap = classroom_ipc::StatusSnapshot {
            service_state: classroom_ipc::ServiceState::Running,
            room: "XK-204".into(),
            online_count: 3,
            nickname: String::new(),
            role: Role::Teacher,
            pending_count: 0,
        };
        let st = retag_reply(&GuiReply::Status(snap));
        assert_eq!(st["t"], "Status");
        assert_eq!(st["snapshot"]["room"], "XK-204");
        assert_eq!(st["snapshot"]["service_state"], "running");
        assert_eq!(st["snapshot"]["online_count"], 3);
    }

    fn item(kind: ItemKind, owner_role: Role, owner_id: &str, title: &str, size: u64) -> ShareItem {
        ShareItem {
            id: "id-1".into(),
            kind,
            owner_id: owner_id.into(),
            owner_role,
            audience: classroom_protocol::Audience::All,
            title: title.into(),
            size,
            sha256: None,
            rel_path: None,
            created_at: 0,
            updated_at: 0,
            version: 1,
            deleted: false,
            sync_state: classroom_protocol::SyncState::Synced,
        }
    }

    /// P0 #79 核心：学生提交到达教师机 → 产出 tips（字段映射正确）。
    #[test]
    fn share_received_from_student_maps_to_tip_for_teacher() {
        let it = item(ItemKind::Text, Role::Student, "小林", "第三题的疑问", 86);
        let (tip, version) = notify_to_tip(&SvcNotify::ShareReceived(it), &Role::Teacher)
            .expect("教师机收到学生提交应弹 tips");
        assert_eq!(version, 1, "version 供去重使用");
        assert_eq!(tip.id, "id-1");
        assert_eq!(tip.kind, "text");
        assert_eq!(tip.name, "第三题的疑问");
        assert_eq!(tip.from, "小林");
        assert_eq!(tip.size_text, "86 字");
    }

    /// 教师广播到达学员机 → 产出 tips（来源显示「教师」、文件大小可读）。
    #[test]
    fn share_received_from_teacher_maps_to_tip_for_student() {
        let it = item(
            ItemKind::File,
            Role::Teacher,
            "teacher",
            "第 3 章课件.pdf",
            2_516_582,
        );
        let (tip, _version) = notify_to_tip(&SvcNotify::ShareReceived(it), &Role::Student)
            .expect("学员机收到教师广播应弹 tips");
        assert_eq!(tip.kind, "file");
        assert_eq!(tip.name, "第 3 章课件.pdf");
        assert_eq!(tip.from, "教师");
        assert_eq!(tip.size_text, "2.4 MB");
    }

    /// 自己产生的条目不弹（服务端在本地发送时也回 ReceivedUpsert）。
    #[test]
    fn own_items_do_not_trigger_tips() {
        let teacher_own = item(ItemKind::Text, Role::Teacher, "teacher", "通知", 10);
        assert!(notify_to_tip(&SvcNotify::ShareReceived(teacher_own), &Role::Teacher).is_none());
        let student_own = item(ItemKind::File, Role::Student, "小林", "作业.docx", 1024);
        assert!(notify_to_tip(&SvcNotify::ShareReceived(student_own), &Role::Student).is_none());
    }

    /// ItemUpdated 的取舍：覆盖（overwritten=true）是用户可见事件 → 推；
    /// 非覆盖与 ShareReceived 同源 → 不推（避免双气泡）。
    #[test]
    fn item_updated_only_pushes_for_overwrite() {
        let it = item(ItemKind::Text, Role::Student, "小林", "疑问", 5);
        let plain = SvcNotify::ItemUpdated(classroom_ipc::ItemUpdatedPayload {
            item: it.clone(),
            overwritten: false,
            old_version: None,
        });
        assert!(
            notify_to_tip(&plain, &Role::Teacher).is_none(),
            "非覆盖的 ItemUpdated 与 ShareReceived 同源，不应二次弹"
        );

        let mut overwritten_item = it;
        overwritten_item.version = 2;
        let overwrite = SvcNotify::ItemUpdated(classroom_ipc::ItemUpdatedPayload {
            item: overwritten_item,
            overwritten: true,
            old_version: Some(1),
        });
        let (tip, version) =
            notify_to_tip(&overwrite, &Role::Teacher).expect("覆盖是用户可见事件，应弹 tips");
        assert_eq!(tip.id, "id-1");
        assert_eq!(version, 2, "覆盖后的 version 用于去重");
    }

    /// 删除/状态类通知不弹（删除不是新共享）。
    #[test]
    fn non_share_notifications_do_not_trigger_tips() {
        assert!(notify_to_tip(&SvcNotify::ItemRemoved("id".into()), &Role::Teacher).is_none());
        assert!(notify_to_tip(&SvcNotify::StatusChanged(String::new()), &Role::Teacher).is_none());
    }

    /// 去重判定：同 version 只推一次；version 增长（覆盖）再推。
    #[test]
    fn dedupe_only_pushes_when_version_advances() {
        assert!(should_push_tip(0, 1), "首次到达");
        assert!(
            !should_push_tip(1, 1),
            "同源第二条（ShareReceived + ItemUpdated）不重复推"
        );
        assert!(should_push_tip(1, 2), "覆盖后 version 增长应再推");
        assert!(!should_push_tip(2, 1), "乱序旧版本不推");
    }

    /// 端到端决策测试（纯函数层）：模拟服务端对同一 upsert 的双通知，断言只弹一次；
    /// 再模拟「仅收到覆盖态 ItemUpdated」的兜底路径仍能弹。
    #[test]
    fn same_upsert_double_notify_yields_single_tip() {
        let mut last: HashMap<String, u64> = HashMap::new();
        let it = item(ItemKind::Text, Role::Student, "小林", "第三题的疑问", 86);
        let share = SvcNotify::ShareReceived(it.clone());
        // 服务端 map_notify：ShareReceived 与 ItemUpdated(overwritten=false) 同时下发
        let plain = SvcNotify::ItemUpdated(classroom_ipc::ItemUpdatedPayload {
            item: it.clone(),
            overwritten: false,
            old_version: None,
        });
        assert!(
            tips_to_push(&share, &Role::Teacher, &mut last).is_some(),
            "首条应弹"
        );
        assert!(
            tips_to_push(&plain, &Role::Teacher, &mut last).is_none(),
            "同源 ItemUpdated 不得二次弹（单气泡）"
        );
        assert!(
            tips_to_push(&share, &Role::Teacher, &mut last).is_none(),
            "重复推送同一通知也不得再弹"
        );

        // 覆盖兜底：若只收到 overwritten 的 ItemUpdated（version 2），仍应弹
        let mut only_overwrite: HashMap<String, u64> = HashMap::new();
        let mut v2 = it;
        v2.version = 2;
        let overwrite = SvcNotify::ItemUpdated(classroom_ipc::ItemUpdatedPayload {
            item: v2,
            overwritten: true,
            old_version: Some(1),
        });
        assert!(
            tips_to_push(&overwrite, &Role::Teacher, &mut only_overwrite).is_some(),
            "仅覆盖通知时也要提示（PRD 4.2/8.3）"
        );
    }

    /// 文件 title 为空时回退到 rel_path 文件名，再回退占位。
    #[test]
    fn file_tip_name_falls_back_to_rel_path_then_placeholder() {
        let mut it = item(ItemKind::File, Role::Teacher, "teacher", "", 100);
        it.rel_path = Some("teacher/repo/实验素材.zip".into());
        assert_eq!(tip_from_share_item(&it).name, "实验素材.zip");
        it.rel_path = None;
        assert_eq!(tip_from_share_item(&it).name, "（未命名文件）");
    }

    /// 大小格式化覆盖 B / KB / MB 三档。
    #[test]
    fn human_size_formats_b_kb_mb() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(880_640), "860 KB");
        assert_eq!(human_size(2_516_582), "2.4 MB");
    }

    /// E12 ②：退避节奏 1s → 2s → 5s 封顶。
    #[test]
    fn backoff_is_one_two_five_capped() {
        assert_eq!(backoff_delay(0), std::time::Duration::from_secs(1));
        assert_eq!(backoff_delay(1), std::time::Duration::from_secs(2));
        assert_eq!(backoff_delay(2), std::time::Duration::from_secs(5));
        assert_eq!(backoff_delay(9), std::time::Duration::from_secs(5));
    }

    /// E12 ②（续）：首连失败后自动退避重试直到成功——对应真实时序
    /// 「GUI 登录自启早于服务就绪」（常驻图标、无人打开主界面也必须自愈）。
    #[tokio::test(start_paused = true)]
    async fn retry_until_connected_recovers_after_failures() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let attempts = Arc::new(AtomicU32::new(0));
        let counter = attempts.clone();
        let value = retry_until_connected(|| {
            let counter = counter.clone();
            async move {
                let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
                if n < 3 {
                    Err("服务未就绪")
                } else {
                    Ok(42u8)
                }
            }
        })
        .await;
        assert_eq!(value, 42, "第 3 次尝试应成功返回");
        assert_eq!(attempts.load(Ordering::SeqCst), 3, "失败 2 次后才成功");
    }
}
