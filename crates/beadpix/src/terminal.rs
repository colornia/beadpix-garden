use std::io::{self, Write};
use std::time::Duration;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

/// 终端会话守卫：进入备用屏幕 + raw mode + 隐藏光标，
/// Drop 时无论正常退出还是 panic 都恢复终端状态。
pub struct TerminalSession {
    out: io::Stdout,
}

impl TerminalSession {
    pub fn begin() -> io::Result<Self> {
        // Windows 控制台默认代码页不是 UTF-8，中文和 Unicode 字符会乱码
        #[cfg(windows)]
        unsafe {
            #[link(name = "kernel32")]
            extern "system" {
                fn SetConsoleOutputCP(code_page_id: u32) -> i32;
            }
            SetConsoleOutputCP(65001);
        }
        let mut out = io::stdout();
        terminal::enable_raw_mode()?;
        execute!(out, EnterAlternateScreen, cursor::Hide)?;
        Ok(TerminalSession { out })
    }

    pub fn out(&mut self) -> &mut io::Stdout {
        &mut self.out
    }

    /// 终端窗口的 (列数, 行数)。
    pub fn size() -> io::Result<(u16, u16)> {
        terminal::size()
    }

    /// 非阻塞轮询一个按键事件，超时返回 None。
    /// Ctrl+C 统一映射为 `Some(KeyCode::Esc)`，调用方只需处理 Esc 退出。
    pub fn poll_key(timeout: Duration) -> io::Result<Option<KeyCode>> {
        if !event::poll(timeout)? {
            return Ok(None);
        }
        match event::read()? {
            Event::Key(KeyEvent {
                code,
                modifiers,
                kind,
                ..
            }) => {
                // Windows 上按下和松开各来一次事件，只响应按下（含长按重复）
                if kind == KeyEventKind::Release {
                    return Ok(None);
                }
                if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
                    Ok(Some(KeyCode::Esc))
                } else {
                    Ok(Some(code))
                }
            }
            _ => Ok(None),
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = execute!(self.out, cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
        let _ = self.out.flush();
    }
}
