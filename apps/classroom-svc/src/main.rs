//! 课堂共享后台服务（Session 0，无 UI）：windows-service 包装 host/agent
//! 核心 + IPC，角色由 svc.json 决定（服务框架在增量 3 接线）。

#[allow(dead_code)] // 增量 2/3 接线
mod config;
#[allow(dead_code)] // 增量 2/3 接线
mod maintenance;

fn main() {
    println!("classroom-svc placeholder");
}
