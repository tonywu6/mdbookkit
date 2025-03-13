#[macro_export]
macro_rules! log_debug {
    () => {
        |err| log::debug!("{err:?}")
    };
}

#[macro_export]
macro_rules! log_warning {
    () => {
        |err| log::warn!("{err}")
    };
}
