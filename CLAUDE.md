# beadpix 项目备忘

终端高分辨率像素项目：Cargo workspace，三个 crate。用户语言：中文。

## 结构

- `crates/beadpix`：渲染引擎库。半块字符 `▀`（1 格=2 像素）差分渲染器 `renderer.rs`；
  盲文混合渲染器 `hybrid.rs`（1 格=8 像素，逐格择优）；`canvas.rs` 像素缓冲；
  `sprite.rs` 索引色精灵；`terminal.rs` 终端守卫（含 Windows UTF-8 代码页设置）
- `crates/garden`：花园挂机游戏（当前重心）。四群系（草甸/林间/石缝/深海，开局随机）、
  连续昼夜循环（`weather.rs` 的 clock，600 秒一天）、光照贴图系统（`render.rs` apply_lighting）、
  五大科系 12 品种×8 色系=96 格图鉴、基因杂交（`genetics.rs`）、JSON 存档。
  设计骨架见 `E:\desktop\beadpix_redesign_plan.md`（cozy 外壳 + 育种脊椎）：
  有限地块+堆肥扩张、遗传预览+钉目标、离线事件日记、vitality 软照料、
  委托脊椎（`quest.rs`）、PlantTraits 五条轴（`traits.rs`）
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

## 核心纪律（来自设计方案，务必遵守）

- **每加一株植物先问它碰哪条轴**（traits.rs 五条轴），碰不到就别加。
  要造角色不是户口本；底纹层的平凡花是稀有花的背景板，别省
- **经济只有一条链**：退役植株 → 堆肥 → 扩张地块。不加货币/商店/加速道具
- **蔫的阈值宁松勿紧**：每天来一次的玩家应永远看不到蔫（当前 24h 才明显）
- **植物永不死亡**，vitality 地板 0.15。任何"我离开导致花没了"都是失败
- 遗传预览（`predict_child`）与真实杂交（`Genes::cross`）必须保持一致

## 路线图

已完成阶段 0-4 + 系统六骨架（见 git log）。后续：
- 布景系统（移动/摆放植株 + 装饰格）——方案里的"真·终局"，吃渲染优势
- 每周轮换稀有种池；委托指向"碰轴"表型的比例再提高
- 新品种按五条轴设计（月见花=NightOnly、雷种=OnRain、昙花=Ephemeral、
  古树=Ancient 与堆肥经济形成反张力）
- 访客/明信片分享——用户说过"不着急"
- face 项目：摄像头驱动是远期目标
