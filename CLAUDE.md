# beadpix 项目备忘

终端高分辨率像素项目：Cargo workspace，三个 crate。用户语言：中文。

## 结构

- `crates/beadpix`：渲染引擎库。半块字符 `▀`（1 格=2 像素）差分渲染器 `renderer.rs`；
  盲文混合渲染器 `hybrid.rs`（1 格=8 像素，逐格择优）；`canvas.rs` 像素缓冲；
  `sprite.rs` 索引色精灵；`terminal.rs` 终端守卫（含 Windows UTF-8 代码页设置）
- `crates/garden`：花园挂机游戏（当前重心）。四群系（草甸/林间/石缝/深海，开局随机）、
  连续昼夜循环（`weather.rs` 的 clock，600 秒一天）、光照贴图系统（`render.rs` apply_lighting）、
  五大科系 12 品种×8 色系=96 格图鉴、基因杂交（`genetics.rs`）、JSON 存档
- `crates/face`：数字人脸，60fps，参数化表情，`R` 键切换半块/混合渲染

## 关键约定

- 编译验证用独立目标目录（用户常开着游戏，`target/release/garden.exe` 会被锁）：
  `cargo build --release --target-dir <scratchpad>/build`
- 视觉验证：`garden.exe --screenshot <目录>` 输出各场景 BMP（6x 放大），
  PowerShell System.Drawing 转 PNG 后用 Read 查看。别盲改渲染代码
- 测试模式：`garden --fast`（生长与昼夜 20 倍速）
- 工具链：Windows + GNU toolchain（MSVC 未装）；`parking_lot_core` 锁在 0.9.9
  绕开 raw-dylib 的 dlltool 依赖，勿升级该依赖
- HUD 中文按显示宽度对齐（CJK=2 格），见 main.rs `pad_line`
- 存档兼容：Garden/Genes 新字段一律 `#[serde(default)]`

## 路线图（用户认可的方向）

- 品种继续扩充（每科 4 种）、血统亲和系统、图鉴翻页查看
- 访客/分享系统（明信片导出）——用户说过"不着急"
- face 项目：摄像头驱动是远期目标
