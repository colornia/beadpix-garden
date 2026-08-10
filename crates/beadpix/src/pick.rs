//! 命中缓冲：与像素画布并行的一张「这一格是什么东西」的表。
//!
//! 鼠标交互的地基。拖拽、悬停、点击都从这里取目标，上层不必自己算几何。
//!
//! **白捡的便宜**：绘制顺序天然是画家算法（后画的盖住先画的），
//! 命中缓冲用同样的顺序涂写，于是**遮挡优先级自动正确**——
//! 点到的永远是最上层那个，不需要额外做 z 排序。
//!
//! 坐标约定：涂写用**像素**坐标（和绘制代码一致），查询用**字符格**坐标
//! （和终端鼠标事件一致）。半块渲染下 1 格 = 纵向 2 像素，换算在内部完成。

/// 一格 = 纵向多少像素。半块渲染器是 2。
const PIXELS_PER_ROW: i32 = 2;

pub struct PickBuffer<T> {
    cols: u32,
    rows: u32,
    cells: Vec<Option<T>>,
}

impl<T: Copy + PartialEq> PickBuffer<T> {
    /// 按**像素**画布尺寸创建；内部按字符格分辨率存储。
    pub fn new(canvas_w: u32, canvas_h: u32) -> Self {
        let cols = canvas_w;
        let rows = canvas_h / PIXELS_PER_ROW as u32;
        PickBuffer {
            cols,
            rows,
            cells: vec![None; (cols * rows) as usize],
        }
    }

    pub fn clear(&mut self) {
        self.cells.iter_mut().for_each(|c| *c = None);
    }

    pub fn cols(&self) -> u32 {
        self.cols
    }

    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// 在像素坐标 (x, y) 处标记目标。越界忽略。
    pub fn paint(&mut self, x: i32, y: i32, target: T) {
        let row = y.div_euclid(PIXELS_PER_ROW);
        if x < 0 || row < 0 || x >= self.cols as i32 || row >= self.rows as i32 {
            return;
        }
        self.cells[(row as u32 * self.cols + x as u32) as usize] = Some(target);
    }

    /// 在像素矩形内标记目标（左上角 + 宽高，像素单位）。
    pub fn paint_rect(&mut self, x: i32, y: i32, w: i32, h: i32, target: T) {
        for dy in 0..h {
            for dx in 0..w {
                self.paint(x + dx, y + dy, target);
            }
        }
    }

    /// 以 (cx, cy) 为中心标记一块像素区域——精灵/植株的点击热区用它，
    /// 比逐像素涂省事，也让细长的茎不至于难点中。
    pub fn paint_around(&mut self, cx: i32, cy: i32, half_w: i32, half_h: i32, target: T) {
        self.paint_rect(
            cx - half_w,
            cy - half_h,
            half_w * 2 + 1,
            half_h * 2 + 1,
            target,
        );
    }

    /// 按**字符格**坐标查询（直接喂终端鼠标事件的 column/row）。
    pub fn at(&self, col: u16, row: u16) -> Option<T> {
        if col as u32 >= self.cols || row as u32 >= self.rows {
            return None;
        }
        self.cells[(row as u32 * self.cols + col as u32) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum T {
        A,
        B,
    }

    #[test]
    fn pixel_rows_collapse_to_character_cells() {
        let mut p: PickBuffer<T> = PickBuffer::new(10, 8);
        assert_eq!(p.rows(), 4);
        // 像素行 2 和 3 属于同一个字符行
        p.paint(5, 2, T::A);
        assert_eq!(p.at(5, 1), Some(T::A));
        p.clear();
        p.paint(5, 3, T::A);
        assert_eq!(p.at(5, 1), Some(T::A));
    }

    /// 画家顺序 = 正确的遮挡优先级，不需要额外 z 排序。
    #[test]
    fn later_paint_wins_giving_free_occlusion_order() {
        let mut p: PickBuffer<T> = PickBuffer::new(10, 8);
        p.paint(3, 0, T::A);
        p.paint(3, 0, T::B); // 后画的（前排）盖住先画的（后排）
        assert_eq!(p.at(3, 0), Some(T::B));
    }

    #[test]
    fn out_of_range_is_ignored_not_panicking() {
        let mut p: PickBuffer<T> = PickBuffer::new(10, 8);
        p.paint(-1, 0, T::A);
        p.paint(0, -5, T::A);
        p.paint(99, 99, T::A);
        assert_eq!(p.at(0, 0), None);
        assert_eq!(p.at(200, 200), None);
    }

    #[test]
    fn paint_around_covers_a_centred_box() {
        let mut p: PickBuffer<T> = PickBuffer::new(20, 20);
        p.paint_around(10, 10, 2, 2, T::A); // 像素 x 8..12, y 8..12 → 格行 4..6
        assert_eq!(p.at(10, 5), Some(T::A));
        assert_eq!(p.at(8, 4), Some(T::A));
        assert_eq!(p.at(13, 5), None);
    }
}
