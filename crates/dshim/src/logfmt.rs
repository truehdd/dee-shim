//! Module diagnostics use Rust log levels and scoped targets.

use std::io::Write;

pub use log::Level;

pub fn line(_log: &mut dyn Write, level: Level, scope: &str, message: &str) {
    let target = if scope.is_empty() {
        "dshim".to_owned()
    } else {
        format!("dshim::{}", scope.replace('/', "::"))
    };
    log::log!(target: &target, level, "{message}");
}

macro_rules! lok {
    ($log:expr, $scope:expr, $($arg:tt)*) => {
        $crate::logfmt::line($log, $crate::logfmt::Level::Debug, $scope, &format!($($arg)*))
    };
}

macro_rules! ldebug {
    ($log:expr, $scope:expr, $($arg:tt)*) => {
        $crate::logfmt::line($log, $crate::logfmt::Level::Debug, $scope, &format!($($arg)*))
    };
}

#[allow(unused_macros)] // only the ddp modules warn
macro_rules! lwarn {
    ($log:expr, $scope:expr, $($arg:tt)*) => {
        $crate::logfmt::line($log, $crate::logfmt::Level::Warn, $scope, &format!($($arg)*))
    };
}

macro_rules! lerr {
    ($log:expr, $scope:expr, $($arg:tt)*) => {
        $crate::logfmt::line($log, $crate::logfmt::Level::Error, $scope, &format!($($arg)*))
    };
}
