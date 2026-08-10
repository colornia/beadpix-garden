mod face;

use std::io::Write;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use rand::Rng;

use beadpix::{Canvas, HalfBlockRenderer, HybridRenderer, Rgb, TerminalSession};
use face::{draw_face, Emotion};

const FPS: u64 = 60;
const HUD_ROWS: u16 = 1;

/// 渲染后端：半块（每格 2 像素全真彩）或混合（每格 8 像素，
/// 平滑区走半块、高对比轮廓自动切盲文点阵）。
enum Renderer {
    Half(HalfBlockRenderer),
    Hybrid(HybridRenderer),
}

impl Renderer {
    /// 该模式下的画布逻辑分辨率
    fn canvas_size(hybrid: bool, cols: u16, rows: u16) -> (u32, u32) {
        let rows = rows.saturating_sub(HUD_ROWS) as u32;
        if hybrid {
            ((cols as u32 * 2).clamp(80, 400), (rows * 4).clamp(80, 360))
        } else {
            ((cols as u32).clamp(40, 200), (rows * 2).clamp(40, 180))
        }
    }

    fn build(hybrid: bool, w: u32, h: u32) -> Renderer {
        if hybrid {
            Renderer::Hybrid(HybridRenderer::new(w, h))
        } else {
            Renderer::Half(HalfBlockRenderer::new(w, h))
        }
    }

    fn present(
        &mut self,
        canvas: &Canvas,
        out: &mut impl Write,
        col: u16,
        row: u16,
    ) -> std::io::Result<()> {
        match self {
            Renderer::Half(r) => r.present(canvas, out, col, row),
            Renderer::Hybrid(r) => r.present(canvas, out, col, row),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Renderer::Half(_) => "half-block",
            Renderer::Hybrid(_) => "hybrid+braille",
        }
    }
}

/// 自主行为状态：眨眼、视线漂移、呼吸——没有输入时脸也是"活"的。
struct Autonomy {
    blink_timer: f32,
    blink_phase: f32, // >0 表示正在眨眼
    gaze_target: (f32, f32),
    gaze_timer: f32,
}

impl Autonomy {
    fn new(rng: &mut impl Rng) -> Self {
        Autonomy {
            blink_timer: rng.gen_range(2.0..5.0),
            blink_phase: 0.0,
            gaze_target: (0.0, 0.0),
            gaze_timer: rng.gen_range(1.5..4.0),
        }
    }

    fn update(&mut self, dt: f32, rng: &mut impl Rng) {
        self.blink_timer -= dt;
        if self.blink_timer <= 0.0 {
            self.blink_phase = 0.25; // 一次眨眼 0.25 秒
            self.blink_timer = rng.gen_range(2.0..6.0);
        }
        if self.blink_phase > 0.0 {
            self.blink_phase -= dt;
        }

        self.gaze_timer -= dt;
        if self.gaze_timer <= 0.0 {
            self.gaze_target = (rng.gen_range(-0.6..0.6), rng.gen_range(-0.4..0.4));
            self.gaze_timer = rng.gen_range(1.5..5.0);
        }
    }

    /// 眨眼系数：眨眼中段为 0（全闭），两端为 1。
    fn blink(&self) -> f32 {
        if self.blink_phase <= 0.0 {
            return 1.0;
        }
        let t = self.blink_phase / 0.25; // 1 -> 0
        let closed = 1.0 - (t * 2.0 - 1.0).abs(); // 三角波
        1.0 - closed
    }
}

