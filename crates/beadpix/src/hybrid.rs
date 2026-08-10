use std::io::{self, Write};

use crate::canvas::Canvas;
use crate::color::Rgb;

/// 混合渲染器：每个字符格覆盖 2×4 像素，逐格在两种表示之间择优——
///
/// - **半块 `▀`**：上半 2×2 平均色作前景、下半 2×2 平均色作背景。
///   色彩保真高（两种独立真彩色），但空间细节被平均掉。
/// - **盲文点阵 `⠁..⣿`**：按亮度把 8 个像素分成明/暗两组，明组像素点亮。
///   空间细节是半块的 4 倍，但整格只有前景/背景两种颜色。
///
/// 判据：分别计算两种表示重建原像素的色彩误差，谁小用谁。
/// 效果是平滑渐变区域（天空、皮肤）自动走半块，高对比轮廓
/// （描边、睫毛、花瓣边缘）自动走盲文。
///
/// 画布尺寸要求：宽 = 终端列数 × 2，高 = 行数 × 4。
pub struct HybridRenderer {
    cols: u32,
    rows: u32,
    /// (字形码点, 前景, 背景)；字形 0x2580 = '▀'，0x2800+bits = 盲文
    prev: Vec<Option<(u16, Rgb, Rgb)>>,
    buf: String,
}

/// 盲文点位：(x, y) -> bit。Unicode 布局为 1-2-3-7 左列、4-5-6-8 右列。
const DOT_BITS: [[u16; 4]; 2] = [
    [0x01, 0x02, 0x04, 0x40], // x = 0
    [0x08, 0x10, 0x20, 0x80], // x = 1
];

fn luma(c: Rgb) -> f32 {
    0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32
}

fn dist2(a: Rgb, b: Rgb) -> f32 {
    let dr = a.r as f32 - b.r as f32;
    let dg = a.g as f32 - b.g as f32;
    let db = a.b as f32 - b.b as f32;
    dr * dr + dg * dg + db * db
}

fn avg(pixels: &[Rgb]) -> Rgb {
    if pixels.is_empty() {
        return Rgb::BLACK;
    }
    let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
    for p in pixels {
        r += p.r as u32;
        g += p.g as u32;
        b += p.b as u32;
    }
    let n = pixels.len() as u32;
    Rgb::new((r / n) as u8, (g / n) as u8, (b / n) as u8)
}

impl HybridRenderer {
    pub fn new(canvas_width: u32, canvas_height: u32) -> Self {
        let cols = canvas_width / 2;
        let rows = canvas_height / 4;
        HybridRenderer {
            cols,
            rows,
            prev: vec![None; (cols * rows) as usize],
            buf: String::with_capacity((cols * rows * 28) as usize),
        }
    }

    pub fn invalidate(&mut self) {
        self.prev.fill(None);
    }

    /// 为一个 2×4 像素块选出最优 (字形, 前景, 背景)。
    fn choose_cell(block: &[Rgb; 8]) -> (u16, Rgb, Rgb) {
        // 方案 A：半块。block 索引 = y*2+x，前 4 个是上半，后 4 个是下半
        let top = avg(&block[0..4]);
        let bottom = avg(&block[4..8]);
        let err_half: f32 = block[0..4].iter().map(|p| dist2(*p, top)).sum::<f32>()
            + block[4..8].iter().map(|p| dist2(*p, bottom)).sum::<f32>();

        // 方案 B：盲文。按亮度中值分成明/暗两组
        let lums: Vec<f32> = block.iter().map(|p| luma(*p)).collect();
        let min = lums.iter().cloned().fold(f32::MAX, f32::min);
        let max = lums.iter().cloned().fold(f32::MIN, f32::max);
        let mid = (min + max) / 2.0;

        let mut on = Vec::with_capacity(8);
        let mut off = Vec::with_capacity(8);
        let mut bits: u16 = 0;
        for y in 0..4 {
            for x in 0..2 {
                let p = block[y * 2 + x];
                if luma(p) > mid {
                    on.push(p);
                    bits |= DOT_BITS[x][y];
                } else {
                    off.push(p);
                }
            }
        }
        let fg = avg(&on);
        let bg = avg(&off);
        let err_braille: f32 = on.iter().map(|p| dist2(*p, fg)).sum::<f32>()
            + off.iter().map(|p| dist2(*p, bg)).sum::<f32>();

        // 平局偏向半块（色彩保真优先）
        if err_half <= err_braille || on.is_empty() || off.is_empty() {
            (0x2580, top, bottom) // '▀'
        } else {
            (0x2800 + bits, fg, bg)
        }
    }

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
        let mut cursor: Option<(u32, u32)> = None;

        for row in 0..self.rows {
            for col in 0..self.cols {
                let mut block = [Rgb::BLACK; 8];
                for y in 0..4u32 {
                    for x in 0..2u32 {
                        block[(y * 2 + x) as usize] =
                            pixels[((row * 4 + y) * w + col * 2 + x) as usize];
                    }
                }
                let cell = Self::choose_cell(&block);
                let idx = (row * self.cols + col) as usize;

                if self.prev[idx] == Some(cell) {
                    continue;
                }
                self.prev[idx] = Some(cell);

                if cursor != Some((col, row)) {
                    self.buf.push_str(&format!(
                        "\x1b[{};{}H",
                        origin_row as u32 + row,
                        origin_col as u32 + col
                    ));
                }
                let (glyph, fg, bg) = cell;
                if last_fg != Some(fg) {
                    self.buf
                        .push_str(&format!("\x1b[38;2;{};{};{}m", fg.r, fg.g, fg.b));
                    last_fg = Some(fg);
                }
                if last_bg != Some(bg) {
                    self.buf
                        .push_str(&format!("\x1b[48;2;{};{};{}m", bg.r, bg.g, bg.b));
                    last_bg = Some(bg);
                }
                self.buf
                    .push(char::from_u32(glyph as u32).unwrap_or('▀'));
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
