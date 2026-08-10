use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::genetics::Genes;
use crate::weather::Weather;

/// 开局地块数：地块稀缺是"决定"的来源。
pub const INITIAL_PLOTS: usize = 5;
/// 扩张上限。
pub const MAX_PLOTS: usize = 16;

/// 开局由世界种子决定的群系：决定整个画面的构图和氛围。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Biome {
    /// 草甸：开阔天空、远山、平地花田
    #[default]
    Meadow,
    /// 林间：树冠遮天，光柱洒落，树干间的空地种花
    Forest,
    /// 石缝：岩壁上错落的岩架，花从石缝里长出来
    Crevice,
    /// 深海：水下花园，光斑摇曳，夜晚生物发光
    Abyss,
}

impl Biome {
    pub fn name(self) -> &'static str {
        match self {
            Biome::Meadow => "草甸",
            Biome::Forest => "林间",
            Biome::Crevice => "石缝",
            Biome::Abyss => "深海",
        }
    }
}

/// 生长阶段由 progress (0-1) 划分。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stage {
    Seed,
    Sprout,
    Bud,
    Bloom,
}

fn default_vitality() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plant {
    pub genes: Genes,
    /// 0.0 = 刚种下, 1.0 = 盛开
    pub progress: f32,
    /// 活力 0.15..1.0：无人照料按真实时间缓降，浇水回升。
    /// 影响发光亮度和堆肥产出，永远不导致死亡（没有 Dead 状态）。
    #[serde(default = "default_vitality")]
    pub vitality: f32,
    /// 上次照料的 Unix 时间戳（0 = 从未，按 last_tick 处理）
    #[serde(default)]
    pub last_care: u64,
}

impl Plant {
    pub fn stage(&self) -> Stage {
        match self.progress {
            p if p < 0.15 => Stage::Seed,
            p if p < 0.5 => Stage::Sprout,
            p if p < 0.85 => Stage::Bud,
            _ => Stage::Bloom,
        }
    }
}

/// 一颗成熟植株完整生长约需的真实秒数（基准值，受基因和天气加成）。
const BASE_GROW_SECS: f32 = 600.0;

#[derive(Serialize, Deserialize)]
pub struct Garden {
    pub plots: Vec<Option<Plant>>,
    /// 库存种子（每颗带基因）
    pub seeds: Vec<Genes>,
    /// 上次结算的 Unix 时间戳，用于离线生长
    pub last_tick: u64,
    /// 群系（开局随机，旧存档默认草甸）
    #[serde(default)]
    pub biome: Biome,
    /// 世界种子：决定装饰细节（山形、树位、岩架分布）
    #[serde(default)]
    pub world_seed: u32,
    /// 图鉴：已开出过的品种名
    #[serde(default)]
    pub discovered: Vec<String>,
    /// 堆肥：唯一软资源。来源只有退役植株，主用途只有扩张地块。
    #[serde(default)]
    pub compost: u32,
    /// 育种笔记：历次杂交的父母 → 结果记录
    #[serde(default)]
    pub breeding_log: Vec<String>,
    /// 玩家钉住的目标表型（定向育种的参照）
    #[serde(default)]
    pub pinned: Option<Genes>,
}

impl Garden {
    pub fn new(rng: &mut impl Rng) -> Self {
        let biome = match rng.gen_range(0..4) {
            0 => Biome::Meadow,
            1 => Biome::Forest,
            2 => Biome::Crevice,
            _ => Biome::Abyss,
        };
        // 开局送 3 颗本群系的野生种子
        let seeds = (0..3).map(|_| Genes::wild(rng, biome)).collect();
        Garden {
            plots: vec![None; INITIAL_PLOTS],
            seeds,
            last_tick: now(),
            biome,
            world_seed: rng.gen(),
            discovered: Vec::new(),
            compost: 0,
            breeding_log: Vec::new(),
            pinned: None,
        }
    }

    /// 一株成熟植株的堆肥价值：稀有科系产出高，发光基因加成，
    /// 按 vitality 缩放——稀有花即使不留，也是价值储备。
    pub fn compost_value(plant: &Plant, rng: &mut impl Rng) -> u32 {
        use crate::genetics::{family, Family};
        let base = match family(plant.genes.petal_shape) {
            Family::Eldritch | Family::Alchemy => rng.gen_range(6..=10),
            _ => rng.gen_range(1..=3),
        };
        let glow_bonus = if plant.genes.glow { 3 } else { 0 };
        let scale = 0.5 + 0.5 * plant.vitality.clamp(0.0, 1.0);
        (((base + glow_bonus) as f32) * scale).round().max(1.0) as u32
    }

