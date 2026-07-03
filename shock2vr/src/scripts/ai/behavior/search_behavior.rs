use dark::motion::MotionQueryItem;

use super::Behavior;

#[allow(dead_code)]
pub struct SearchBehavior;

impl Behavior for SearchBehavior {
    fn name(&self) -> &'static str {
        "Search"
    }

    fn animation(self: &SearchBehavior) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("search"),
            MotionQueryItem::new("scan").optional(),
        ]
    }
}
