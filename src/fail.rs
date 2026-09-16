use std::fmt;

pub const EXIT_OK: i32 = 0;
pub const EXIT_SESSION_FAILED: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_HARNESS_UNAVAILABLE: i32 = 3;
pub const EXIT_INTERNAL: i32 = 4;
pub const EXIT_LEDGER_FAILED: i32 = 5;
pub const EXIT_WAIT_TIMEOUT: i32 = 6;

#[derive(Debug)]
pub struct Fail {
    pub code: i32,
    pub message: String,
    pub help: Vec<String>,
}

impl Fail {
    pub fn usage(message: impl Into<String>, help: Vec<String>) -> Fail {
        Fail {
            code: EXIT_USAGE,
            message: message.into(),
            help,
        }
    }

    pub fn harness_unavailable(message: impl Into<String>, help: Vec<String>) -> Fail {
        Fail {
            code: EXIT_HARNESS_UNAVAILABLE,
            message: message.into(),
            help,
        }
    }

    pub fn wait_timeout(message: impl Into<String>, help: Vec<String>) -> Fail {
        Fail {
            code: EXIT_WAIT_TIMEOUT,
            message: message.into(),
            help,
        }
    }
}

impl fmt::Display for Fail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for Fail {}
