//! 房间准入：协议版本与 PIN 校验（PRD 3.3 房间码加 PIN 准入、FR-T-01）。
//! 昵称查重在 hub 连接表侧（需要在线名单）。

use classroom_net::{Hello, HelloErr};
use classroom_protocol::PROTOCOL_VERSION;

pub(crate) struct Admitter {
    pin: Option<String>,
}

impl Admitter {
    pub(crate) fn new(pin: Option<String>) -> Self {
        Self { pin }
    }

    /// 校验协议版本与 PIN；昵称非空校验归学员端（PRD 6.1），此处只做房间级检查。
    pub(crate) fn check(&self, hello: &Hello) -> Result<(), HelloErr> {
        let Hello::Join {
            protocol_version,
            pin,
            ..
        } = hello;
        if *protocol_version != PROTOCOL_VERSION {
            return Err(HelloErr::VersionMismatch);
        }
        match (&self.pin, pin) {
            (Some(want), Some(got)) if want == got => {}
            (None, None) => {}
            _ => return Err(HelloErr::WrongPin),
        }
        Ok(())
    }
}
