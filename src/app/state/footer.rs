// SPDX-License-Identifier: Apache-2.0

use crate::agent::model;

#[derive(Default)]
pub struct FooterState {
    pub status_line: Option<String>,
    pub todo_plan: Vec<model::TodoPlanItem>,
}

impl FooterState {
    pub fn clear(&mut self) {
        self.status_line = None;
        self.todo_plan.clear();
    }
}
