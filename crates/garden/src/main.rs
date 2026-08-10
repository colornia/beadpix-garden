mod bmp;
mod decor;
mod garden;
mod genetics;
mod quest;
mod render;
mod traits;
mod weather;

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;

use beadpix::{Canvas, HalfBlockRenderer, TerminalSession};
use garden::{Garden, Stage};
use genetics::{display_name, family, predict_child, species_name, Family};
use render::Scene;
use weather::Weather;

const FPS: u64 = 30;
/// 画面下方保留的文字行数（HUD）
const HUD_ROWS: u16 = 2;
/// 图鉴总品种数：8 色系 x 14 品种（六大科系）
const TOTAL_SPECIES: usize = 136;

fn save_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join("garden_save.json")
}

/// 按终端显示宽度（CJK 算 2 格）截断并补空格到 width。
fn pad_line(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = if (ch as u32) < 0x2E80 { 1 } else { 2 };
        if w + cw > width {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push_str(&" ".repeat(width.saturating_sub(w)));
    out
}

fn main() -> std::io::Result<()> {
    let mut rng = rand::thread_rng();

    let args: Vec<String> = std::env::args().collect();
    // 调试：`garden --screenshot <目录>` 渲染各群系/天气的静态帧到 BMP 后退出
    if let Some(i) = args.iter().position(|a| a == "--screenshot") {
        let dir = args.get(i + 1).cloned().unwrap_or_else(|| ".".into());
        return screenshot(&dir, &mut rng);
    }
    // 测试模式：`garden --fast` 生长速度 x20，约 30 秒开花
    let fast = args.iter().any(|a| a == "--fast");
    let time_scale: f32 = if fast { 20.0 } else { 1.0 };

    let mut garden = Garden::load(&save_path()).unwrap_or_else(|| Garden::new(&mut rng));
    let (offline, journal) = garden.settle_offline();

    let (cols, rows) = TerminalSession::size()?;
    let canvas_w = (cols as u32).clamp(40, 200);
    let canvas_h = ((rows.saturating_sub(HUD_ROWS)) as u32 * 2).clamp(40, 160);

    let mut session = TerminalSession::begin()?;
    let mut canvas = Canvas::new(canvas_w, canvas_h);
    let mut renderer = HalfBlockRenderer::new(canvas_w, canvas_h);
    let mut scene = Scene::new();
    let mut weather = Weather::new(&mut rng);

    let mut selected: usize = 0;
    let mut time = 0.0f32;
    let mut gallery = false;
    // HUD 第一行的显示模式：0=状态 1=委托列表
    let mut hud_mode = 0u8;
    // 移植模式：Some(源地块)，再选一块地即完成搬迁/互换
    let mut moving: Option<usize> = None;
    // 布景模式：Some(装饰槽位)——←→ 换槽位，回车摆放/收回
    let mut decor_cursor: Option<usize> = None;
    // 花头位置：锚点上方株高处（蜜蜂和迸发粒子的目标点）
    let head_of = |garden: &Garden, i: usize| -> Option<(f32, f32)> {
        let pts = render::anchors(
            garden.biome,
            canvas_w,
            canvas_h,
            garden.world_seed,
            garden.plots.len(),
        );
        garden.plots[i]
            .as_ref()
            .map(|p| (pts[i].0 as f32, (pts[i].1 - p.genes.height as i32) as f32))
    };
    // 离线日记：回访第一眼看到"你不在时发生了什么"，按任意键进入花园
    let mut message = if !journal.is_empty() {
        show_journal(&mut session, cols, rows, offline, &journal)?;
        renderer.invalidate();
        String::from("欢迎回来。")
    } else if offline > 60 {
        format!("欢迎回来！离开的 {} 分钟里植物们一直在生长。", offline / 60)
    } else {
        format!("欢迎来到{}花园！", garden.biome.name())
    };

    let frame = Duration::from_millis(1000 / FPS);
    let mut last = Instant::now();

    loop {
        // --- 输入 ---
        let key = TerminalSession::poll_key(Duration::from_millis(1))?;
        // 图鉴页打开时：B/Esc 关闭，其他键忽略
        if gallery {
            if matches!(
                key,
                Some(KeyCode::Esc) | Some(KeyCode::Char('b')) | Some(KeyCode::Char('B'))
            ) {
                gallery = false;
                renderer.invalidate();
            }
        } else if let Some(slot) = decor_cursor {
            // 布景模式：把花园当画布摆东西（系统五终局）
            match key {
                Some(KeyCode::Esc) | Some(KeyCode::Char('d')) | Some(KeyCode::Char('D')) => {
                    decor_cursor = None;
                    message = String::from("离开布景模式。");
                }
                Some(KeyCode::Left) => {
                    decor_cursor = Some((slot + decor::DECOR_SLOTS - 1) % decor::DECOR_SLOTS)
                }
                Some(KeyCode::Right) => decor_cursor = Some((slot + 1) % decor::DECOR_SLOTS),
                Some(KeyCode::Enter) => {
                    message = match garden.place_decor(slot) {
                        Some(d) => format!("在 {} 号位摆上了「{}」——{}。", slot + 1, d.name(), d.hint()),
                        None => String::from("库存里没有可摆的布景了（完成委托可以获得）。"),
                    };
                }
                Some(KeyCode::Backspace) | Some(KeyCode::Delete) => {
                    message = match garden.take_decor(slot) {
                        Some(d) => format!("收起了「{}」，放回库存。", d.name()),
                        None => String::from("这个位置是空的。"),
                    };
                }
                _ => {}
            }
        } else {
            match key {
            Some(KeyCode::Esc) => break,
            Some(KeyCode::Char('b')) | Some(KeyCode::Char('B')) => {
                gallery = true;
                renderer.invalidate();
            }
            Some(KeyCode::Char('d')) | Some(KeyCode::Char('D')) => {
                decor_cursor = Some(0);
                moving = None;
                message = String::from("布景模式：[←→]选位置 [回车]摆放 [退格]收起 [D]退出");
            }
            Some(KeyCode::Left) => selected = selected.saturating_sub(1),
            Some(KeyCode::Right) => selected = (selected + 1).min(garden.plots.len() - 1),
            // 3/4 视角下前后排切换（后排先分满）
            Some(KeyCode::Up) => {
                let back_n = garden.plots.len().div_ceil(2);
                if selected >= back_n {
                    selected -= back_n;
                }
            }
            Some(KeyCode::Down) => {
                let back_n = garden.plots.len().div_ceil(2);
                if selected < back_n {
                    selected = (selected + back_n).min(garden.plots.len() - 1);
                }
            }
            Some(KeyCode::Enter) => {
                message = if garden.plant_seed(selected) {
                    String::from("种下了一颗种子。")
                } else if garden.seeds.is_empty() {
                    String::from("没有种子了！收获盛开的花 [H] 或杂交 [X] 获取。")
                } else {
                    String::from("这块地已经种着植物了。")
                };
            }
            Some(KeyCode::Char('h')) | Some(KeyCode::Char('H')) => {
                // 收获前先记下花头位置和颜色，用于迸发粒子
                let burst_at = head_of(&garden, selected).map(|pos| {
                    let g = &garden.plots[selected].as_ref().unwrap().genes;
                    (pos, beadpix::Rgb::from_hsv(g.hue, g.saturation, 0.82))
                });
                message = if garden.harvest(selected, &mut rng) {
                    if let Some(((hx, hy), color)) = burst_at {
                        scene.emit_burst(hx, hy, color);
                    }
                    String::from("收获了！获得了继承它基因的种子。")
                } else {
                    String::from("这里没有盛开的花可以收获。")
                };
            }
            Some(KeyCode::Char('q')) | Some(KeyCode::Char('Q')) => {
                hud_mode = if hud_mode == 0 { 1 } else { 0 };
            }
            Some(KeyCode::Char('m')) | Some(KeyCode::Char('M')) => {
                message = match moving {
                    None => match &garden.plots[selected] {
                        Some(p) => {
                            moving = Some(selected);
                            format!(
                                "拿起了「{}」，移动光标到目标地块再按 M 放下（M 取消）。",
                                display_name(&p.genes)
                            )
                        }
                        None => String::from("这块地是空的，没有可移植的植株。"),
                    },
                    Some(src) if src == selected => {
                        moving = None;
                        String::from("放回原处了。")
                    }
                    Some(src) => {
                        let swapped = garden.plots[selected].is_some();
                        garden.relocate(src, selected);
                        moving = None;
                        if swapped {
                            String::from("两株换了位置。")
                        } else {
                            String::from("移植完成。")
                        }
                    }
                };
            }
            Some(KeyCode::Char('w')) | Some(KeyCode::Char('W')) => {
                // 浇水：选中株；已经很精神就全园浇一遍
                message = match garden.water(selected) {
                    Some(v) => format!("浇了水，它舒展开来（活力 {:.0}%）。", v * 100.0),
                    None => match garden.water_all() {
                        0 => String::from("花园里的植物都很精神，不需要浇水。"),
                        n => format!("给整座花园浇了水，{} 株舒展开来。", n),
                    },
                };
            }
            Some(KeyCode::Char('c')) | Some(KeyCode::Char('C')) => {
                // 退役换堆肥：先记名字再操作
                let name = garden.plots[selected]
                    .as_ref()
                    .map(|p| display_name(&p.genes));
                message = match garden.compost_plant(selected, &mut rng) {
                    Some(v) => format!(
                        "「{}」化作了 {} 堆肥（现有 {}）。它滋养的土地会记得它。",
                        name.unwrap_or_default(),
                        v,
                        garden.compost
                    ),
                    None => String::from("只有盛开的植株可以退役堆肥。"),
                };
            }
            Some(KeyCode::Char('e')) | Some(KeyCode::Char('E')) => {
                message = match garden.expand_cost() {
                    None => String::from("花园已达最大规模（16 块地）。"),
                    Some(cost) if garden.expand() => {
                        format!("花园扩张到 {} 块地！（花费 {} 堆肥）", garden.plots.len(), cost)
                    }
                    Some(cost) => format!(
                        "扩张第 {} 块地需要 {} 堆肥（现有 {}）。退役盛开的植株 [C] 可获得堆肥。",
                        garden.plots.len() + 1,
                        cost,
                        garden.compost
                    ),
                };
            }
            Some(KeyCode::Char('x')) | Some(KeyCode::Char('X')) => {
                message = match garden.cross_breed(selected, &mut rng) {
                    Some(child) => {
                        // 派一只蜜蜂完成这次授粉
                        if let (Some(a), Some(b)) =
                            (head_of(&garden, selected), head_of(&garden, selected + 1))
                        {
                            scene.spawn_bee(a, b);
                        }
                        format!("授粉成功！获得种子：{}（已记入育种笔记）", display_name(&child))
                    }
                    None => String::from("杂交需要当前地块和右侧地块都开着花。"),
                };
            }
            Some(KeyCode::Char('p')) | Some(KeyCode::Char('P')) => {
                // 钉目标表型：以选中植株为参照做定向育种
                message = match &garden.plots[selected] {
                    Some(p) => {
                        garden.pinned = Some(p.genes.clone());
                        format!("已钉住目标表型：{}。HUD 会显示其他植株与它的差距。", display_name(&p.genes))
                    }
                    None => {
                        if garden.pinned.take().is_some() {
                            String::from("已取消目标表型。")
                        } else {
                            String::from("在一株植物上按 P 可钉住它作为育种目标。")
                        }
                    }
                };
            }
            _ => {}
            }
        }

        // --- 更新 ---
        let dt = last.elapsed().as_secs_f32();
        last = Instant::now();
        time += dt;
        let ground_y = canvas_h as f32 * render::ground_frac(garden.biome);
        // 昼夜时钟在测试模式下同步加速，粒子始终用真实时间
        weather.update(dt, dt * time_scale, canvas_w, canvas_h, ground_y, &mut rng);
        garden.grow(dt * time_scale, &weather);

        // --- 图鉴：新开的花自动收录 ---
        let mut newly: Option<(String, bool)> = None;
        for plant in garden.plots.iter().flatten() {
            if plant.stage() == Stage::Bloom {
                let sp = species_name(&plant.genes);
                let glow = plant.genes.glow;
                if !garden.discovered.iter().any(|s| s == &sp) {
                    newly = Some((sp, glow));
                    break;
                }
            }
        }
        // --- 古株馈赠：活得久的植株不定期白送种子 ---
        let gifts = garden.collect_venerable_gifts(&mut rng);
        if !gifts.is_empty() {
            message = format!("古株结籽了：{}。", gifts.join("、"));
        }

        // --- 昙花：天亮即谢（留种）。轴5 时间尺度的唯一"失去" ---
        for note in garden.settle_ephemeral(&weather, &mut rng) {
            message = note;
        }

        // --- 委托：轮换 + 达成检查（脊椎）---
        for done in garden.update_quests(&mut rng) {
            message = done;
        }

        if let Some((sp, glow)) = newly {
            garden.discover(&sp);
            message = format!(
                "★ 图鉴新收录：{}{}！（{}/{}）",
                sp,
                if glow { "（发光形态！）" } else { "" },
                garden.discovered.len(),
                TOTAL_SPECIES
            );
        }

        // --- 渲染 ---
        if gallery {
            scene.draw_gallery(&mut canvas, &garden, time);
        } else {
            scene.draw(
                &mut canvas,
                &garden,
                &weather,
                selected,
                time,
                dt,
                moving,
                decor_cursor,
            );
        }
        renderer.present(&canvas, session.out(), 1, 1)?;
        if gallery {
            draw_gallery_hud(&mut session, canvas_h / 2 + 1, cols, &garden)?;
        } else {
            draw_hud(
                &mut session,
                canvas_h / 2 + 1,
                cols,
                &garden,
                &weather,
                selected,
                &message,
                fast,
                hud_mode,
                decor_cursor,
            )?;
        }

        std::thread::sleep(frame.saturating_sub(last.elapsed()));
    }

    garden.save(&save_path())?;
    drop(session);
    println!("花园已保存到 garden_save.json，下次见！");
    Ok(())
}

/// 生成固定状态的截图帧：三个群系 x 晴天/夜晚。
fn screenshot(dir: &str, rng: &mut impl rand::Rng) -> std::io::Result<()> {
    use crate::garden::{Biome, Plant};
    use crate::genetics::Genes;
    use std::path::Path;

    let (canvas_w, canvas_h) = (160u32, 100u32);
    let mut canvas = Canvas::new(canvas_w, canvas_h);
    let mut scene = Scene::new();

    let mut g = Garden::new(rng);

    std::fs::create_dir_all(dir)?;
    // (群系, 标签, 时刻, 彩虹秒数, 八个地块的品种)
    let shots: [(Biome, &str, f32, f32, [u8; 8]); 12] = [
        (Biome::Meadow, "meadow_noon", 0.28, 0.0, [0, 4, 1, 3, 2, 4, 3, 0]),
        (Biome::Meadow, "meadow_rainbow", 0.3, 25.0, [0, 4, 1, 3, 2, 4, 3, 0]),
        (Biome::Meadow, "meadow_night", 0.72, 0.0, [0, 4, 1, 3, 2, 4, 3, 0]),
        (Biome::Forest, "forest_noon", 0.3, 0.0, [5, 0, 5, 1, 5, 3, 2, 5]),
        // 异界 + 炼金品种展示（眼球花、触手藤、曼德拉、贤者金花）
        (Biome::Meadow, "exotic_noon", 0.3, 0.0, [6, 7, 8, 9, 6, 7, 8, 9]),
        (Biome::Crevice, "exotic_night", 0.72, 0.0, [6, 7, 8, 9, 6, 7, 8, 9]),
        // 深海群系（海葵、珊瑚）
        (Biome::Abyss, "abyss_noon", 0.3, 0.0, [10, 11, 10, 11, 10, 11, 10, 11]),
        (Biome::Abyss, "abyss_night", 0.72, 0.0, [10, 11, 10, 11, 10, 11, 10, 11]),
        // 天时科：同样两株，白天闭合 / 夜里绽放
        (Biome::Meadow, "celestial_noon", 0.3, 0.0, [12, 13, 12, 13, 12, 13, 12, 13]),
        (Biome::Meadow, "celestial_night", 0.72, 0.0, [12, 13, 12, 13, 12, 13, 12, 13]),
        // 雷纹花 / 陨星花 / 昙花：夜里最出效果（昙花只有夜里开）
        (Biome::Meadow, "storm_meteor_night", 0.72, 0.0, [14, 15, 16, 14, 15, 16, 14, 15]),
        (Biome::Meadow, "storm_meteor_noon", 0.3, 0.0, [14, 15, 16, 14, 15, 16, 14, 15]),
    ];
    for (biome, tag, clock, rainbow, shapes) in shots {
        let stages = [1.0, 0.3, 0.6, 1.0, 1.0, 1.0, 1.0, 0.9];
        let hues = [0.0f32, 120.0, 210.0, 25.0, 55.0, 210.0, 300.0, 160.0];
        g.plots = (0..8)
            .map(|i| {
                Some(Plant {
                    genes: Genes {
                        hue: hues[i],
                        saturation: 0.8,
                        petal_shape: shapes[i],
                        height: 10 + (i as u8 % 4) * 3,
                        growth_rate: 1.0,
                        glow: i == 6, // 第 7 株带发光基因，展示光效
                    },
                    progress: stages[i],
                    vitality: 1.0,
                    last_care: 0,
                    planted_at: 0,
                    last_gift: 0,
                    had_bloom_night: false,
                })
            })
            .collect();
        g.biome = biome;
        g.world_seed = 12345;
        let ground_y = canvas_h as f32 * render::ground_frac(biome);
        let mut w = Weather::new(rng);
        w.kind = weather::Kind::Sunny;
        w.clock = clock;
        for _ in 0..80 {
            // clock_dt = 0：时间冻结，只演化粒子
            w.update(0.1, 0.0, canvas_w, canvas_h, ground_y, rng);
            w.kind = weather::Kind::Sunny;
            w.clock = clock;
        }
        w.rainbow = rainbow;
        // 布景：石灯笼是真实光源，夜景里应该能看到它照亮周围
        g.decor = vec![
            Some(decor::Decoration::Lantern),
            Some(decor::Decoration::Pond),
            Some(decor::Decoration::Fence),
            Some(decor::Decoration::Windchime),
            Some(decor::Decoration::Lantern),
            Some(decor::Decoration::Birdbath),
        ];
        scene.draw(&mut canvas, &g, &w, 2, 1.5, 0.033, None, None);
        let name = format!("garden_{}.bmp", tag);
        bmp::write_bmp(&Path::new(dir).join(&name), &canvas, 6)?;
        println!("wrote {}", name);
    }

    // 图鉴页（收集了一部分的状态）
    for sp in [
        "绯红圆瓣", "橙四瓣", "金黄圆瓣", "翠绿星芒", "湛蓝星芒", "品红重瓣", "青圆瓣",
    ] {
        g.discover(sp);
    }
    scene.draw_gallery(&mut canvas, &g, 1.5);
    bmp::write_bmp(&Path::new(dir).join("garden_gallery.bmp"), &canvas, 6)?;
    println!("wrote garden_gallery.bmp");
    Ok(())
}

/// 离线日记页：回访时先讲"你不在时发生了什么"，按任意键进入花园。
fn show_journal(
    session: &mut TerminalSession,
    cols: u16,
    rows: u16,
    offline: u64,
    journal: &[String],
) -> std::io::Result<()> {
    let width = cols as usize;
    let away = if offline >= 86_400 {
        format!("{} 天", offline / 86_400)
    } else if offline >= 3600 {
        format!("{} 小时", offline / 3600)
    } else {
        format!("{} 分钟", offline.max(60) / 60)
    };
    let mut lines = vec![
        String::new(),
        format!("  你离开了 {}。这段时间里——", away),
        String::new(),
    ];
    for entry in journal {
        lines.push(format!("   · {}", entry));
        lines.push(String::new());
    }
    lines.push(String::from("  按任意键回到花园"));

    let out = session.out();
    write!(out, "\x1b[2J\x1b[0m\x1b[48;2;18;20;28m")?;
    for r in 1..=rows {
        let content = lines.get(r as usize - 2).cloned().unwrap_or_default();
        let styled = if content.contains('·') {
            format!("\x1b[38;2;190;225;175m{}", pad_line(&content, width))
        } else {
            format!("\x1b[38;2;225;225;235m{}", pad_line(&content, width))
        };
        write!(out, "\x1b[{};1H{}", r, styled)?;
    }
    write!(out, "\x1b[0m")?;
    out.flush()?;

    // 阻塞等待按键
    loop {
        if TerminalSession::poll_key(Duration::from_millis(50))?.is_some() {
            break;
        }
    }
    write!(session.out(), "\x1b[2J")?;
    session.out().flush()
}

/// 图鉴页 HUD：行=花型、列=色系的导读 + 收集进度。
fn draw_gallery_hud(
    session: &mut TerminalSession,
    hud_row: u32,
    cols: u16,
    garden: &Garden,
) -> std::io::Result<()> {
    // 按稀有度分层统计——传说才显得像传说
    let (mut backdrop, mut character, mut legend) = (0usize, 0usize, 0usize);
    for v in 0..genetics::VARIETIES {
        let found = (0..8)
            .filter(|&r| {
                const HUES: [f32; 8] = [0.0, 30.0, 57.0, 115.0, 180.0, 225.0, 270.0, 315.0];
                let sp = format!("{}{}", genetics::hue_name(HUES[r]), genetics::variety_name(v));
                garden.discovered.iter().any(|s| s == &sp)
            })
            .count();
        match traits::traits_of(v).tier() {
            traits::Tier::Backdrop => backdrop += found,
            traits::Tier::Character => character += found,
            traits::Tier::Legend => legend += found,
        }
    }
    // 每层的品种数 × 8 色系 = 该层总格数
    let (mut n_b, mut n_c, mut n_l) = (0usize, 0usize, 0usize);
    for v in 0..genetics::VARIETIES {
        match traits::traits_of(v).tier() {
            traits::Tier::Backdrop => n_b += 8,
            traits::Tier::Character => n_c += 8,
            traits::Tier::Legend => n_l += 8,
        }
    }
    let line1 = format!(
        " ❀ 图鉴 {}/{} | ▬常见 {}/{} · ▬角色 {}/{} · ▬传说 {}/{} | 顶部色带分层，左→右 由常见到传说",
        garden.discovered.len(),
        TOTAL_SPECIES,
        backdrop,
        n_b,
        character,
        n_c,
        legend,
        n_l,
    );
    let line2 = String::from(
        " 剪影=未培育 | 异界/炼金/天时靠杂交突变，水生来自深海，陨星花只由流星带来 | [B/Esc]返回",
    );
    let width = cols as usize;
    let out = session.out();
    write!(
        out,
        "\x1b[{};1H\x1b[0m\x1b[97m\x1b[48;2;30;30;40m{}\x1b[{};1H\x1b[38;2;150;150;160m\x1b[48;2;20;20;28m{}\x1b[0m",
        hud_row,
        pad_line(&line1, width),
        hud_row + 1,
        pad_line(&line2, width),
    )?;
    out.flush()
}

#[allow(clippy::too_many_arguments)]
fn draw_hud(
    session: &mut TerminalSession,
    hud_row: u32,
    cols: u16,
    garden: &Garden,
    weather: &Weather,
    selected: usize,
    message: &str,
    fast: bool,
    hud_mode: u8,
    decor_cursor: Option<usize>,
) -> std::io::Result<()> {
    // 杂交预览优先：当前块与右侧块都盛开时，显示子代预测范围
    let preview = garden.plots.get(selected).and_then(|s| s.as_ref()).and_then(|a| {
        garden
            .plots
            .get(selected + 1)
            .and_then(|s| s.as_ref())
            .filter(|b| a.stage() == Stage::Bloom && b.stage() == Stage::Bloom)
            .map(|b| predict_child(&a.genes, &b.genes))
    });
    let info = if let Some(f) = preview {
        format!(
            "[X]杂交预览: 色相{:.0}°~{:.0}° 株高{}-{} 发光{:.0}% 稀有觉醒{:.1}%",
            f.hue_lo,
            f.hue_hi,
            f.height_lo,
            f.height_hi,
            f.glow_chance * 100.0,
            f.rare_chance * 100.0
        )
    } else {
        match &garden.plots[selected] {
            Some(p) => {
                let stage = match p.stage() {
                    Stage::Seed => "种子",
                    Stage::Sprout => "幼苗",
                    Stage::Bud => "花苞",
                    Stage::Bloom => "盛开",
                };
                let fam = match family(p.genes.petal_shape) {
                    Family::Flora => "花卉",
                    Family::Herb => "草蕨",
                    Family::Eldritch => "异界",
                    Family::Alchemy => "炼金",
                    Family::Aquatic => "水生",
                    Family::Celestial => "天时",
                };
                // 钉了目标表型时显示与目标的差距（定向育种导航）
                let pin_delta = garden
                    .pinned
                    .as_ref()
                    .map(|t| {
                        let dh = ((t.hue - p.genes.hue + 540.0) % 360.0 - 180.0).abs();
                        format!(
                            " |距目标:色差{:.0}° 高差{} 发光{}",
                            dh,
                            (t.height as i32 - p.genes.height as i32).abs(),
                            if t.glow == p.genes.glow { "✓" } else { "✗" }
                        )
                    })
                    .unwrap_or_default();
                let vit = if p.vitality < 0.6 {
                    format!(" 活力{:.0}%(需浇水)", p.vitality * 100.0)
                } else {
                    String::new()
                };
                // 系统六：把这一株的"规则"讲给玩家，它才是角色而不是调色板
                let t = traits::traits_of(p.genes.petal_shape);
                let rule = match t.neighbor {
                    Some(traits::NeighborEffect::Wither) => " ⚠邻株枯萎",
                    Some(traits::NeighborEffect::Nourish) => " ✦夜间滋养邻株",
                    Some(traits::NeighborEffect::ShadeCast) => " ☂投影",
                    None => "",
                };
                let tier = match t.tier() {
                    traits::Tier::Backdrop => String::new(),
                    other => format!("·{}", other.name()),
                };
                // 古株：活得久的会不定期白送种子，堆肥它就断了这份馈赠
                let age = if p.is_venerable() {
                    format!(" 古株{}h(每2h结籽)", p.age() / 3600)
                } else {
                    String::new()
                };
                // 轴1/轴5 的两条"你得赶上那一刻"必须提前讲明白——
                // 昙花是全局唯一会谢的品种，玩家有权预知并安排好在场
                let timing = if p.is_ephemeral() {
                    match p.stage() {
                        Stage::Bloom if p.had_bloom_night => " ❀今夜之后天明即谢(留2-3种子)",
                        Stage::Bloom => " ❀绽放中·天明即谢(留2-3种子)",
                        _ => " ❀入夜才开·开一夜即谢(留种)",
                    }
                } else if p.needs_rain_to_sprout() && p.stage() == Stage::Seed {
                    " ⛈等一场雨才发芽"
                } else {
                    ""
                };
                format!(
                    "{}号地: [{}{}] {} {} {:.0}%{}{}{}{}{}",
                    selected + 1,
                    fam,
                    tier,
                    display_name(&p.genes),
                    stage,
                    p.progress * 100.0,
                    age,
                    timing,
                    rule,
                    vit,
                    pin_delta,
                )
            }
            None => format!("{}号地: 空", selected + 1),
        }
    };
    let expand_hint = match garden.expand_cost() {
        Some(c) => format!("{}", c),
        None => String::from("满"),
    };
    let line1 = if let Some(slot) = decor_cursor {
        // 布景面板
        let here = garden.decor.get(slot).and_then(|d| *d);
        let stock: Vec<&str> = garden.decor_stock.iter().map(|d| d.name()).collect();
        format!(
            " ❖布景 {}/{} 号位: {} | 库存: {} | 完成委托可获得新布景",
            slot + 1,
            decor::DECOR_SLOTS,
            match here {
                Some(d) => format!("{}（{}）", d.name(), d.hint()),
                None => String::from("空"),
            },
            if stock.is_empty() {
                String::from("无")
            } else {
                stock.join("、")
            },
        )
    } else if hud_mode == 1 {
        // 委托面板：脊椎——够得着又永远差一点的追逐
        let items: Vec<String> = garden
            .quests
            .iter()
            .map(|q| {
                format!(
                    "{}{}",
                    if q.done { "✔" } else { "○" },
                    q.describe()
                )
            })
            .collect();
        format!(
            " ✦本周委托(累计完成{}) {} | 本周客座种:{} | [Q]返回状态",
            garden.quests_done,
            items.join("  "),
            genetics::variety_name(genetics::guest_variety(quest::current_week())),
        )
    } else {
        format!(
            " {} · {}·{}{} | 种子x{} 堆肥x{}(扩张需{}) | 图鉴{}/{} | {}",
            garden.biome.name(),
            weather.period_name(),
            weather.name(),
            if fast { " [x20]" } else { "" },
            garden.seeds.len(),
            garden.compost,
            expand_hint,
            garden.discovered.len(),
            TOTAL_SPECIES,
            info
        )
    };
    let line2 = if decor_cursor.is_some() {
        format!(" [←→]选位置 [回车]摆放 [退格]收起 [D]退出布景 | {}", message)
    } else {
        format!(
            " [←→↑↓]选 [回车]种 [W]浇 [H]收 [X]杂交 [M]移植 [P]钉 [C]堆肥 [E]扩张 [D]布景 [Q]委托 [B]图鉴 [Esc]退出 | {}",
            message
        )
    };
    let width = cols as usize;
    let out = session.out();
    write!(
        out,
        "\x1b[{};1H\x1b[0m\x1b[97m\x1b[48;2;30;30;40m{}\x1b[{};1H\x1b[38;2;150;150;160m\x1b[48;2;20;20;28m{}\x1b[0m",
        hud_row,
        pad_line(&line1, width),
        hud_row + 1,
        pad_line(&line2, width),
    )?;
    out.flush()
}
