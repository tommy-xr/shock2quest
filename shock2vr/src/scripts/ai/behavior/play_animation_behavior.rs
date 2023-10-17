use dark::motion::MotionQueryItem;

use super::Behavior;

pub struct PlayAnimationBehavior {
    animation_name: String,
}

impl PlayAnimationBehavior {
    pub fn new(animation_name: String) -> PlayAnimationBehavior {
        PlayAnimationBehavior { animation_name }
    }
}

impl Behavior for PlayAnimationBehavior {
    fn animation(self: &PlayAnimationBehavior) -> Vec<MotionQueryItem> {
        println!("!!debug: playing animation: {}", self.animation_name);

        if let Some(index) = self.animation_name.find(' ') {
            let (motion, value_str) = self.animation_name.split_at(index);
            if let Ok(value) = value_str.trim().parse::<i32>() {
                // I'm assuming the number is an i32, adjust as needed
                return vec![MotionQueryItem::with_value(motion, value)];
            }
        }

        vec![MotionQueryItem::new(&self.animation_name)]
    }
}
