//! 启动维护：仓库残留 `*.part` 清理（PRD 9.2：崩溃重启后依据索引与
//! 临时文件清理恢复；issue #14 补充评论）。
//!
//! 覆盖两类残留：store 的 `*.docx.part`（覆盖流程中断）与 agent 的
//! `.incoming/<item_id>.part`（下载中断）。

use std::path::Path;

/// 递归删除 `root` 下所有扩展名为 `part` 的文件，返回删除数量。
/// 目录不存在视为无残留（首次启动）。
pub fn sweep_part_files(root: &Path) -> usize {
    let mut removed = 0usize;
    visit(root, &mut removed);
    removed
}

fn visit(dir: &Path, removed: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit(&path, removed);
        } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("part")) {
            if std::fs::remove_file(&path).is_ok() {
                *removed += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweeps_only_part_files_recursively() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join("teacher")).unwrap();
        std::fs::create_dir_all(repo.join(".incoming")).unwrap();
        // 两个 .part 残留 + 正常文件
        std::fs::write(repo.join("teacher/a.docx.part"), b"half").unwrap();
        std::fs::write(repo.join(".incoming/item-1.part"), b"half").unwrap();
        std::fs::write(repo.join("teacher/a.docx"), b"full").unwrap();
        std::fs::write(repo.join("notice.txt"), b"ok").unwrap();

        let removed = sweep_part_files(&repo);
        assert_eq!(removed, 2, "仅删除 .part 残留");
        assert!(repo.join("teacher/a.docx").exists());
        assert!(repo.join("notice.txt").exists());
        assert!(!repo.join("teacher/a.docx.part").exists());
        assert!(!repo.join(".incoming/item-1.part").exists());
    }

    #[test]
    fn missing_repo_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(sweep_part_files(&dir.path().join("no-such-repo")), 0);
    }
}
