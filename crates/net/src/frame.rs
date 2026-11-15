//! TCP 帧协议：`4B 大端长度 + 1B 类型 + payload`，帧上限 [`MAX_FRAME_SIZE`] 防滥用。
//!
//! 控制帧（Hello/HelloReply/Event/Manifest/Ping/Pong/FileHeader/FileEnd/FileAck）
//! payload 为 JSON；`FileChunk` 为二进制帧（计划注明「≤1MB 二进制帧」），
//! payload 布局：`4B item_id 长度(大端) + item_id UTF-8 + 8B 大端 seq + 原始字节`，
//! 避免 JSON 字节数组带来的 ~4 倍膨胀（PRD 9.1 分发性能）。

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

use classroom_protocol::{Event, Role};

/// 单帧上限：8 MB（长度字段含类型字节与 payload）。
///
/// 不变量：1MB 分块（[`FileChunk`]）编码后的整帧必须远低于本上限，
/// 由 `chunk_1mb_encodes_under_frame_cap` 测试守护。
pub const MAX_FRAME_SIZE: u32 = 8 * 1024 * 1024;

/// 类型字节。
mod frame_type {
    pub const HELLO: u8 = 0;
    pub const HELLO_REPLY: u8 = 1;
    pub const EVENT: u8 = 2;
    pub const FILE_HEADER: u8 = 3;
    pub const FILE_CHUNK: u8 = 4;
    pub const FILE_END: u8 = 5;
    pub const FILE_ACK: u8 = 6;
    pub const MANIFEST_REQ: u8 = 7;
    pub const MANIFEST: u8 = 8;
    pub const PING: u8 = 9;
    pub const PONG: u8 = 10;
}

/// 加入请求（学员 → 教师）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Hello {
    /// 携带协议版本、昵称、角色与 PIN。
    Join {
        /// 协议版本，不匹配则被拒（PRD 4.5）。
        protocol_version: u32,
        /// 学员昵称（教师端查重）。
        nickname: String,
        /// 角色。
        role: Role,
        /// 房间 PIN（未开启为 None）。
        pin: Option<String>,
    },
}

/// 加入应答（教师 → 学员）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum HelloReply {
    /// 成功，分配会话 id。
    Ok {
        /// 会话标识。
        session_id: String,
    },
    /// 失败原因。
    Err {
        /// 错误码。
        code: HelloErr,
    },
}

/// 加入失败原因。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum HelloErr {
    /// PIN 错误。
    WrongPin,
    /// 昵称重复（PRD 8.3：该昵称已被使用，请换一个）。
    DupNickname,
    /// 昵称不可用（Windows/NTFS 保留设备名等；PR #30 检查点）。
    /// 独立于 DupNickname：语义是「昵称本身不可用」而非「已被使用」，
    /// GUI 文案区分（PRD 8.3 风格：该昵称不可用，请换一个）。
    InvalidNickname,
    /// 协议版本不匹配。
    VersionMismatch,
}

/// 文件传输头（分块传输前发送）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileHeader {
    /// 条目 id。
    pub item_id: String,
    /// 净化后文件名。
    pub file_name: String,
    /// 总字节数。
    pub size: u64,
    /// 整体 SHA-256（hex）。
    pub sha256: String,
}

/// 文件分块（≤1MB 二进制帧）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileChunk {
    /// 条目 id。
    pub item_id: String,
    /// 块序号（从 0 起）。
    pub seq: u64,
    /// 原始字节。
    pub bytes: Vec<u8>,
}

/// 文件传输结束。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileEnd {
    /// 条目 id。
    pub item_id: String,
}

/// 文件传输确认（接收端校验整体 SHA-256 后回）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileAck {
    /// 条目 id。
    pub item_id: String,
    /// 哈希是否校验通过。
    pub sha256_ok: bool,
}

/// TCP 帧枚举。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Frame {
    /// 加入请求。
    Hello(Hello),
    /// 加入应答。
    HelloReply(HelloReply),
    /// 同步事件（upsert/delete/rename/ack）。
    Event(Event),
    /// 文件传输头。
    FileHeader(FileHeader),
    /// 文件分块（二进制帧）。
    FileChunk(FileChunk),
    /// 文件传输结束。
    FileEnd(FileEnd),
    /// 文件传输确认。
    FileAck(FileAck),
    /// 请求版本清单（重连对齐，PRD 7.5）。
    ManifestReq,
    /// 版本清单：(id, version, deleted)。
    Manifest(Vec<(String, u64, bool)>),
    /// 心跳。
    Ping,
    /// 心跳应答。
    Pong,
}

