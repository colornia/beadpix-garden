//! 系统六：把每一株从"调色板"变成"带规则的角色"。
//!
//! 纪律：**每加一株，先问它碰哪条轴；碰不到，就别加。**
//! 特别度 = 它碰到了几条轴。碰一条 = 有性格，碰三条以上 = 传说级。
//!
//! 这套之所以对本项目近乎免费：连续昼夜、天气事件、逐光源光照、粒子系统
//! 都已存在——月见花只是"绑定夜间的开花状态"，雷种只是"绑定 rain 的发芽"。

use serde::{Deserialize, Serialize};

/// 轴 1 · 存在条件：什么时候／哪里才存在。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpawnCond {
    /// 只在雨中发芽
    OnRain,
    /// 只在夜里开花（白天回来只能看到"昨晚开过"）
    NightOnly,
    /// 流星砸中土地才掉落的种
    MeteorDrop,
    /// 锁定群系（深海的水生科已在用）
    BiomeLocked,
}

/// 轴 2 · 实时行为：活着的时候在干嘛。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Behavior {
    /// 眨眼（眼球花已实现）
    Blink,
    /// 蠕动（触手藤已实现）
    Writhe,
    /// 向日性：花头追着太阳转
    Heliotropic,
    /// 光标靠近就合拢（含羞草）
    CloseOnCursor,
    /// 按整点开合（林奈"花钟"）
    ClockOpen,
    /// 电弧迸发／星芒闪烁（雷纹花、陨星花）
    Spark,
}

/// 轴 3 · 邻居关系：对旁边的花做什么。
/// 这条让系统一的"留谁"和系统五的"布景"升级成摆放谜题。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NeighborEffect {
    /// 让相邻株活力掉得更快
    Wither,
    /// 夜间给相邻株补活力
    Nourish,
    /// 投影：喜阴邻居茂盛，喜阳邻居蔫
    ShadeCast,
}

/// 轴 4 · 遗传特殊性：特别来自你怎么育。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeneticTag {
    /// 跨科系嵌合（触手 × 发光）
    Chimera,
    /// 只能靠突变觉醒（异界/炼金已在用）
    MutationOnly,
    /// 能"记住"血统并显在外观上
    RemembersLineage,
}

/// 轴 5 · 时间尺度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Lifespan {
    /// 昙花一现：开一夜，得赶上那一刻
    Ephemeral,
    #[default]
    Normal,
    /// 古株：越老越有沧桑感（苔痕虬枝），每两小时无偿结籽——
    /// 奖励你**别**堆肥它，与系统一的扩张经济形成反张力。
    /// 已实现为**所有品种**的通用时间尺度（见 Plant::is_venerable），
    /// 而不是某个品种的专属标签
    Ancient,
}

/// 一株的"性格"：它碰了哪几条轴。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlantTraits {
    pub spawn_cond: Option<SpawnCond>,
    pub behavior: Vec<Behavior>,
    pub neighbor: Option<NeighborEffect>,
    pub genetic: Option<GeneticTag>,
    pub lifespan: Lifespan,
}

impl PlantTraits {
    /// 特别度打分：碰了几条轴。直接用来排稀有度/图鉴分层。
    pub fn richness_score(&self) -> u8 {
        let mut n = 0;
        if self.spawn_cond.is_some() {
            n += 1;
        }
        if !self.behavior.is_empty() {
            n += 1;
        }
        if self.neighbor.is_some() {
            n += 1;
        }
        if self.genetic.is_some() {
            n += 1;
        }
        if self.lifespan != Lifespan::Normal {
            n += 1;
        }
        n
    }

    /// 稀有度分层：底纹层（常见换色）→ 角色层（碰行为轴）→ 传说层（碰条件+遗传）。
    /// 图鉴 UI 应按此分层展示，传说才显得像传说。
    pub fn tier(&self) -> Tier {
        match self.richness_score() {
            0 => Tier::Backdrop,
            1..=2 => Tier::Character,
            _ => Tier::Legend,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// 底纹层：平凡的连续换色，是稀有花的背景板——千万别省
    Backdrop,
    /// 角色层：每个是一次发现
    Character,
    /// 传说层：得制造条件或定向育出，每个是一个故事
    Legend,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::Backdrop => "常见",
            Tier::Character => "角色",
            Tier::Legend => "传说",
        }
    }
}

