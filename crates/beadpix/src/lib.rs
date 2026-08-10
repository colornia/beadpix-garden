//! beadpix — 终端高分辨率像素渲染引擎
//!
//! 用半块字符 `▀` 把每个终端字符格拆成上下两个真彩色像素，
//! 配合差分渲染（只重绘变化的格子）实现高帧率不闪烁的输出。

pub mod canvas;
pub mod color;
pub mod hybrid;
pub mod renderer;
pub mod sprite;
pub mod terminal;

pub use canvas::Canvas;
pub use color::Rgb;
pub use hybrid::HybridRenderer;
pub use renderer::HalfBlockRenderer;
pub use sprite::{Palette, Sprite};
pub use terminal::TerminalSession;
