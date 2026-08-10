//! 系统五：花园即画布——真·终局。
//!
//! 图鉴集齐只是开场，最后玩家做的事是**用育出来的花布置一座好看的花园**。
//! 装饰格直接吃渲染优势：石灯笼是真实光源，夜里会照亮周围的花。
//!
//! 经济纪律（坑 机制-1）：装饰**不卖**，只从委托奖励里来——
//! 堆肥的唯一主用途仍然只有扩张地块，别把它变成手游农场的商店。

use serde::{Deserialize, Serialize};

use crate::garden::Biome;

/// 布景槽位数。
pub const DECOR_SLOTS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decoration {
    /// 石灯笼：**真实光源**，夜里在周围投出暖光
    Lantern,
    /// 风铃：随风摆动，摆到极处闪一下微光
    Windchime,
    /// 鸟浴盆：一汪浅水，白天会有小鸟落下来
    Birdbath,
    /// 矮篱：把花园框起来，纯构图用
    Fence,
    /// 小池塘：会映出天光的水面
    Pond,
}

impl Decoration {
    pub fn name(self) -> &'static str {
        match self {
            Decoration::Lantern => "石灯笼",
            Decoration::Windchime => "风铃",
            Decoration::Birdbath => "鸟浴盆",
            Decoration::Fence => "矮篱",
            Decoration::Pond => "小池塘",
        }
    }

    /// 一句话说明它做什么——装饰也该有"值得拥有的理由"。
    pub fn hint(self) -> &'static str {
        match self {
            Decoration::Lantern => "夜里照亮周围的花",
            Decoration::Windchime => "随风摇曳，风大时闪光",
            Decoration::Birdbath => "白天引来小鸟",
            Decoration::Fence => "为花园收个边",
            Decoration::Pond => "映出天光与月色",
        }
    }

    /// 按委托难度发放：越难的委托给越"有戏"的装饰。
    pub fn reward_for_tier(tier: usize) -> Decoration {
        match tier {
            0 => Decoration::Fence,
            1 => Decoration::Windchime,
            _ => Decoration::Lantern,
        }
    }
}

/// 布景槽位坐标：与植株锚点错开，分三个景深层。
pub fn decor_anchors(biome: Biome, w: u32, h: u32, seed: u32) -> Vec<(i32, i32)> {
    let ground = match biome {
        Biome::Crevice => 0.9,
        _ => 1.0,
    };
    (0..DECOR_SLOTS)
        .map(|i| {
            let t = (i as f32 + 0.5) / DECOR_SLOTS as f32;
            let jitter = (hash(i as i32, seed as i32) % 5) as i32 - 2;
            let x = (w as f32 * t) as i32 + jitter;
            // 三个景深层交替，避免连成一条线
            let depth = match i % 3 {
                0 => 0.60,
                1 => 0.755,
                _ => 0.93,
            };
            let y = (h as f32 * depth * ground) as i32;
            (x.clamp(5, w as i32 - 6), y)
        })
        .collect()
}

fn hash(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h ^= h >> 13;
    h.wrapping_mul(1_274_126_177)
}
