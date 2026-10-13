// SPDX-License-Identifier: Apache-2.0

use crate::agent::model;

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub enum PlanSource {
    #[default]
    None,
    Tasks,
    Todo,
}

#[derive(Default)]
pub struct FooterState {
    pub status_line: Option<String>,
    pub todo_plan: Vec<model::TodoPlanItem>,
    pub plan_source: PlanSource,
}

impl FooterState {
    pub fn apply_todo_plan(&mut self, items: Vec<model::TodoPlanItem>) {
        self.todo_plan = items;
        self.plan_source = PlanSource::Todo;
    }

    pub fn clear(&mut self) {
        self.status_line = None;
        self.todo_plan.clear();
        self.plan_source = PlanSource::None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_todo_selects_itself_even_when_empty_and_reset_clears_selection() {
        let mut footer = FooterState { plan_source: PlanSource::Tasks, ..FooterState::default() };
        footer.apply_todo_plan(vec![model::TodoPlanItem {
            content: "New plan".to_owned(),
            active_form: None,
            status: model::TaskStatus::Pending,
        }]);
        assert!(footer.plan_source == PlanSource::Todo);
        assert_eq!(footer.todo_plan.len(), 1);
        footer.apply_todo_plan(Vec::new());
        assert!(footer.plan_source == PlanSource::Todo);
        assert!(footer.todo_plan.is_empty());
        footer.clear();
        assert!(footer.plan_source == PlanSource::None);
    }
}
