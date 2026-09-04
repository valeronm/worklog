use chrono::{Local, SecondsFormat};

use crate::domain::ports::Clock;

pub struct SystemClock;

impl Clock for SystemClock {
    fn today(&self) -> String {
        Local::now().format("%Y-%m-%d").to_string()
    }

    fn now(&self) -> String {
        Local::now().to_rfc3339_opts(SecondsFormat::Secs, false)
    }
}
