//! Test-only logger that accepts every record, so log lines are formatted (and covered) in
//! tests no matter which test runs first.

struct NullLogger;

impl log::Log for NullLogger {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let _ = record.args().to_string();
    }

    fn flush(&self) {}
}

/// Installs the logger once per test process; later calls do nothing.
pub fn init() {
    if log::set_logger(&NullLogger).is_ok() {
        log::set_max_level(log::LevelFilter::Trace);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn logs_at_every_level() {
        super::init();
        super::init();
        assert!(log::log_enabled!(log::Level::Trace));
        log::trace!("formatted {}", 1);
        log::logger().flush();
    }
}
