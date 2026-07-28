//! The Perpetum harness core — batch 1, "the spine".
//!
//! What exists so far is the durable record everything else in the design reads
//! and writes: a binding the engine refuses to run without, step ids that every
//! surface can cite, an append-only journal, and a state file that is a
//! projection of that journal rather than a second source of truth.
//!
//! Requirements are defined in `docs/perpetum.md` and only cited here. The
//! modules name the ones they implement:
//!
//! | Module | Requirements |
//! |---|---|
//! | [`binding`] | `L-21` |
//! | [`step`] | `L-22` |
//! | [`journal`] | `L-3`, `N-8` |
//! | [`state`] | `L-4`, `O-1`, `L-8` |
//! | [`atomic`] | `N-10` |
//! | [`error`] | `N-9` |
//! | [`json`], [`time`] | no dependencies, per `N-11` |
//!
//! Nothing here talks to a model, runs a command, or touches git. Those are
//! batches 2, 4 and 6 — and the spine is deliberately testable without any of
//! them, or the tests would need a GPU to run.

pub mod atomic;
pub mod binding;
pub mod error;
pub mod journal;
pub mod json;
pub mod state;
pub mod step;
pub mod time;

#[cfg(test)]
mod testutil;

pub use binding::Binding;
pub use error::{Error, Result};
pub use journal::{Journal, Kind, Record};
pub use state::{replay, Projection};
pub use step::StepId;

/// The crate version, for the state file and the board to report.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
