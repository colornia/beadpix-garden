//! 委托：够得着又永远差一点的追逐脊椎。
//!
//! 委托描述一个**表型条件**（不是具体某株），玩家培出满足条件的花即达成。
//! 因为色彩连续 × 品种组合，目标空间无限——图鉴集齐只是开场。
//! 好委托指向"碰轴"的表型（发光的、跨科系的），而不是纯换色目标。

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::garden::{Garden, Stage};
use crate::genetics::{family, hue_name, variety_name, Family, Genes};

/// 委托的条件项：全部满足才算达成。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Requirement {
    /// 色相落在 [lo, hi]（沿色环，可跨 0）
    HueBand { lo: f32, hi: f32, label: String },
    /// 指定品种
    Variety(u8),
    /// 指定科系
    FamilyOf(u8),
    /// 必须发光
    Glowing,
    /// 株高不低于
    MinHeight(u8),
    /// 活力不低于（照料得好）
    MinVitality(f32),
}

impl Requirement {
    pub fn describe(&self) -> String {
        match self {
            Requirement::HueBand { label, .. } => format!("{}色系", label),
            Requirement::Variety(v) => variety_name(*v).to_string(),
            Requirement::FamilyOf(v) => format!(
                "{}科",
                match family(*v) {
                    Family::Flora => "花卉",
                    Family::Herb => "草蕨",
                    Family::Eldritch => "异界",
                    Family::Alchemy => "炼金",
                    Family::Aquatic => "水生",
                    Family::Celestial => "天时",
                }
            ),
            Requirement::Glowing => "会发光".to_string(),
            Requirement::MinHeight(h) => format!("株高≥{}", h),
            Requirement::MinVitality(v) => format!("活力≥{:.0}%", v * 100.0),
        }
    }

    fn satisfied(&self, genes: &Genes, vitality: f32) -> bool {
        match self {
            Requirement::HueBand { lo, hi, .. } => {
                let h = genes.hue.rem_euclid(360.0);
                if lo <= hi {
                    h >= *lo && h <= *hi
                } else {
                    h >= *lo || h <= *hi // 跨 0 的红色带
                }
            }
            Requirement::Variety(v) => genes.petal_shape == *v,
            Requirement::FamilyOf(v) => family(genes.petal_shape) == family(*v),
            Requirement::Glowing => genes.glow,
            Requirement::MinHeight(h) => genes.height >= *h,
            Requirement::MinVitality(v) => vitality >= *v,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quest {
    pub title: String,
    pub reqs: Vec<Requirement>,
    /// 达成奖励的堆肥
    pub reward_compost: u32,
    /// 是否附赠一颗稀有种
    pub reward_seed: bool,
    pub done: bool,
}

impl Quest {
    /// 委托描述："一株 湛蓝·会发光·株高≥16 的触手藤"
    pub fn describe(&self) -> String {
        let parts: Vec<String> = self.reqs.iter().map(|r| r.describe()).collect();
        format!("{}（{}）", self.title, parts.join(" · "))
    }

    /// 花园里有任何一株盛开的植株满足全部条件即达成。
    pub fn check(&self, garden: &Garden) -> bool {
        garden.plots.iter().flatten().any(|p| {
            p.stage() == Stage::Bloom
                && self.reqs.iter().all(|r| r.satisfied(&p.genes, p.vitality))
        })
    }
}

const HUE_BANDS: [(f32, f32, &str); 8] = [
    (345.0, 14.0, "绯红"),
    (15.0, 44.0, "橙"),
    (45.0, 69.0, "金黄"),
    (70.0, 159.0, "翠绿"),
    (160.0, 199.0, "青"),
    (200.0, 249.0, "湛蓝"),
    (250.0, 289.0, "紫"),
    (290.0, 344.0, "品红"),
];

/// 生成一批委托。种子按"真实周"取，所以同一周内委托稳定、每周自动轮换。
pub fn weekly_quests(week: u64, count: usize) -> Vec<Quest> {
    let mut rng = StdRng::seed_from_u64(week.wrapping_mul(0x9E37_79B9));
    let mut out = Vec::new();
    for i in 0..count {
        // 难度分三档：第一个简单（够得着），最后一个是传说级追逐
        let tier = i;
        let mut reqs = Vec::new();

        let (lo, hi, label) = HUE_BANDS[rng.gen_range(0..HUE_BANDS.len())];
        reqs.push(Requirement::HueBand {
            lo,
            hi,
            label: label.to_string(),
        });

        let (title, variety) = match tier {
            0 => {
                // 简单：常见品种 + 色系
                let v = [0u8, 1, 2, 3, 4, 5][rng.gen_range(0..6)];
                reqs.push(Requirement::Variety(v));
                ("园丁的心愿", v)
            }
            1 => {
                // 中等：指定品种 + 高株 / 高活力
                let v = [0u8, 1, 2, 3, 4, 5, 10, 11][rng.gen_range(0..8)];
                reqs.push(Requirement::Variety(v));
                if rng.gen_bool(0.5) {
                    reqs.push(Requirement::MinHeight(rng.gen_range(16..=19)));
                } else {
                    reqs.push(Requirement::MinVitality(0.9));
                }
                ("植物学者的订单", v)
            }
            _ => {
                // 传说：碰轴的稀有品种 + 发光（异界/炼金/天时）
                let v = [6u8, 7, 8, 9, 12, 13][rng.gen_range(0..6)];
                reqs.push(Requirement::Variety(v));
                reqs.push(Requirement::Glowing);
                ("收藏家的执念", v)
            }
        };

        out.push(Quest {
            title: format!("{}·{}{}", title, hue_name(lo), variety_name(variety)),
            reqs,
            reward_compost: match tier {
                0 => 4,
                1 => 9,
                _ => 20,
            },
            reward_seed: tier >= 1,
            done: false,
        });
    }
    out
}

/// 当前真实周序号（用于委托轮换）。
pub fn current_week() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 604_800)
        .unwrap_or(0)
}
