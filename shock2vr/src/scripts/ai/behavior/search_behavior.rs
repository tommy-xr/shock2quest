use dark::motion::MotionQueryItem;

use super::Behavior;

pub struct SearchBehavior;

impl Behavior for SearchBehavior {
    fn animation(self: &SearchBehavior) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("search"),
            MotionQueryItem::new("scan").optional(),
        ]
    }
}
