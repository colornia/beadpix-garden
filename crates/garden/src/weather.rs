use beadpix::Rgb;
use rand::Rng;

/// 天气只管气象（晴/雨/雾），昼夜由 `clock` 连续演进——
/// 天色时刻都在缓慢变化，这是画面"活着"的关键。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Sunny,
    Rain,
    Fog,
}

/// 一个完整昼夜的真实秒数（测试模式下按倍率加速）。
const DAY_CYCLE_SECS: f32 = 600.0;

pub struct Weather {
    pub kind: Kind,
    /// 一天中的时刻 0..1：0=日出，0.5=日落，0.75=午夜
    pub clock: f32,
    /// 距下次天气切换的秒数
    timer: f32,
    /// 雨后彩虹剩余秒数
    pub rainbow: f32,
    pub raindrops: Vec<(f32, f32, f32)>,
    pub splashes: Vec<(f32, f32, f32)>,
    pub fireflies: Vec<(f32, f32, f32)>,
    pub butterflies: Vec<(f32, f32, f32)>,
    /// 流星 (x, y, vx, vy, 剩余寿命)
    pub shooting: Vec<(f32, f32, f32, f32, f32)>,
    shoot_timer: f32,
    /// 雁群 (x, y, 速度)；None = 天上没有鸟
    pub birds: Option<(f32, f32, f32)>,
    bird_timer: f32,
}

impl Weather {
    pub fn new(rng: &mut impl Rng) -> Self {
        Weather {
            kind: Kind::Sunny,
            clock: 0.08, // 清晨开局
            timer: rng.gen_range(45.0..90.0),
            rainbow: 0.0,
            raindrops: Vec::new(),
            splashes: Vec::new(),
            fireflies: Vec::new(),
            butterflies: Vec::new(),
            shooting: Vec::new(),
            shoot_timer: 8.0,
            birds: None,
            bird_timer: 20.0,
        }
    }

    /// 黑暗程度 0（白天）..1（深夜），黎明黄昏平滑过渡。
    pub fn darkness(&self) -> f32 {
        let c = self.clock;
        if c < 0.05 {
            1.0 - c / 0.05 // 黎明破晓
        } else if c < 0.45 {
            0.0 // 白天
        } else if c < 0.55 {
            (c - 0.45) / 0.1 // 黄昏入夜
        } else {
            1.0 // 夜晚
        }
    }

    pub fn is_day(&self) -> bool {
        self.darkness() < 0.5
    }

