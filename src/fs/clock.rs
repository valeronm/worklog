use chrono::{Local, SecondsFormat};

use crate::domain::ports::Clock;

pub struct SystemClock;

impl Clock for SystemClock {
    fn today(&self) -> String {
        Local::now().format("%Y-%m-%d").to_string()
    }

    /// Versions are ordered by comparing microseconds, so a finer stamp
    /// would be lost in the comparison.
    fn now(&self) -> String {
        Local::now().to_rfc3339_opts(SecondsFormat::Micros, false)
    }
}
