#[derive(Clone)]
pub struct KnitState {
    pub active: bool,

    pub row: usize,
    pub col: usize,

    pub width: usize,
    pub height: usize,

    pub ksl_high: bool,
    pub nd1_high: bool,
    pub dir_right_to_left: bool,
}

impl KnitState {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            active: false,
            row: 0,
            col: 0,
            width,
            height,
            ksl_high: false,
            nd1_high: false,
            dir_right_to_left: false,
        }
    }

    pub fn reset(&mut self) {
        self.row = 0;
        self.col = 0;
        self.active = true;
    }

    pub fn stop(&mut self) {
        self.active = false;
    }
}