/// 编码一帧（4B 大端长度(含类型字节) + 1B 类型 + payload）。
pub fn encode_frame(f: &Frame) -> Vec<u8> {
    let (type_byte, payload) = encode_payload(f);
    let len = (payload.len() + 1) as u32;
    let mut out = Vec::with_capacity(5 + payload.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.push(type_byte);
    out.extend_from_slice(&payload);
    out
}

/// 从异步读取流读一帧（先读 4B 长度与帧体，再按类型解码）。
///
/// 超过 [`MAX_FRAME_SIZE`]、类型未知或负载损坏时返回 Err。
pub async fn read_frame<R: AsyncReadExt + Unpin>(r: &mut R) -> anyhow::Result<Frame> {
    let mut head = [0u8; 4];
    r.read_exact(&mut head).await.context("读帧长度失败")?;
    let len = u32::from_be_bytes(head);
    if len == 0 || len > MAX_FRAME_SIZE {
        return Err(anyhow!("帧长度 {len} 非法（上限 {MAX_FRAME_SIZE}）"));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await.context("读帧体失败")?;
    decode_payload(body[0], &body[1..])
}

fn json_payload<T: Serialize>(v: &T) -> Vec<u8> {
    serde_json::to_vec(v).expect("帧 payload 序列化不会失败")
}

fn encode_payload(f: &Frame) -> (u8, Vec<u8>) {
    match f {
        Frame::Hello(h) => (frame_type::HELLO, json_payload(h)),
        Frame::HelloReply(r) => (frame_type::HELLO_REPLY, json_payload(r)),
        Frame::Event(e) => (frame_type::EVENT, json_payload(e)),
        Frame::FileHeader(h) => (frame_type::FILE_HEADER, json_payload(h)),
        Frame::FileChunk(c) => (frame_type::FILE_CHUNK, encode_chunk_payload(c)),
        Frame::FileEnd(e) => (frame_type::FILE_END, json_payload(e)),
        Frame::FileAck(a) => (frame_type::FILE_ACK, json_payload(a)),
        Frame::ManifestReq => (frame_type::MANIFEST_REQ, Vec::new()),
        Frame::Manifest(m) => (frame_type::MANIFEST, json_payload(m)),
        Frame::Ping => (frame_type::PING, Vec::new()),
        Frame::Pong => (frame_type::PONG, Vec::new()),
    }
}

fn decode_payload(type_byte: u8, payload: &[u8]) -> anyhow::Result<Frame> {
    Ok(match type_byte {
        t if t == frame_type::HELLO => Frame::Hello(serde_json::from_slice(payload)?),
        t if t == frame_type::HELLO_REPLY => Frame::HelloReply(serde_json::from_slice(payload)?),
        t if t == frame_type::EVENT => Frame::Event(serde_json::from_slice(payload)?),
        t if t == frame_type::FILE_HEADER => Frame::FileHeader(serde_json::from_slice(payload)?),
        t if t == frame_type::FILE_CHUNK => Frame::FileChunk(decode_chunk_payload(payload)?),
        t if t == frame_type::FILE_END => Frame::FileEnd(serde_json::from_slice(payload)?),
        t if t == frame_type::FILE_ACK => Frame::FileAck(serde_json::from_slice(payload)?),
        t if t == frame_type::MANIFEST_REQ => Frame::ManifestReq,
        t if t == frame_type::MANIFEST => Frame::Manifest(serde_json::from_slice(payload)?),
        t if t == frame_type::PING => Frame::Ping,
        t if t == frame_type::PONG => Frame::Pong,
        t => return Err(anyhow!("未知帧类型字节 {t:#04x}")),
    })
}

/// FileChunk 二进制负载：`4B item_id 长度 + item_id + 8B seq + 原始字节`。
fn encode_chunk_payload(c: &FileChunk) -> Vec<u8> {
    let mut p = Vec::with_capacity(4 + c.item_id.len() + 8 + c.bytes.len());
    p.extend_from_slice(&(c.item_id.len() as u32).to_be_bytes());
    p.extend_from_slice(c.item_id.as_bytes());
    p.extend_from_slice(&c.seq.to_be_bytes());
    p.extend_from_slice(&c.bytes);
    p
}

fn decode_chunk_payload(p: &[u8]) -> anyhow::Result<FileChunk> {
    if p.len() < 12 {
        return Err(anyhow!("FileChunk 负载过短（{} 字节）", p.len()));
    }
    let id_len = u32::from_be_bytes(p[..4].try_into()?) as usize;
    if p.len() < 4 + id_len + 8 {
        return Err(anyhow!("FileChunk 负载长度与 item_id 长度不符"));
    }
    let item_id = std::str::from_utf8(&p[4..4 + id_len])?.to_string();
    let seq = u64::from_be_bytes(p[4 + id_len..4 + id_len + 8].try_into()?);
    let bytes = p[4 + id_len + 8..].to_vec();
    Ok(FileChunk {
        item_id,
        seq,
        bytes,
    })
}
