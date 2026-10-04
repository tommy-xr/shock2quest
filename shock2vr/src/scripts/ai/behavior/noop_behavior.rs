use super::Behavior;

pub struct NoopBehavior;

impl Behavior for NoopBehavior {
    fn animation_queries(&self) -> Vec<Vec<dark::motion::MotionQueryItem>> {
        vec![]
    }

    fn name(&self) -> &'static str {
        "Noop"
    }
}
