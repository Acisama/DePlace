use deplace_core::state::AppState;

pub struct Home {
    state: AppState,
}

impl Home {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}
