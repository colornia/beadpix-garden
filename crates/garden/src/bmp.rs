//! 调试用：把画布导出为 BMP 图片（放大若干倍），用于离线检查渲染效果。

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use beadpix::Canvas;

/// 24-bit 无压缩 BMP，每个逻辑像素放大为 scale×scale 的方块。
pub fn write_bmp(path: &Path, canvas: &Canvas, scale: u32) -> io::Result<()> {
    let w = canvas.width() * scale;
    let h = canvas.height() * scale;
    let row_bytes = (w * 3 + 3) & !3; // 每行对齐到 4 字节
    let data_size = row_bytes * h;
    let file_size = 54 + data_size;

    let mut out = BufWriter::new(File::create(path)?);
    // 文件头
    out.write_all(b"BM")?;
    out.write_all(&file_size.to_le_bytes())?;
    out.write_all(&[0u8; 4])?;
    out.write_all(&54u32.to_le_bytes())?;
    // 信息头
    out.write_all(&40u32.to_le_bytes())?;
    out.write_all(&(w as i32).to_le_bytes())?;
    out.write_all(&(h as i32).to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?;
    out.write_all(&24u16.to_le_bytes())?;
    out.write_all(&[0u8; 24])?; // 压缩方式等全 0

    // 像素数据：BMP 行序自下而上，颜色 BGR
    let pad = vec![0u8; (row_bytes - w * 3) as usize];
    let mut row = Vec::with_capacity((w * 3) as usize);
    for y in (0..canvas.height()).rev() {
        row.clear();
        for x in 0..canvas.width() {
            let p = canvas.get(x as i32, y as i32).unwrap();
            for _ in 0..scale {
                row.extend_from_slice(&[p.b, p.g, p.r]);
            }
        }
        for _ in 0..scale {
            out.write_all(&row)?;
            out.write_all(&pad)?;
        }
    }
    out.flush()
}
