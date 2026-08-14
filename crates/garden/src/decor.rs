//! 系统五：花园即画布——真·终局。
//!
//! 图鉴集齐只是开场，最后玩家做的事是**用育出来的花布置一座好看的花园**。
//! 装饰格直接吃渲染优势：石灯笼是真实光源，夜里会照亮周围的花。
//!
//! 经济纪律（坑 机制-1）：装饰**不卖**，只从委托奖励里来——
//! 堆肥的唯一主用途仍然只有扩张地块，别把它变成手游农场的商店。

use serde::{Deserialize, Serialize};

/// 旧存档里的固定槽位数（仅用于迁移到自由坐标）。
pub const DECOR_SLOTS: usize = 6;

/// 一件摆好的装饰。坐标存**画布比例**而非像素——
/// 终端窗口大小会变，存像素会让布景在改窗口后散架。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub kind: Decoration,
    /// 0.0..1.0，相对画布宽
    pub x: f32,
    /// 0.0..1.0，相对画布高
    pub y: f32,
}

impl Placed {
    /// 换算成当前画布的像素坐标。
    pub fn pixel(&self, w: u32, h: u32) -> (i32, i32) {
        (
            (self.x.clamp(0.0, 1.0) * w as f32) as i32,
            (self.y.clamp(0.0, 1.0) * h as f32) as i32,
        )
    }
}

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

