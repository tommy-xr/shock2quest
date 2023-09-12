use std::time::Duration;

use shipyard::Unique;

#[derive(Unique, Clone, Debug, Default)]
pub struct Time {
    pub elapsed: Duration,
    pub total: Duration,
}
