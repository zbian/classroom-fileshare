//! 同步事件与幂等合并决策（PRD 3.6）。

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::item::ShareItem;

/// 事件类型：网络层只传递四类事件（PRD 3.6）。
///
/// 说明：`Debug, PartialEq` 为接口修订 #3 的必要传导（`Event` 需派生，见下）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum EventKind {
    /// 新增或覆盖。
    Upsert,
    /// 删除（离线端转墓碑记录）。
    Delete,
    /// 重命名。
    Rename,
    /// 确认。
    Ack,
}

/// 同步事件：携带全局唯一 `event_id` 与条目 `version`。
///
/// 说明：`Debug, PartialEq` 为接口修订 #3（lead 批准）——net 层 `Frame`
/// 需要帧往返断言，与 T-2 的 `ShareItem` 补 `Debug` 同理。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Event {
    /// 全局唯一事件标识，接收端按其去重。
    pub event_id: String,
    /// 事件类型。
    pub kind: EventKind,
    /// 事件载荷（对应版本的条目快照）。
    pub item: ShareItem,
    /// 仅 Rename：新名称。
    pub new_name: Option<String>,
    /// 仅 Ack：确认方标识。
    pub acked_by: Option<String>,
    /// 事件时间戳（Unix 秒）。
    pub ts: i64,
}

/// 幂等合并决策。
#[derive(Debug, PartialEq)]
pub enum MergeDecision {
    /// 本地无此条目或事件版本更高：覆盖为事件携带的条目。
    Overwrite(Box<ShareItem>),
    /// 事件重复（同 version 重放）：忽略，不产生第二条记录。
    IgnoreDuplicate,
    /// 事件版本更旧：丢弃。
    IgnoreStale,
}

/// 幂等合并：None(本地无) 或 version 较高 → Overwrite；event 重复/旧版 → Ignore*。
///
/// 例外（接口修订 #5①，PRD 7.4 删除同步）：同 version 但事件携带删除意图
/// （`deleted=true`）而本地未删 → Overwrite——教师删除不加 version，若按重放
/// 忽略将使删除永远无法在学员端落地；本地已是墓碑的重复 delete 仍幂等忽略。
pub fn merge(local: Option<&ShareItem>, ev: &Event) -> MergeDecision {
    let Some(local) = local else {
        return MergeDecision::Overwrite(Box::new(ev.item.clone()));
    };
    match ev.item.version.cmp(&local.version) {
        Ordering::Greater => MergeDecision::Overwrite(Box::new(ev.item.clone())),
        Ordering::Less => MergeDecision::IgnoreStale,
        Ordering::Equal => {
            if ev.item.deleted && !local.deleted {
                MergeDecision::Overwrite(Box::new(ev.item.clone()))
            } else {
                MergeDecision::IgnoreDuplicate
            }
        }
    }
}