    /// 退役一株盛开的植株换堆肥。返回产出（失败返回 None）。
    pub fn compost_plant(&mut self, plot: usize, rng: &mut impl Rng) -> Option<u32> {
        let plant = self.plots.get(plot)?.as_ref()?;
        if plant.stage() != Stage::Bloom {
            return None;
        }
        let value = Self::compost_value(plant, rng);
        self.plots[plot] = None;
        self.compost += value;
        Some(value)
    }

    /// 扩张下一块地的堆肥成本：round(6 * 1.6^(n-6))，n 为扩后的地块数。
    pub fn expand_cost(&self) -> Option<u32> {
        let n = self.plots.len() + 1;
        if n > MAX_PLOTS {
            return None;
        }
        Some((6.0 * 1.6f32.powi(n as i32 - 6)).round().max(1.0) as u32)
    }

    /// 花堆肥扩张一块地。
    pub fn expand(&mut self) -> bool {
        if let Some(cost) = self.expand_cost() {
            if self.compost >= cost {
                self.compost -= cost;
                self.plots.push(None);
                return true;
            }
        }
        false
    }

    /// 图鉴收录；是新品种则返回 true。
    pub fn discover(&mut self, species: &str) -> bool {
        if self.discovered.iter().any(|s| s == species) {
            false
        } else {
            self.discovered.push(species.to_string());
            true
        }
    }

    /// 推进生长。`dt` 为真实秒数，天气提供倍率（雨天加速等）。
    pub fn grow(&mut self, dt: f32, weather: &Weather) {
        let weather_mul = weather.growth_multiplier();
        for plant in self.plots.iter_mut().flatten() {
            if plant.progress < 1.0 {
                plant.progress = (plant.progress
                    + dt * plant.genes.growth_rate * weather_mul / BASE_GROW_SECS)
                    .min(1.0);
            }
        }
        self.last_tick = now();
    }

    /// 读档后结算离线时间（离线期间按晴天速率算）。返回离线秒数用于提示。
    pub fn settle_offline(&mut self) -> u64 {
        let elapsed = now().saturating_sub(self.last_tick);
        if elapsed > 0 {
            for plant in self.plots.iter_mut().flatten() {
                if plant.progress < 1.0 {
                    plant.progress = (plant.progress
                        + elapsed as f32 * plant.genes.growth_rate / BASE_GROW_SECS)
                        .min(1.0);
                }
            }
        }
        self.last_tick = now();
        elapsed
    }

    /// 在指定地块种下库存里的第一颗种子。
    pub fn plant_seed(&mut self, plot: usize) -> bool {
        if self.plots[plot].is_some() || self.seeds.is_empty() {
            return false;
        }
        let genes = self.seeds.remove(0);
        self.plots[plot] = Some(Plant {
            genes,
            progress: 0.0,
            vitality: 1.0,
            last_care: now(),
        });
        true
    }

    /// 收获盛开的花：移除植株，获得 2-3 颗继承其基因的种子。
    pub fn harvest(&mut self, plot: usize, rng: &mut impl Rng) -> bool {
        let Some(plant) = &self.plots[plot] else {
            return false;
        };
        if plant.stage() != Stage::Bloom {
            return false;
        }
        let genes = plant.genes.clone();
        let n = rng.gen_range(2..=3);
        for _ in 0..n {
            // 自交后代：基因小幅漂移
            self.seeds.push(Genes::cross(&genes, &genes, rng));
        }
        self.plots[plot] = None;
        true
    }

    /// 杂交：指定地块与其右侧相邻地块都盛开时，产出 1 颗杂交种子（不消耗植株）。
    pub fn cross_breed(&mut self, plot: usize, rng: &mut impl Rng) -> bool {
        if plot + 1 >= self.plots.len() {
            return false;
        }
        let (Some(a), Some(b)) = (&self.plots[plot], &self.plots[plot + 1]) else {
            return false;
        };
        if a.stage() != Stage::Bloom || b.stage() != Stage::Bloom {
            return false;
        }
        let child = Genes::cross(&a.genes, &b.genes, rng);
        self.seeds.push(child);
        true
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        fs::write(path, serde_json::to_string_pretty(self).unwrap())
    }

    pub fn load(path: &Path) -> Option<Garden> {
        let text = fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
