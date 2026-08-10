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
    /// 种下的时间戳，用于计算树龄（轴5 时间尺度）
    #[serde(default)]
    pub planted_at: u64,
    /// 上次结出无偿种子的时间戳（古株的被动馈赠）
    #[serde(default)]
    pub last_gift: u64,
}

/// 成为"古株"所需的存活真实秒数（约 6 小时）。
pub const VENERABLE_SECS: u64 = 6 * 3600;
/// 古株结出无偿种子的间隔（约 2 小时）。
pub const GIFT_INTERVAL_SECS: u64 = 2 * 3600;

impl Plant {
    /// 存活的真实秒数。
    pub fn age(&self) -> u64 {
        if self.planted_at == 0 {
            return 0;
        }
        now().saturating_sub(self.planted_at)
    }

    /// 古株：活得够久的植株。苔痕虬枝，会不定期结出无偿种子——
    /// 这是奖励你**别**堆肥它，与扩张经济形成反张力。
    pub fn is_venerable(&self) -> bool {
        self.stage() == Stage::Bloom && self.age() >= VENERABLE_SECS
    }

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
    /// 当前活跃委托（每真实周轮换）
    #[serde(default)]
    pub quests: Vec<crate::quest::Quest>,
    /// 委托所属周序号；与当前周不同则轮换
    #[serde(default)]
    pub quest_week: u64,
    /// 累计完成的委托数（长期成就感）
    #[serde(default)]
    pub quests_done: u32,
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
            quests: crate::quest::weekly_quests(crate::quest::current_week(), 3),
            quest_week: crate::quest::current_week(),
            quests_done: 0,
        }
    }

    /// 委托轮换（每真实周）+ 达成检查。返回本次新达成的委托描述与奖励。
    pub fn update_quests(&mut self, rng: &mut impl Rng) -> Vec<String> {
        let week = crate::quest::current_week();
        if self.quests.is_empty() || self.quest_week != week {
            self.quests = crate::quest::weekly_quests(week, 3);
            self.quest_week = week;
        }
        let mut completed = Vec::new();
        for i in 0..self.quests.len() {
            if self.quests[i].done || !self.quests[i].check(self) {
                continue;
            }
            self.quests[i].done = true;
            self.quests_done += 1;
            let reward = self.quests[i].reward_compost;
            let seed_reward = self.quests[i].reward_seed;
            let title = self.quests[i].title.clone();
            self.compost += reward;
            let mut extra = String::new();
            if seed_reward {
                // 稀有种：异界/炼金科系
                let mut gift = Genes::wild(rng, self.biome);
                gift.petal_shape = [6u8, 7, 8, 9][rng.gen_range(0..4)];
                extra = format!("，以及一颗稀有种「{}」", crate::genetics::display_name(&gift));
                self.seeds.push(gift);
            }
            completed.push(format!(
                "✦ 委托达成「{}」！获得 {} 堆肥{}。",
                title, reward, extra
            ));
        }
        completed
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
        // 古株积累的年岁也是价值——但它活着时每两小时白送一颗种子，
        // 所以"堆肥换地"与"留着让它继续送"始终在互相拉扯
        let age_bonus = if plant.is_venerable() { 4 } else { 0 };
        let scale = 0.5 + 0.5 * plant.vitality.clamp(0.0, 1.0);
        (((base + glow_bonus + age_bonus) as f32) * scale)
            .round()
            .max(1.0) as u32
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

    /// 移植：把 from 的植株搬到 to（空地则搬过去，有植株则两株互换）。
    /// 花园即画布——配合邻居效果，"什么该挨着什么"成了摆放谜题。
    /// 移植不损伤植株（cozy 契约：没有失败惩罚）。
    pub fn relocate(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= self.plots.len() || to >= self.plots.len() {
            return false;
        }
        if self.plots[from].is_none() {
            return false;
        }
        self.plots.swap(from, to);
        true
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
        self.apply_neighbor_effects(dt, weather);
        self.last_tick = now();
    }

    /// 系统六轴 3：邻居关系。触手藤让两侧枯萎、曼德拉草夜间供养两侧——
    /// "这块地留谁"因此从单株取舍升级成摆放谜题。
    fn apply_neighbor_effects(&mut self, dt: f32, weather: &Weather) {
        use crate::traits::{traits_of, NeighborEffect};
        let night = weather.darkness() > 0.5;
        // 先收集影响，再施加，避免借用冲突
        let mut deltas = vec![0.0f32; self.plots.len()];
        for i in 0..self.plots.len() {
            let Some(p) = &self.plots[i] else { continue };
            if p.stage() != Stage::Bloom {
                continue;
            }
            let Some(effect) = traits_of(p.genes.petal_shape).neighbor else {
                continue;
            };
            let amount = match effect {
                NeighborEffect::Wither => -dt / 86_400.0 * 0.9,
                NeighborEffect::Nourish if night => dt / 86_400.0 * 1.2,
                _ => 0.0,
            };
            if amount == 0.0 {
                continue;
            }
            for n in [i.wrapping_sub(1), i + 1] {
                if n < self.plots.len() {
                    deltas[n] += amount;
                }
            }
        }
        for (i, d) in deltas.iter().enumerate() {
            if *d != 0.0 {
                if let Some(p) = &mut self.plots[i] {
                    p.vitality = (p.vitality + d).clamp(0.15, 1.0);
                }
            }
        }
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
                if !planted.is_empty() {
                    let i = planted[rng.gen_range(0..planted.len())];
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
            planted_at: now(),
            last_gift: now(),
        });
        true
    }

    /// 古株的被动馈赠：每隔一段时间无偿结出一颗继承自身基因的种子。
    /// 返回本次收到的种子名（用于提示）。
    pub fn collect_venerable_gifts(&mut self, rng: &mut impl Rng) -> Vec<String> {
        use crate::genetics::display_name;
        let t = now();
        let mut gifts = Vec::new();
        let mut new_seeds = Vec::new();
        for p in self.plots.iter_mut().flatten() {
            if !p.is_venerable() {
                continue;
            }
            if t.saturating_sub(p.last_gift) < GIFT_INTERVAL_SECS {
                continue;
            }
            p.last_gift = t;
            let child = Genes::cross(&p.genes, &p.genes, rng);
            gifts.push(display_name(&child));
            new_seeds.push(child);
        }
        self.seeds.extend(new_seeds);
        gifts
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genetics::Genes;

    fn bloom(variety: u8) -> Plant {
        Plant {
            genes: Genes {
                hue: 200.0,
                saturation: 0.8,
                petal_shape: variety,
                height: 14,
                growth_rate: 1.0,
                glow: false,
            },
            progress: 1.0,
            vitality: 1.0,
            last_care: 0,
            planted_at: now(),
            last_gift: now(),
        }
    }

    /// 一株已经活了 `secs` 秒的成熟植株。
    fn aged_bloom(variety: u8, secs: u64) -> Plant {
        let mut p = bloom(variety);
        p.planted_at = now().saturating_sub(secs);
        p.last_gift = now().saturating_sub(secs);
        p
    }

    #[test]
    fn expand_cost_rises_and_caps_at_max() {
        let mut rng = rand::thread_rng();
        let mut g = Garden::new(&mut rng);
        assert_eq!(g.plots.len(), INITIAL_PLOTS);
        let first = g.expand_cost().unwrap();
        g.compost = 10_000;
        assert!(g.expand());
        // 成本随地块数递增
        assert!(g.expand_cost().unwrap() > first);
        while g.plots.len() < MAX_PLOTS {
            assert!(g.expand(), "should expand up to the cap");
        }
        assert_eq!(g.expand_cost(), None, "no cost past the cap");
        assert!(!g.expand(), "cannot expand past the cap");
    }

    #[test]
    fn expand_refused_without_enough_compost() {
        let mut rng = rand::thread_rng();
        let mut g = Garden::new(&mut rng);
        g.compost = 0;
        assert!(!g.expand());
        assert_eq!(g.plots.len(), INITIAL_PLOTS);
    }

    #[test]
    fn composting_requires_bloom_and_yields_compost() {
        let mut rng = rand::thread_rng();
        let mut g = Garden::new(&mut rng);
        // 未开花不能堆肥
        let mut young = bloom(0);
        young.progress = 0.3;
        g.plots[0] = Some(young);
        assert_eq!(g.compost_plant(0, &mut rng), None);
        assert!(g.plots[0].is_some(), "failed compost must not remove plant");

        g.plots[0] = Some(bloom(0));
        let got = g.compost_plant(0, &mut rng).expect("bloom composts");
        assert!(got >= 1);
        assert_eq!(g.compost, got);
        assert!(g.plots[0].is_none());
    }

    #[test]
    fn rare_families_compost_for_more_than_common() {
        let mut rng = rand::thread_rng();
        // 眼球花(6)属异界科，产出 6-10；圆瓣(0)属花卉科，产出 1-3
        let rare = Garden::compost_value(&bloom(6), &mut rng);
        let common = Garden::compost_value(&bloom(0), &mut rng);
        assert!(rare > common, "rare {rare} should beat common {common}");
    }

    #[test]
    fn low_vitality_reduces_compost_yield() {
        let mut rng = rand::thread_rng();
        let mut wilted = bloom(6);
        wilted.vitality = 0.15;
        // 同科系下，蔫的产出不会超过健康的
        let healthy_max = 10; // 异界科上限
        let got = Garden::compost_value(&wilted, &mut rng);
        assert!(got <= healthy_max / 2 + 2, "wilted yield {got} too high");
        assert!(got >= 1, "yield never drops to zero");
    }

    #[test]
    fn watering_restores_and_never_exceeds_one() {
        let mut rng = rand::thread_rng();
        let mut g = Garden::new(&mut rng);
        let mut p = bloom(0);
        p.vitality = 0.2;
        g.plots[0] = Some(p);
        let v = g.water(0).expect("thirsty plant accepts water");
        assert!((v - 0.5).abs() < 1e-5, "0.2 + 0.3 = 0.5, got {v}");
        g.water(0);
        g.water(0);
        let v = g.plots[0].as_ref().unwrap().vitality;
        assert!(v <= 1.0, "vitality caps at 1.0, got {v}");
        // 满活力时拒绝浇水（让 UI 可以回退到全园浇水）
        assert_eq!(g.water(0), None);
    }

    #[test]
    fn cross_breed_needs_two_adjacent_blooms() {
        let mut rng = rand::thread_rng();
        let mut g = Garden::new(&mut rng);
        g.plots[0] = Some(bloom(0));
        assert!(g.cross_breed(0, &mut rng).is_none(), "needs a partner");

        g.plots[1] = Some(bloom(1));
        let seeds_before = g.seeds.len();
        assert!(g.cross_breed(0, &mut rng).is_some());
        assert_eq!(g.seeds.len(), seeds_before + 1, "child seed is stored");
        assert_eq!(g.breeding_log.len(), 1, "cross is logged");
        // 双亲都还在（杂交不消耗植株）
        assert!(g.plots[0].is_some() && g.plots[1].is_some());
    }

    #[test]
    fn cross_breed_at_last_plot_does_not_panic() {
        let mut rng = rand::thread_rng();
        let mut g = Garden::new(&mut rng);
        let last = g.plots.len() - 1;
        g.plots[last] = Some(bloom(0));
        assert!(g.cross_breed(last, &mut rng).is_none());
    }

    #[test]
    fn a_plant_becomes_venerable_only_after_the_threshold() {
        assert!(!bloom(0).is_venerable(), "a fresh bloom is not venerable");
        assert!(
            !aged_bloom(0, VENERABLE_SECS - 60).is_venerable(),
            "just under the threshold is not venerable"
        );
        assert!(aged_bloom(0, VENERABLE_SECS + 60).is_venerable());
        // 未开花的植株再老也不算古株
        let mut young = aged_bloom(0, VENERABLE_SECS * 10);
        young.progress = 0.2;
        assert!(!young.is_venerable());
    }

    #[test]
    fn venerable_plants_gift_seeds_on_an_interval() {
        let mut rng = rand::thread_rng();
        let mut g = Garden::new(&mut rng);
        g.plots[0] = Some(aged_bloom(0, VENERABLE_SECS + GIFT_INTERVAL_SECS));
        let before = g.seeds.len();
        let gifts = g.collect_venerable_gifts(&mut rng);
        assert_eq!(gifts.len(), 1, "one gift is due");
        assert_eq!(g.seeds.len(), before + 1);
        // 刚送完不会立刻再送
        assert!(g.collect_venerable_gifts(&mut rng).is_empty());
    }

    #[test]
    fn young_plants_never_gift_seeds() {
        let mut rng = rand::thread_rng();
        let mut g = Garden::new(&mut rng);
        g.plots[0] = Some(bloom(0));
        let before = g.seeds.len();
        assert!(g.collect_venerable_gifts(&mut rng).is_empty());
        assert_eq!(g.seeds.len(), before);
    }

    #[test]
    fn venerable_plants_compost_for_more() {
        let mut rng = rand::thread_rng();
        let young = Garden::compost_value(&bloom(0), &mut rng);
        let old = Garden::compost_value(&aged_bloom(0, VENERABLE_SECS * 2), &mut rng);
        assert!(old > young, "venerable {old} should beat young {young}");
    }

    #[test]
    fn relocate_moves_into_empty_and_swaps_when_occupied() {
        let mut rng = rand::thread_rng();
        let mut g = Garden::new(&mut rng);
        g.plots[0] = Some(bloom(6)); // 眼球花
        assert!(g.relocate(0, 2), "move into empty bed");
        assert!(g.plots[0].is_none());
        assert_eq!(g.plots[2].as_ref().unwrap().genes.petal_shape, 6);

        g.plots[3] = Some(bloom(9)); // 贤者金花
        assert!(g.relocate(2, 3), "swap with occupied bed");
        assert_eq!(g.plots[3].as_ref().unwrap().genes.petal_shape, 6);
        assert_eq!(g.plots[2].as_ref().unwrap().genes.petal_shape, 9);
    }

    #[test]
    fn relocate_rejects_empty_source_and_out_of_range() {
        let mut rng = rand::thread_rng();
        let mut g = Garden::new(&mut rng);
        assert!(!g.relocate(0, 1), "empty source is a no-op");
        g.plots[0] = Some(bloom(0));
        assert!(!g.relocate(0, 0), "same bed is a no-op");
        assert!(!g.relocate(0, 99), "out of range is a no-op");
        assert!(g.plots[0].is_some(), "plant survives every rejected move");
    }

    #[test]
    fn withering_neighbour_drains_only_its_neighbours() {
        let mut rng = rand::thread_rng();
        let mut g = Garden::new(&mut rng);
        // 触手藤(7) 使两侧枯萎；把它放在 1 号位
        g.plots[1] = Some(bloom(7));
        g.plots[0] = Some(bloom(0));
        g.plots[2] = Some(bloom(0));
        g.plots[4] = Some(bloom(0)); // 不相邻，应不受影响
        let w = Weather::new(&mut rng);
        // 推进相当于数天的邻居效果
        for _ in 0..200 {
            g.apply_neighbor_effects(3600.0, &w);
        }
        let left = g.plots[0].as_ref().unwrap().vitality;
        let far = g.plots[4].as_ref().unwrap().vitality;
        assert!(left < far, "neighbour {left} should wither below distant {far}");
        assert!(left >= 0.15, "even withered plants keep the floor");
    }

    #[test]
    fn offline_settlement_is_deterministic_for_same_save() {
        let mut rng = rand::thread_rng();
        let mut a = Garden::new(&mut rng);
        a.plots[0] = Some(bloom(0));
        a.plots[1] = Some(bloom(1));
        a.world_seed = 4242;
        a.last_tick = now().saturating_sub(30 * 3600);
        let mut b = Garden {
            plots: a.plots.clone(),
            seeds: a.seeds.clone(),
            last_tick: a.last_tick,
            biome: a.biome,
            world_seed: a.world_seed,
            discovered: a.discovered.clone(),
            compost: a.compost,
            breeding_log: a.breeding_log.clone(),
            pinned: None,
            quests: a.quests.clone(),
            quest_week: a.quest_week,
            quests_done: a.quests_done,
        };
        let (_, ja) = a.settle_offline();
        let (_, jb) = b.settle_offline();
        assert_eq!(ja, jb, "same save must replay identically (no reroll farming)");
    }

    #[test]
    fn vitality_never_hits_zero_however_long_you_are_away() {
        let mut rng = rand::thread_rng();
        let mut g = Garden::new(&mut rng);
        g.plots[0] = Some(bloom(0));
        // 离开一年
        g.last_tick = now().saturating_sub(365 * 86_400);
        g.settle_offline();
        let v = g.plots[0].as_ref().unwrap().vitality;
        assert!(v >= 0.15, "vitality floor is 0.15, got {v}");
        assert!(g.plots[0].is_some(), "plants never die");
    }

    #[test]
    fn a_daily_player_never_sees_wilting() {
        let mut rng = rand::thread_rng();
        let mut g = Garden::new(&mut rng);
        g.plots[0] = Some(bloom(0));
        g.last_tick = now().saturating_sub(24 * 3600);
        g.settle_offline();
        let v = g.plots[0].as_ref().unwrap().vitality;
        // 24 小时后仍应明显高于"蔫"的提示阈值 0.6
        assert!(v > 0.6, "24h away should not wilt; vitality was {v}");
    }
}
