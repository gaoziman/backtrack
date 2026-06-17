//! 监听 ~/.claude 与 ~/.codex，防抖后触发增量索引并通知前端。
//!
//! 设计：notify 递归监听 → mpsc 通道 → 常驻防抖线程吸收事件风暴 →
//! 静默期结束触发现成的增量 `build_index` → emit `index-updated` 事件。
//! 仅纯逻辑（`is_relevant_path` / `coalesce_triggers`）做单测，
//! notify/线程/Tauri 运行时胶水不测（同 terminal.rs 惯例）。
use crate::commands::AppState;
use crate::indexer::build_index;
use notify::{RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// 防抖窗口：高频追加（Claude 每轮写入）合并为一次刷新。
const DEBOUNCE: Duration = Duration::from_millis(1000);

/// 仅 `.jsonl` 文件事件才值得触发重索引（纯函数，可测）。
pub fn is_relevant_path(path: &Path) -> bool {
    path.extension().map(|e| e == "jsonl").unwrap_or(false)
}

/// 把一串带时间戳(ms)的相关事件按防抖窗口合并为「触发次数」。
/// 语义：相邻间隔 ≤ window 的连续事件合并为 1 次触发；间隔 > window 断开为新一段。
///
/// 这是下方 `spawn_watcher` 防抖循环（`recv_timeout(DEBOUNCE)`）行为的**可执行规格**：
/// 一段连续写入风暴只产生一次重索引。生产路径用阻塞 `recv_timeout` 实现等价语义，
/// 故本函数仅作为该不变量的单测锚点（不在生产路径调用）。
#[allow(dead_code)]
pub fn coalesce_triggers(event_times_ms: &[i64], window_ms: i64) -> usize {
    if event_times_ms.is_empty() {
        return 0;
    }
    let mut triggers = 1;
    for w in event_times_ms.windows(2) {
        if w[1] - w[0] > window_ms {
            triggers += 1;
        }
    }
    triggers
}

/// 判断一个 notify 事件是否值得触发重索引。
/// - 增/改：路径须命中 `.jsonl`；
/// - 删：放宽不强制 `.jsonl`——`rm -rf` 整个项目目录时 notify 上报的是目录路径
///   （无扩展名），若强制 `.jsonl` 会漏掉删除同步。`build_index` 是增量的，
///   偶尔多触发一次成本极低。
fn relevant(ev: notify::Result<notify::Event>) -> bool {
    let e = match ev {
        Ok(e) => e,
        Err(_) => return false,
    };
    let k = e.kind;
    if k.is_remove() {
        return true;
    }
    (k.is_create() || k.is_modify()) && e.paths.iter().any(|p| is_relevant_path(p))
}

/// 启动监听（在 Tauri setup 中调用，move AppHandle 进常驻线程）。
/// 失败时打印日志并返回，主功能不受影响（仍可手动刷新）。
pub fn spawn_watcher(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[watcher] 初始化失败，降级为手动刷新: {e}");
            return;
        }
    };

    // 仅监听存在的根目录（~/.codex 可能不存在）。
    {
        let state = app.state::<AppState>();
        for root in [state.claude_root.clone(), state.codex_root.clone()] {
            if root.exists() {
                if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
                    eprintln!("[watcher] watch {root:?} 失败: {e}");
                }
            }
        }
    }

    // 常驻防抖线程：watcher 一并 move 入内以保持存活（drop 即停止监听）。
    std::thread::spawn(move || {
        let _keep = watcher;
        loop {
            // 阻塞等首个事件
            let first = match rx.recv() {
                Ok(ev) => ev,
                Err(_) => break, // 发送端断开 → 退出
            };
            let mut pending = relevant(first);
            // 进入防抖窗口，吸收后续事件直到静默
            loop {
                match rx.recv_timeout(DEBOUNCE) {
                    Ok(ev) => pending |= relevant(ev),
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            if pending {
                reindex_and_emit(&app);
            }
        }
    });
}

/// 锁库 → 增量重索引 → 通知前端静默刷新。
fn reindex_and_emit(app: &AppHandle) {
    let state = app.state::<AppState>();
    let summary = {
        let store = match state.store.lock() {
            Ok(s) => s,
            Err(_) => return, // poisoned：跳过本次，不 panic
        };
        build_index(&store, &state.claude_root, &state.codex_root)
    };
    let _ = app.emit("index-updated", summary);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn jsonl_is_relevant() {
        assert!(is_relevant_path(Path::new("/a/b/x.jsonl")));
    }

    #[test]
    fn non_jsonl_is_irrelevant() {
        assert!(!is_relevant_path(Path::new("/a/b/x.txt")));
        assert!(!is_relevant_path(Path::new("/a/b/noext")));
    }

    #[test]
    fn coalesce_empty_is_zero() {
        assert_eq!(coalesce_triggers(&[], 1000), 0);
    }

    #[test]
    fn coalesce_single_burst_is_one() {
        // 相邻间隔均 ≤ 1000 → 合并为 1 次
        assert_eq!(coalesce_triggers(&[0, 100, 200, 900], 1000), 1);
    }

    #[test]
    fn coalesce_splits_on_gap() {
        // 间隔 100,200,2000(>1000 断开),100 → 2 段
        assert_eq!(coalesce_triggers(&[0, 100, 300, 2300, 2400], 1000), 2);
    }
}
