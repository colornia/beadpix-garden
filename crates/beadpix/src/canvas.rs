use crate::color::Rgb;

/// 与终端无关的 RGB 像素画布。所有绘制先落到这里，再交给渲染器输出。
pub struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<Rgb>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Canvas {
            width,
            height,
            pixels: vec![Rgb::BLACK; (width * height) as usize],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn clear(&mut self, color: Rgb) {
        self.pixels.fill(color);
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Rgb> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        Some(self.pixels[(y as u32 * self.width + x as u32) as usize])
    }

    /// 画一个像素；越界自动忽略，调用方不用做裁剪。
    pub fn set(&mut self, x: i32, y: i32, color: Rgb) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        self.pixels[(y as u32 * self.width + x as u32) as usize] = color;
    }

    /// 带透明度画像素。
    pub fn blend(&mut self, x: i32, y: i32, color: Rgb, alpha: u8) {
        if let Some(base) = self.get(x, y) {
            self.set(x, y, color.over(base, alpha));
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: Rgb) {
        for dy in 0..h as i32 {
            for dx in 0..w as i32 {
                self.set(x + dx, y + dy, color);
            }
        }
    }

    /// 实心圆（人脸的脸颊、虹膜等大量用到）。
    pub fn fill_circle(&mut self, cx: i32, cy: i32, r: i32, color: Rgb) {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    self.set(cx + dx, cy + dy, color);
                }
            }
        }
    }

    /// 全屏乘法染色，用于天气/昼夜光照。
    pub fn tint_all(&mut self, tr: f32, tg: f32, tb: f32) {
        for p in &mut self.pixels {
            *p = p.tint(tr, tg, tb);
        }
    }

    pub fn pixels(&self) -> &[Rgb] {
        &self.pixels
    }

    /// 可写像素缓冲（光照贴图等后处理用）。
    pub fn pixels_mut(&mut self) -> &mut [Rgb] {
        &mut self.pixels
    }
}
