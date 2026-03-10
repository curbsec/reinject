//! Core library for context rot prevention.
//!
//! Provides transcript parsing, throttle decision logic, state management,
//! and hook JSON output — everything needed to implement the reinject CLI
//! and any future consumers.

#![deny(missing_docs)]

pub mod monitor;
pub mod output;
pub mod parser;
pub mod state;
pub mod throttle;
pub mod types;

pub use monitor::update_monitor;
pub use output::hook_output;
pub use parser::parse_transcript_delta;
pub use state::{
    read_consumer_state, read_monitor_status, read_offset, reset_state, state_dir,
    write_consumer_state, write_monitor_status, write_offset, MonitorStatus,
};
pub use throttle::{record, should_reinject};
pub use types::{InjectReason, ThrottleConfig, ThrottleDecision, Tier};
