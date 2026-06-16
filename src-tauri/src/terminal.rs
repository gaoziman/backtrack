//! 唤起终端并执行 `cd <cwd> && <resume cmd>`。
use std::process::Command;

/// 生成在指定终端执行命令的 AppleScript。
pub fn build_resume_script(cwd: &str, cmd: &str, terminal: &str) -> String {
    // 单引号包裹路径，转义其中的单引号。
    let safe_cwd = cwd.replace('\'', "'\\''");
    let full = format!("cd '{}' && {}", safe_cwd, cmd);
    let escaped = full.replace('\\', "\\\\").replace('"', "\\\"");

    match terminal {
        "iTerm" | "iTerm2" => format!(
            "tell application \"iTerm\"\n\
                 activate\n\
                 create window with default profile\n\
                 tell current session of current window to write text \"{}\"\n\
             end tell",
            escaped
        ),
        // Warp 没有稳定的 AppleScript 注入，回退到系统 Terminal。
        "Terminal" | "Warp" | _ => format!(
            "tell application \"Terminal\"\n\
                 activate\n\
                 do script \"{}\"\n\
             end tell",
            escaped
        ),
    }
}

/// 实际执行（调用 osascript）。
pub fn resume_in_terminal(cwd: &str, cmd: &str, terminal: &str) -> Result<(), String> {
    let script = build_resume_script(cwd, cmd, terminal);
    Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .spawn()
        .map_err(|e| format!("无法启动终端: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterm_script_contains_command() {
        let s = build_resume_script("/Users/leo/ai", "claude --resume abc", "iTerm");
        assert!(s.contains("iTerm"));
        assert!(s.contains("cd '/Users/leo/ai' && claude --resume abc"));
        assert!(s.contains("write text"));
    }

    #[test]
    fn terminal_script_uses_do_script() {
        let s = build_resume_script("/p", "codex resume x", "Terminal");
        assert!(s.contains("Terminal"));
        assert!(s.contains("do script"));
    }

    #[test]
    fn warp_falls_back_to_terminal() {
        let s = build_resume_script("/p", "codex resume x", "Warp");
        assert!(s.contains("Terminal"));
    }
}
