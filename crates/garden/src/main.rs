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

use beadpix::{Canvas, HalfBlockRenderer, Input, MouseKind, TerminalSession};
use garden::{Garden, Stage};
use genetics::{display_name, family, predict_child, species_name, Family};
use render::Scene;
use weather::Weather;

const FPS: u64 = 30;
/// 画面下方保留的文字行数（HUD）
const HUD_ROWS: u16 = 2;
/// 图鉴总品种数：8 色系 x 17 品种（六大科系）
const TOTAL_SPECIES: usize = 136;
/// 悬停多久才把 HUD 换成该目标的详情。
/// NN/g 建议 0.3–0.5 秒：短于此的是"路过"，不该打断阅读。
/// 轻量高亮是立刻给的，不受这个延迟影响。
const HOVER_DELAY: Duration = Duration::from_millis(350);

fn save_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join("garden_save.json")
}

/// 这块地此刻"最自然的那个动作"——再点一次已选中的地块就执行它。
///
/// 这是上下文动作条的雏形：新玩家不必背 11 个热键，点两下就能推进；
/// 老玩家的热键一个没动。返回热键本身，让鼠标和键盘走同一条代码路径。
fn primary_action_key(garden: &Garden, plot: usize) -> KeyCode {
    match &garden.plots[plot] {
        None => KeyCode::Enter,                       // 空地 → 播种
        Some(p) if p.vitality < 0.6 => KeyCode::Char('w'), // 蔫了 → 先浇水
        Some(p) if p.stage() == Stage::Bloom => KeyCode::Char('h'), // 开花 → 收获
        Some(_) => KeyCode::Char('w'),                // 还在长 → 浇水（唯一有意义的照料）
    }
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
    // 鼠标是与键盘并行的可发现层；Drop 保证崩了也不会把终端卡在鼠标模式
    session.enable_mouse()?;
    let mut canvas = Canvas::new(canvas_w, canvas_h);
    let mut renderer = HalfBlockRenderer::new(canvas_w, canvas_h);
    let mut scene = Scene::new();
    let mut weather = Weather::new(&mut rng);

    let mut selected: usize = 0;
    let mut time = 0.0f32;
    // 图鉴：None=关闭，Some((列,行)) 是查看光标
    let mut gallery: Option<(usize, usize)> = None;
    // HUD 第一行的显示模式：0=状态 1=委托列表
    let mut hud_mode = 0u8;
    // 移植模式：Some(源地块)，再选一块地即完成搬迁/互换
    let mut moving: Option<usize> = None;
    // 布景模式：Some(装饰槽位)——←→ 换槽位，回车摆放/收回
    let mut decor_cursor: Option<usize> = None;
    // 拖拽：按下时记住起点，移出起点格才算真拖，松开时决定是"点击"还是"放下"。
    // 这样点击和拖拽共用左键而不打架。
    let mut drag: Option<(render::PickTarget, bool)> = None;
    // 鼠标悬停：目标 + 停留起点。
    // 按 NN/g 的悬停规范分两级——立刻给轻量视觉反馈（看得出指着谁），
    // 停留够久才换 HUD 详情（避免鼠标划过时信息栏疯狂闪烁）。
    let mut hover: Option<render::PickTarget> = None;
    let mut hover_since = Instant::now();
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
        // 鼠标与键盘并行：鼠标先翻译成"点到了什么"，再和热键走同一套动作
        let input = TerminalSession::poll_input(Duration::from_millis(1))?;
        let mut key = match input {
            Some(Input::Key(k)) => Some(k),
            _ => None,
        };
        if let Some(Input::Mouse(m)) = input {
            let target = scene.pick_at(m.col, m.row);
            // 鼠标位置换算成画布比例坐标（布景自由摆放用）
            let frac = (
                m.col as f32 / canvas_w as f32,
                (m.row as f32 * 2.0) / canvas_h as f32,
            );
            match (m.kind, target) {
                // 按下：记住起点，先不动作——松开时才知道这是点击还是拖拽
                (MouseKind::LeftDown, Some(t)) => {
                    drag = Some((t, false));
                    if let (render::PickTarget::DexCell(c, r), Some(_)) = (t, gallery) {
                        gallery = Some((c, r));
                    }
                }
                // 拖出起点格 = 真的在拖
                (MouseKind::LeftDrag, t) => {
                    if let Some((from, moved)) = &mut drag {
                        if t != Some(*from) {
                            *moved = true;
                        }
                    }
                }
                (MouseKind::LeftUp, t) => {
                    if let Some((from, moved)) = drag.take() {
                        match (from, moved, t) {
                            // 拖植株到另一块地 = 移植
                            (render::PickTarget::Bed(a), true, Some(render::PickTarget::Bed(b)))
                                if a != b =>
                            {
                                message = if garden.relocate(a, b) {
                                    selected = b;
                                    format!("把 {} 号地的植株挪到了 {} 号地。", a + 1, b + 1)
                                } else {
                                    String::from("这块地是空的，没什么可挪。")
                                };
                            }
                            // 拖布景到任意位置 = 自由摆放
                            (render::PickTarget::Decor(i), true, _) => {
                                if garden.move_decor(i, frac.0, frac.1) {
                                    message = String::from("布景已挪好。");
                                }
                            }
                            // 没拖动 = 点击
                            (render::PickTarget::Bed(i), false, _) => {
                                if gallery.is_none() {
                                    if selected == i && decor_cursor.is_none() {
                                        key = Some(primary_action_key(&garden, i));
                                    } else {
                                        selected = i;
                                    }
                                }
                            }
                            (render::PickTarget::Decor(i), false, _) => {
                                if gallery.is_none() {
                                    decor_cursor = Some(i);
                                    message = format!(
                                        "选中布景「{}」——拖动可挪位置，[退格]收回库存。",
                                        garden.decor_items[i].kind.name()
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                    // 布景模式下点空地 = 把库存里的下一件摆在这儿
                    if decor_cursor.is_some() && matches!(t, Some(render::PickTarget::Bed(_)) | None)
                    {
                        if let Some(d) = garden.place_decor_at(frac.0, frac.1) {
                            message = format!("摆上了「{}」——{}。", d.name(), d.hint());
                        }
                    }
                }
                // 悬停：换了目标就重新计时，详情要停够 HOVER_DELAY 才显示
                (MouseKind::Moved, t) => {
                    if t != hover {
                        hover = t;
                        hover_since = Instant::now();
                    }
                }
                (MouseKind::ScrollUp, _) => {
                    if let Some((c, r)) = gallery {
                        gallery = Some((c, (r + 7) % 8));
                    }
                }
                (MouseKind::ScrollDown, _) => {
                    if let Some((c, r)) = gallery {
                        gallery = Some((c, (r + 1) % 8));
                    }
                }
                _ => {}
            }
        }
        // 图鉴页打开时：方向键移动查看光标，B/Esc 关闭
        if let Some((gc, gr)) = gallery {
            let ncols = genetics::VARIETIES as usize;
            match key {
                Some(KeyCode::Esc) | Some(KeyCode::Char('b')) | Some(KeyCode::Char('B')) => {
                    gallery = None;
                    renderer.invalidate();
                }
                Some(KeyCode::Left) => gallery = Some(((gc + ncols - 1) % ncols, gr)),
                Some(KeyCode::Right) => gallery = Some(((gc + 1) % ncols, gr)),
                Some(KeyCode::Up) => gallery = Some((gc, (gr + 7) % 8)),
                Some(KeyCode::Down) => gallery = Some((gc, (gr + 1) % 8)),
                _ => {}
            }
        } else if let Some(slot) = decor_cursor {
            // 布景模式：把花园当画布摆东西（系统五终局）。
            // 鼠标拖拽是主要交互；键盘保留一套等价操作（←→ 选件、方向键微调位置）
            let n = garden.decor_items.len().max(1);
            match key {
                Some(KeyCode::Esc) | Some(KeyCode::Char('d')) | Some(KeyCode::Char('D')) => {
                    decor_cursor = None;
                    message = String::from("离开布景模式。");
                }
                Some(KeyCode::Left) => decor_cursor = Some((slot + n - 1) % n),
                Some(KeyCode::Right) => decor_cursor = Some((slot + 1) % n),
                // 键盘微调选中件的位置（鼠标用户直接拖）
                Some(KeyCode::Up) | Some(KeyCode::Down) => {
                    if let Some(d) = garden.decor_items.get(slot).copied() {
                        let dy = if key == Some(KeyCode::Up) { -0.02 } else { 0.02 };
                        garden.move_decor(slot, d.x, d.y + dy);
                    }
                }
                Some(KeyCode::Enter) => {
                    // 摆在花园中间偏下，之后可拖到想要的位置
                    message = match garden.place_decor_at(0.5, 0.8) {
                        Some(d) => {
                            decor_cursor = Some(garden.decor_items.len() - 1);
                            format!("摆上了「{}」——{}。拖动它到想要的位置。", d.name(), d.hint())
                        }
                        None => String::from("库存里没有可摆的布景了（完成委托可以获得）。"),
                    };
                }
                Some(KeyCode::Backspace) | Some(KeyCode::Delete) => {
                    message = match garden.take_decor(slot) {
                        Some(d) => {
                            decor_cursor = Some(slot.min(garden.decor_items.len().saturating_sub(1)));
                            format!("收起了「{}」，放回库存。", d.name())
                        }
                        None => String::from("这里没有布景可收。"),
                    };
                }
                _ => {}
            }
        } else {
            match key {
            Some(KeyCode::Esc) => break,
            Some(KeyCode::Char('b')) | Some(KeyCode::Char('B')) => {
                gallery = Some((0, 0));
                renderer.invalidate();
            }
            Some(KeyCode::Char('d')) | Some(KeyCode::Char('D')) => {
                decor_cursor = Some(0);
                moving = None;
                message = String::from("布景模式：[←→]选位置 [回车]摆放 [退格]收起 [D]退出");
            }
            Some(KeyCode::Tab) => {
                garden.density = garden.density.next();
                message = format!(
                    "提示密度：{}。{}",
                    garden.density.name(),
                    match garden.density {
                        garden::Density::Quiet => "只报名字和阶段，悬停不打扰。",
                        garden::Density::Normal => "标注规则与状态。",
                        garden::Density::Verbose => "悬停即时响应，并念出它碰的每条轴。",
                    }
                );
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
        if let Some(cur) = gallery {
            scene.draw_gallery(&mut canvas, &garden, time, cur);
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
                // 立刻高亮：0.1 秒内就得让你看出指着谁（详情才延迟）
                match hover {
                    Some(render::PickTarget::Bed(i)) => Some(i),
                    _ => None,
                },
            );
        }
        renderer.present(&canvas, session.out(), 1, 1)?;
        if let Some(cur) = gallery {
            draw_gallery_hud(&mut session, canvas_h / 2 + 1, cols, &garden, cur)?;
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
                // 停留够久才让详情接管 HUD；密度设定改变这个门槛
                match garden.density {
                    garden::Density::Quiet => None, // 清净：悬停从不接管
                    garden::Density::Verbose => hover, // 详尽：立刻响应
                    garden::Density::Normal => {
                        if hover_since.elapsed() >= HOVER_DELAY {
                            hover
                        } else {
                            None
                        }
                    }
                },
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
        g.decor_items = [
            (decor::Decoration::Lantern, 0.12, 0.62),
            (decor::Decoration::Pond, 0.28, 0.90),
            (decor::Decoration::Fence, 0.44, 0.72),
            (decor::Decoration::Windchime, 0.60, 0.60),
            (decor::Decoration::Lantern, 0.78, 0.88),
            (decor::Decoration::Birdbath, 0.92, 0.70),
        ]
        .into_iter()
        .map(|(kind, x, y)| decor::Placed { kind, x, y })
        .collect();
        scene.draw(&mut canvas, &g, &w, 2, 1.5, 0.033, None, None, None);
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
    scene.draw_gallery(&mut canvas, &g, 1.5, (12, 3)); // 光标停在传说层看效果
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
    cursor: (usize, usize),
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
    // 光标所指的那一格：把这个品种的规则讲清楚
    const HUES: [f32; 8] = [0.0, 30.0, 57.0, 115.0, 180.0, 225.0, 270.0, 315.0];
    let order = render::Scene::gallery_order();
    let variety = order[cursor.0.min(order.len() - 1)];
    let hue = HUES[cursor.1.min(7)];
    let species = format!("{}{}", genetics::hue_name(hue), genetics::variety_name(variety));
    let found = garden.discovered.iter().any(|s| s == &species);
    let t = traits::traits_of(variety);

    let line1 = format!(
        " ❀图鉴 {}/{} | ▬常见{}/{} ▬角色{}/{} ▬传说{}/{} | ▶{} [{}科·{}] {}",
        garden.discovered.len(),
        TOTAL_SPECIES,
        backdrop,
        n_b,
        character,
        n_c,
        legend,
        n_l,
        species,
        genetics::family(variety).name(),
        t.tier().name(),
        if found { "✓已收录" } else { "未培育" },
    );
    let reasons = t.reasons();
    let line2 = if reasons.is_empty() {
        format!(
            " {} 是底纹层的平凡花——正是它衬出稀有 | 获取：{} | [←→↑↓]查看 [B/Esc]返回",
            genetics::variety_name(variety),
            t.how_to_get(),
        )
    } else {
        format!(
            " {} | 获取：{} | [←→↑↓]查看 [B/Esc]返回",
            reasons.join("；"),
            t.how_to_get(),
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
    hover: Option<render::PickTarget>,
) -> std::io::Result<()> {
    // 悬停优先：鼠标划到哪就读哪一株，不用先点选
    if let Some(render::PickTarget::Bed(h)) = hover {
        if h != selected {
            let expand_hint = match garden.expand_cost() {
                Some(c) => format!("{}", c),
                None => String::from("满"),
            };
            return draw_hud_lines(
                session,
                hud_row,
                cols,
                garden,
                weather,
                message,
                fast,
                hud_mode,
                decor_cursor,
                format!("👁 {}", describe_bed(garden, h)),
                expand_hint,
            );
        }
    }
    // 杂交预览优先：当前块与右侧块都盛开时，显示子代预测范围
    let preview = garden.plots.get(selected).and_then(|s| s.as_ref()).and_then(|a| {
        garden
            .plots
            .get(selected + 1)
            .and_then(|s| s.as_ref())
            .filter(|b| a.stage() == Stage::Bloom && b.stage() == Stage::Bloom)
            .map(|b| predict_child(&a.genes, &b.genes, &garden.affinity))
    });
    let info = if let Some(f) = preview {
        // 同科配种时把亲和加成显出来，"专攻一科"才成为看得见的策略
        let aff = match f.affinity_family {
            Some(fam) => format!(
                " |{}血统 Lv{}{}",
                fam.name(),
                f.affinity_level,
                if f.affinity_level < genetics::AFFINITY_MAX { "↑" } else { "(满)" }
            ),
            None => String::from(" |异科配种·无血统加成"),
        };
        format!(
            "[X]杂交预览: 色相{:.0}°~{:.0}° 株高{}-{} 发光{:.0}% 稀有觉醒{:.1}%{}",
            f.hue_lo,
            f.hue_hi,
            f.height_lo,
            f.height_hi,
            f.glow_chance * 100.0,
            f.rare_chance * 100.0,
            aff
        )
    } else {
        describe_bed(garden, selected)
    };
    let expand_hint = match garden.expand_cost() {
        Some(c) => format!("{}", c),
        None => String::from("满"),
    };
    draw_hud_lines(
        session, hud_row, cols, garden, weather, message, fast, hud_mode, decor_cursor, info,
        expand_hint,
    )
}

/// 一块地的完整描述。选中和**鼠标悬停**共用它——
/// 悬停即可读出这一株是什么、碰了哪条轴、为什么还没发芽。
///
/// 详略随 `garden.density`：清净只报名字和阶段，详尽会把它碰的每条轴都念出来。
fn describe_bed(garden: &Garden, idx: usize) -> String {
    let density = garden.density;
    match garden.plots.get(idx).and_then(|s| s.as_ref()) {
        Some(p) if density == garden::Density::Quiet => {
            let stage = match p.stage() {
                Stage::Seed => "种子",
                Stage::Sprout => "幼苗",
                Stage::Bud => "花苞",
                Stage::Bloom => "盛开",
            };
            format!(
                "{}号地: {} {} {:.0}%",
                idx + 1,
                display_name(&p.genes),
                stage,
                p.progress * 100.0
            )
        }
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
                // 详尽模式：把它碰的每一条轴都念出来（图鉴里那套自述）
                let axes = if density == garden::Density::Verbose {
                    let r = t.reasons();
                    if r.is_empty() {
                        String::new()
                    } else {
                        format!(" ◆{}", r.join("；"))
                    }
                } else {
                    String::new()
                };
                format!(
                    "{}号地: [{}{}] {} {} {:.0}%{}{}{}{}{}{}",
                    idx + 1,
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
                    axes,
                )
        }
        None => format!("{}号地: 空", idx + 1),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_hud_lines(
    session: &mut TerminalSession,
    hud_row: u32,
    cols: u16,
    garden: &Garden,
    weather: &Weather,
    message: &str,
    fast: bool,
    hud_mode: u8,
    decor_cursor: Option<usize>,
    info: String,
    expand_hint: String,
) -> std::io::Result<()> {
    let line1 = if let Some(slot) = decor_cursor {
        // 布景面板：自由摆放，拖拽是主要交互
        let here = garden.decor_items.get(slot);
        let stock: Vec<&str> = garden.decor_stock.iter().map(|d| d.name()).collect();
        format!(
            " ❖布景 已摆{}件{} | 库存: {} | 🖱拖动挪位·点空地摆放 | 完成委托可获得新布景",
            garden.decor_items.len(),
            match here {
                Some(d) => format!("，选中「{}」（{}）", d.kind.name(), d.kind.hint()),
                None => String::new(),
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
        let focus = match garden.affinity.focused() {
            Some((f, lv)) => format!(" 专攻{}Lv{}", f.name(), lv),
            None => String::new(),
        };
        format!(
            " {} · {}·{}{} | 种子x{} 堆肥x{}(扩张需{}){} | 图鉴{}/{} | {}",
            garden.biome.name(),
            weather.period_name(),
            weather.name(),
            if fast { " [x20]" } else { "" },
            garden.seeds.len(),
            garden.compost,
            expand_hint,
            focus,
            garden.discovered.len(),
            TOTAL_SPECIES,
            info
        )
    };
    let line2 = if decor_cursor.is_some() {
        format!(" [←→]选位置 [回车]摆放 [退格]收起 [D]退出布景 | {}", message)
    } else {
        format!(
            " 🖱点选/再点执行·拖动移植·悬停查看 | [回车]种 [W]浇 [H]收 [X]杂交 [P]钉 [C]堆肥 [E]扩张 [D]布景 [Q]委托 [B]图鉴 [Tab]{} [Esc]退出 | {}",
            garden.density.name(),
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
