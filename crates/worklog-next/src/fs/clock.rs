use chrono::{Local, SecondsFormat};

use crate::domain::ports::Clock;
use crate::domain::version::Stamp;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Stamp {
        let text = Local::now().to_rfc3339_opts(SecondsFormat::Micros, false);
        Stamp::parse(&text).expect("chrono writes RFC 3339 with an offset")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_today_at_the_local_offset() {
        let local = || {
            let now = Local::now();
            (
                now.format("%Y-%m-%d").to_string(),
                now.format("%:z").to_string(),
            )
        };
        let before = local();
        let now = SystemClock.now();
        let after = local();
        let text = now.to_string();
        let offset = text[text.len() - 6..].to_owned();
        assert!([before, after].contains(&(now.day(), offset)), "{now}");
    }
}
