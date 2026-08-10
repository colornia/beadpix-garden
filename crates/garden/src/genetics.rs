use rand::Rng;
use serde::{Deserialize, Serialize};

/// 一株花的基因。杂交时逐项混合 + 小概率变异，
/// 花色/花型/株高都直接反映在渲染上。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Genes {
    /// 花色色相 0-360（HSV）
    pub hue: f32,
    /// 饱和度 0.5-1.0
    pub saturation: f32,
    /// 花型（对应不同花朵精灵图）
    pub petal_shape: u8,
    /// 成熟株高（像素）
    pub height: u8,
    /// 生长速度倍率 0.6-1.6
    pub growth_rate: f32,
    /// 稀有变异：发光花。只能通过杂交突变获得，白天也泛着光。
    #[serde(default)]
    pub glow: bool,
}

/// 品种总数：4 花卉 + 2 草蕨 + 2 异界 + 2 炼金 + 2 水生 + 2 天时
pub const VARIETIES: u8 = 14;

/// 五大科系。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// 花卉：常规花朵
    Flora,
    /// 草蕨：芒草、蕨类
    Herb,
    /// 异界：克苏鲁风的活体植物
    Eldritch,
    /// 炼金：西方炼金术传说植物
    Alchemy,
    /// 水生：深海群系原生
    Aquatic,
    /// 天时：开合绑定昼夜/天气，得赶上那一刻
    Celestial,
}

pub fn family(variety: u8) -> Family {
    match variety % VARIETIES {
        0..=3 => Family::Flora,
        4 | 5 => Family::Herb,
        6 | 7 => Family::Eldritch,
        8 | 9 => Family::Alchemy,
        10 | 11 => Family::Aquatic,
        _ => Family::Celestial,
    }
}

pub fn variety_name(v: u8) -> &'static str {
    match v % VARIETIES {
        0 => "圆瓣",
        1 => "四瓣",
        2 => "星芒",
        3 => "重瓣",
        4 => "芒草",
        5 => "蕨叶",
        6 => "眼球花",
        7 => "触手藤",
        8 => "曼德拉",
        9 => "贤者金花",
        10 => "海葵",
        11 => "珊瑚",
        12 => "月见花",
        _ => "日晷花",
    }
}

/// 色相 → 中文颜色名。
pub fn hue_name(hue: f32) -> &'static str {
    match hue.rem_euclid(360.0) as u32 {
        0..=14 | 345..=360 => "绯红",
        15..=44 => "橙",
        45..=69 => "金黄",
        70..=159 => "翠绿",
        160..=199 => "青",
        200..=249 => "湛蓝",
        250..=289 => "紫",
        _ => "品红",
    }
}

/// 本周客座品种：野生种子池里额外轮换的那一个。
/// 只从"够得着"的常见/角色层里选——传说层始终得靠自己育出来。
pub fn guest_variety(week: u64) -> u8 {
    const GUESTS: [u8; 6] = [2, 3, 4, 5, 10, 11];
    GUESTS[(week % GUESTS.len() as u64) as usize]
}

/// 子代预测：杂交前显示给玩家的遗传范围。
/// 与 `Genes::cross` 共用同一套推导（那边 roll 具体值，这边给区间），
/// 预览和结果才互相可信——这是"可规划"的基础。
pub struct ChildForecast {
    /// 花色落点弧段（沿色环短弧从 lo 到 hi）
    pub hue_lo: f32,
    pub hue_hi: f32,
    pub height_lo: u8,
    pub height_hi: u8,
    /// 发光概率：双亲 65% / 单亲 40% / 无 2%
    pub glow_chance: f32,
    /// 异界/炼金科系觉醒概率
    pub rare_chance: f32,
}

pub fn predict_child(a: &Genes, b: &Genes) -> ChildForecast {
    let dh = ((b.hue - a.hue + 540.0) % 360.0) - 180.0;
    ChildForecast {
        hue_lo: (a.hue + dh * 0.3).rem_euclid(360.0),
        hue_hi: (a.hue + dh * 0.7).rem_euclid(360.0),
        height_lo: a.height.min(b.height),
        height_hi: a.height.max(b.height),
        glow_chance: if a.glow && b.glow {
            0.65
        } else if a.glow || b.glow {
            0.4
        } else {
            0.02
        },
        rare_chance: 0.025,
    }
}

