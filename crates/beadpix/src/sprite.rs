use crate::canvas::Canvas;
use crate::color::Rgb;

/// 索引色调色板。索引 0 约定为透明。
#[derive(Clone, Debug)]
pub struct Palette {
    colors: Vec<Rgb>,
}

impl Palette {
    pub fn new(colors: Vec<Rgb>) -> Self {
        Palette { colors }
    }

    pub fn get(&self, index: u8) -> Option<Rgb> {
        if index == 0 {
            return None;
        }
        self.colors.get(index as usize - 1).copied()
    }

    /// 替换某个槽位的颜色——同一个花朵精灵换调色板就是另一个品种。
    pub fn set(&mut self, index: u8, color: Rgb) {
        if index == 0 {
            return;
        }
        if let Some(slot) = self.colors.get_mut(index as usize - 1) {
            *slot = color;
        }
    }
}

/// 拼豆风格的索引色精灵。用字符串字面量定义像素图，直观可编辑：
///
/// ```
/// use beadpix::{Sprite, Palette, Rgb};
/// let heart = Sprite::from_ascii(&[
///     ".1.1.",
///     "11111",
///     ".111.",
///     "..1..",
/// ]);
/// ```
/// `.` 为透明，`1`-`9`、`a`-`z` 映射到调色板索引 1-35。
#[derive(Clone, Debug)]
pub struct Sprite {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Sprite {
    pub fn from_ascii(rows: &[&str]) -> Self {
        let height = rows.len() as u32;
        let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0) as u32;
        let mut data = vec![0u8; (width * height) as usize];
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                data[y * width as usize + x] = match ch {
                    '.' | ' ' => 0,
                    '1'..='9' => ch as u8 - b'0',
                    'a'..='z' => ch as u8 - b'a' + 10,
                    _ => 0,
                };
            }
        }
        Sprite {
            width,
            height,
            data,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// 以 (x, y) 为左上角绘制到画布，透明像素跳过。
    pub fn draw(&self, canvas: &mut Canvas, x: i32, y: i32, palette: &Palette) {
        for py in 0..self.height {
            for px in 0..self.width {
                let idx = self.data[(py * self.width + px) as usize];
                if let Some(color) = palette.get(idx) {
                    canvas.set(x + px as i32, y + py as i32, color);
                }
            }
        }
    }

    /// 水平翻转绘制（人脸左右眼共用一张精灵图）。
    pub fn draw_flipped(&self, canvas: &mut Canvas, x: i32, y: i32, palette: &Palette) {
        for py in 0..self.height {
            for px in 0..self.width {
                let idx = self.data[(py * self.width + px) as usize];
                if let Some(color) = palette.get(idx) {
                    canvas.set(x + (self.width - 1 - px) as i32, y + py as i32, color);
                }
            }
        }
    }
}
