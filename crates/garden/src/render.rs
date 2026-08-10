use beadpix::{Canvas, Palette, Rgb, Sprite};

use crate::garden::{Biome, Garden, Plant, Stage};
use crate::weather::{Kind, Weather};

/// "主地面"高度比例：雨滴水花落点、蝴蝶活动带的基准。
pub fn ground_frac(biome: Biome) -> f32 {
    match biome {
        Biome::Meadow => 0.85,
        Biome::Forest => 0.85,
        Biome::Crevice => 0.92,
        Biome::Abyss => 0.85,
    }
}

/// 光源：位置、半径、RGB 光强（叠加到环境光之上）。
type LightSource = (i32, i32, i32, (f32, f32, f32));

/// 阵风波形：多层不同频率正弦叠加，相位随 x 偏移，风"扫过"花园。
fn gust(time: f32, x: i32) -> f32 {
    let xf = x as f32;
    (time * 0.7 + xf * 0.045).sin() * 0.55
        + (time * 1.9 + xf * 0.11).sin() * 0.28
        + (time * 0.23 + xf * 0.02).sin() * 0.45
}

fn hash(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h ^= h >> 13;
    h.wrapping_mul(1_274_126_177)
}

fn soft_circle(canvas: &mut Canvas, cx: i32, cy: i32, r: i32, color: Rgb, max_alpha: u8) {
    let r2 = (r * r) as f32;
    for dy in -r..=r {
        for dx in -r..=r {
            let d2 = (dx * dx + dy * dy) as f32;
            if d2 <= r2 {
                let a = max_alpha as f32 * (1.0 - d2 / r2);
                canvas.blend(cx + dx, cy + dy, color, a as u8);
            }
        }
    }
}

/// 每株植物的锚点（数量随扩张增长）。草甸/林间/深海是 3/4 视角双排花田
/// （后排先分满，前排遮挡后排）；石缝是岩壁上错落的岩架。
/// pub 供主程序计算花头位置（粒子/蜜蜂的目标点）。
pub fn anchors(biome: Biome, w: u32, h: u32, seed: u32, count: usize) -> Vec<(i32, i32)> {
    let back_n = count.div_ceil(2);
    let front_n = count - back_n;
    (0..count)
        .map(|i| match biome {
            Biome::Meadow | Biome::Forest | Biome::Abyss => {
                let back = i < back_n;
                let (col, row_n) = if back {
                    (i, back_n)
                } else {
                    (i - back_n, front_n)
                };
                let spacing = w as f32 / row_n.max(1) as f32;
                // 前排相对后排错半格，避免正对遮挡
                let x = spacing * col as f32
                    + spacing / 2.0
                    + if back { 0.0 } else { spacing * 0.22 };
                let jitter = (hash(i as i32, seed as i32) % 3) as i32 - 1;
                let y = if back { h as f32 * 0.68 } else { h as f32 * 0.85 };
                ((x as i32 + jitter).clamp(4, w as i32 - 5), y as i32)
            }
            Biome::Crevice => {
                let spacing = w as f32 / count.max(1) as f32;
                let base_x = (spacing * i as f32 + spacing / 2.0) as i32;
                let hi = hash(i as i32 * 7 + 3, seed as i32);
                let jitter = (hash(i as i32, seed as i32) % 5) as i32 - 2;
                let y = h as f32 * (0.42 + (hi % 1000) as f32 / 1000.0 * 0.42);
                ((base_x + jitter).clamp(4, w as i32 - 5), y as i32)
            }
        })
        .collect()
}

/// 四种花型精灵。调色板：1=花瓣主色 2=高光 3=花心 4=暗部
fn flower_sprites() -> [Sprite; 4] {
    [
        Sprite::from_ascii(&[
            "..212..",
            ".21112.",
            "2111114",
            "2113114",
            "4111114",
            ".41114.",
            "..444..",
        ]),
        Sprite::from_ascii(&[
            "..121..",
            "..111..",
            "1112111",
            "2113112",
            "1112111",
            "..111..",
            "..141..",
        ]),
        Sprite::from_ascii(&[
            "1...2...1",
            ".1..2..1.",
            "..1.2.1..",
            "...121...",
            "222131222",
            "...121...",
            "..1.2.1..",
            ".1..2..1.",
            "1...2...1",
        ]),
        Sprite::from_ascii(&[
            "..21112..",
            ".2111112.",
            "211222112",
            "212111212",
            "212131212",
            "412111214",
            "411222114",
            ".4111114.",
            "..44444..",
        ]),
    ]
}

pub struct Scene {
    sprites: [Sprite; 4],
    /// 光照贴图缓冲（每帧重建）
    light: Vec<[f32; 3]>,
    /// 飘落的花瓣 (x, y, 相位, 颜色, 剩余寿命)
    petals: Vec<(f32, f32, f32, Rgb, f32)>,
    /// 收获迸发粒子 (x, y, vx, vy, 颜色, 剩余寿命)
    bursts: Vec<(f32, f32, f32, f32, Rgb, f32)>,
    /// 授粉蜜蜂 (进度 0..1, 起点, 终点)
    bees: Vec<(f32, (f32, f32), (f32, f32))>,
    /// 深海气泡 (x, y, 相位)
    bubbles: Vec<(f32, f32, f32)>,
}

impl Scene {
    pub fn new() -> Self {
        Scene {
            sprites: flower_sprites(),
            light: Vec::new(),
            petals: Vec::new(),
            bursts: Vec::new(),
            bees: Vec::new(),
            bubbles: Vec::new(),
        }
    }

    /// 收获时的花瓣迸发：以花头为中心向四周炸开一圈花色粒子。
    pub fn emit_burst(&mut self, x: f32, y: f32, color: Rgb) {
        for k in 0..16 {
            let jitter = (hash(k, 55) % 100) as f32 / 100.0;
            let angle = (k as f32 + jitter) / 16.0 * std::f32::consts::TAU;
            let speed = 9.0 + (hash(k, 77) % 90) as f32 / 10.0;
            self.bursts.push((
                x,
                y,
                angle.cos() * speed,
                angle.sin() * speed - 4.0, // 稍微向上抛
                color,
                0.9,
            ));
        }
    }

