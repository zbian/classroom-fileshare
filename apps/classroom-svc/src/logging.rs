//! 日志初始化：tracing 双写（文件 + stderr），默认 RFC3339 亚秒级时间戳
//! （含毫秒，取证关键行要求）。按天切分在 M3 完善（issue #14）。

use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use tracing_subscriber::fmt::MakeWriter;

/// 初始化全局日志：写入 `<logs_dir>/svc.log` 并镜像到 stderr
/// （服务模式 stderr 由 SCM 丢弃；--console 模式可见）。
/// 过滤级别取 `RUST_LOG`，默认 info。
pub fn init(logs_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(logs_dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join("svc.log"))?;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(Tee {
            file: Mutex::new(file),
        })
        .init();
    Ok(())
}

/// 文件 + stderr 双写。
struct Tee {
    file: Mutex<std::fs::File>,
}

impl Write for &Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.file.lock().unwrap().write_all(buf)?;
        std::io::stderr().write_all(buf)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.lock().unwrap().flush()
    }
}

impl<'a> MakeWriter<'a> for Tee {
    type Writer = &'a Tee;
    fn make_writer(&'a self) -> Self::Writer {
        self
    }
}