fn main() -> std::io::Result<()> {
    let mut rng = rand::thread_rng();

    let (cols, rows) = TerminalSession::size()?;
    let mut hybrid = true; // 默认用混合渲染
    let (w, h) = Renderer::canvas_size(hybrid, cols, rows);

    let mut session = TerminalSession::begin()?;
    let mut canvas = Canvas::new(w, h);
    let mut renderer = Renderer::build(hybrid, w, h);

    let mut emotion = Emotion::Neutral;
    let mut params = emotion.params();
    let mut autonomy = Autonomy::new(&mut rng);
    let mut gaze = (0.0f32, 0.0f32);
    let mut manual_gaze: Option<(f32, f32)> = None;
    let mut talking = false;
    let mut time = 0.0f32;

    let frame = Duration::from_micros(1_000_000 / FPS);
    let mut last = Instant::now();
    let mut fps_avg = 0.0f32;

    loop {
        match TerminalSession::poll_key(Duration::from_millis(1))? {
            Some(KeyCode::Esc) => break,
            Some(KeyCode::Char(c @ '1'..='6')) => {
                emotion = match c {
                    '1' => Emotion::Neutral,
                    '2' => Emotion::Happy,
                    '3' => Emotion::Surprised,
                    '4' => Emotion::Sad,
                    '5' => Emotion::Angry,
                    _ => Emotion::Sleepy,
                };
            }
            Some(KeyCode::Char('t')) | Some(KeyCode::Char('T')) => talking = !talking,
            Some(KeyCode::Char('r')) | Some(KeyCode::Char('R')) => {
                // 切换渲染后端：重建画布和渲染器，清屏后全量重绘
                hybrid = !hybrid;
                let (w, h) = Renderer::canvas_size(hybrid, cols, rows);
                canvas = Canvas::new(w, h);
                renderer = Renderer::build(hybrid, w, h);
                write!(session.out(), "\x1b[2J")?;
            }
            Some(KeyCode::Left) => nudge(&mut manual_gaze, -0.3, 0.0),
            Some(KeyCode::Right) => nudge(&mut manual_gaze, 0.3, 0.0),
            Some(KeyCode::Up) => nudge(&mut manual_gaze, 0.0, -0.3),
            Some(KeyCode::Down) => nudge(&mut manual_gaze, 0.0, 0.3),
            Some(KeyCode::Char(' ')) => manual_gaze = None, // 空格交还自主视线
            _ => {}
        }

        let dt = last.elapsed().as_secs_f32();
        last = Instant::now();
        time += dt;
        if dt > 0.0 {
            fps_avg = fps_avg * 0.95 + (1.0 / dt) * 0.05;
        }

        autonomy.update(dt, &mut rng);

        // 表情参数插值
        let mut target = emotion.params();
        if talking {
            // 说话：嘴部开合叠加正弦节奏
            target.mouth_open =
                (target.mouth_open + 0.5 + (time * 12.0).sin() * 0.4).clamp(0.0, 1.0);
        }
        params.approach(&target, dt, 8.0);

        // 视线：手动优先，否则自主漂移；平滑跟随
        let gt = manual_gaze.unwrap_or(autonomy.gaze_target);
        let t = 1.0 - (-6.0 * dt).exp();
        gaze.0 += (gt.0 - gaze.0) * t;
        gaze.1 += (gt.1 - gaze.1) * t;

        let breath = (time * 1.8).sin() * canvas.height() as f32 * 0.008;

        // 渲染
        canvas.clear(Rgb::new(24, 26, 34));
        draw_face(&mut canvas, &params, gaze, breath, autonomy.blink());
        renderer.present(&canvas, session.out(), 1, 1)?;

        let hud = format!(
            " [1-6] Emotions  [Arrows] Gaze  [Space] Auto-gaze  [T] Talk  [R] Renderer  [Esc] Quit | {} {} | {} | {:.0} fps",
            emotion.name(),
            if talking { "(talking)" } else { "" },
            renderer.name(),
            fps_avg
        );
        let out = session.out();
        write!(
            out,
            "\x1b[{};1H\x1b[0m\x1b[97m\x1b[48;2;30;30;40m{:<w$.w$}\x1b[0m",
            rows,
            hud,
            w = cols as usize
        )?;
        out.flush()?;

        std::thread::sleep(frame.saturating_sub(last.elapsed()));
    }
    Ok(())
}

fn nudge(gaze: &mut Option<(f32, f32)>, dx: f32, dy: f32) {
    let g = gaze.get_or_insert((0.0, 0.0));
    g.0 = (g.0 + dx).clamp(-1.0, 1.0);
    g.1 = (g.1 + dy).clamp(-1.0, 1.0);
}
