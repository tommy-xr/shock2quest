use std::collections::HashSet;

use once_cell::sync::Lazy;

static ALLOWED_HAND_MODELS_SET: Lazy<HashSet<&str>> = Lazy::new(|| {
    let ALLOWED_HAND_MODELS: Vec<&str> = vec!["atek_h", "amp_h", "lasehand"];

    let mut set = HashSet::new();
    for model in ALLOWED_HAND_MODELS.iter() {
        set.insert(*model);
    }
    set
});

pub fn is_allowed_hand_model(model_name: &str) -> bool {
    ALLOWED_HAND_MODELS_SET.contains(model_name)
}
