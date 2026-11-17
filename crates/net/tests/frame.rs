//! Task 4（net）帧协议测试：全枚举往返、二进制分块、超限与坏帧拒绝。

use classroom_net::{
    encode_frame, read_frame, FileAck, FileChunk, FileEnd, FileHeader, Frame, Hello, HelloErr,
    HelloReply, MAX_FRAME_SIZE,
};
use classroom_protocol::{
    Audience, Event, EventKind, ItemKind, Role, ShareItem, SyncState, PROTOCOL_VERSION,
};

fn sample_event(title: &str) -> Event {
    Event {
        event_id: format!("ev-{title}"),
        kind: EventKind::Upsert,
        item: ShareItem {
            id: format!("id-{title}"),
            kind: ItemKind::File,
            owner_id: "teacher".into(),
            owner_role: Role::Teacher,
            audience: Audience::All,
            title: title.into(),
            size: 42,
            sha256: Some("ab".repeat(32)),
            rel_path: Some("teacher/课件.pdf".into()),
            created_at: 1000,
            updated_at: 1000,
            version: 1,
            deleted: false,
            sync_state: SyncState::Synced,
            retry_count: 0,
        },
        new_name: None,
        acked_by: None,
        ts: 1234,
    }
}

fn hello(pin: Option<&str>) -> Frame {
    Frame::Hello(Hello::Join {
        protocol_version: PROTOCOL_VERSION,
        nickname: "alice".into(),
        role: Role::Student,
        pin: pin.map(str::to_string),
    })
}

/// 覆盖全部 12 种帧变体的构造清单。
fn all_frames() -> Vec<Frame> {
    vec![
        hello(None),
        Frame::HelloReply(HelloReply::Ok {
            session_id: "s-1".into(),
        }),
        Frame::HelloReply(HelloReply::Err {
            code: HelloErr::WrongPin,
        }),
        Frame::Event(sample_event("课件.pdf")),
        Frame::FileHeader(FileHeader {
            item_id: "id-1".into(),
            file_name: "课件.pdf".into(),
            size: 10,
            sha256: "cd".repeat(32),
        }),
        Frame::FileChunk(FileChunk {
            item_id: "id-1".into(),
            seq: 3,
            bytes: vec![0u8, 1, 2, 255, 254],
        }),
        Frame::FileEnd(FileEnd {
            item_id: "id-1".into(),
        }),
        Frame::FileAck(FileAck {
            item_id: "id-1".into(),
            sha256_ok: true,
        }),
        Frame::ManifestReq,
        Frame::Manifest(vec![("a".into(), 1, false), ("b".into(), 3, true)]),
        Frame::Ping,
        Frame::Pong,
    ]
}

#[tokio::test]
async fn frame_roundtrip_all_variants() {
    for f in all_frames() {
        let enc = encode_frame(&f);
        let mut r: &[u8] = &enc;
        let back = read_frame(&mut r)
            .await
            .unwrap_or_else(|e| panic!("{f:?} 往返失败: {e}"));
        assert_eq!(back, f, "往返不一致");
        // 读完后流应耗尽（无粘包残留）
        assert!(r.is_empty(), "帧解码后残留 {} 字节", r.len());
    }
}

#[tokio::test]
async fn frame_header_layout_is_len_plus_type() {
    // 帧头：4B 大端长度（1B 类型 + payload）+ 1B 类型
    let enc = encode_frame(&Frame::Ping);
    assert_eq!(enc.len(), 5, "Ping 帧应为 4B 长度 + 1B 类型");
    assert_eq!(&enc[..4], &[0, 0, 0, 1], "Ping 帧长度字段应为 1");
}

#[tokio::test]
async fn chunk_1mb_encodes_under_frame_cap() {
    // lead 要求的不变量：1MB 分块（payload + 头）必须远低于 8MB 帧上限，
    // 防止后续调参（分块大小/帧上限）无意突破。
    let chunk = FileChunk {
        item_id: "id-1".into(),
        seq: 0,
        bytes: vec![0xAB; 1024 * 1024],
    };
    let enc = encode_frame(&Frame::FileChunk(chunk.clone()));
    assert!(
        enc.len() < MAX_FRAME_SIZE as usize,
        "1MB 分块编码后 {} 字节，必须 < 帧上限 {}",
        enc.len(),
        MAX_FRAME_SIZE
    );
    let mut r: &[u8] = &enc;
    assert_eq!(read_frame(&mut r).await.unwrap(), Frame::FileChunk(chunk));
}

#[tokio::test]
async fn oversized_frame_rejected() {
    // 声明长度超过 8MB 上限：在读 payload 之前即拒绝
    let len = (MAX_FRAME_SIZE + 1).to_be_bytes();
    let mut stream = Vec::new();
    stream.extend_from_slice(&len);
    stream.push(0x02); // 类型字节（内容无关，长度检查在前）
    stream.extend_from_slice(&[0u8; 8]);
    let mut r: &[u8] = &stream;
    assert!(read_frame(&mut r).await.is_err(), "超限帧必须被拒绝");
}

#[tokio::test]
async fn unknown_type_byte_rejected() {
    let mut stream = Vec::new();
    stream.extend_from_slice(&2u32.to_be_bytes()); // 长度 = 1B 类型 + 1B payload
    stream.push(0xFF); // 未知类型
    stream.push(0x00);
    let mut r: &[u8] = &stream;
    assert!(read_frame(&mut r).await.is_err(), "未知类型字节必须被拒绝");
}

#[tokio::test]
async fn truncated_frame_is_error() {
    // 只给长度头、不给 payload：读不满应报错而非挂死
    let enc = encode_frame(&Frame::Event(sample_event("通知")));
    let mut r: &[u8] = &enc[..6];
    assert!(read_frame(&mut r).await.is_err());
}
