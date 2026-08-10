use std::io::{self, Write};

use crate::canvas::Canvas;
use crate::color::Rgb;

/// 一个终端字符格 = 上下两个像素（`▀` 前景色为上像素、背景色为下像素）。
#[derive(Clone, Copy, PartialEq, Eq)]
struct Cell {
    top: Rgb,
    bottom: Rgb,
}

/// 半块差分渲染器。
///
/// 每帧把 Canvas 压缩成字符格网格，与上一帧比较，只对发生变化的格子
/// 输出光标移动 + 颜色转义序列。静态画面几乎零输出，局部动画（眨眼、
/// 雨滴）只花变化区域的带宽，这是终端里做高帧率的关键。
pub struct HalfBlockRenderer {
    cols: u32,
    rows: u32,
    prev: Vec<Option<Cell>>, // None = 该格子从未画过，强制输出
    buf: String,             // 复用的输出缓冲，避免每帧分配
}

impl HalfBlockRenderer {
    /// canvas 高度应为偶数；奇数时最后一行像素会被舍弃。
    pub fn new(canvas_width: u32, canvas_height: u32) -> Self {
        let cols = canvas_width;
        let rows = canvas_height / 2;
        HalfBlockRenderer {
            cols,
            rows,
            prev: vec![None; (cols * rows) as usize],
            buf: String::with_capacity((cols * rows * 24) as usize),
        }
    }

    /// 丢弃缓存，下一帧全量重绘（终端尺寸变化或首帧时用）。
    pub fn invalidate(&mut self) {
        self.prev.fill(None);
    }

    /// 把 canvas 当前内容差分输出到终端。
    /// `origin_col`/`origin_row` 是画面在终端里的左上角字符坐标（1-based）。
    pub fn present(
        &mut self,
        canvas: &Canvas,
        out: &mut impl Write,
        origin_col: u16,
        origin_row: u16,
    ) -> io::Result<()> {
        self.buf.clear();
        let pixels = canvas.pixels();
        let w = canvas.width();

        let mut last_fg: Option<Rgb> = None;
        let mut last_bg: Option<Rgb> = None;
        // 光标当前应在的位置；None 表示需要重新定位
        let mut cursor: Option<(u32, u32)> = None;

        for row in 0..self.rows {
            for col in 0..self.cols {
                let top = pixels[(row * 2 * w + col) as usize];
                let bottom = pixels[((row * 2 + 1) * w + col) as usize];
                let cell = Cell { top, bottom };
                let idx = (row * self.cols + col) as usize;

                if self.prev[idx] == Some(cell) {
                    continue; // 未变化，跳过
                }
                self.prev[idx] = Some(cell);

                // 光标不在紧邻的上一个输出位置时才移动
                if cursor != Some((col, row)) {
                    self.buf.push_str(&format!(
                        "\x1b[{};{}H",
                        origin_row as u32 + row,
                        origin_col as u32 + col
                    ));
                }

                if last_fg != Some(cell.top) {
                    self.buf.push_str(&format!(
                        "\x1b[38;2;{};{};{}m",
                        cell.top.r, cell.top.g, cell.top.b
                    ));
                    last_fg = Some(cell.top);
                }
                if last_bg != Some(cell.bottom) {
                    self.buf.push_str(&format!(
                        "\x1b[48;2;{};{};{}m",
                        cell.bottom.r, cell.bottom.g, cell.bottom.b
                    ));
                    last_bg = Some(cell.bottom);
                }
                self.buf.push('▀');
                cursor = Some((col + 1, row));
            }
        }

        if !self.buf.is_empty() {
            self.buf.push_str("\x1b[0m");
            out.write_all(self.buf.as_bytes())?;
            out.flush()?;
        }
        Ok(())
    }
}
