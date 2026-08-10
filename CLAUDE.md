# beadpix 项目备忘

终端高分辨率像素项目：Cargo workspace，三个 crate。用户语言：中文。

## 结构

- `crates/beadpix`：渲染引擎库。半块字符 `▀`（1 格=2 像素）差分渲染器 `renderer.rs`；
  盲文混合渲染器 `hybrid.rs`（1 格=8 像素，逐格择优）；`canvas.rs` 像素缓冲；
  `sprite.rs` 索引色精灵；`terminal.rs` 终端守卫（含 Windows UTF-8 代码页设置）
- `crates/garden`：花园挂机游戏（当前重心）。四群系（草甸/林间/石缝/深海，开局随机）、
  连续昼夜循环（`weather.rs` 的 clock，600 秒一天）、光照贴图系统（`render.rs` apply_lighting）、
  六大科系 14 品种×8 色系=112 格图鉴、基因杂交（`genetics.rs`）、JSON 存档。
  设计骨架见 `E:\desktop\beadpix_redesign_plan.md`（cozy 外壳 + 育种脊椎），已全部实现：
  有限地块+堆肥扩张、遗传预览+钉目标、离线事件日记、vitality 软照料、
  委托脊椎（`quest.rs`）、PlantTraits 五条轴（`traits.rs`）、移植布景
- 游戏逻辑有 14 项测试（`garden.rs` 底部 mod tests），改经济/照料/离线规则后跑
  `cargo test -p garden --target-dir <scratchpad>/build`。其中两项守护 cozy 契约：
  植物永不死亡、每天来一次的玩家看不到蔫——**这两条测试不许改成通过为止**
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

## 加新品种的标准流程（系统六纪律，照做）

以天时科（月见花/日晷花）为范例，改这五处：
1. `traits.rs` 的 `traits_of`：**先在这里回答它碰哪条轴**，碰不到就别加
2. `genetics.rs`：`VARIETIES` 计数、`variety_name`、`family`、突变表、`wild` 池
3. `render.rs`：`draw_plant` 分发 + 一个 `draw_xxx` 函数（需要昼夜就用 `dark` 参数）
4. `main.rs`：`TOTAL_SPECIES`（=8×品种数）、`Family` 匹配分支、截图配置加一行
5. 光源规则：`draw` 里 `intrinsic` 与常亮分支（自发光品种）

## 路线图

方案阶段 0-4 与系统六已全部落地（见 git log）。后续候选：
- **昙花（Ephemeral）尚未实现**：开一夜就谢、得赶上那一刻。注意它是唯一会
  "失去"的机制，与 cozy 契约有张力——要做的话务必让玩家事先知道并能预期，
  且谢掉时留下种子（不是净损失）
- 轴1 的 OnRain/MeteorDrop 存在条件（雷种、陨种）——引擎已有事件，接上即可
- 布景深化：装饰格、自由摆放坐标（当前只有地块间移植）
- 图鉴按 常见/角色/传说 分层展示（现在只有统计，网格仍是扁平的）
- 访客/明信片分享——用户说过"不着急"
- face 项目：摄像头驱动是远期目标
