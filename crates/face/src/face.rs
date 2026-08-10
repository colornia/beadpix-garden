use beadpix::{Canvas, Rgb};

/// 驱动脸部渲染的全部连续参数。
/// 表情 = 一组目标参数；动画 = 当前参数向目标平滑插值。
#[derive(Debug, Clone, Copy)]
pub struct FaceParams {
    /// 眼睛睁开程度 0-1（眨眼时被临时压到 0）
    pub eye_open: f32,
    /// 眉毛抬高（-1 压低 .. 1 高挑）
    pub brow_raise: f32,
    /// 眉毛内侧倾斜（-1 愤怒下压 .. 1 悲伤上挑）
    pub brow_tilt: f32,
    /// 嘴角弯曲（-1 下垂 .. 1 微笑）
    pub mouth_curve: f32,
    /// 张嘴程度 0-1
    pub mouth_open: f32,
    /// 脸颊红晕 0-1
    pub blush: f32,
    /// 瞳孔缩放 0.6-1.4（惊讶放大、愤怒缩小）
    pub pupil_scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emotion {
    Neutral,
    Happy,
    Surprised,
    Sad,
    Angry,
    Sleepy,
}

impl Emotion {
    pub fn params(self) -> FaceParams {
        match self {
            Emotion::Neutral => FaceParams {
                eye_open: 1.0,
                brow_raise: 0.0,
                brow_tilt: 0.0,
                mouth_curve: 0.15,
                mouth_open: 0.0,
                blush: 0.1,
                pupil_scale: 1.0,
            },
            Emotion::Happy => FaceParams {
                eye_open: 0.7, // 笑起来眯眼
                brow_raise: 0.3,
                brow_tilt: 0.1,
                mouth_curve: 1.0,
                mouth_open: 0.35,
                blush: 0.7,
                pupil_scale: 1.05,
            },
            Emotion::Surprised => FaceParams {
                eye_open: 1.0,
                brow_raise: 1.0,
                brow_tilt: 0.2,
                mouth_curve: 0.0,
                mouth_open: 0.9,
                blush: 0.2,
                pupil_scale: 1.35,
            },
            Emotion::Sad => FaceParams {
                eye_open: 0.6,
                brow_raise: 0.2,
                brow_tilt: 0.9,
                mouth_curve: -0.8,
                mouth_open: 0.0,
                blush: 0.0,
                pupil_scale: 0.95,
            },
            Emotion::Angry => FaceParams {
                eye_open: 0.75,
                brow_raise: -0.6,
                brow_tilt: -1.0,
                mouth_curve: -0.5,
                mouth_open: 0.15,
                blush: 0.0,
                pupil_scale: 0.7,
            },
            Emotion::Sleepy => FaceParams {
                eye_open: 0.25,
                brow_raise: -0.1,
                brow_tilt: 0.3,
                mouth_curve: 0.0,
                mouth_open: 0.1,
                blush: 0.15,
                pupil_scale: 0.9,
            },
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Emotion::Neutral => "Neutral",
            Emotion::Happy => "Happy",
            Emotion::Surprised => "Surprised",
            Emotion::Sad => "Sad",
            Emotion::Angry => "Angry",
            Emotion::Sleepy => "Sleepy",
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl FaceParams {
    /// 向目标参数按速率插值（指数平滑，帧率无关）。
    pub fn approach(&mut self, target: &FaceParams, dt: f32, speed: f32) {
        let t = 1.0 - (-speed * dt).exp();
        self.eye_open = lerp(self.eye_open, target.eye_open, t);
        self.brow_raise = lerp(self.brow_raise, target.brow_raise, t);
        self.brow_tilt = lerp(self.brow_tilt, target.brow_tilt, t);
        self.mouth_curve = lerp(self.mouth_curve, target.mouth_curve, t);
        self.mouth_open = lerp(self.mouth_open, target.mouth_open, t);
        self.blush = lerp(self.blush, target.blush, t);
        self.pupil_scale = lerp(self.pupil_scale, target.pupil_scale, t);
    }
}

/// 把参数化的脸画到画布上。所有几何量都按画布尺寸比例计算，任意分辨率可用。
pub fn draw_face(
    canvas: &mut Canvas,
    p: &FaceParams,
    gaze: (f32, f32), // 视线方向 -1..1
    breath: f32,      // 呼吸相位偏移（像素）
    blink: f32,       // 眨眼系数 0(闭)-1(开)，与 eye_open 相乘
) {
    let w = canvas.width() as f32;
    let h = canvas.height() as f32;
    let cx = (w / 2.0) as i32;
    let cy = (h / 2.0 + breath) as i32;

    let skin = Rgb::new(255, 213, 170);
    let skin_dark = Rgb::new(225, 178, 135);
    let outline = Rgb::new(120, 80, 60);

    // --- 脸底：椭圆 ---
    let rx = w * 0.36;
    let ry = h * 0.42;
    for dy in -(ry as i32)..=(ry as i32) {
        let span = (1.0 - (dy as f32 / ry).powi(2)).max(0.0).sqrt() * rx;
        for dx in -(span as i32)..=(span as i32) {
            // 边缘一圈描边 + 底部微暗做体积感
            let edge = span - dx.abs() as f32;
            let c = if edge < 1.5 {
                outline
            } else if dy > (ry * 0.5) as i32 {
                skin.lerp(skin_dark, (dy as f32 / ry - 0.5) * 1.2)
            } else {
                skin
            };
            canvas.set(cx + dx, cy + dy, c);
        }
    }

    let eye_dx = (rx * 0.42) as i32;
    let eye_y = cy - (ry * 0.15) as i32;
    let eye_r = (rx * 0.22).max(3.0) as i32;
    let open = (p.eye_open * blink).clamp(0.0, 1.0);

    for side in [-1i32, 1] {
        let ex = cx + side * eye_dx;
        draw_eye(canvas, ex, eye_y, eye_r, open, gaze, p.pupil_scale);

        // --- 眉毛 ---
        let brow_y = eye_y - eye_r - 2 - (p.brow_raise * eye_r as f32 * 0.6) as i32;
        let tilt = p.brow_tilt * 2.5;
        for i in -eye_r..=eye_r {
            // tilt>0: 内侧高（悲伤）；tilt<0: 内侧低压（愤怒）
            let inner = -side * i; // 越靠内越大
            let dy = (inner as f32 / eye_r as f32 * tilt) as i32;
            canvas.set(ex + i, brow_y - dy, outline);
            canvas.set(ex + i, brow_y - dy + 1, outline);
        }

        // --- 腮红 ---
        if p.blush > 0.05 {
            let bx = cx + side * (rx * 0.55) as i32;
            let by = cy + (ry * 0.18) as i32;
            let alpha = (p.blush * 160.0) as u8;
            let r = (rx * 0.12) as i32;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx * dx + dy * dy <= r * r {
                        canvas.blend(bx + dx, by + dy, Rgb::new(255, 120, 130), alpha);
                    }
                }
            }
        }
    }

    // --- 嘴 ---
    let mouth_y = cy + (ry * 0.45) as i32;
    let mouth_w = (rx * 0.45) as i32;
    let curve = p.mouth_curve * rx * 0.18;
    let open_h = (p.mouth_open * ry * 0.22) as i32;

    if open_h > 1 {
        // 张嘴：暗色椭圆 + 舌头
        for dy in -open_h..=open_h {
            let span = (1.0 - (dy as f32 / open_h as f32).powi(2)).max(0.0).sqrt()
                * mouth_w as f32
                * 0.8;
            for dx in -(span as i32)..=(span as i32) {
                let c = if dy > open_h / 3 {
                    Rgb::new(230, 120, 120) // 舌
                } else {
                    Rgb::new(120, 40, 50)
                };
                canvas.set(cx + dx, mouth_y + dy - (curve * 0.3) as i32, c);
            }
        }
    }
    // 唇线：抛物线，mouth_curve 决定弯向
    for dx in -mouth_w..=mouth_w {
        let t = dx as f32 / mouth_w as f32;
        let dy = (-curve * (1.0 - t * t)) as i32;
        let y = mouth_y + dy + if open_h > 1 { -open_h } else { 0 };
        canvas.set(cx + dx, y, outline);
        canvas.set(cx + dx, y + 1, outline);
    }
}

fn draw_eye(
    canvas: &mut Canvas,
    ex: i32,
    ey: i32,
    r: i32,
    open: f32,
    gaze: (f32, f32),
    pupil_scale: f32,
) {
    let outline = Rgb::new(120, 80, 60);
    let skin = Rgb::new(255, 213, 170);

    if open < 0.12 {
        // 闭眼：一条弧线
        for dx in -r..=r {
            let dy = ((dx as f32 / r as f32).powi(2) * 2.0) as i32;
            canvas.set(ex + dx, ey + dy, outline);
        }
        return;
    }

    // 眼白（椭圆，纵向被 open 压缩）
    let ry = (r as f32 * open).max(1.0);
    for dy in -(ry as i32)..=(ry as i32) {
        let span = (1.0 - (dy as f32 / ry).powi(2)).max(0.0).sqrt() * r as f32;
        for dx in -(span as i32)..=(span as i32) {
            canvas.set(ex + dx, ey + dy, Rgb::WHITE);
        }
    }

    // 虹膜 + 瞳孔，位置随视线偏移（限制在眼眶内）
    let max_off = r as f32 * 0.4;
    let ix = ex + (gaze.0 * max_off) as i32;
    let iy = ey + (gaze.1 * max_off * open) as i32;
    let iris_r = (r as f32 * 0.55 * pupil_scale) as i32;
    clipped_circle(canvas, ix, iy, iris_r, Rgb::new(70, 110, 180), ex, ey, r, ry);
    clipped_circle(
        canvas,
        ix,
        iy,
        (iris_r as f32 * 0.55) as i32,
        Rgb::new(25, 25, 35),
        ex,
        ey,
        r,
        ry,
    );
    // 高光：一个固定在左上的亮点，是"活气"的关键
    clipped_circle(
        canvas,
        ix - iris_r / 3,
        iy - iris_r / 3,
        (iris_r / 3).max(1),
        Rgb::WHITE,
        ex,
        ey,
        r,
        ry,
    );

    // 上眼睑边缘线
    for dx in -r..=r {
        let t = dx as f32 / r as f32;
        let dy = -((1.0 - t * t).max(0.0).sqrt() * ry) as i32;
        canvas.set(ex + dx, ey + dy, outline);
    }
    // 半睁时上方用肤色盖住（眼皮）
    if open < 0.95 {
        let lid = ((1.0 - open) * r as f32) as i32;
        for dy in 0..lid {
            let y = ey - ry as i32 - dy - 1;
            for dx in -r..=r {
                if canvas.get(ex + dx, y) == Some(Rgb::WHITE) {
                    canvas.set(ex + dx, y, skin);
                }
            }
        }
    }
}

/// 画圆但裁剪在眼白椭圆范围内。
#[allow(clippy::too_many_arguments)]
fn clipped_circle(
    canvas: &mut Canvas,
    cx: i32,
    cy: i32,
    r: i32,
    color: Rgb,
    eye_x: i32,
    eye_y: i32,
    eye_rx: i32,
    eye_ry: f32,
) {
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let px = cx + dx;
            let py = cy + dy;
            let nx = (px - eye_x) as f32 / eye_rx as f32;
            let ny = (py - eye_y) as f32 / eye_ry;
            if nx * nx + ny * ny <= 1.0 {
                canvas.set(px, py, color);
            }
        }
    }
}
