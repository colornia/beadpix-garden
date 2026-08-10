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

    /// 浇水：+0.3 vitality，蔫的立刻复原。永远是正向动作，没有惩罚。
    /// 返回 Some(浇水后的活力) 表示成功。
    pub fn water(&mut self, plot: usize) -> Option<f32> {
        let plant = self.plots.get_mut(plot)?.as_mut()?;
        if plant.vitality >= 0.995 {
            return None; // 已经很精神了，不用浇
        }
        plant.vitality = (plant.vitality + 0.3).min(1.0);
        plant.last_care = now();
        Some(plant.vitality)
    }

    /// 全园浇水（地块多了以后逐块浇太繁琐）。返回浇了几株。
    pub fn water_all(&mut self) -> usize {
        let t = now();
        let mut n = 0;
        for p in self.plots.iter_mut().flatten() {
            if p.vitality < 0.995 {
                p.vitality = (p.vitality + 0.3).min(1.0);
                p.last_care = t;
                n += 1;
            }
        }
        n
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
    /// vitality 按真实时间极缓下降（约 24h 才明显蔫），下雨自动补水——
    /// 每天来一次的玩家永远看不到蔫。
    pub fn grow(&mut self, dt: f32, weather: &Weather) {
        let weather_mul = weather.growth_multiplier();
        let raining = weather.kind == crate::weather::Kind::Rain;
        for plant in self.plots.iter_mut().flatten() {
            if plant.progress < 1.0 {
                plant.progress = (plant.progress
                    + dt * plant.genes.growth_rate * weather_mul / BASE_GROW_SECS)
                    .min(1.0);
            }
            if raining {
                plant.vitality = (plant.vitality + dt / 90.0).min(1.0); // 雨天润物
            } else {
                plant.vitality = (plant.vitality - dt / 86_400.0 * 0.35).max(0.15);
            }
        }
        self.last_tick = now();
    }

    /// 读档后结算离线时间：生长照常，另外用 seeded RNG（以存档时间做种，
    /// 防回档刷事件）roll 出"你不在时发生了什么"——事件留下可发现的痕迹
    /// （流星真的改了花色、野蜂真的授了粉），日记只挑高光展示。
    /// 返回 (离线秒数, 日记条目)。
    pub fn settle_offline(&mut self) -> (u64, Vec<String>) {
        use crate::genetics::display_name;
        use rand::rngs::StdRng;
        use rand::SeedableRng;

        let elapsed = now().saturating_sub(self.last_tick);
        let mut journal = Vec::new();
        let mut rng = StdRng::seed_from_u64(self.last_tick ^ self.world_seed as u64);

        if elapsed > 0 {
            // --- 生长 + 记录谁在离线时开了花 ---
            let mut newly_bloomed = Vec::new();
            for (i, plant) in self.plots.iter_mut().enumerate() {
                if let Some(p) = plant {
                    let before = p.stage();
                    if p.progress < 1.0 {
                        p.progress = (p.progress
                            + elapsed as f32 * p.genes.growth_rate / BASE_GROW_SECS)
                            .min(1.0);
                    }
                    if before != Stage::Bloom && p.stage() == Stage::Bloom {
                        newly_bloomed.push((i, display_name(&p.genes)));
                    }
                }
            }
            for (i, name) in newly_bloomed.iter().take(2) {
                journal.push(format!("「{}」在你离开时悄悄开花了（{}号地）。", name, i + 1));
            }

            // --- vitality 按真实时间缓降：约 24h 无照料才明显，地板 0.15 永不为 0 ---
            let decay = elapsed as f32 / 86_400.0 * 0.35;
            let mut wilted = false;
            for p in self.plots.iter_mut().flatten() {
                let old = p.vitality;
                p.vitality = (p.vitality - decay).max(0.15);
                if old >= 0.6 && p.vitality < 0.6 {
                    wilted = true;
                }
            }
            if wilted {
                journal.push(String::from("有几株植物轻轻垂下了头——浇点水 [W] 它们就会精神起来。"));
            }

            let hours = elapsed as f32 / 3600.0;

            // --- 流星：离开超过 2 小时有机会砸中一株，花色真的变了 ---
            if hours > 2.0 && rng.gen_bool(0.25) {
                let planted: Vec<usize> = self
                    .plots
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.is_some())
                    .map(|(i, _)| i)
                    .collect();
                if let Some(&i) = planted.get(rng.gen_range(0..planted.len().max(1)).min(planted.len().saturating_sub(1))) {
                    if let Some(p) = &mut self.plots[i] {
                        let shift: f32 = rng.gen_range(20.0..40.0) * if rng.gen_bool(0.5) { 1.0 } else { -1.0 };
                        p.genes.hue = (p.genes.hue + shift).rem_euclid(360.0);
                        journal.push(format!(
                            "夜里一颗流星掠过 {} 号地——「{}」的颜色变得和以前不一样了。",
                            i + 1,
                            display_name(&p.genes)
                        ));
                    }
                }
            }

            // --- 野蜂：有相邻盛开的两株时，免费替你授一次粉 ---
            if hours > 1.0 {
                let pair = (0..self.plots.len().saturating_sub(1)).find(|&i| {
                    matches!((&self.plots[i], &self.plots[i + 1]),
                        (Some(a), Some(b)) if a.stage() == Stage::Bloom && b.stage() == Stage::Bloom)
                });
                if let Some(i) = pair {
                    if rng.gen_bool(0.6) {
                        if let Some(child) = self.cross_breed(i, &mut rng) {
                            journal.push(format!(
                                "一只野蜂在花间忙碌过——它替你授了粉，种子袋里多了「{}」。",
                                display_name(&child)
                            ));
                        }
                    }
                }
            }

            // --- 旅人：约每 12 小时一次机会，留下一颗种子（偶尔是稀有科系）---
            let visitor_chances = ((hours / 12.0) as u32).min(2);
            for _ in 0..visitor_chances {
                if rng.gen_bool(0.5) {
                    let mut gift = Genes::wild(&mut rng, self.biome);
                    if rng.gen_bool(0.15) {
                        gift.petal_shape = [6u8, 7, 8, 9][rng.gen_range(0..4)];
                    }
                    journal.push(format!(
                        "一位旅人在篱笆边歇了歇脚，留下一颗种子作谢礼：「{}」。",
                        display_name(&gift)
                    ));
                    self.seeds.push(gift);
                }
            }

            // --- 天气流水（风味）---
            if hours > 3.0 && journal.len() < 5 {
                let rains = (hours / 6.0) as u32;
                if rains > 0 {
                    journal.push(format!("你离开的这些时候，花园里下了 {} 场雨。", rains));
                }
            }
            journal.truncate(5);
        }
        self.last_tick = now();
        (elapsed, journal)
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
    /// 返回子代基因；同时记入育种笔记——玩家玩着玩着自己摸出遗传规律。
    pub fn cross_breed(&mut self, plot: usize, rng: &mut impl Rng) -> Option<Genes> {
        use crate::genetics::display_name;
        if plot + 1 >= self.plots.len() {
            return None;
        }
        let (Some(a), Some(b)) = (&self.plots[plot], &self.plots[plot + 1]) else {
            return None;
        };
        if a.stage() != Stage::Bloom || b.stage() != Stage::Bloom {
            return None;
        }
        let child = Genes::cross(&a.genes, &b.genes, rng);
        self.breeding_log.push(format!(
            "{} × {} → {}",
            display_name(&a.genes),
            display_name(&b.genes),
            display_name(&child)
        ));
        if self.breeding_log.len() > 60 {
            self.breeding_log.remove(0);
        }
        self.seeds.push(child.clone());
        Some(child)
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