/// 图鉴品种名（不含发光前缀，发光是同品种的稀有形态）。
pub fn species_name(genes: &Genes) -> String {
    format!("{}{}", hue_name(genes.hue), variety_name(genes.petal_shape))
}

/// 展示名：发光形态带 ✦ 前缀。
pub fn display_name(genes: &Genes) -> String {
    if genes.glow {
        format!("✦发光·{}", species_name(genes))
    } else {
        species_name(genes)
    }
}

impl Genes {
    /// 野生种子：品种池由群系决定（草甸有芒草、林间有蕨、深海是水生），
    /// 永远不发光，稀有科系（异界/炼金）只能靠杂交突变。
    pub fn wild(rng: &mut impl Rng, biome: crate::garden::Biome) -> Self {
        use crate::garden::Biome;
        let base: &[u8] = match biome {
            Biome::Meadow => &[0, 1, 4],
            Biome::Forest => &[0, 1, 5],
            Biome::Crevice => &[0, 1, 4],
            Biome::Abyss => &[10, 11],
        };
        // 本周客座品种：每真实周轮换一次，给长期回访一个理由
        let mut pool = base.to_vec();
        pool.push(guest_variety(crate::quest::current_week()));
        Genes {
            hue: rng.gen_range(0.0..360.0),
            saturation: rng.gen_range(0.5..0.85),
            petal_shape: pool[rng.gen_range(0..pool.len())],
            height: rng.gen_range(10..16),
            growth_rate: rng.gen_range(0.8..1.1),
            glow: false,
        }
    }

    /// 杂交：每个基因随机取自父母一方并混合，5% 概率突变。
    /// 稀有花型和极端株高只能靠杂交/突变获得。
    pub fn cross(a: &Genes, b: &Genes, rng: &mut impl Rng) -> Self {
        fn mix(x: f32, y: f32, rng: &mut impl Rng) -> f32 {
            x + (y - x) * rng.gen_range(0.3..0.7)
        }
        // 色相在色环上取较短弧插值，红+蓝才会出紫而不是绕到绿
        let dh = ((b.hue - a.hue + 540.0) % 360.0) - 180.0;
        let mut hue = (a.hue + dh * rng.gen_range(0.3..0.7)).rem_euclid(360.0);
        let mut saturation = mix(a.saturation, b.saturation, rng);
        let mut petal_shape = if rng.gen_bool(0.5) { a.petal_shape } else { b.petal_shape };
        let mut height = if rng.gen_bool(0.5) { a.height } else { b.height };
        let growth_rate = mix(a.growth_rate, b.growth_rate, rng).clamp(0.6, 1.6);

        if rng.gen_bool(0.05) {
            hue = rng.gen_range(0.0..360.0);
        }
        if rng.gen_bool(0.05) {
            saturation = rng.gen_range(0.5..1.0);
        }
        // 品种突变：普通池常见，异界/炼金极稀有，水生只从水生血统变出
        if rng.gen_bool(0.06) {
            petal_shape = [0, 1, 2, 3, 4, 5][rng.gen_range(0..6)];
        } else if rng.gen_bool(0.025) {
            petal_shape = [6, 7, 8, 9][rng.gen_range(0..4)]; // 异界 / 炼金觉醒
        } else if rng.gen_bool(0.02) {
            petal_shape = [12, 13][rng.gen_range(0..2)]; // 天时科觉醒
        }
        if (a.petal_shape >= 10 || b.petal_shape >= 10) && rng.gen_bool(0.35) {
            petal_shape = 10 + rng.gen_range(0..2); // 水生血统强遗传
        }
        if rng.gen_bool(0.05) {
            height = rng.gen_range(8..22);
        }
        // 发光基因：双亲发光 65% / 单亲 40% / 无 2% 突变觉醒（与 predict_child 一致）
        let glow = if a.glow && b.glow {
            rng.gen_bool(0.65)
        } else if a.glow || b.glow {
            rng.gen_bool(0.4)
        } else {
            rng.gen_bool(0.02)
        };

        Genes {
            hue,
            saturation,
            petal_shape,
            height: height.clamp(8, 22),
            growth_rate,
            glow,
        }
    }
}
