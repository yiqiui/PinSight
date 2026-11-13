//! Pure geometry for arranging pinned image windows.
//!
//! Kept free of window handles so the layout rules are unit-testable on every
//! platform. `pin::Pin::move_to` performs the actual OS move.

use crate::util::PhysRect;

/// A pin's id and its on-screen size (already zoom-adjusted by the caller).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinBox {
    pub id: u64,
    pub w: u32,
    pub h: u32,
}

/// Lay pins out in a uniform, non-overlapping grid inside `area`.
///
/// Cells are sized to the largest pin so every row/column lines up; smaller
/// pins are centred in their cell. The column count aims for a grid whose shape
/// roughly matches the monitor's aspect ratio, but never exceeds what fits in
/// `area.w`. Rows may extend past the bottom if there are more pins than fit —
/// a pin window is allowed to sit partly off-screen, and silently dropping
/// pins would be worse.
pub fn tile(pins: &[PinBox], area: PhysRect, gap: u32) -> Vec<(u64, i32, i32)> {
    if pins.is_empty() {
        return Vec::new();
    }
    let g = gap.min(64) as i64;
    let max_w = pins.iter().map(|p| p.w as i64).max().unwrap_or(1).max(1);
    let max_h = pins.iter().map(|p| p.h as i64).max().unwrap_or(1).max(1);
    let cell_w = max_w + g;
    let cell_h = max_h + g;
    let n = pins.len();

    let cols_by_width = ((area.w as i64 + g) / cell_w).clamp(1, n as i64) as usize;
    let ideal = ((n as f64 * area.w as f64 * cell_h as f64)
        / (area.h.max(1) as f64 * cell_w as f64))
        .sqrt()
        .ceil()
        .max(1.0) as usize;
    let cols = ideal.clamp(1, cols_by_width).max(1);

    let mut out = Vec::with_capacity(n);
    for (i, p) in pins.iter().enumerate() {
        let col = (i % cols) as i64;
        let row = (i / cols) as i64;
        // Centre a smaller pin within its cell.
        let dx = (max_w - p.w as i64) / 2;
        let dy = (max_h - p.h as i64) / 2;
        let x = (area.x as i64 + col * cell_w + dx).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let y = (area.y as i64 + row * cell_h + dy).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        out.push((p.id, x, y));
    }
    out
}

/// Round every pin position to the nearest multiple of `grid`, so pins dragged
/// by hand can be aligned afterwards. A zero / absurd grid is clamped.
pub fn snap(positions: &[(u64, i32, i32)], grid: u32) -> Vec<(u64, i32, i32)> {
    let g = grid.clamp(1, 256) as f64;
    positions
        .iter()
        .map(|&(id, x, y)| {
            let sx = ((x as f64 / g).round() * g).clamp(i32::MIN as f64, i32::MAX as f64) as i32;
            let sy = ((y as f64 / g).round() * g).clamp(i32::MIN as f64, i32::MAX as f64) as i32;
            (id, sx, sy)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> PhysRect {
        PhysRect {
            x: 0,
            y: 0,
            w: 1000,
            h: 800,
        }
    }

    #[test]
    fn empty_input_produces_no_moves() {
        assert!(tile(&[], area(), 10).is_empty());
        assert!(snap(&[], 16).is_empty());
    }

    #[test]
    fn tiled_pins_never_overlap_and_stay_inside_the_area() {
        let pins: Vec<PinBox> = (0..6)
            .map(|i| PinBox {
                id: i,
                w: 200,
                h: 150,
            })
            .collect();
        let out = tile(&pins, area(), 10);
        assert_eq!(out.len(), pins.len());
        for (i, &(id, x, y)) in out.iter().enumerate() {
            assert_eq!(id, i as u64);
            assert!(x >= 0 && y >= 0, "pin {id} went off the top-left: {x},{y}");
            assert!(x + 200 <= 1000, "pin {id} exceeds the right edge: {x}");
        }
        // Every pair is separated by at least one cell on x or y.
        for a in 0..out.len() {
            for b in (a + 1)..out.len() {
                let (_, ax, ay) = out[a];
                let (_, bx, by) = out[b];
                assert!(
                    (ax - bx).abs() >= 200 || (ay - by).abs() >= 150,
                    "pins {a} and {b} overlap"
                );
            }
        }
    }

    #[test]
    fn smaller_pins_are_centred_in_their_cells() {
        let pins = [
            PinBox {
                id: 1,
                w: 200,
                h: 100,
            },
            PinBox {
                id: 2,
                w: 100,
                h: 100,
            },
        ];
        let out = tile(&pins, area(), 0);
        // With a 1000-wide area both fit in one row of 200-wide cells.
        assert_eq!(out[0], (1, 0, 0));
        assert_eq!(out[1], (2, 250, 0));
    }

    #[test]
    fn a_single_huge_pin_stays_at_one_column() {
        let pins = [PinBox {
            id: 9,
            w: 1900,
            h: 100,
        }];
        let out = tile(&pins, area(), 8);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], (9, 0, 0));
    }

    #[test]
    fn snap_rounds_both_directions_and_clamps_bad_grid() {
        let pos = [(1, 17, -17), (2, 24, 31), (3, 0, 100)];
        assert_eq!(snap(&pos, 16), [(1, 16, -16), (2, 32, 32), (3, 0, 96)]);
        assert_eq!(snap(&[(1, 17, 17)], 0), [(1, 17, 17)]);
        assert_eq!(snap(&[(1, 17, 17)], 9999), [(1, 0, 0)]);
    }
}
