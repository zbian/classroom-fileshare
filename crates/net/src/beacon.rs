//! UDP 发现：教师机每 1s 向 255.255.255.255:[`crate::DISCOVERY_PORT`] 广播信标，
//! 学员端监听同一端口发现教师机（PRD 3.1）。

use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::DISCOVERY_PORT;

/// 教师机房间广播信标。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Beacon {
    /// 协议版本。
    pub protocol_version: u32,
    /// 房间码（如 XK-204）。
    pub room: String,
    /// TCP 服务端口。
    pub tcp_port: u16,
    /// 是否需要 PIN。
    pub pin_required: bool,
    /// 教师机名。
    pub host_name: String,
}

/// 广播目标（受限广播地址）。
fn broadcast_addr() -> SocketAddr {
    format!("255.255.255.255:{DISCOVERY_PORT}")
        .parse()
        .expect("合法广播地址")
}

/// 教师端：每 1s 广播房间信标（可关——返回的 JoinHandle abort 即停）。
pub fn spawn_beacon(info: Beacon) -> JoinHandle<()> {
    tokio::spawn(async move {
        let sock = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "beacon socket 绑定失败，停止广播");
                return;
            }
        };
        if let Err(e) = sock.set_broadcast(true) {
            tracing::warn!(error = %e, "beacon 无法开启广播选项");
        }
        let payload = serde_json::to_vec(&info).expect("Beacon 序列化");
        let addr = broadcast_addr();
        tracing::debug!(room = %info.room, %addr, "beacon 启动");
        loop {
            if let Err(e) = sock.send_to(&payload, addr).await {
                tracing::warn!(error = %e, "beacon 发送失败");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}

/// 学员端：监听 [`DISCOVERY_PORT`]，把收到的信标推给 `sink`（解析失败的包忽略）。
pub fn spawn_discovery(sink: mpsc::Sender<Beacon>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let sock = match UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, port = DISCOVERY_PORT, "discovery 绑定失败");
                return;
            }
        };
        tracing::debug!(port = DISCOVERY_PORT, "discovery 监听中");
        let mut buf = vec![0u8; 2048];
        loop {
            match sock.recv_from(&mut buf).await {
                Ok((n, peer)) => match serde_json::from_slice::<Beacon>(&buf[..n]) {
                    Ok(b) => {
                        if sink.send(b).await.is_err() {
                            break; // 接收方已关闭
                        }
                    }
                    Err(_) => tracing::debug!(%peer, "忽略无法解析的 UDP 包"),
                },
                Err(e) => {
                    tracing::warn!(error = %e, "discovery 接收失败，停止监听");
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use classroom_protocol::PROTOCOL_VERSION;

    #[test]
    fn beacon_json_roundtrip() {
        let b = Beacon {
            protocol_version: PROTOCOL_VERSION,
            room: "XK-204".into(),
            tcp_port: 47821,
            pin_required: true,
            host_name: "TEACHER-PC".into(),
        };
        let json = serde_json::to_string(&b).unwrap();
        let back: Beacon = serde_json::from_str(&json).unwrap();
        assert_eq!(back, b);
    }
}
