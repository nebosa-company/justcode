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
//! | [`engine`] | the driver — `L-1`, `L-9`, `L-14`, `L-17`, `L-20` |
//! | [`phase`] | `L-1`, `L-2`, `L-14` |
//! | [`budget`] | `L-9`, `L-10` |
//! | [`lock`] | `L-17`, `L-18`, `L-20` |
//! | [`ladder`] | `M-8` |
//! | [`chat`] | `C-1`–`C-5` |
//! | [`command`] | `C-6`, `C-7` |
//! | [`btw`] | `C-8`–`C-12` |
//! | [`artifact`] | `A-1`–`A-7`, `O-2` |
//! | [`metrics`] | `O-5`, `O-7` |
//! | [`step`] | `L-22` |
//! | [`journal`] | `L-3`, `N-8` |
//! | [`state`] | `L-4`, `O-1`, `L-8` |
//! | [`atomic`] | `N-10` |
//! | [`error`] | `N-9` |
//! | [`process`] | `T-3`, `T-4`, `X-4`, `X-12` |
//! | [`gate`] | `V-2`, `L-16` |
//! | [`session`] | `L-5`, `L-6`, `L-7`, `L-15`, `N-1`, `N-2` |
//! | [`watchdog`] | `L-11`, `L-12`, `L-13` |
//! | [`git`] | `G-1`–`G-8`, `G-10`, `G-14` |
//! | [`verify`] | `V-1`, `V-3`, `V-4`, `V-7`–`V-10` |
//! | [`tool`] | `T-1`, `T-2`, `T-5`–`T-7`, `T-12` |
//! | [`approval`] | `T-13`–`T-17`, `L-19` |
//! | [`link`] | `M-1`–`M-5`, `M-14` |
//! | [`probe`] | `M-6`, `M-7` |
//! | [`local`] | `M-16`–`M-19` |
//! | [`net`] | the transport, over `curl` — `S-2`, `T-3` |
//! | [`client`] | `M-6`–`M-10`, `M-21`, `M-22` |
//! | [`cost`] | `M-11` |
//! | [`prompt`] | `M-12`, `M-13` |
//! | [`json`], [`time`] | no dependencies, per `N-11` |
//!
//! Nothing here talks to a model or touches git. Those are batches 4 and 6 —
//! and everything so far is deliberately testable without either, or the tests
//! would need a GPU to run.

pub mod approval;
pub mod artifact;
pub mod atomic;
pub mod binding;
pub mod btw;
pub mod budget;
pub mod chat;
pub mod client;
pub mod command;
pub mod cost;
pub mod engine;
pub mod error;
pub mod gate;
pub mod git;
pub mod journal;
pub mod ladder;
pub mod json;
pub mod link;
pub mod local;
pub mod metrics;
pub mod lock;
pub mod net;
pub mod phase;
pub mod probe;
pub mod process;
pub mod prompt;
pub mod session;
pub mod state;
pub mod step;
pub mod time;
pub mod tool;
pub mod verify;
pub mod watchdog;

#[cfg(test)]
mod testutil;

pub use binding::Binding;
pub use artifact::{Artifact, Provenance};
pub use btw::Btw;
pub use budget::{Budget, Budgets, Spend};
pub use engine::{Done, Engine, Gates, Task, Work};
pub use chat::{Streaming, Turn};
pub use command::{Chain, Command, Input};
pub use error::{Error, Result};
pub use gate::{Attempts, Gate, GateResult, Verdict};
pub use journal::{Journal, Kind, Record};
pub use ladder::{Ladder, Next, Rung};
pub use link::{Link, Links, Mode, Role};
pub use metrics::{Cycle, Snapshot};
pub use lock::{Concurrency, Lock};
pub use phase::{Machine, Measured, Park, Phase, Stop};
pub use probe::{Capabilities, ModelFacts, ProbeCache};
pub use process::{Env, Exit, Nursery, Run, Spec};
pub use session::{Decision, Finding, Probe, Session, StepGuard};
pub use state::{replay, Projection};
pub use step::StepId;
pub use watchdog::{Watch, Watchdogs};

/// The crate version, for the state file and the board to report.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
