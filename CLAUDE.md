# beadpix-garden 项目备忘

终端像素花园挂机游戏：Cargo workspace，两个 crate。用户语言：中文。

> 人脸项目已拆到独立仓库 [beadpix-face](https://github.com/colornia/beadpix-face)
> （本地 `E:\desktop\beadpix-face`），**各自持有一份 `crates/beadpix` 副本**，
> 允许独立演进。引擎改动若两边都需要，手动同步那个文件。

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

## 鼠标交互（P1 地基，已落地第一步）

- `beadpix/pick.rs` 的 `PickBuffer<T>`：与像素画布并行的字符格命中表。
  **涂写用像素坐标、查询用字符格坐标**（半块下 1 格 = 纵向 2 像素，内部换算）。
  依赖画家顺序天然获得正确遮挡优先级——后画的盖先画的，不需要 z 排序
- `Scene::draw` 每帧重建它；`Scene::pick_at(col,row)` 直接喂终端鼠标事件坐标
- `TerminalSession::enable_mouse()` + Drop 自动关闭（**绝不能让崩溃把终端卡在鼠标模式**）
- `poll_input()` 返回 `Input::{Key,Mouse}`；鼠标翻译成动作后走**和热键同一条代码路径**
  （见 `primary_action_key`），所以两种输入永远不会行为分叉
- 键盘全部保留。鼠标是并行的可发现层，不是替代
- **拖拽**：LeftDown 记起点 → 移出起点格才算真拖 → LeftUp 时决定是"点击"还是"放下"。
  点击与拖拽因此共用左键而不打架。拖植株=移植，拖布景=自由挪位
- **悬停两级**（NN/g 规范）：立刻给轻量高亮，350ms 后 HUD 才换详情。
  `Density` 可调三档——清净(悬停不接管)/标准/详尽(即时+念出所有轴)
- 布景已从 6 个固定槽位改为**自由坐标**（`decor::Placed` 存画布比例，
  窗口缩放不会散架）；旧存档在 `load()` 里自动迁移

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
- 遗传预览（`predict_child`）与真实杂交（`Genes::cross_with`）必须保持一致，
  两者共用 `Affinity::rare_chance`；测试 `affinity_raises_both_the_forecast_and_the_real_odds`
  守着这条——预览骗人比没有预览更糟

## 加新品种的标准流程（系统六纪律，照做）

以天时科（月见花/日晷花）为范例，改这五处：
1. `traits.rs` 的 `traits_of`：**先在这里回答它碰哪条轴**，碰不到就别加
2. `genetics.rs`：`VARIETIES` 计数、`variety_name`、`family`、突变表、`wild` 池
3. `render.rs`：`draw_plant` 分发 + 一个 `draw_xxx` 函数（需要昼夜就用 `dark` 参数）
4. `main.rs`：`TOTAL_SPECIES`（=8×品种数）、`Family` 匹配分支、截图配置加一行
5. 光源规则：`draw` 里 `intrinsic` 与常亮分支（自发光品种）

## 已落地的完整轴覆盖（系统六）

五条轴现已全部有实现范例，加新品种时照抄最接近的那个：

- 轴1 存在条件：月见花/昙花（NightOnly）、雷纹花（OnRain，`grow()` 里门控发芽）、
  陨星花（MeteorDrop，`Garden::meteor_seed`，**突变表里没有它**）、海葵珊瑚（BiomeLocked）
- 轴2 实时行为：眨眼、蠕动、向日、按时开合、Spark（电弧/星芒）
- 轴3 邻居关系：触手藤枯萎、曼德拉草与日晷花夜间滋养
- 轴4 遗传特殊性：MutationOnly（异界/炼金/天时）
- 轴5 时间尺度：古株（通用，越老越结籽）、昙花（Ephemeral，见下）

**昙花的 cozy 处理**（唯一会"谢"的品种，务必保持这三条）：
花苞期 HUD 就预告"入夜才开·开一夜即谢(留种)"；谢时留 2-3 颗种子是净赚；
测试 `only_ephemeral_ever_withers` 守着"其余品种永不消失"。
真正失去的只有"亲眼看见"这件事——奖励在场，而不是惩罚离开。

## 路线图

方案阶段 0-4、系统六五条轴、图鉴分层、布景装饰格、血统亲和均已落地（见 git log）。
后续候选：
- 布景再深化：自由摆放坐标（当前是 6 个固定槽位）、更多装饰种类
- 访客/明信片分享——用户说过"不着急"
- 音效/标题画面这类包装

图鉴光标（`B` 进入后方向键移动）会念出 `PlantTraits::reasons()`——
**加新品种时若 reasons() 是空的，说明它没碰任何轴，按纪律就不该加。**