    /// 时段名（HUD 显示）。
    pub fn period_name(&self) -> &'static str {
        let c = self.clock;
        if c < 0.06 {
            "黎明"
        } else if c < 0.22 {
            "清晨"
        } else if c < 0.35 {
            "正午"
        } else if c < 0.45 {
            "午后"
        } else if c < 0.56 {
            "黄昏"
        } else if c < 0.8 {
            "夜晚"
        } else {
            "深夜"
        }
    }

    /// 当前天空颜色 (顶部, 底部)：按时刻在关键帧之间插值，再叠加天气影响。
    pub fn sky_colors(&self) -> (Rgb, Rgb) {
        // (时刻, 顶色, 底色)
        const KEYS: [(f32, (u8, u8, u8), (u8, u8, u8)); 7] = [
            (0.00, (110, 80, 140), (255, 170, 120)), // 日出
            (0.06, (90, 160, 235), (175, 215, 250)), // 上午
            (0.44, (95, 150, 225), (250, 190, 140)), // 傍晚前
            (0.50, (70, 45, 100), (255, 140, 95)),   // 日落
            (0.56, (12, 14, 44), (40, 40, 90)),      // 入夜
            (0.96, (12, 14, 44), (40, 40, 90)),      // 深夜
            (1.00, (110, 80, 140), (255, 170, 120)), // 回到日出
        ];
        let c = self.clock;
        let mut top = Rgb::new(90, 160, 235);
        let mut bottom = Rgb::new(175, 215, 250);
        for i in 0..KEYS.len() - 1 {
            let (t0, a0, b0) = KEYS[i];
            let (t1, a1, b1) = KEYS[i + 1];
            if c >= t0 && c <= t1 {
                let t = (c - t0) / (t1 - t0).max(1e-6);
                top = Rgb::new(a0.0, a0.1, a0.2).lerp(Rgb::new(a1.0, a1.1, a1.2), t);
                bottom = Rgb::new(b0.0, b0.1, b0.2).lerp(Rgb::new(b1.0, b1.1, b1.2), t);
                break;
            }
        }
        // 天气叠加：雨/雾把天空压灰
        match self.kind {
            Kind::Rain => (
                top.lerp(Rgb::new(75, 85, 105), 0.75),
                bottom.lerp(Rgb::new(125, 135, 150), 0.75),
            ),
            Kind::Fog => (
                top.lerp(Rgb::new(155, 160, 170), 0.7),
                bottom.lerp(Rgb::new(190, 195, 200), 0.7),
            ),
            Kind::Sunny => (top, bottom),
        }
    }

    /// 环境光：昼夜插值 + 黄昏暖调 + 天气修正。
    pub fn ambient(&self) -> (f32, f32, f32) {
        let d = self.darkness();
        let mut r = 1.0 - d * 0.5;
        let mut g = 1.0 - d * 0.47;
        let mut b = 1.0 - d * 0.24;
        // 黎明/黄昏的暖光
        let c = self.clock;
        let warm = if c < 0.08 {
            1.0 - (c / 0.08 - 1.0).abs()
        } else if (0.42..0.56).contains(&c) {
            1.0 - ((c - 0.49) / 0.07).abs()
        } else {
            0.0
        };
        r *= 1.0 + warm * 0.1;
        g *= 1.0 - warm * 0.08;
        b *= 1.0 - warm * 0.18;
        match self.kind {
            Kind::Rain => (r * 0.72, g * 0.78, b * 0.92),
            Kind::Fog => (r * 0.85, g * 0.87, b * 0.9),
            Kind::Sunny => (r, g, b),
        }
    }

    /// 风力：雨天狂风，夜里风小。
    pub fn wind(&self) -> f32 {
        let base = match self.kind {
            Kind::Sunny => 1.0,
            Kind::Rain => 1.8,
            Kind::Fog => 0.55,
        };
        base * (1.0 - self.darkness() * 0.3)
    }

    pub fn growth_multiplier(&self) -> f32 {
        let base = match self.kind {
            Kind::Sunny => 1.0,
            Kind::Rain => 1.6,
            Kind::Fog => 0.9,
        };
        base * (1.0 - self.darkness() * 0.3)
    }

    pub fn name(&self) -> &'static str {
        match self.kind {
            Kind::Sunny => "晴",
            Kind::Rain => "雨",
            Kind::Fog => "雾",
        }
    }

    /// `dt`：真实帧时间（粒子用）；`clock_dt`：世界时间（昼夜推进，测试模式下加速）。
    pub fn update(
        &mut self,
        dt: f32,
        clock_dt: f32,
        width: u32,
        height: u32,
        ground_y: f32,
        rng: &mut impl Rng,
    ) {
        self.clock = (self.clock + clock_dt / DAY_CYCLE_SECS).rem_euclid(1.0);

        self.timer -= dt;
        if self.timer <= 0.0 {
            let old = self.kind;
            self.kind = match rng.gen_range(0..5) {
                0 | 1 | 2 => Kind::Sunny, // 晴天概率高一些
                3 => Kind::Rain,
                _ => Kind::Fog,
            };
            self.timer = rng.gen_range(45.0..90.0);
            // 雨后天晴且在白天 → 彩虹
            if old == Kind::Rain && self.kind == Kind::Sunny && self.is_day() {
                self.rainbow = 30.0;
            }
        }
        self.rainbow = (self.rainbow - dt).max(0.0);

        // --- 雨 ---
        if self.kind == Kind::Rain {
            while self.raindrops.len() < (width / 3) as usize {
                self.raindrops.push((
                    rng.gen_range(0.0..width as f32),
                    rng.gen_range(-(height as f32)..0.0),
                    rng.gen_range(25.0..45.0),
                ));
            }
        }
        let mut i = 0;
        while i < self.raindrops.len() {
            let d = &mut self.raindrops[i];
            d.1 += d.2 * dt;
            if d.1 >= ground_y {
                self.splashes.push((d.0, ground_y - 1.0, 0.0));
                self.raindrops.swap_remove(i);
            } else if d.1 >= height as f32 {
                self.raindrops.swap_remove(i);
            } else {
                i += 1;
            }
        }
        for s in &mut self.splashes {
            s.2 += dt;
        }
        self.splashes.retain(|s| s.2 < 0.35);

        // --- 萤火虫：天黑且不下雨 ---
        if self.darkness() > 0.6 && self.kind != Kind::Rain {
            while self.fireflies.len() < 12 {
                self.fireflies.push((
                    rng.gen_range(0.0..width as f32),
                    rng.gen_range(height as f32 * 0.3..height as f32 * 0.85),
                    rng.gen_range(0.0..6.28),
                ));
            }
        } else {
            self.fireflies.clear();
        }
        for fly in &mut self.fireflies {
            fly.2 += dt * 2.0;
            fly.0 += fly.2.sin() * dt * 3.0;
            fly.1 += (fly.2 * 0.7).cos() * dt * 2.0;
        }

        // --- 蝴蝶：晴朗的白天 ---
        if self.kind == Kind::Sunny && self.is_day() {
            while self.butterflies.len() < 3 {
                self.butterflies.push((
                    rng.gen_range(0.0..width as f32),
                    rng.gen_range(ground_y * 0.55..ground_y * 0.9),
                    rng.gen_range(0.0..6.28),
                ));
            }
        } else {
            self.butterflies.clear();
        }
        for b in &mut self.butterflies {
            b.2 += dt * 3.0;
            b.0 += ((b.2 * 0.6).sin() * 7.0 + 2.0) * dt;
            b.1 += (b.2 * 2.1).sin() * 6.0 * dt;
            b.0 = b.0.rem_euclid(width as f32);
            b.1 = b.1.clamp(ground_y * 0.45, ground_y * 0.95);
        }

        // --- 流星：深夜偶发 ---
        if self.darkness() > 0.8 && self.kind == Kind::Sunny {
            self.shoot_timer -= dt;
            if self.shoot_timer <= 0.0 {
                self.shooting.push((
                    rng.gen_range(width as f32 * 0.2..width as f32),
                    rng.gen_range(2.0..height as f32 * 0.15),
                    rng.gen_range(-60.0..-35.0),
                    rng.gen_range(14.0..26.0),
                    0.9,
                ));
                self.shoot_timer = rng.gen_range(12.0..35.0);
            }
        }
        for s in &mut self.shooting {
            s.0 += s.2 * dt;
            s.1 += s.3 * dt;
            s.4 -= dt;
        }
        self.shooting.retain(|s| s.4 > 0.0);

        // --- 雁群：白天偶尔飞过 ---
        if self.is_day() && self.birds.is_none() {
            self.bird_timer -= dt;
            if self.bird_timer <= 0.0 {
                self.birds = Some((
                    -8.0,
                    rng.gen_range(height as f32 * 0.06..height as f32 * 0.22),
                    rng.gen_range(5.0..9.0),
                ));
                self.bird_timer = rng.gen_range(30.0..70.0);
            }
        }
        if let Some(b) = &mut self.birds {
            b.0 += b.2 * dt;
            if b.0 > width as f32 + 14.0 {
                self.birds = None;
            }
        }
    }
}
