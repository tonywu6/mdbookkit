use log::LevelFilter;
use mdbookkit::logging::ConsoleLogger;

pub fn setup_logging() {
    ConsoleLogger::try_install(env!("CARGO_PKG_NAME")).ok();
    log::set_max_level(LevelFilter::Debug);
}
