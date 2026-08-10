use std::io::{self, Write};
use std::time::Duration;

use crossterm::{
    cursor,
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

/// 一次输入：键盘或鼠标。鼠标是与键盘**并行**的可发现层，不是替代——
/// 老玩家继续用热键，新玩家用鼠标就不必先背快捷键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Key(KeyCode),
    Mouse(Mouse),
}

/// 鼠标事件。坐标是**字符格**（0 基），不是像素——
/// 半块渲染下一格 = 上下两个像素，所以纵向精度只到格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mouse {
    pub col: u16,
    pub row: u16,
    pub kind: MouseKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    LeftDown,
    LeftUp,
    LeftDrag,
    RightDown,
    Moved,
    ScrollUp,
    ScrollDown,
}

/// 终端会话守卫：进入备用屏幕 + raw mode + 隐藏光标，
/// Drop 时无论正常退出还是 panic 展开都恢复终端状态（含鼠标捕获）。
pub struct TerminalSession {
    out: io::Stdout,
    mouse: bool,
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
        Ok(TerminalSession { out, mouse: false })
    }

    /// 打开鼠标捕获。Drop 时自动关闭——绝不能让程序崩了把终端卡在鼠标模式。
    ///
    /// 注意：捕获期间终端自带的「拖选复制文本」被接管，用户按住 Shift 可绕过
    /// （终端通用惯例，不需要我们处理，提示一句即可）。
    pub fn enable_mouse(&mut self) -> io::Result<()> {
        if !self.mouse {
            execute!(self.out, EnableMouseCapture)?;
            self.mouse = true;
        }
        Ok(())
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

    /// 非阻塞轮询一个输入事件（键盘或鼠标），超时返回 None。
    /// Ctrl+C 同样统一映射为 `KeyCode::Esc`。
    pub fn poll_input(timeout: Duration) -> io::Result<Option<Input>> {
        if !event::poll(timeout)? {
            return Ok(None);
        }
        Ok(match event::read()? {
            Event::Key(KeyEvent {
                code,
                modifiers,
                kind,
                ..
            }) => {
                if kind == KeyEventKind::Release {
                    None
                } else if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
                    Some(Input::Key(KeyCode::Esc))
                } else {
                    Some(Input::Key(code))
                }
            }
            Event::Mouse(m) => {
                let kind = match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => Some(MouseKind::LeftDown),
                    MouseEventKind::Up(MouseButton::Left) => Some(MouseKind::LeftUp),
                    MouseEventKind::Drag(MouseButton::Left) => Some(MouseKind::LeftDrag),
                    MouseEventKind::Down(MouseButton::Right) => Some(MouseKind::RightDown),
                    MouseEventKind::Moved => Some(MouseKind::Moved),
                    MouseEventKind::ScrollUp => Some(MouseKind::ScrollUp),
                    MouseEventKind::ScrollDown => Some(MouseKind::ScrollDown),
                    _ => None,
                };
                kind.map(|kind| {
                    Input::Mouse(Mouse {
                        col: m.column,
                        row: m.row,
                        kind,
                    })
                })
            }
            _ => None,
        })
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.mouse {
            let _ = execute!(self.out, DisableMouseCapture);
        }
        let _ = execute!(self.out, cursor::Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
        let _ = self.out.flush();
    }
}
