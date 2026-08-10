/// 24-bit 真彩色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb::new(0, 0, 0);
    pub const WHITE: Rgb = Rgb::new(255, 255, 255);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    /// 从 HSV 构造（h: 0-360, s/v: 0-1）。花卉基因用 HSV 表达花色，杂交时插值更自然。
    pub fn from_hsv(h: f32, s: f32, v: f32) -> Self {
        let h = h.rem_euclid(360.0);
        let c = v * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = v - c;
        let (r, g, b) = match h as u32 / 60 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        Rgb::new(
            ((r + m) * 255.0) as u8,
            ((g + m) * 255.0) as u8,
            ((b + m) * 255.0) as u8,
        )
    }

    /// 线性插值，t: 0-1。
    pub fn lerp(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        Rgb::new(
            (self.r as f32 + (other.r as f32 - self.r as f32) * t) as u8,
            (self.g as f32 + (other.g as f32 - self.g as f32) * t) as u8,
            (self.b as f32 + (other.b as f32 - self.b as f32) * t) as u8,
        )
    }

    /// 乘法染色（天气滤镜、光照用）。tint 为 (1,1,1) 时不变。
    pub fn tint(self, tr: f32, tg: f32, tb: f32) -> Rgb {
        Rgb::new(
            (self.r as f32 * tr).clamp(0.0, 255.0) as u8,
            (self.g as f32 * tg).clamp(0.0, 255.0) as u8,
            (self.b as f32 * tb).clamp(0.0, 255.0) as u8,
        )
    }

    /// 按 alpha (0-255) 叠加到底色上。
    pub fn over(self, base: Rgb, alpha: u8) -> Rgb {
        base.lerp(self, alpha as f32 / 255.0)
    }
}