    /// 杂交时派出一只蜜蜂：从 A 花飞到 B 花悬停授粉再飞回。
    pub fn spawn_bee(&mut self, from: (f32, f32), to: (f32, f32)) {
        self.bees.push((0.0, from, to));
    }

    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        garden: &Garden,
        weather: &Weather,
        selected: usize,
        time: f32,
        dt: f32,
        moving: Option<usize>,
    ) {
        let w = canvas.width();
        let h = canvas.height();
        let biome = garden.biome;
        let seed = garden.world_seed;
        let main_ground = (h as f32 * ground_frac(biome)) as i32;
        let mut lights: Vec<LightSource> = Vec::new();

        // --- 群系背景 ---
        match biome {
            Biome::Meadow => self.bg_meadow(canvas, weather, time, seed),
            Biome::Forest => self.bg_forest(canvas, weather, time, seed, &mut lights),
            Biome::Crevice => self.bg_crevice(canvas, weather, time, seed),
            Biome::Abyss => self.bg_abyss(canvas, weather, time, seed, dt),
        }

        // --- 植株（按锚点顺序绘制：后排先画，前排自然遮挡）---
        // 天时科（月见花/日晷花）的开合绑定这个值
        let dark0 = weather.darkness();
        // 雷纹花的发芽与电弧绑定这个值
        let raining0 = weather.kind == Kind::Rain;
        let pts = anchors(biome, w, h, seed, garden.plots.len());
        // (x, y, 主色, 发光基因, 品种, vitality)
        let mut blooms: Vec<(i32, i32, Rgb, bool, u8, f32)> = Vec::new();
        for (i, slot) in garden.plots.iter().enumerate() {
            let (ax, ay) = pts[i];
            match biome {
                Biome::Crevice => {
                    for dx in -4..=4 {
                        canvas.set(ax + dx, ay + 1, Rgb::new(130, 130, 140));
                        canvas.set(ax + dx, ay + 2, Rgb::new(70, 70, 82));
                    }
                    for dx in -2..=2 {
                        canvas.set(ax + dx, ay, Rgb::new(88, 62, 42));
                    }
                }
                Biome::Abyss => {
                    // 沙丘种植坑
                    for dx in -4i32..=4 {
                        let wdt = if dx.abs() > 2 { 0 } else { 1 };
                        for dy in 0..=wdt {
                            canvas.set(ax + dx, ay + 1 + dy, Rgb::new(120, 104, 80));
                        }
                    }
                }
                _ => {
                    // 花圃土斑（椭圆）
                    for dx in -4i32..=4 {
                        let wdt = if dx.abs() > 2 { 0 } else { 1 };
                        for dy in 0..=wdt {
                            canvas.set(ax + dx, ay + 1 + dy, Rgb::new(74, 52, 34));
                        }
                    }
                }
            }
            if let Some(plant) = slot {
                // 活力低 → 额外的固定"垂头"弯曲，视觉上就是蔫了（可逆，浇水即复原）
                let droop = (1.0 - plant.vitality).max(0.0) * 1.6;
                // 古株木质化，风中晃得更少
                let stiff = if plant.is_venerable() { 0.55 } else { 1.0 };
                let wind =
                    weather.wind() * gust(time, ax) * (0.5 + plant.vitality * 0.5) * stiff + droop;
                if let Some(b) =
                    self.draw_plant(canvas, plant, ax, ay, wind, time, dark0, raining0)
                {
                    blooms.push((b.0, b.1, b.2, b.3, b.4, plant.vitality));
                }
                // 古株的苔痕：茎基一圈青苔与虬结，越老越明显
                if plant.is_venerable() {
                    let moss = Rgb::new(78, 116, 62);
                    for dx in -2..=2i32 {
                        if (dx + ax) % 2 == 0 {
                            canvas.set(ax + dx, ay - 1, moss);
                        }
                    }
                    canvas.set(ax - 1, ay - 2, moss);
                    canvas.set(ax + 1, ay - 3, Rgb::new(96, 132, 74));
                    canvas.set(ax - 1, ay - 5, moss);
                }
            }
            if i == selected || Some(i) == moving {
                let cy = ay + 4;
                let blink = ((time * 3.0).sin() * 0.5 + 0.5) * 255.0;
                // 被拿起的那株用琥珀色标记，光标仍是白色
                let (c, a) = if Some(i) == moving {
                    (Rgb::new(255, 200, 90), 220.0)
                } else {
                    (Rgb::new(255, 255, 255), blink)
                };
                canvas.blend(ax, cy, c, a as u8);
                canvas.blend(ax - 1, cy + 1, c, a as u8);
                canvas.blend(ax, cy + 1, c, a as u8);
                canvas.blend(ax + 1, cy + 1, c, a as u8);
            }
        }

        // --- 花瓣飘落：只有花卉科会撒花瓣（草蕨撒孢子没那么浪漫，触手就更别提了）---
        for (i, &(bx, by, c, _, variety, _)) in blooms.iter().enumerate() {
            if variety <= 3 && ((time * 0.31 + i as f32 * 0.77).fract()) < dt * 0.12 {
                self.petals.push((bx as f32, by as f32 + 2.0, i as f32, c, 4.0));
            }
        }
        let wind_now = weather.wind();
        for p in &mut self.petals {
            p.2 += dt * 3.0;
            p.0 += (wind_now * gust(time, p.0 as i32) * 1.5 + (p.2 * 2.0).sin() * 2.0) * dt * 2.0;
            p.1 += (5.0 + (p.2 * 1.3).sin() * 2.5) * dt;
            p.4 -= dt;
        }
        let h_f = h as f32;
        self.petals.retain(|p| p.4 > 0.0 && p.1 < h_f);
        for &(x, y, _, c, life) in &self.petals {
            canvas.blend(x as i32, y as i32, c, (life.min(1.0) * 200.0) as u8);
        }

        // --- 收获迸发粒子：带微重力抛物线，随寿命淡出 ---
        for p in &mut self.bursts {
            p.0 += p.2 * dt;
            p.1 += p.3 * dt;
            p.3 += 14.0 * dt; // 重力
            p.5 -= dt;
        }
        self.bursts.retain(|p| p.5 > 0.0);
        for &(x, y, _, _, c, life) in &self.bursts {
            canvas.blend(x as i32, y as i32, c, (life.min(0.7) / 0.7 * 230.0) as u8);
        }

        // --- 授粉蜜蜂：飞去 → 悬停授粉 → 飞回，路径带弧线和嗡嗡抖动 ---
        for bee in &mut self.bees {
            bee.0 += dt / 4.0;
        }
        self.bees.retain(|b| b.0 < 1.0);
        for &(t, (ax, ay), (bx, by)) in &self.bees {
            // 0-0.4 飞去，0.4-0.6 悬停，0.6-1.0 飞回
            let (fx, fy, hover) = if t < 0.4 {
                let k = t / 0.4;
                (ax + (bx - ax) * k, ay + (by - ay) * k - (k * 3.14).sin() * 6.0, false)
            } else if t < 0.6 {
                (bx, by, true)
            } else {
                let k = (t - 0.6) / 0.4;
                (bx + (ax - bx) * k, by + (ay - by) * k - (k * 3.14).sin() * 6.0, false)
            };
            let buzz_x = (time * 21.0).sin() * 0.8;
            let buzz_y = (time * 17.0).cos() * if hover { 1.4 } else { 0.7 };
            let (xi, yi) = ((fx + buzz_x) as i32, (fy + buzz_y) as i32);
            canvas.set(xi, yi, Rgb::new(240, 200, 60)); // 黄身
            canvas.set(xi + 1, yi, Rgb::new(60, 50, 40)); // 黑纹
            if (time * 30.0).sin() > 0.0 {
                canvas.blend(xi, yi - 1, Rgb::new(235, 240, 250), 170); // 翅膀
            }
            // 悬停授粉时洒花粉微粒
            if hover {
                let pt = (time * 5.0).fract();
                canvas.blend(
                    xi + ((time * 7.0).sin() * 2.0) as i32,
                    yi + 1 + (pt * 3.0) as i32,
                    Rgb::new(255, 230, 140),
                    ((1.0 - pt) * 180.0) as u8,
                );
            }
        }

        // --- 蝴蝶（深海群系里换成小鱼群）---
        for (i, &(x, y, phase)) in weather.butterflies.iter().enumerate() {
            let (xi, yi) = (x as i32, y as i32);
            if biome == Biome::Abyss {
                let body = [
                    Rgb::new(240, 160, 70),
                    Rgb::new(150, 200, 235),
                    Rgb::new(235, 210, 120),
                ][i % 3];
                canvas.set(xi, yi, body);
                canvas.set(xi + 1, yi, body.tint(0.85, 0.85, 0.9));
                // 尾鳍摆动
                let tail = if (phase * 8.0).sin() > 0.0 { -1 } else { 0 };
                canvas.set(xi - 1, yi + tail, body.tint(0.7, 0.7, 0.75));
            } else {
                let body = [
                    Rgb::new(240, 150, 60),
                    Rgb::new(235, 235, 245),
                    Rgb::new(230, 130, 190),
                ][i % 3];
                let flap = (phase * 9.0).sin() > 0.0;
                if flap {
                    canvas.set(xi - 1, yi, body);
                    canvas.set(xi + 1, yi, body);
                } else {
                    canvas.set(xi - 1, yi - 1, body);
                    canvas.set(xi + 1, yi - 1, body);
                }
                canvas.set(xi, yi, body.tint(0.7, 0.7, 0.7));
            }
        }

        // --- 雨（水下没有雨）---
        if biome != Biome::Abyss {
            for &(x, y, _) in &weather.raindrops {
                canvas.blend(x as i32, y as i32, Rgb::new(160, 190, 240), 200);
                canvas.blend(x as i32, y as i32 - 1, Rgb::new(160, 190, 240), 120);
            }
            for &(x, y, age) in &weather.splashes {
                let t = age / 0.35;
                let a = ((1.0 - t) * 190.0) as u8;
                let spread = (t * 3.0) as i32;
                let c = Rgb::new(175, 205, 245);
                canvas.blend(x as i32 - spread, y as i32 - (spread / 2), c, a);
                canvas.blend(x as i32 + spread, y as i32 - (spread / 2), c, a);
            }
        }

        // --- 雾带（深海里表现为水体浑浊，跳过）---
        if weather.kind == Kind::Fog && biome != Biome::Abyss {
            for y in (main_ground - 14)..(main_ground + 6) {
                let depth = 1.0 - ((main_ground - y).abs() as f32 / 14.0).min(1.0);
                for x in 0..w as i32 {
                    let w1 = (x as f32 * 0.11 + time * 0.5 + y as f32 * 0.55).sin() * 0.5 + 0.5;
                    let w2 = (x as f32 * 0.045 - time * 0.3).cos() * 0.5 + 0.5;
                    let a = ((0.35 + 0.65 * w1 * w2) * depth * 140.0) as u8;
                    canvas.blend(x, y, Rgb::new(212, 216, 224), a);
                }
            }
        }

        // --- 收集光源（光强随黑暗度渐变，黄昏时花光慢慢亮起）---
        let dark = weather.darkness();
        for (i, &(x, y, c, glows, variety, vit)) in blooms.iter().enumerate() {
            let pulse = ((time * 1.1 + i as f32 * 1.7).sin() * 0.5 + 0.5) * 0.25;
            // 活力直接调制光强——蔫的花光会暗下去，浇水就亮回来
            let v = 0.35 + 0.65 * vit.clamp(0.0, 1.0);
            let (nr, ng, nb) = (
                c.r as f32 / 255.0 * v,
                c.g as f32 / 255.0 * v,
                c.b as f32 / 255.0 * v,
            );
            // 异界/水生/月见花自带微光（贤者金花与日晷花另有常亮规则）
            let intrinsic = matches!(variety, 6 | 7 | 10 | 11 | 12);
            if glows {
                let s = 0.55 + dark * 0.55 + pulse;
                let r = 9 + (dark * 4.0) as i32;
                lights.push((x, y, r, (nr * s, ng * s, nb * s)));
            } else if variety == 9 || variety == 13 {
                let s = 0.35 + dark * 0.45 + pulse * 0.5;
                lights.push((x, y, 7, (0.95 * s, 0.8 * s, 0.45 * s)));
            } else if intrinsic && dark > 0.25 {
                let s = (0.55 + pulse * 0.6) * dark;
                lights.push((x, y, 7, (nr * s, ng * s, nb * s)));
            } else if dark > 0.3 {
                let s = (0.7 + pulse) * dark;
                lights.push((x, y, 8, (nr * s, ng * s, nb * s)));
            }
        }
        for &(x, y, phase) in &weather.fireflies {
            let p = phase.sin() * 0.5 + 0.5;
            lights.push((x as i32, y as i32, 4, (0.45 * p, 0.65 * p, 0.15 * p)));
        }

        // --- 光照贴图：环境光 + 光源叠加，真正"照亮"周围像素 ---
        let ambient = {
            let (ar, ag, ab) = weather.ambient();
            // 群系修正：林间常年遮荫，石缝岩壁偏冷
            let (br, bg2, bb) = match biome {
                Biome::Meadow => (1.0, 1.0, 1.0),
                Biome::Forest => (0.82, 0.88, 0.8),
                Biome::Crevice => (0.92, 0.92, 1.0),
                Biome::Abyss => (0.52, 0.74, 0.95), // 水体吸收红光
            };
            (ar * br, ag * bg2, ab * bb)
        };
        self.apply_lighting(canvas, ambient, &lights);

        // --- 光照后的自发光装饰（光核、光尘）---
        for &(x, y, phase) in &weather.fireflies {
            let glow = (phase.sin() * 0.5 + 0.5) * 255.0;
            canvas.blend(x as i32, y as i32, Rgb::new(225, 255, 160), glow as u8);
        }
        for (i, &(x, y, color, glows, _, _)) in blooms.iter().enumerate() {
            if glows {
                // 发光花的花体提亮 + 光尘上飘
                soft_circle(canvas, x, y, 4, color.lerp(Rgb::WHITE, 0.5), 60);
                let sparkle_t = (time * 0.9 + i as f32 * 2.3).fract();
                let sy = y - 2 - (sparkle_t * 8.0) as i32;
                let sx = x + ((time * 2.0 + i as f32).sin() * 2.5) as i32;
                canvas.blend(sx, sy, Rgb::WHITE, ((1.0 - sparkle_t) * 190.0) as u8);
            }
        }
    }

    /// 图鉴页：12 品种（列）x 8 色系（行）的收集网格。
    /// 已收集的用真实渲染器画出活的迷你植株（眼球在眨、法阵在转），
    /// 未收集的是暗色剪影——"还差几个"一目了然。
    pub fn draw_gallery(&mut self, canvas: &mut Canvas, garden: &Garden, time: f32) {
        use crate::genetics::{hue_name, variety_name, VARIETIES};
        use crate::traits::{traits_of, Tier};

        let w = canvas.width();
        let h = canvas.height();
        let bg_at = |y: i32| -> Rgb {
            let t = y as f32 / h as f32;
            Rgb::new(22, 24, 34).lerp(Rgb::new(30, 32, 46), t)
        };
        for y in 0..h as i32 {
            let c = bg_at(y);
            for x in 0..w as i32 {
                canvas.set(x, y, c);
            }
        }

        const HUES: [f32; 8] = [0.0, 30.0, 57.0, 115.0, 180.0, 225.0, 270.0, 315.0];
        // 按稀有度分层排列列序：常见 → 角色 → 传说。
        // 扁平网格里传说花和墙纸花并排，传说就不像传说了
        let mut cols: Vec<(u8, Tier)> = (0..VARIETIES)
            .map(|v| (v, traits_of(v).tier()))
            .collect();
        cols.sort_by_key(|(v, t)| {
            (
                match t {
                    Tier::Backdrop => 0,
                    Tier::Character => 1,
                    Tier::Legend => 2,
                },
                *v,
            )
        });

        let n = cols.len() as u32;
        let band_h = 3i32; // 顶部分层色带
        let grid_h = h as i32 - band_h;
        let cell_w = (w / n).max(4);
        let cell_h = (grid_h as u32 / 8).max(4);
        // 哨兵色画布：每格先在小画布上渲染，再裁剪拷贝——不会串格
        const MAGIC: Rgb = Rgb::new(255, 0, 255);
        let mut tmp = Canvas::new(cell_w, cell_h);

        for (col, &(variety, tier)) in cols.iter().enumerate() {
            let x0 = (col as u32 * w / n) as i32;
            let x1 = ((col as u32 + 1) * w / n) as i32;
            let tier_color = match tier {
                Tier::Backdrop => Rgb::new(96, 104, 120),
                Tier::Character => Rgb::new(104, 162, 196),
                Tier::Legend => Rgb::new(214, 168, 74),
            };
            // 分层色带：同层连成一条，层与层之间留一格缺口作分隔
            let prev_tier = col.checked_sub(1).map(|p| cols[p].1);
            let new_group = prev_tier != Some(tier);
            for x in x0..x1 {
                if new_group && x < x0 + 1 {
                    continue; // 组首留缺口
                }
                for y in 0..band_h - 1 {
                    canvas.set(x, y, tier_color);
                }
            }

            for (row, &hue) in HUES.iter().enumerate() {
                let y0 = band_h + (row as u32 * grid_h as u32 / 8) as i32;
                let species = format!("{}{}", hue_name(hue), variety_name(variety));
                let found = garden.discovered.iter().any(|s| s == &species);

                let mini = Plant {
                    genes: crate::genetics::Genes {
                        hue,
                        saturation: 0.75,
                        petal_shape: variety,
                        height: 7,
                        growth_rate: 1.0,
                        glow: false,
                    },
                    progress: 1.0,
                    vitality: 1.0,
                    last_care: 0,
                    planted_at: 0,
                    last_gift: 0,
                    had_bloom_night: false,
                };
                tmp.clear(MAGIC);
                // 图鉴里按"最好看的那一刻"展示：dark=1 让月见花/昙花开着，
                // raining=true 让雷纹花迸着电弧
                self.draw_plant(
                    &mut tmp,
                    &mini,
                    cell_w as i32 / 2,
                    cell_h as i32 - 2,
                    0.0,
                    time,
                    1.0,
                    true,
                );

                // 已收集：脚下微光基座（先画，植株盖在上面）
                if found {
                    let pulse = ((time * 1.3 + (col as f32 + row as f32 * 2.0)).sin() * 0.5 + 0.5)
                        * 22.0;
                    soft_circle(
                        canvas,
                        x0 + cell_w as i32 / 2,
                        y0 + cell_h as i32 - 1,
                        4,
                        Rgb::from_hsv(hue, 0.6, 0.9),
                        pulse as u8 + 14,
                    );
                }
                for ty in 0..cell_h as i32 {
                    for tx in 0..cell_w as i32 {
                        if let Some(p) = tmp.get(tx, ty) {
                            if p != MAGIC {
                                let c = if found { p } else { Rgb::new(46, 49, 60) };
                                canvas.set(x0 + tx, y0 + ty, c);
                            }
                        }
                    }
                }
            }
        }
    }

    /// 光照贴图：环境光打底，逐光源线性衰减叠加，最后逐像素相乘。
    fn apply_lighting(
        &mut self,
        canvas: &mut Canvas,
        ambient: (f32, f32, f32),
        sources: &[LightSource],
    ) {
        let w = canvas.width() as i32;
        let h = canvas.height() as i32;
        self.light.clear();
        self.light
            .resize((w * h) as usize, [ambient.0, ambient.1, ambient.2]);

        for &(sx, sy, r, (lr, lg, lb)) in sources {
            let rf = r as f32;
            for dy in -r..=r {
                let y = sy + dy;
                if y < 0 || y >= h {
                    continue;
                }
                for dx in -r..=r {
                    let x = sx + dx;
                    if x < 0 || x >= w {
                        continue;
                    }
                    let d = ((dx * dx + dy * dy) as f32).sqrt();
                    if d > rf {
                        continue;
                    }
                    let f = 1.0 - d / rf;
                    let cell = &mut self.light[(y * w + x) as usize];
                    cell[0] += lr * f;
                    cell[1] += lg * f;
                    cell[2] += lb * f;
                }
            }
        }

        for (i, p) in canvas.pixels_mut().iter_mut().enumerate() {
            let l = self.light[i];
            p.r = (p.r as f32 * l[0].min(1.55)).min(255.0) as u8;
            p.g = (p.g as f32 * l[1].min(1.55)).min(255.0) as u8;
            p.b = (p.b as f32 * l[2].min(1.55)).min(255.0) as u8;
        }
    }

    /// 草甸（3/4 视角）：天空 38%，远山带，草坪一直铺到画面底部。
    fn bg_meadow(&self, canvas: &mut Canvas, weather: &Weather, time: f32, seed: u32) {
        let w = canvas.width();
        let h = canvas.height();
        let lawn_top = (h as f32 * 0.55) as i32;
        let horizon = (h as f32 * 0.38) as i32;

        self.sky(canvas, weather, time, lawn_top, true);

        // 两层远山
        let s = (seed % 628) as f32 / 100.0;
        for x in 0..w as i32 {
            let xf = x as f32;
            let far = horizon as f32 + 3.0
                - ((xf * 0.03 + s).sin() + (xf * 0.011 + s * 2.0).sin()) * 5.0;
            let near = lawn_top as f32 - 2.0
                - ((xf * 0.05 + s + 2.0).sin() + (xf * 0.017 + s).cos()) * 4.0;
            for y in far as i32..lawn_top {
                canvas.set(x, y, Rgb::new(96, 140, 110));
            }
            for y in near as i32..lawn_top {
                canvas.set(x, y, Rgb::new(74, 122, 82));
            }
        }

        self.lawn(canvas, weather, time, lawn_top, Rgb::new(88, 140, 76), Rgb::new(60, 106, 56));

        // 草坪装饰：碎石堆 + 野花小点
        for k in 0..3i32 {
            let sx = (hash(k * 31 + 9, seed as i32) % w) as i32;
            let sy = lawn_top + 4 + (hash(k, seed as i32) % (h - lawn_top as u32 - 8)) as i32;
            canvas.set(sx, sy, Rgb::new(150, 148, 145));
            canvas.set(sx + 1, sy, Rgb::new(120, 118, 118));
            canvas.set(sx, sy + 1, Rgb::new(105, 104, 102));
        }
        for k in 0..14i32 {
            let fx = (hash(k * 17 + 3, seed as i32 + 1) % w) as i32;
            let fy = lawn_top + 3 + (hash(k * 5, seed as i32 + 2) % (h - lawn_top as u32 - 6)) as i32;
            let c = [
                Rgb::new(250, 250, 240),
                Rgb::new(250, 230, 120),
                Rgb::new(240, 170, 200),
            ][(k % 3) as usize];
            canvas.set(fx, fy, c);
        }
    }

    /// 林间（3/4 视角）：树冠 + 树干 + 苔藓地面，晴天光柱是真光源。
    fn bg_forest(
        &self,
        canvas: &mut Canvas,
        weather: &Weather,
        time: f32,
        seed: u32,
        lights: &mut Vec<LightSource>,
    ) {
        let w = canvas.width();
        let h = canvas.height();
        let lawn_top = (h as f32 * 0.52) as i32;

        // 林间背景
        for y in 0..lawn_top {
            let t = y as f32 / lawn_top as f32;
            let c = Rgb::new(38, 58, 46).lerp(Rgb::new(64, 88, 60), t);
            for x in 0..w as i32 {
                canvas.set(x, y, c);
            }
        }

        // 远景树干
        for k in 0..3 {
            let tx = (hash(k * 13 + 5, seed as i32) % (w - 8)) as i32 + 4;
            for y in 0..lawn_top + 2 {
                canvas.set(tx, y, Rgb::new(48, 42, 38));
                canvas.set(tx + 1, y, Rgb::new(42, 36, 33));
            }
        }
        // 近景树干（延伸到草地里，扎根感）
        for k in 0..2 {
            let tx = (hash(k * 29 + 11, seed as i32 + 7) % (w - 16)) as i32 + 8;
            let root_y = lawn_top + 6 + k as i32 * 10;
            for y in 0..root_y {
                let wob = ((y as f32 * 0.15 + k as f32).sin() * 1.2) as i32;
                canvas.set(tx + wob, y, Rgb::new(88, 68, 50));
                canvas.set(tx + wob + 1, y, Rgb::new(70, 54, 42));
                canvas.set(tx + wob + 2, y, Rgb::new(52, 40, 34));
            }
        }

        // 树冠
        for x in 0..w as i32 {
            let cb = (h as f32 * 0.2 + ((x as f32 * 0.06 + seed as f32 * 0.01).sin() * 5.0)) as i32;
            for y in 0..cb {
                let hs = hash(x, y);
                let c = if hs % 7 == 0 {
                    Rgb::new(40, 78, 44)
                } else if hs % 11 == 0 {
                    Rgb::new(24, 44, 30)
                } else {
                    Rgb::new(30, 56, 36)
                };
                canvas.set(x, y, c);
            }
        }

        self.lawn(canvas, weather, time, lawn_top, Rgb::new(62, 104, 56), Rgb::new(46, 80, 46));

        // 蘑菇
        for k in 0..2i32 {
            let mx = (hash(k * 41 + 13, seed as i32) % w) as i32;
            let my = lawn_top + 5 + (hash(k * 3, seed as i32 + 5) % (h - lawn_top as u32 - 9)) as i32;
            canvas.set(mx, my, Rgb::new(228, 226, 220)); // 柄
            canvas.set(mx - 1, my - 1, Rgb::new(200, 60, 50));
            canvas.set(mx, my - 1, Rgb::new(225, 80, 60));
            canvas.set(mx + 1, my - 1, Rgb::new(180, 50, 45));
            canvas.set(mx, my - 2, Rgb::new(235, 240, 235));
        }

        // 光柱：贴图画淡光带 + 挂真实光源照亮地面（只在晴朗白天）
        if weather.kind == Kind::Sunny && weather.darkness() < 0.35 {
            for k in 0..3i32 {
                let base = (hash(k * 17 + 3, seed as i32) % w) as f32;
                let sway = (time * 0.3 + k as f32 * 2.0).sin() * 4.0;
                for y in (h as f32 * 0.16) as i32..(h as i32) {
                    let x0 = base + sway + y as f32 * 0.28;
                    let fade = 1.0 - y as f32 / h as f32;
                    for dx in 0..3 {
                        canvas.blend(
                            x0 as i32 + dx,
                            y,
                            Rgb::new(255, 245, 200),
                            (fade * 40.0) as u8,
                        );
                    }
                    if y % 5 == 0 {
                        lights.push((x0 as i32 + 1, y, 5, (0.14, 0.13, 0.09)));
                    }
                }
            }
        }
    }

    /// 深海：水体渐变 + 水面波光 + 光柱 + 海带 + 上升的气泡。
    /// 天黑后水体转墨蓝，发光生物成为唯一光源——水下夜光花园。
    fn bg_abyss(
        &mut self,
        canvas: &mut Canvas,
        weather: &Weather,
        time: f32,
        seed: u32,
        dt: f32,
    ) {
        let w = canvas.width();
        let h = canvas.height();
        let sand_top = (h as f32 * 0.58) as i32;
        let dark = weather.darkness();

        // 水体：上明下暗（表层透光）
        let top = Rgb::new(46, 120, 150).lerp(Rgb::new(10, 26, 52), dark * 0.8);
        let bottom = Rgb::new(10, 36, 62).lerp(Rgb::new(4, 10, 26), dark * 0.8);
        for y in 0..sand_top {
            let t = y as f32 / sand_top as f32;
            let c = top.lerp(bottom, t);
            for x in 0..w as i32 {
                canvas.set(x, y, c);
            }
        }
        // 水面波光
        for x in 0..w as i32 {
            let s1 = ((x as f32 * 0.35 + time * 2.2).sin() * 0.5 + 0.5) * 130.0;
            let s2 = ((x as f32 * 0.13 - time * 1.4).sin() * 0.5 + 0.5) * 90.0;
            canvas.blend(x, 0, Rgb::new(170, 225, 235), (s1 * (1.0 - dark)) as u8);
            canvas.blend(x, 1, Rgb::new(140, 200, 220), (s2 * (1.0 - dark)) as u8);
        }
        // 阳光光柱（白天）
        if dark < 0.5 {
            for k in 0..3i32 {
                let base = (hash(k * 23 + 7, seed as i32) % w) as f32;
                let sway = (time * 0.4 + k as f32 * 2.1).sin() * 5.0;
                for y in 2..sand_top + 4 {
                    let x0 = base + sway + y as f32 * 0.2;
                    let fade = (1.0 - y as f32 / h as f32) * (1.0 - dark);
                    for dx in 0..3 {
                        canvas.blend(
                            x0 as i32 + dx,
                            y,
                            Rgb::new(190, 235, 240),
                            (fade * 34.0) as u8,
                        );
                    }
                }
            }
        }

        // 沙底
        for y in sand_top..h as i32 {
            for x in 0..w as i32 {
                let t = (y - sand_top) as f32 / (h as i32 - sand_top).max(1) as f32;
                let mut c = Rgb::new(142, 126, 96).lerp(Rgb::new(96, 84, 66), t);
                let hs = hash(x, y);
                if hs % 19 == 0 {
                    c = c.tint(0.8, 0.8, 0.82);
                } else if hs % 23 == 1 {
                    c = c.tint(1.15, 1.12, 1.05);
                }
                canvas.set(x, y, c);
            }
        }
        // 贝壳点缀
        for k in 0..3i32 {
            let sx = (hash(k * 37 + 5, seed as i32) % w) as i32;
            let sy = sand_top + 4 + (hash(k, seed as i32 + 9) % (h - sand_top as u32 - 8)) as i32;
            canvas.set(sx, sy, Rgb::new(230, 215, 200));
            canvas.set(sx + 1, sy, Rgb::new(200, 175, 170));
        }

        // 海带：从沙底长到中层，整条随水流摆
        for k in 0..4i32 {
            let kx = (hash(k * 41 + 3, seed as i32) % w) as i32;
            let kelp_h = (h as f32 * (0.28 + (hash(k, 5) % 100) as f32 / 500.0)) as i32;
            for i in 0..kelp_h {
                let y = sand_top + 2 - i;
                let sway = ((time * 0.8 + k as f32 * 1.7 + i as f32 * 0.12).sin()
                    * (i as f32 / kelp_h as f32)
                    * 3.5) as i32;
                let g = 60 + (hash(kx, i) % 20) as u8;
                canvas.set(kx + sway, y, Rgb::new(18, g, 52));
                if i % 5 == 2 {
                    canvas.set(kx + sway + 1, y, Rgb::new(24, g + 14, 60)); // 叶片
                }
            }
        }

        // 气泡：随机涌出，摇摆上升，到水面破掉
        if self.bubbles.len() < 10 && hash((time * 60.0) as i32, seed as i32) % 7 == 0 {
            let bx = (hash((time * 97.0) as i32, 3) % w) as f32;
            self.bubbles.push((bx, (sand_top - 1) as f32, 0.0));
        }
        for b in &mut self.bubbles {
            b.2 += dt * 4.0;
            b.1 -= (8.0 + (b.2 * 0.5).sin() * 2.0) * dt;
            b.0 += b.2.sin() * dt * 2.0;
        }
        self.bubbles.retain(|b| b.1 > 2.0);
        for &(x, y, _) in &self.bubbles {
            canvas.blend(x as i32, y as i32, Rgb::new(200, 235, 245), 150);
            canvas.blend(x as i32, y as i32 - 1, Rgb::new(160, 210, 230), 70);
        }
    }

    /// 石缝：一线天 + 岩壁 + 错落岩架。
    fn bg_crevice(&self, canvas: &mut Canvas, weather: &Weather, time: f32, seed: u32) {
        let w = canvas.width();
        let h = canvas.height();
        let horizon = (h as f32 * 0.2) as i32;

        self.sky(canvas, weather, time, horizon, false);

        for y in horizon..h as i32 {
            for x in 0..w as i32 {
                let patch = hash(x / 3, (y + seed as i32) / 2);
                let fine = (hash(x, y + seed as i32) % 5) as i32;
                let strata = ((y as f32 * 0.35).sin() * 8.0) as i32;
                let mut c = Rgb::new(
                    (100 + strata + fine) as u8,
                    (98 + strata + fine) as u8,
                    (108 + strata) as u8,
                );
                if patch % 31 == 0 {
                    c = c.tint(0.72, 0.72, 0.78);
                } else if patch % 37 == 3 {
                    c = c.tint(1.22, 1.2, 1.18);
                }
                canvas.set(x, y, c);
            }
        }
        for x in 0..w as i32 {
            canvas.set(x, horizon, Rgb::new(140, 138, 148));
        }
    }

    /// 通用天空：颜色随时刻连续变化，太阳/月亮沿弧线移动，
    /// 星星随黑暗度淡入，外加彩虹/流星/雁群。
    fn sky(&self, canvas: &mut Canvas, weather: &Weather, time: f32, bottom: i32, full: bool) {
        let w = canvas.width();
        let (sky_top, sky_bottom) = weather.sky_colors();
        let dark = weather.darkness();
        for y in 0..bottom {
            let t = y as f32 / bottom as f32;
            let c = sky_top.lerp(sky_bottom, t);
            for x in 0..w as i32 {
                canvas.set(x, y, c);
            }
        }

        // 星星随黑暗度淡入
        if dark > 0.05 {
            for i in 0..26u32 {
                let sx = (i * 37 + 11) % w;
                let sy = ((i * 23 + 5) % (bottom as u32).max(1)) as i32;
                if sy < bottom - 2 {
                    let twinkle = ((time * 2.0 + i as f32).sin() * 0.5 + 0.5) * 200.0 * dark;
                    canvas.blend(sx as i32, sy, Rgb::WHITE, twinkle as u8);
                }
            }
        }

        // 太阳（0..0.5）和月亮（0.5..1）沿弧线横穿天空
        let c = weather.clock;
        let arc = |t: f32| -> (i32, i32) {
            let x = w as f32 * (0.06 + 0.88 * t);
            let y = bottom as f32 * (0.85 - (t * std::f32::consts::PI).sin() * 0.62);
            (x as i32, y as i32)
        };
        if c < 0.52 && weather.kind == Kind::Sunny {
            let (sx, sy) = arc((c / 0.5).min(1.0));
            // 日出日落时太阳偏橙红
            let warm = 1.0 - ((c - 0.25).abs() / 0.25).powi(2) * 0.55;
            let core = Rgb::new(255, (236.0 * warm + 19.0) as u8, (120.0 * warm) as u8);
            soft_circle(canvas, sx, sy, 10, Rgb::new(255, 220, 150), 90);
            canvas.fill_circle(sx, sy, 5, core);
            canvas.fill_circle(sx, sy, 3, Rgb::new(255, 250, 200));
        }
        if c >= 0.5 {
            let (mx, my) = arc((c - 0.5) / 0.5);
            canvas.fill_circle(mx, my, 4, Rgb::new(230, 230, 210));
            canvas.fill_circle(mx - 2, my - 1, 3, sky_top);
            soft_circle(canvas, mx + 1, my, 8, Rgb::new(200, 205, 230), 45);
        }

        // 彩虹：雨后天晴的礼物，淡入淡出
        if weather.rainbow > 0.0 && dark < 0.4 && full {
            let strength = (weather.rainbow / 6.0).min(1.0) * (1.0 - dark * 2.0).max(0.0);
            let bands = [
                Rgb::new(255, 80, 80),
                Rgb::new(255, 160, 60),
                Rgb::new(255, 235, 90),
                Rgb::new(110, 220, 110),
                Rgb::new(90, 160, 250),
                Rgb::new(160, 110, 230),
            ];
            let cx = w as f32 / 2.0;
            let cy = bottom as f32 * 1.55;
            let r_out = w as f32 * 0.52;
            for y in 0..bottom {
                for x in 0..w as i32 {
                    let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                    let band = r_out - d;
                    if (0.0..6.0).contains(&band) {
                        let color = bands[band as usize];
                        canvas.blend(x, y, color, (strength * 70.0) as u8);
                    }
                }
            }
        }

        // 流星：亮头 + 渐隐尾迹
        for &(x, y, vx, vy, life) in &weather.shooting {
            let a = (life.min(0.5) * 2.0 * 255.0) as u8;
            let (xi, yi) = (x as i32, y as i32);
            canvas.blend(xi, yi, Rgb::WHITE, a);
            for t in 1..5 {
                let tx = x - vx * 0.016 * t as f32;
                let ty = y - vy * 0.016 * t as f32;
                canvas.blend(
                    tx as i32,
                    ty as i32,
                    Rgb::new(200, 210, 255),
                    (a as f32 * (1.0 - t as f32 / 5.0)) as u8,
                );
            }
            let _ = yi;
        }

        // 雁群：人字形小队掠过
        if let Some((bx, by, _)) = weather.birds {
            let dark_body = Rgb::new(50, 45, 55);
            for k in 0..5i32 {
                let ox = -k.abs() * 3;
                let oy = k.abs() * 2 - 1;
                let fx = bx as i32 + ox + (k.signum() * k.abs()) * 0;
                let side = if k % 2 == 0 { 1 } else { -1 };
                let fy = by as i32 + oy * side.max(0);
                let flap = ((time * 6.0 + k as f32).sin() > 0.0) as i32;
                canvas.set(fx - 1, fy - flap, dark_body);
                canvas.set(fx, fy, dark_body);
                canvas.set(fx + 1, fy - flap, dark_body);
            }
        }

        // 云：夜里变暗、变淡
        if full {
            let (cloud_color, cloud_alpha) = match weather.kind {
                Kind::Sunny => (
                    Rgb::WHITE.lerp(Rgb::new(60, 62, 92), dark),
                    (120.0 * (1.0 - dark * 0.5)) as u8,
                ),
                Kind::Rain => (Rgb::new(70, 76, 92), 170),
                Kind::Fog => (Rgb::WHITE, 0),
            };
            if cloud_alpha > 0 {
                for i in 0..4i32 {
                    let speed = 1.5 + i as f32 * 0.6;
                    let span = w as f32 + 60.0;
                    let cx = ((i as f32 * 53.0 + 17.0 + time * speed) % span) - 30.0;
                    let cy = (bottom as f32 * (0.12 + i as f32 * 0.1)) as i32;
                    for (ox, oy, r) in [(-8, 1, 5), (0, -2, 7), (7, 1, 5), (-3, 3, 4), (4, 3, 4)] {
                        soft_circle(canvas, cx as i32 + ox, cy + oy, r, cloud_color, cloud_alpha);
                    }
                }
            }
        }
    }

    /// 3/4 视角草坪：从 lawn_top 铺到画面底部，带透视渐变、杂色、随风散草。
    fn lawn(
        &self,
        canvas: &mut Canvas,
        weather: &Weather,
        time: f32,
        lawn_top: i32,
        near: Rgb,
        far: Rgb,
    ) {
        let w = canvas.width();
        let h = canvas.height();
        let span = (h as i32 - lawn_top).max(1) as f32;
        for y in lawn_top..h as i32 {
            let t = (y - lawn_top) as f32 / span;
            for x in 0..w as i32 {
                let mut c = far.lerp(near, t);
                let hs = hash(x, y);
                if hs % 13 == 0 {
                    c = c.tint(0.88, 0.9, 0.88);
                } else if hs % 17 == 1 {
                    c = c.tint(1.1, 1.08, 1.05);
                }
                canvas.set(x, y, c);
            }
        }
        // 散布草叶：越靠下（近处）越密
        for k in 0..(w as i32) {
            let hx = hash(k, 8888);
            if hx % 3 != 0 {
                continue;
            }
            let gx = (hx % w) as i32;
            let gy = lawn_top + 2 + (hash(k, 999) % (span as u32).max(1)) as i32;
            let blade_h = 1 + (hx / 7 % 2) as i32;
            let sway = (weather.wind() * gust(time, gx) * 1.1) as i32;
            let green = Rgb::new(
                52 + (hx % 28) as u8,
                118 + (hx / 11 % 42) as u8,
                48 + (hx / 5 % 18) as u8,
            );
            for i in 0..=blade_h {
                let ox = if i == blade_h { sway } else { 0 };
                canvas.set(gx + ox, gy - i, green);
            }
        }
    }

    /// 绘制植株：按品种分发到各科系渲染器。
    /// 盛开时返回 (花头位置, 主色, 是否发光基因, 品种)，供光照和粒子系统使用。
    fn draw_plant(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        wind: f32,
        time: f32,
        dark: f32,
        raining: bool,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        use crate::genetics::VARIETIES;
        let genes = &plant.genes;
        let stage = plant.stage();
        let variety = genes.petal_shape % VARIETIES;
        if stage == Stage::Seed {
            // 雷种在土里等雨，有自己的种壳画法（其余品种共用土包）
            if variety == 14 {
                return self.draw_stormflower(canvas, plant, px, ground_y, wind, time, 0.0, raining);
            }
            canvas.set(px, ground_y, Rgb::new(70, 50, 30));
            canvas.set(px - 1, ground_y, Rgb::new(70, 50, 30));
            canvas.set(px + 1, ground_y, Rgb::new(70, 50, 30));
            return None;
        }
        // 生长比例 0..1（发芽到成熟）
        let gp = ((plant.progress - 0.15) / 0.85).clamp(0.0, 1.0);
        match variety {
            12 => self.draw_moonflower(canvas, plant, px, ground_y, wind, time, gp, dark),
            13 => self.draw_sundial(canvas, plant, px, ground_y, wind, time, gp, dark),
            14 => self.draw_stormflower(canvas, plant, px, ground_y, wind, time, gp, raining),
            15 => self.draw_meteorflower(canvas, plant, px, ground_y, wind, time, gp),
            16 => self.draw_ephemeral(canvas, plant, px, ground_y, wind, time, gp, dark),
            4 => self.draw_grass(canvas, plant, px, ground_y, wind, gp),
            5 => self.draw_fern(canvas, plant, px, ground_y, wind, gp),
            6 => self.draw_eyeflower(canvas, plant, px, ground_y, wind, time, gp),
            7 => self.draw_tentacles(canvas, plant, px, ground_y, wind, time, gp),
            8 => self.draw_mandrake(canvas, plant, px, ground_y, gp),
            9 => self.draw_goldflower(canvas, plant, px, ground_y, wind, time, gp),
            10 => self.draw_anemone(canvas, plant, px, ground_y, wind, time, gp),
            11 => self.draw_coral(canvas, plant, px, ground_y, gp),
            _ => self.draw_flora(canvas, plant, px, ground_y, wind, gp),
        }
    }

    /// 花卉科（0-3）：茎 + 叶 + 花朵精灵，亚像素摇曳。
    fn draw_flora(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        wind: f32,
        gp: f32,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let stage = plant.stage();
        let stem_color = Rgb::new(60, 140, 60);
        let stem_dark = Rgb::new(45, 110, 48);
        let leaf_color = Rgb::new(80, 170, 70);
        let leaf_dark = Rgb::new(62, 145, 58);
        let full_h = genes.height as f32;
        let cur_h = (full_h * gp) as i32;
        let bend = wind * 2.4;
        let offf = |i: i32| -> f32 { bend * (i as f32 / full_h).powf(1.6) };

        for i in 0..cur_h {
            let o = offf(i);
            let base = o.floor();
            let frac = o - base;
            let x = px + base as i32;
            let y = ground_y - i;
            canvas.blend(x, y, stem_color, ((1.0 - frac) * 255.0) as u8);
            canvas.blend(x + 1, y, stem_color, (frac * 255.0) as u8);
            if cur_h > 9 && i < cur_h / 2 {
                canvas.blend(x + 1, y, stem_dark, ((1.0 - frac) * 200.0) as u8);
                canvas.blend(x + 2, y, stem_dark, (frac * 200.0) as u8);
            }
        }
        let top_x = px + offf(cur_h).round() as i32;
        let top_y = ground_y - cur_h;

        let tip_kick = (wind * 0.9).round() as i32;
        for &(frac_h, dir) in &[(0.35f32, -1i32), (0.6, 1)] {
            let li = (full_h * frac_h) as i32;
            if cur_h > li + 2 {
                let ly = ground_y - li;
                let lx = px + offf(li).round() as i32;
                canvas.set(lx + dir, ly, leaf_color);
                canvas.set(lx + dir * 2, ly, leaf_dark);
                canvas.set(lx + dir * 3 + tip_kick, ly - 1, leaf_color);
            }
        }

        match stage {
            Stage::Bud => {
                let bud = Rgb::from_hsv(genes.hue, genes.saturation, 0.55);
                let bud_hi = Rgb::from_hsv(genes.hue, genes.saturation * 0.8, 0.7);
                canvas.set(top_x, top_y - 1, bud_hi);
                canvas.set(top_x - 1, top_y, bud);
                canvas.set(top_x, top_y, bud);
                canvas.set(top_x + 1, top_y, bud);
                canvas.set(top_x, top_y + 1, Rgb::new(70, 150, 65));
                None
            }
            Stage::Bloom => {
                let main = Rgb::from_hsv(genes.hue, genes.saturation, 0.82);
                let sprite = &self.sprites[genes.petal_shape as usize % 4];
                let palette = Palette::new(vec![
                    main,
                    Rgb::from_hsv(genes.hue, genes.saturation * 0.6, 0.98),
                    Rgb::new(250, 220, 90),
                    Rgb::from_hsv(genes.hue, (genes.saturation * 1.1).min(1.0), 0.5),
                ]);
                sprite.draw(
                    canvas,
                    top_x - sprite.width() as i32 / 2,
                    top_y - sprite.height() as i32 / 2,
                    &palette,
                );
                Some((top_x, top_y, main, genes.glow, genes.petal_shape))
            }
            _ => None,
        }
    }

    /// 芒草（4）：一丛弧形草叶，成熟后抽出染着花色的穗。
    fn draw_grass(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        wind: f32,
        gp: f32,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let full_h = (genes.height as f32 * 1.1 * gp).max(2.0);
        let plume_c = Rgb::from_hsv(genes.hue, genes.saturation * 0.6, 0.92);

        for b in 0..6i32 {
            let dir = if b % 2 == 0 { 1.0 } else { -1.0 };
            let spread = dir * (1.0 + (b / 2) as f32);
            let len = full_h * (0.55 + (hash(b, px) % 45) as f32 / 100.0);
            let g = Rgb::new(
                74 + (hash(b, 3) % 24) as u8,
                140 + (hash(b, 7) % 36) as u8,
                58,
            );
            let mut tip = (px, ground_y);
            for i in 0..len as i32 {
                let t = i as f32 / len;
                let x = px as f32 + spread * t * t * 2.2 + wind * 2.2 * t * t;
                let y = ground_y - i;
                canvas.set(x as i32, y, g);
                tip = (x as i32, y);
            }
            // 前两条最长的草叶抽穗
            if b < 2 && plant.stage() == Stage::Bloom {
                for s in 0..3 {
                    canvas.set(tip.0 + (s % 2), tip.1 - s, plume_c);
                    canvas.set(tip.0 - 1 + (s % 2), tip.1 - s, plume_c.tint(0.85, 0.85, 0.85));
                }
            }
        }
        if plant.stage() == Stage::Bloom {
            Some((
                px + (wind * 1.5) as i32,
                ground_y - full_h as i32,
                plume_c,
                genes.glow,
                4,
            ))
        } else {
            None
        }
    }

    /// 蕨叶（5）：中轴 + 羽状小叶，花苞期是卷曲的拳芽，成熟后小叶背面结孢子。
    fn draw_fern(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        wind: f32,
        gp: f32,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let full_h = genes.height as f32;
        let sh = (full_h * gp) as i32;
        let spore = Rgb::from_hsv(genes.hue, 0.8, 0.72);
        let stalk = Rgb::new(48, 118, 56);
        let stage = plant.stage();

        for i in 0..sh {
            let o = wind * 1.0 * (i as f32 / full_h).powf(1.5);
            let x = px + o.round() as i32;
            let y = ground_y - i;
            canvas.set(x, y, stalk);
            // 羽状小叶对：向上递减，微微下垂
            if i >= 2 && i % 2 == 0 && i < sh - 1 {
                let pl = (((sh - i) as f32 * 0.5).min(4.0)) as i32 + 1;
                for d in 1..=pl {
                    let droop = d / 3;
                    canvas.set(x - d, y + droop, Rgb::new(58, 138, 64));
                    canvas.set(x + d, y + droop, Rgb::new(52, 128, 58));
                    if stage == Stage::Bloom && d == pl && (i / 2) % 2 == 0 {
                        canvas.set(x - d, y + droop + 1, spore);
                        canvas.set(x + d, y + droop + 1, spore);
                    }
                }
            }
        }
        // 拳芽：未展开的螺旋顶
        if stage == Stage::Bud && sh > 2 {
            let tx = px + (wind * 1.0).round() as i32;
            let ty = ground_y - sh;
            canvas.set(tx, ty, stalk);
            canvas.set(tx + 1, ty, Rgb::new(70, 150, 76));
            canvas.set(tx + 1, ty + 1, Rgb::new(70, 150, 76));
        }
        if stage == Stage::Bloom {
            Some((px, ground_y - sh, Rgb::from_hsv(genes.hue, 0.5, 0.8), genes.glow, 5))
        } else {
            None
        }
    }

    /// 眼球花（6）：粗壮短茎顶着一颗会眨眼的眼球，瞳孔四处张望。
    /// 花苞期眼睛闭着——开花的瞬间是它第一次睁眼。
    fn draw_eyeflower(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        wind: f32,
        time: f32,
        gp: f32,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let full_h = genes.height as f32;
        let sh = (full_h * 0.65 * gp) as i32;
        let stage = plant.stage();
        // 灰绿色肉质茎，两像素粗
        for i in 0..sh {
            let o = wind * 1.1 * (i as f32 / full_h).powf(1.6);
            let x = px + o.round() as i32;
            canvas.set(x, ground_y - i, Rgb::new(88, 112, 86));
            canvas.set(x + 1, ground_y - i, Rgb::new(66, 88, 68));
        }
        let hx = px + (wind * 0.9).round() as i32;
        let hy = ground_y - sh - 2;

        if stage == Stage::Bud || stage == Stage::Bloom {
            let iris = Rgb::from_hsv(genes.hue, genes.saturation, 0.7);
            // 眨眼节律：每株相位不同；花苞期永远闭眼
            let blink = ((time * 0.45 + px as f32 * 0.13).fract()) < 0.09;
            let open = stage == Stage::Bloom && !blink;

            canvas.fill_circle(hx, hy, 3, Rgb::new(238, 234, 224)); // 眼白
            if open {
                let look_x = ((time * 0.6 + px as f32 * 0.3).sin() * 1.4).round() as i32;
                let look_y = ((time * 0.4 + px as f32).cos() * 0.8).round() as i32;
                canvas.fill_circle(hx + look_x, hy + look_y, 2, iris);
                canvas.set(hx + look_x, hy + look_y, Rgb::new(20, 18, 24)); // 瞳孔
                canvas.set(hx + look_x - 1, hy + look_y - 1, Rgb::WHITE); // 高光
                // 眼角血丝
                canvas.set(hx - 3, hy, Rgb::new(205, 130, 130));
                canvas.set(hx + 3, hy, Rgb::new(205, 130, 130));
            } else {
                // 闭眼：一条水平眼睑线
                for dx in -2..=2 {
                    canvas.set(hx + dx, hy, Rgb::new(120, 128, 104));
                }
            }
        }
        if stage == Stage::Bloom {
            Some((
                hx,
                hy,
                Rgb::from_hsv(genes.hue, genes.saturation, 0.7),
                genes.glow,
                6,
            ))
        } else {
            None
        }
    }

    /// 触手藤（7）：一簇不停蠕动的触须，带吸盘，尖端有夜光球。
    fn draw_tentacles(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        wind: f32,
        time: f32,
        gp: f32,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let full_h = genes.height as f32;
        let stage = plant.stage();
        let base_c = Rgb::from_hsv(genes.hue, genes.saturation, 0.42);
        let tip_c = Rgb::from_hsv(genes.hue, genes.saturation * 0.7, 0.85);

        for t_i in 0..4i32 {
            let phase = t_i as f32 * 1.7 + px as f32 * 0.1;
            let len = (full_h * gp * (0.55 + t_i as f32 * 0.14)) as i32;
            let base_x = px + t_i * 2 - 3;
            let mut tip = (base_x, ground_y);
            for i in 0..len {
                let t = i as f32 / len.max(1) as f32;
                let x = base_x as f32 + ((time * 1.6 + phase + t * 4.2).sin() * 3.0 + wind) * t;
                let y = ground_y - i;
                let c = base_c.lerp(tip_c, t);
                canvas.set(x as i32, y, c);
                if i % 3 == 1 && t > 0.25 {
                    canvas.set(x as i32 - 1, y, c.tint(1.35, 1.35, 1.35)); // 吸盘
                }
                tip = (x as i32, y);
            }
            // 成熟触须的尖端凝着光球
            if stage == Stage::Bloom && len > 3 {
                canvas.set(tip.0, tip.1 - 1, tip_c.tint(1.3, 1.3, 1.3));
            }
        }
        if stage == Stage::Bloom {
            Some((px, ground_y - (full_h * gp * 0.9) as i32, tip_c, genes.glow, 7))
        } else {
            None
        }
    }

    /// 曼德拉草（8）：平时只是一丛叶子，开花时人形根须半钻出土，
    /// 头顶还开着一朵小花。
    fn draw_mandrake(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        gp: f32,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let stage = plant.stage();
        let emerged = stage == Stage::Bloom;
        let body = Rgb::new(206, 180, 138);
        let body_dark = Rgb::new(172, 146, 108);
        // 根身钻出后叶冠被顶高
        let crown_y = if emerged { ground_y - 6 } else { ground_y };

        if emerged {
            // 人形根：圆头方身，两只黑豆眼 + 张开的小嘴
            let widths = [2, 3, 3, 3, 2, 2];
            for (dy, &hw) in widths.iter().enumerate() {
                for dx in -hw..=hw {
                    let c = if dx > hw - 2 { body_dark } else { body };
                    canvas.set(px + dx, ground_y - 5 + dy as i32, c);
                }
            }
            canvas.set(px - 1, ground_y - 4, Rgb::new(40, 32, 28));
            canvas.set(px + 1, ground_y - 4, Rgb::new(40, 32, 28));
            canvas.set(px, ground_y - 2, Rgb::new(110, 80, 62)); // 嘴
            // 小短臂
            canvas.set(px - 3, ground_y - 3, body);
            canvas.set(px + 3, ground_y - 3, body_dark);
        }

        // 叶冠：五片小叶
        let leaves = (2.0 + gp * 3.0) as i32;
        for l in 0..leaves {
            let dir = l - leaves / 2;
            let lh = 2 + (hash(l, px) % 2) as i32;
            for i in 0..lh {
                canvas.set(
                    px + dir + dir.signum() * i / 2,
                    crown_y - 1 - i,
                    Rgb::new(58, 132, 60),
                );
            }
        }
        if emerged {
            // 头顶的花
            let bloom_c = Rgb::from_hsv(genes.hue, genes.saturation, 0.85);
            canvas.set(px, crown_y - 4, bloom_c);
            canvas.set(px - 1, crown_y - 3, bloom_c);
            canvas.set(px + 1, crown_y - 3, bloom_c.tint(0.8, 0.8, 0.8));
            canvas.set(px, crown_y - 3, Rgb::new(250, 225, 110));
            Some((px, ground_y - 4, body, genes.glow, 8))
        } else {
            None
        }
    }

    /// 贤者金花（9）：几何菱形花瓣 + 缓缓旋转的炼金法阵光点，
    /// 昼夜都散发着淡淡的金光。
    fn draw_goldflower(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        wind: f32,
        time: f32,
        gp: f32,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let full_h = genes.height as f32;
        let cur_h = (full_h * gp) as i32;
        let stage = plant.stage();
        let gold = Rgb::new(232, 192, 88);
        let gold_hi = Rgb::new(252, 236, 160);
        let tinted = gold.lerp(Rgb::from_hsv(genes.hue, genes.saturation, 0.8), 0.3);

        for i in 0..cur_h {
            let o = wind * 1.6 * (i as f32 / full_h).powf(1.6);
            canvas.set(px + o.round() as i32, ground_y - i, Rgb::new(120, 130, 62));
        }
        let tx = px + (wind * 1.2).round() as i32;
        let ty = ground_y - cur_h;

        match stage {
            Stage::Bud => {
                canvas.set(tx, ty, gold);
                canvas.set(tx, ty - 1, gold_hi);
                None
            }
            Stage::Bloom => {
                // 菱形四瓣
                for d in 1..=3i32 {
                    let c = if d == 3 { tinted } else { gold };
                    canvas.set(tx, ty - d, c);
                    canvas.set(tx, ty + d - 1, c);
                    canvas.set(tx - d, ty, c);
                    canvas.set(tx + d, ty, c);
                }
                canvas.set(tx, ty, gold_hi);
                canvas.set(tx - 1, ty - 1, gold.tint(1.1, 1.1, 1.05));
                canvas.set(tx + 1, ty - 1, gold.tint(0.9, 0.9, 0.85));
                // 旋转的炼金法阵：六个光点绕花头缓转
                for k in 0..6 {
                    let ang = time * 0.9 + k as f32 * std::f32::consts::TAU / 6.0;
                    let ox = (ang.cos() * 5.5).round() as i32;
                    let oy = (ang.sin() * 5.5).round() as i32;
                    canvas.blend(tx + ox, ty + oy, Rgb::new(255, 236, 150), 130);
                }
                Some((tx, ty, gold, genes.glow, 9))
            }
            _ => None,
        }
    }

    /// 海葵（10）：底座圆顶 + 一圈随水流摆动的触手花瓣。
    fn draw_anemone(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        wind: f32,
        time: f32,
        gp: f32,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let stage = plant.stage();
        let base_c = Rgb::from_hsv(genes.hue, genes.saturation * 0.9, 0.45);
        let tip_c = Rgb::from_hsv(genes.hue, genes.saturation * 0.5, 0.95);

        // 底座
        for dy in 0..3i32 {
            let hw = 3 - dy;
            for dx in -hw..=hw {
                canvas.set(px + dx, ground_y - dy, base_c.tint(0.9, 0.9, 0.95));
            }
        }
        // 触手花瓣
        let n = 7;
        let len = (genes.height as f32 * 0.5 * gp).max(2.0);
        for k in 0..n {
            let ang = (k as f32 / (n - 1) as f32 - 0.5) * 2.2;
            for i in 0..len as i32 {
                let t = i as f32 / len;
                let sway = (time * 1.8 + k as f32 * 0.9).sin() * 1.6 * t + wind * 0.6 * t;
                let x = px as f32 + ang.sin() * i as f32 * 0.75 + sway;
                let y = ground_y as f32 - 3.0 - ang.cos() * i as f32;
                canvas.set(x as i32, y as i32, base_c.lerp(tip_c, t));
            }
        }
        if stage == Stage::Bloom {
            Some((px, ground_y - len as i32 - 3, tip_c, genes.glow, 10))
        } else {
            None
        }
    }

    /// 月见花（12）：白天是闭合的纺锤形花苞，入夜绽放成银白色的星形花，
    /// 花瓣随黑暗度连续张开——想看它开，你得夜里来。
    fn draw_moonflower(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        wind: f32,
        time: f32,
        gp: f32,
        dark: f32,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let full_h = genes.height as f32;
        let cur_h = (full_h * gp) as i32;
        let stage = plant.stage();
        // 张开度：夜色越深开得越大（0.35 以下完全闭合）
        let open = ((dark - 0.35) / 0.5).clamp(0.0, 1.0);

        for i in 0..cur_h {
            let o = wind * 1.5 * (i as f32 / full_h).powf(1.6);
            canvas.set(px + o.round() as i32, ground_y - i, Rgb::new(74, 118, 92));
        }
        let tx = px + (wind * 1.2).round() as i32;
        let ty = ground_y - cur_h;
        // 细长的叶片
        if cur_h > 5 {
            let ly = ground_y - cur_h / 2;
            for d in 1..=3 {
                canvas.set(px - d, ly + d / 2, Rgb::new(66, 108, 84));
                canvas.set(px + d, ly - 1 + d / 2, Rgb::new(58, 96, 76));
            }
        }
        if stage != Stage::Bloom {
            return None;
        }

        let pale = Rgb::from_hsv(genes.hue, genes.saturation * 0.35, 0.97);
        let deep = Rgb::from_hsv(genes.hue, genes.saturation * 0.6, 0.66);
        if open < 0.05 {
            // 白天：闭合的纺锤花苞
            for d in -2..=2i32 {
                let w = if d.abs() >= 2 { 0 } else { 1 };
                for dx in -w..=w {
                    canvas.set(tx + dx, ty + d, deep);
                }
            }
            canvas.set(tx, ty - 3, deep.tint(1.15, 1.15, 1.15));
            return Some((tx, ty, deep, genes.glow, 12));
        }
        // 夜里：六瓣星形，花瓣长度随张开度伸展
        let petal_len = 1.0 + open * 3.2;
        for k in 0..6 {
            let ang = k as f32 * std::f32::consts::TAU / 6.0 + time * 0.12;
            for i in 1..=petal_len as i32 {
                let t = i as f32 / petal_len;
                let x = tx + (ang.cos() * i as f32).round() as i32;
                let y = ty + (ang.sin() * i as f32 * 0.8).round() as i32;
                canvas.set(x, y, deep.lerp(pale, t));
            }
        }
        canvas.fill_circle(tx, ty, 1, Rgb::new(252, 246, 205));
        Some((tx, ty, pale, genes.glow, 12))
    }

    /// 日晷花（13）：花盘追着太阳/月亮转（向日性），
    /// 盘面刻着放射状纹路，正午开到最大、夜里半合。
    fn draw_sundial(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        wind: f32,
        time: f32,
        gp: f32,
        dark: f32,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let full_h = genes.height as f32;
        let cur_h = (full_h * gp) as i32;
        let stage = plant.stage();

        for i in 0..cur_h {
            let o = wind * 1.4 * (i as f32 / full_h).powf(1.6);
            let x = px + o.round() as i32;
            canvas.set(x, ground_y - i, Rgb::new(92, 132, 58));
            if i < cur_h / 2 {
                canvas.set(x + 1, ground_y - i, Rgb::new(74, 108, 48));
            }
        }
        let tx = px + (wind * 1.1).round() as i32;
        let ty = ground_y - cur_h;
        if cur_h > 5 {
            let ly = ground_y - cur_h / 2;
            for d in 1..=3 {
                canvas.set(px - d, ly, Rgb::new(84, 142, 62));
                canvas.set(px + d, ly + 1, Rgb::new(70, 124, 54));
            }
        }
        if stage != Stage::Bloom {
            return None;
        }

        // 向日：花盘朝向天体（白天偏太阳、夜里偏月亮），用倾斜偏移表达
        let lean = if dark < 0.5 { -1 } else { 1 };
        let cx = tx + lean;
        let petal = Rgb::from_hsv(genes.hue, genes.saturation, 0.86);
        let petal_hi = Rgb::from_hsv(genes.hue, genes.saturation * 0.5, 1.0);
        // 夜里半合
        let spread = if dark > 0.6 { 2 } else { 3 };
        for k in 0..12 {
            let ang = k as f32 * std::f32::consts::TAU / 12.0;
            for i in 2..=spread {
                let x = cx + (ang.cos() * i as f32).round() as i32;
                let y = ty + (ang.sin() * i as f32 * 0.85).round() as i32;
                canvas.set(x, y, if k % 2 == 0 { petal } else { petal_hi });
            }
        }
        // 盘心 + 时针状指针（缓慢转动，像日晷）
        canvas.fill_circle(cx, ty, 1, Rgb::new(96, 70, 40));
        let hand = time * 0.25;
        canvas.set(
            cx + (hand.cos() * 2.0).round() as i32,
            ty + (hand.sin() * 1.6).round() as i32,
            Rgb::new(250, 234, 168),
        );
        Some((cx, ty, petal, genes.glow, 13))
    }

    /// 雷纹花（14）：种子期在土里静静等雨（画成一颗带电纹的种壳），
    /// 一旦下雨就抽出锯齿状枝干，花瓣是分叉的闪电纹，雨中不断迸发电弧。
    #[allow(clippy::too_many_arguments)]
    fn draw_stormflower(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        wind: f32,
        time: f32,
        gp: f32,
        raining: bool,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let stage = plant.stage();
        let arc = Rgb::new(226, 238, 255);
        let bolt = Rgb::from_hsv(genes.hue, genes.saturation * 0.5, 1.0);
        let stalk = Rgb::new(78, 84, 104);

        if stage == Stage::Seed {
            // 等雨的种壳：暗色小球，表面电纹偶尔亮一下
            canvas.set(px, ground_y, Rgb::new(62, 66, 84));
            canvas.set(px - 1, ground_y, Rgb::new(50, 54, 70));

            canvas.set(px + 1, ground_y, Rgb::new(50, 54, 70));
            let pulse = ((time * 2.2 + px as f32).sin() * 0.5 + 0.5) * 190.0;
            canvas.blend(px, ground_y - 1, arc, pulse as u8);
            return None;
        }

        let full_h = genes.height as f32;
        let cur_h = (full_h * gp) as i32;
        // 锯齿状茎：每隔几格折一次，像一道定住的闪电
        let mut zx = px;
        for i in 0..cur_h {
            if i % 3 == 0 {
                zx += if (i / 3) % 2 == 0 { 1 } else { -1 };
            }
            let o = wind * 1.2 * (i as f32 / full_h).powf(1.6);
            canvas.set(zx + o.round() as i32, ground_y - i, stalk);
        }
        let tx = zx + (wind * 1.0).round() as i32;
        let ty = ground_y - cur_h;

        if stage != Stage::Bloom {
            canvas.set(tx, ty, bolt.tint(0.6, 0.6, 0.7));
            return None;
        }

        // 花：四道分叉的闪电纹从中心射出
        for k in 0..4 {
            let ang = k as f32 * std::f32::consts::TAU / 4.0 + 0.4;
            let mut x = tx as f32;
            let mut y = ty as f32;
            for i in 0..4 {
                x += ang.cos() + if i % 2 == 0 { 0.6 } else { -0.6 };
                y += ang.sin() * 0.8;
                canvas.set(x as i32, y as i32, if i < 2 { bolt } else { arc });
            }
        }
        canvas.fill_circle(tx, ty, 1, arc);
        // 雨中迸发电弧：短促的随机火花
        if raining {
            let t = (time * 6.0 + px as f32).fract();
            if t < 0.35 {
                let sx = tx + ((time * 13.0).sin() * 4.0) as i32;
                let sy = ty + ((time * 17.0).cos() * 3.0) as i32;
                canvas.blend(sx, sy, arc, 235);
                canvas.blend(sx, sy - 1, arc, 140);
            }
        }
        Some((tx, ty, bolt, genes.glow, 14))
    }

    /// 陨星花（15）：流星带来的种。天生自发光，花瓣是四芒星，
    /// 星芒长度随时间脉动闪烁，脚下落着一圈星尘。
    #[allow(clippy::too_many_arguments)]
    fn draw_meteorflower(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        wind: f32,
        time: f32,
        gp: f32,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let stage = plant.stage();
        let full_h = genes.height as f32;
        let cur_h = (full_h * gp) as i32;
        let core = Rgb::from_hsv(genes.hue, genes.saturation * 0.5, 1.0);
        let halo = Rgb::from_hsv(genes.hue, genes.saturation, 0.8);

        for i in 0..cur_h {
            let o = wind * 1.3 * (i as f32 / full_h).powf(1.6);
            canvas.set(px + o.round() as i32, ground_y - i, Rgb::new(70, 78, 96));
        }
        let tx = px + (wind * 1.0).round() as i32;
        let ty = ground_y - cur_h;
        // 星尘：脚下一圈缓慢明灭的碎屑
        for k in 0..4i32 {
            let a = (time * 0.5 + k as f32 * 1.6).sin();
            canvas.blend(
                px + (a * 3.0) as i32,
                ground_y - (k % 2),
                core,
                (((a * 0.5 + 0.5) * 120.0) as u8).max(30),
            );
        }
        if stage != Stage::Bloom {
            if cur_h > 0 {
                canvas.blend(tx, ty, halo, 200);
            }
            return None;
        }

        // 四芒星：纵横两轴长、对角短，长度脉动
        let pulse = (time * 1.6 + px as f32 * 0.4).sin() * 0.5 + 0.5;
        let long = 3 + (pulse * 2.0) as i32;
        for d in 1..=long {
            let t = d as f32 / long as f32;
            let c = core.lerp(halo, t);
            canvas.set(tx, ty - d, c);
            canvas.set(tx, ty + d, c);
            canvas.set(tx - d, ty, c);
            canvas.set(tx + d, ty, c);
        }
        for d in 1..=1 {
            canvas.set(tx - d, ty - d, halo);
            canvas.set(tx + d, ty - d, halo);
            canvas.set(tx - d, ty + d, halo);
            canvas.set(tx + d, ty + d, halo);
        }
        canvas.fill_circle(tx, ty, 1, Rgb::WHITE);
        Some((tx, ty, core, true, 15)) // 天生自发光
    }

    /// 昙花（16）：白天是低垂的纺锤花苞，入夜绽放成一朵巨大的白色喇叭花，
    /// 层层花瓣向外舒展、丝状花蕊四射——天亮就谢（留种，见 settle_ephemeral）。
    #[allow(clippy::too_many_arguments)]
    fn draw_ephemeral(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        wind: f32,
        time: f32,
        gp: f32,
        dark: f32,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let stage = plant.stage();
        let full_h = genes.height as f32;
        let cur_h = (full_h * gp) as i32;
        // 绽放度：入夜后缓缓张开
        let open = ((dark - 0.4) / 0.45).clamp(0.0, 1.0);
        let stalk = Rgb::new(62, 106, 72);

        for i in 0..cur_h {
            let o = wind * 1.6 * (i as f32 / full_h).powf(1.6);
            canvas.set(px + o.round() as i32, ground_y - i, stalk);
        }
        let tx = px + (wind * 1.3).round() as i32;
        let ty = ground_y - cur_h;
        // 厚实的肉质叶
        if cur_h > 5 {
            let ly = ground_y - cur_h / 3;
            for d in 1..=3 {
                canvas.set(px - d, ly + d / 3, Rgb::new(56, 116, 74));
                canvas.set(px + d, ly - 1 + d / 3, Rgb::new(48, 100, 66));
            }
        }
        if stage != Stage::Bloom {
            return None;
        }

        let petal = Rgb::from_hsv(genes.hue, genes.saturation * 0.18, 0.99);
        let shade = Rgb::from_hsv(genes.hue, genes.saturation * 0.4, 0.74);
        let throat = Rgb::from_hsv(genes.hue, genes.saturation * 0.55, 0.88);

        if open < 0.05 {
            // 白天：低垂的长纺锤花苞
            for d in 0..5i32 {
                let w = if d == 0 || d == 4 { 0 } else { 1 };
                for dx in -w..=w {
                    canvas.set(tx + dx + d / 3, ty + d, shade);
                }
            }
            return Some((tx, ty + 2, shade, genes.glow, 16));
        }

        // 夜里：巨大的喇叭花，两层花瓣 + 丝状花蕊
        let len = 2.0 + open * 4.5;
        for k in 0..10 {
            let ang = k as f32 * std::f32::consts::TAU / 10.0 + time * 0.05;
            for i in 1..=len as i32 {
                let t = i as f32 / len;
                let x = tx + (ang.cos() * i as f32 * 1.1).round() as i32;
                let y = ty + (ang.sin() * i as f32 * 0.72).round() as i32;
                canvas.set(x, y, if t > 0.75 { petal } else { shade.lerp(petal, t) });
            }
        }
        // 内层短瓣
        for k in 0..8 {
            let ang = k as f32 * std::f32::consts::TAU / 8.0 + 0.35;
            for i in 1..=(len * 0.45) as i32 {
                canvas.set(
                    tx + (ang.cos() * i as f32).round() as i32,
                    ty + (ang.sin() * i as f32 * 0.7).round() as i32,
                    throat,
                );
            }
        }
        // 花蕊：一簇细丝，尖端点着金黄
        for k in 0..5 {
            let ang = -1.2 + k as f32 * 0.28;
            let l = 2.0 + open * 1.6;
            let ex = tx + (ang.cos() * l).round() as i32;
            let ey = ty + (ang.sin() * l).round() as i32;
            canvas.set(ex, ey, Rgb::new(250, 240, 190));
            canvas.set(ex, ey - 1, Rgb::new(252, 226, 120));
        }
        canvas.fill_circle(tx, ty, 1, Rgb::new(255, 252, 240));
        Some((tx, ty, petal, genes.glow, 16))
    }

    /// 珊瑚枝（11）：分叉的枝干，末端缀着发光的珊瑚虫。
    fn draw_coral(
        &self,
        canvas: &mut Canvas,
        plant: &Plant,
        px: i32,
        ground_y: i32,
        gp: f32,
    ) -> Option<(i32, i32, Rgb, bool, u8)> {
        let genes = &plant.genes;
        let stage = plant.stage();
        let ch = (genes.height as f32 * 0.85 * gp) as i32;
        let main = Rgb::from_hsv(genes.hue, genes.saturation, 0.72);
        let dark = Rgb::from_hsv(genes.hue, genes.saturation, 0.5);
        let polyp = Rgb::from_hsv(genes.hue, genes.saturation * 0.4, 0.98);

        // 主干
        for i in 0..(ch * 2 / 3).max(1) {
            canvas.set(px, ground_y - i, main);
            canvas.set(px + 1, ground_y - i, dark);
        }
        // 斜伸的分枝
        for &(frac, dir) in &[(0.3f32, -1i32), (0.5, 1), (0.66, -1)] {
            let by = ground_y - (ch as f32 * frac) as i32;
            let blen = (ch as f32 * 0.4) as i32;
            for i in 0..blen {
                canvas.set(px + dir * i, by - i, main);
            }
            if stage == Stage::Bloom && blen > 0 {
                canvas.set(px + dir * blen, by - blen, polyp);
            }
        }
        canvas.set(px, ground_y - ch * 2 / 3, polyp);
        if stage == Stage::Bloom {
            Some((px, ground_y - ch * 2 / 3, polyp, genes.glow, 11))
        } else {
            None
        }
    }
}
