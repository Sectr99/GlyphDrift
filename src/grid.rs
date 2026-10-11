//! Character grid that animations write into and renderers read from.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    /// 0 = invisible, 255 = full brightness.
    pub brightness: u8,
}

impl Cell {
    pub const EMPTY: Cell = Cell {
        ch: ' ',
        brightness: 0,
    };
}

#[derive(Clone, Debug)]
pub struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![Cell::EMPTY; cols * rows],
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
        self.cells.clear();
        self.cells.resize(cols * rows, Cell::EMPTY);
    }

    pub fn clear(&mut self) {
        self.cells.fill(Cell::EMPTY);
    }

    /// Out-of-range writes are ignored.
    pub fn set(&mut self, x: usize, y: usize, cell: Cell) {
        if x < self.cols && y < self.rows {
            self.cells[y * self.cols + x] = cell;
        }
    }

    pub fn get(&self, x: usize, y: usize) -> Option<Cell> {
        (x < self.cols && y < self.rows).then(|| self.cells[y * self.cols + x])
    }

    pub fn row(&self, y: usize) -> &[Cell] {
        &self.cells[y * self.cols..(y + 1) * self.cols]
    }

    /// Plain-text dump for snapshot tests.
    pub fn to_text(&self) -> String {
        let mut s = String::with_capacity((self.cols + 1) * self.rows);
        for y in 0..self.rows {
            s.extend(self.row(y).iter().map(|c| c.ch));
            s.push('\n');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_and_bounds() {
        let mut g = Grid::new(3, 2);
        g.set(
            2,
            1,
            Cell {
                ch: 'x',
                brightness: 200,
            },
        );
        assert_eq!(g.get(2, 1).unwrap().ch, 'x');
        g.set(
            3,
            0,
            Cell {
                ch: 'y',
                brightness: 1,
            },
        ); // ignored
        assert!(g.get(3, 0).is_none());
    }

    #[test]
    fn resize_clears() {
        let mut g = Grid::new(2, 2);
        g.set(
            0,
            0,
            Cell {
                ch: 'a',
                brightness: 9,
            },
        );
        g.resize(4, 1);
        assert_eq!((g.cols(), g.rows()), (4, 1));
        assert_eq!(g.get(0, 0), Some(Cell::EMPTY));
    }
}