/// 现有 12 品种的性格表——已实现的行为在此登记，
/// 新品种加入时先在这里回答"它碰哪条轴"。
pub fn traits_of(variety: u8) -> PlantTraits {
    match variety % crate::genetics::VARIETIES {
        // 0-3 花卉：底纹层，连续换色的墙纸，正是它的平凡衬出稀有
        0..=3 => PlantTraits::default(),
        // 4 芒草 / 5 蕨叶：草蕨科，风中摇曳明显但无特殊规则
        4 | 5 => PlantTraits::default(),
        // 6 眼球花：会眨眼、会盯着你看（轴2）
        6 => PlantTraits {
            behavior: vec![Behavior::Blink],
            genetic: Some(GeneticTag::MutationOnly),
            ..Default::default()
        },
        // 7 触手藤：蠕动（轴2）+ 让邻居枯萎（轴3）+ 突变觉醒（轴4）→ 传说级
        7 => PlantTraits {
            behavior: vec![Behavior::Writhe],
            neighbor: Some(NeighborEffect::Wither),
            genetic: Some(GeneticTag::MutationOnly),
            ..Default::default()
        },
        // 8 曼德拉草：夜里给邻居供养（轴3）+ 突变觉醒（轴4）
        8 => PlantTraits {
            neighbor: Some(NeighborEffect::Nourish),
            genetic: Some(GeneticTag::MutationOnly),
            ..Default::default()
        },
        // 9 贤者金花：向日性（轴2）+ 突变觉醒（轴4）
        9 => PlantTraits {
            behavior: vec![Behavior::Heliotropic],
            genetic: Some(GeneticTag::MutationOnly),
            ..Default::default()
        },
        // 10 海葵 / 11 珊瑚：群系锁定（轴1）
        10 | 11 => PlantTraits {
            spawn_cond: Some(SpawnCond::BiomeLocked),
            ..Default::default()
        },
        // 12 月见花：只在夜里绽放（轴1）+ 按时开合（轴2）+ 突变觉醒（轴4）→ 传说级。
        // 白天回来只看得到闭合的花苞——想看它开，你得夜里来。
        12 => PlantTraits {
            spawn_cond: Some(SpawnCond::NightOnly),
            behavior: vec![Behavior::ClockOpen],
            genetic: Some(GeneticTag::MutationOnly),
            ..Default::default()
        },
        // 13 日晷花：向日 + 按时开合（轴2）+ 夜间滋养邻株（轴3）+ 突变觉醒（轴4）
        13 => PlantTraits {
            behavior: vec![Behavior::Heliotropic, Behavior::ClockOpen],
            neighbor: Some(NeighborEffect::Nourish),
            genetic: Some(GeneticTag::MutationOnly),
            ..Default::default()
        },
        // 14 雷纹花：**只在雨中发芽**（轴1）+ 雨天电弧迸发（轴2）+ 突变觉醒（轴4）→ 传说级。
        // 种下后若不下雨，它就一直是颗种子等着——你得等一场雨。
        14 => PlantTraits {
            spawn_cond: Some(SpawnCond::OnRain),
            behavior: vec![Behavior::Spark],
            genetic: Some(GeneticTag::MutationOnly),
            ..Default::default()
        },

        // 15 陨星花：**只能由流星带来**（轴1）+ 星芒闪烁（轴2）+ 夜间滋养邻株（轴3）→ 传说级。
        // 突变表里没有它——只有流星砸中花园才会留下这颗种子
        15 => PlantTraits {
            spawn_cond: Some(SpawnCond::MeteorDrop),
            behavior: vec![Behavior::Spark],
            neighbor: Some(NeighborEffect::Nourish),
            ..Default::default()
        },
        // 16 昙花：只在夜里绽放（轴1）+ 开一夜天明即谢（轴5）+ 突变觉醒（轴4）→ 传说级。
        // 全局唯一会"谢"的品种。谢时留下 2-3 颗种子（净赚），
        // 失去的只是"那一刻的景象"——所以 HUD 必须提前预告，见 main.rs
        _ => PlantTraits {
            spawn_cond: Some(SpawnCond::NightOnly),
            genetic: Some(GeneticTag::MutationOnly),
            lifespan: Lifespan::Ephemeral,
            ..Default::default()
        },
    }
}
