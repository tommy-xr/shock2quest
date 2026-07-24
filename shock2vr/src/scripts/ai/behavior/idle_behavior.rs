use dark::motion::MotionQueryItem;

use super::Behavior;

pub struct IdleBehavior;

impl Behavior for IdleBehavior {
    fn name(&self) -> &'static str {
        "Idle"
    }

    fn animation(self: &IdleBehavior) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("idlegesture")]
    }

    fn animation_queries(&self) -> Vec<Vec<MotionQueryItem>> {
        vec![self.animation(), vec![MotionQueryItem::new("stand")]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_prefers_gesture_then_stand() {
        let queries = IdleBehavior.animation_queries();
        let tags = queries
            .iter()
            .map(|query| {
                query
                    .iter()
                    .map(MotionQueryItem::tag_name)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        assert_eq!(tags, vec![vec!["idlegesture"], vec!["stand"]]);
    }
}
