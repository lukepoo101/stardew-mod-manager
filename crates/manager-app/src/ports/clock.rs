use chrono::{DateTime, Utc};

pub trait ClockPort: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl ClockPort for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
