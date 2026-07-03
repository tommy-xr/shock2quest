use dark::motion::MotionQueryItem;

use super::Behavior;

pub struct IdleBehavior;

impl Behavior for IdleBehavior {
    fn name(&self) -> &'static str {
        "Idle"
    }

    fn animation(self: &IdleBehavior) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("idlegesture")]
        //vec![MotionQueryItem::new("stand")]
    }
}
