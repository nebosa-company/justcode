//! Local-server realities (`M-16`–`M-20`).
//!
//! Everything here exists because a model on your own hardware behaves nothing
//! like an endpoint. It has to be *loaded*, which takes minutes for a large
//! one; two of them do not fit on one GPU; and the machine it lives on can
//! simply go away.
//!
//! ## What was measured, on 2026-07-28, against a live LM Studio
//!
//! - `lms load <model> --ttl <n>` loads and sets an idle unload timer. An 80 MiB
//!   embedding model took **7.69 s**; a 30B would be minutes, which is the
//!   entire argument for `M-16`.
//! - Afterwards `/api/v0/models` reports `state: loaded`, with `quantization`
//!   and `max_context_length` — the facts `M-7` reads.
//! - **`lms ls` listed the same model twice, once for `Local` and once for the
//!   LM Link peer `KUR`; `/api/v0/models` listed it once.** A peer's models are
//!   *not* served through the local OpenAI-compatible API. This settles the
//!   open question that has been carried since the design was written, and it
//!   is why [`Peer`] exists at all.

use std::time::Duration;

use crate::error::{Error, Result};
use crate::link::{Kind, Link};
use crate::probe::{LoadState, ModelFacts};
use crate::process::{self, Env, Spec};

/// How long a warmed model is asked to stay resident, unless configured
/// otherwise. Longer than a batch, shorter than a night.
pub const DEFAULT_TTL_SECS: u64 = 1800;

/// Loading is slow enough that it needs its own budget.
const LOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// The `lms` CLI. The only way to reach LM Link (see the module note).
#[derive(Debug, Clone)]
pub struct Lms {
    program: String,
    cwd: std::path::PathBuf,
}

impl Default for Lms {
    fn default() -> Lms {
        Lms::new()
    }
}

impl Lms {
    pub fn new() -> Lms {
        Lms { program: "lms".to_string(), cwd: std::env::temp_dir() }
    }

    pub fn with_program(mut self, program: impl Into<String>) -> Lms {
        self.program = program.into();
        self
    }

    fn run(&self, args: &str, timeout: Duration) -> Result<process::Run> {
        let spec = Spec::new(format!("{} {args}", self.program), &self.cwd, timeout)
            .with_env(Env::declared());
        process::run(&spec)
    }

    /// `lms link status` (`M-19`).
    pub fn link_status(&self) -> Result<LinkStatus> {
        let run = self.run("link status", Duration::from_secs(30))?;
        if !run.is_success() {
            return Err(Error::unbound(
                "lms link status",
                format!("{}: {}", run.exit.describe(), run.stderr_tail.trim()),
            ));
        }
        Ok(LinkStatus::parse(&run.stdout_tail))
    }

    /// `lms load <model> --ttl <n> -y` (`M-16`).
    pub fn load(&self, model: &str, ttl: Duration) -> Result<Duration> {
        let started = std::time::Instant::now();
        let run = self.run(
            &format!("load {model} --ttl {} -y", ttl.as_secs()),
            LOAD_TIMEOUT,
        )?;
        if !run.is_success() {
            return Err(Error::unbound(
                format!("lms load {model}"),
                format!("{}: {}", run.exit.describe(), run.stderr_tail.trim()),
            ));
        }
        Ok(started.elapsed())
    }

    pub fn unload(&self, model: &str) -> Result<()> {
        self.run(&format!("unload {model}"), Duration::from_secs(60)).map(|_| ())
    }
}

/// One machine on the LM Link network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub name: String,
    pub connected: bool,
    pub identifier: Option<String>,
}

/// What `lms link status` reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkStatus {
    pub this_device: Option<String>,
    pub online: bool,
    pub peers: Vec<Peer>,
}

impl LinkStatus {
    /// Parse the CLI's output.
    ///
    /// Deliberately forgiving — this is a human-facing format that can change
    /// under us, and the failure mode of a strict parser here is a healthy peer
    /// reported as gone. Written against real output, quoted in the tests.
    pub fn parse(text: &str) -> LinkStatus {
        let mut status = LinkStatus::default();
        let mut current: Option<Peer> = None;

        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(name) = trimmed.strip_prefix("This device:") {
                status.this_device = Some(name.trim().to_string());
            } else if let Some(state) = trimmed.strip_prefix("Status:") {
                let state = state.trim();
                match &mut current {
                    Some(peer) => peer.connected = state.eq_ignore_ascii_case("connected"),
                    None => status.online = state.eq_ignore_ascii_case("online"),
                }
            } else if let Some(identifier) = trimmed.strip_prefix("Identifier:") {
                if let Some(peer) = &mut current {
                    peer.identifier = Some(identifier.trim().to_string());
                }
            } else if let Some(name) = trimmed.strip_prefix("- ") {
                if let Some(peer) = current.take() {
                    status.peers.push(peer);
                }
                current = Some(Peer {
                    name: name.trim().to_string(),
                    connected: false,
                    identifier: None,
                });
            }
        }
        if let Some(peer) = current.take() {
            status.peers.push(peer);
        }
        status
    }

    pub fn peer(&self, name: &str) -> Option<&Peer> {
        self.peers.iter().find(|peer| peer.name == name)
    }

    /// Is the device this link names actually there (`M-19`)?
    pub fn is_reachable(&self, link: &Link) -> bool {
        match (&link.device, link.kind) {
            (Some(device), Kind::LmLink) => {
                self.online && self.peer(device).is_some_and(|peer| peer.connected)
            }
            // Not an LM Link link; this says nothing about it.
            _ => true,
        }
    }
}

/// What warming a link did, or did not have to do (`M-16`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warmed {
    pub link: String,
    pub model: String,
    pub already_loaded: bool,
    pub took_ms: u128,
}

impl Warmed {
    pub fn describe(&self) -> String {
        if self.already_loaded {
            format!("{} · {} already resident", self.link, self.model)
        } else {
            format!("{} · {} loaded in {}ms", self.link, self.model, self.took_ms)
        }
    }
}

/// Decide whether a link needs warming, from the facts it reported (`M-16`).
///
/// Separated from the loading so the decision is testable without a GPU.
pub fn needs_warming(facts: &ModelFacts) -> bool {
    facts.state != LoadState::Loaded
}

/// Load a link's model if it is not already resident.
pub fn warm(lms: &Lms, link: &Link, facts: &ModelFacts, ttl: Duration) -> Result<Warmed> {
    if !needs_warming(facts) {
        return Ok(Warmed {
            link: link.name.clone(),
            model: link.model.clone(),
            already_loaded: true,
            took_ms: 0,
        });
    }
    let took = lms.load(&link.model, ttl)?;
    Ok(Warmed {
        link: link.name.clone(),
        model: link.model.clone(),
        already_loaded: false,
        took_ms: took.as_millis(),
    })
}

/// Which machine a link's model occupies (`M-17`).
///
/// Two links pointing at `localhost` are two models on one GPU, however
/// different their names are.
pub fn host_of(link: &Link) -> String {
    match (&link.device, &link.base_url) {
        (Some(device), _) => format!("device:{device}"),
        (None, Some(url)) => {
            let without_scheme = url.split("://").nth(1).unwrap_or(url);
            let authority = without_scheme.split('/').next().unwrap_or(without_scheme);
            format!("host:{authority}")
        }
        (None, None) => "host:unknown".to_string(),
    }
}

/// VRAM as a lease (`M-17`).
///
/// A host holds so many models at once — one, unless the operator says
/// otherwise. Asking for a second is refused rather than queued: the honest
/// answer is "that will not fit", not a wait that ends in an out-of-memory
/// error minutes later.
#[derive(Debug, Clone, Default)]
pub struct Vram {
    capacity: Vec<(String, u32)>,
    resident: Vec<(String, Vec<String>)>,
}

impl Vram {
    pub fn new() -> Vram {
        Vram::default()
    }

    /// How many models this host may hold. Default 1.
    pub fn with_capacity(mut self, host: impl Into<String>, models: u32) -> Vram {
        self.capacity.push((host.into(), models.max(1)));
        self
    }

    fn capacity_of(&self, host: &str) -> u32 {
        self.capacity
            .iter()
            .find(|(name, _)| name == host)
            .map(|(_, models)| *models)
            .unwrap_or(1)
    }

    pub fn resident_on(&self, host: &str) -> &[String] {
        self.resident
            .iter()
            .find(|(name, _)| name == host)
            .map(|(_, models)| models.as_slice())
            .unwrap_or(&[])
    }

    /// Claim room for a link's model. `Err` when the host is full, naming what
    /// is already there.
    pub fn claim(&mut self, link: &Link) -> Result<()> {
        let host = host_of(link);
        let capacity = self.capacity_of(&host);
        let slot = match self.resident.iter_mut().find(|(name, _)| name == &host) {
            Some(slot) => slot,
            None => {
                self.resident.push((host.clone(), Vec::new()));
                self.resident.last_mut().ok_or_else(|| {
                    Error::unbound("vram", "the host list vanished, which cannot happen")
                })?
            }
        };
        if slot.1.contains(&link.model) {
            return Ok(());
        }
        if slot.1.len() as u32 >= capacity {
            return Err(Error::unbound(
                format!("link.{}", link.name),
                format!(
                    "{host} already holds {} — loading `{}` as well would not fit. \
                     Release one first; a second large model on one GPU is an out-of-memory \
                     error several minutes from now, not a queue.",
                    slot.1.join(", "),
                    link.model
                ),
            ));
        }
        slot.1.push(link.model.clone());
        Ok(())
    }

    pub fn release(&mut self, link: &Link) {
        let host = host_of(link);
        if let Some((_, models)) = self.resident.iter_mut().find(|(name, _)| name == &host) {
            models.retain(|model| model != &link.model);
        }
    }
}

/// What a link generated, and how fast (`M-18`).
///
/// LM Studio's native API reports these per response; the OpenAI-compatible one
/// does not, which is one more reason `M-7` prefers `/api/v0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub tokens_per_second: f64,
    pub time_to_first_token_ms: f64,
}

/// A rolling throughput estimate per link.
#[derive(Debug, Clone, Default)]
pub struct Throughput {
    window: usize,
    samples: Vec<(String, Vec<Sample>)>,
}

impl Throughput {
    pub fn new(window: usize) -> Throughput {
        Throughput { window: window.max(1), samples: Vec::new() }
    }

    pub fn record(&mut self, link: &str, sample: Sample) {
        let slot = match self.samples.iter_mut().find(|(name, _)| name == link) {
            Some(slot) => slot,
            None => {
                self.samples.push((link.to_string(), Vec::new()));
                match self.samples.last_mut() {
                    Some(slot) => slot,
                    None => return,
                }
            }
        };
        slot.1.push(sample);
        while slot.1.len() > self.window {
            slot.1.remove(0);
        }
    }

    fn mean(&self, link: &str, pick: impl Fn(&Sample) -> f64) -> Option<f64> {
        let samples = &self.samples.iter().find(|(name, _)| name == link)?.1;
        if samples.is_empty() {
            return None;
        }
        Some(samples.iter().map(&pick).sum::<f64>() / samples.len() as f64)
    }

    pub fn tokens_per_second(&self, link: &str) -> Option<f64> {
        self.mean(link, |sample| sample.tokens_per_second)
    }

    pub fn time_to_first_token_ms(&self, link: &str) -> Option<f64> {
        self.mean(link, |sample| sample.time_to_first_token_ms)
    }

    /// How long this link would take to produce that many tokens.
    ///
    /// `None` when the link has never been measured — an unknown duration, not
    /// an optimistic one. A wall-clock budget built on a guess is worse than no
    /// budget, because it looks like a plan.
    pub fn estimate(&self, link: &str, tokens: i64) -> Option<Duration> {
        let rate = self.tokens_per_second(link)?;
        if rate <= 0.0 {
            return None;
        }
        let ttft = self.time_to_first_token_ms(link).unwrap_or(0.0);
        Some(Duration::from_millis(
            (ttft + (tokens as f64 / rate) * 1000.0).max(0.0) as u64,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::Links;

    /// Verbatim output from `lms link status` on 2026-07-28, with a peer
    /// connected. Recorded rather than invented: a parser written against
    /// imagined output is a parser that has never been tested.
    const REAL_STATUS: &str = "This device: ROG Z13 RTX 3080\n\
                               Status: Online\n\
                               \n\
                               Found 1 device:\n\
                               \n\
                                 - KUR\n\
                                   Status: connected\n\
                                   Identifier: 249f12e9a0ce27e285de21d08ee37ffd\n";

    fn links() -> Links {
        Links::parse(
            "```perp-links\n\
             link.here.kind = lmstudio\n\
             link.here.base_url = http://localhost:1234\n\
             link.here.model = small\n\
             link.also-here.kind = lmstudio\n\
             link.also-here.base_url = http://localhost:1234\n\
             link.also-here.model = big\n\
             link.rig.kind = lmlink\n\
             link.rig.device = KUR\n\
             link.rig.model = coder\n\
             role.coder = rig, here\n```\n",
        )
        .expect("parse")
    }

    fn facts(state: LoadState) -> ModelFacts {
        ModelFacts {
            id: "small".into(),
            kind: "llm".into(),
            publisher: None,
            arch: None,
            quantization: Some("Q4_K_M".into()),
            state,
            max_context_length: Some(2048),
        }
    }

    #[test]
    fn the_real_link_status_parses() {
        // `M-19`.
        let status = LinkStatus::parse(REAL_STATUS);
        assert_eq!(status.this_device.as_deref(), Some("ROG Z13 RTX 3080"));
        assert!(status.online);
        assert_eq!(status.peers.len(), 1);

        let peer = status.peer("KUR").expect("KUR");
        assert!(peer.connected);
        assert_eq!(peer.identifier.as_deref(), Some("249f12e9a0ce27e285de21d08ee37ffd"));
    }

    #[test]
    fn a_link_naming_a_connected_peer_is_reachable() {
        let links = links();
        let status = LinkStatus::parse(REAL_STATUS);
        assert!(status.is_reachable(links.get("rig").expect("rig")));
    }

    #[test]
    fn a_peer_that_vanished_is_not_reachable() {
        // `M-19`: a peer that went away fails the step, not the cycle — and it
        // has to be detected for that to happen at all.
        let links = links();
        let gone = LinkStatus::parse("This device: ROG Z13 RTX 3080\nStatus: Online\n\nFound 0 devices:\n");
        assert!(!gone.is_reachable(links.get("rig").expect("rig")));

        let disconnected = LinkStatus::parse(
            "This device: X\nStatus: Online\n\n  - KUR\n    Status: disconnected\n",
        );
        assert!(!disconnected.is_reachable(links.get("rig").expect("rig")));

        let offline = LinkStatus::parse("This device: X\nStatus: Offline\n\n  - KUR\n    Status: connected\n");
        assert!(!offline.is_reachable(links.get("rig").expect("rig")), "the local end is down");
    }

    #[test]
    fn link_status_says_nothing_about_a_link_that_is_not_lmlink() {
        let links = links();
        let status = LinkStatus::parse(REAL_STATUS);
        assert!(status.is_reachable(links.get("here").expect("here")));
    }

    #[test]
    fn a_loaded_model_does_not_need_warming() {
        // `M-16`. Measured: an 80 MiB embedding model took 7.69s to load; a
        // 30B is minutes, which is why this check exists at all.
        assert!(!needs_warming(&facts(LoadState::Loaded)));
        assert!(needs_warming(&facts(LoadState::NotLoaded)));
        assert!(needs_warming(&facts(LoadState::Unknown)), "unknown is not loaded");
    }

    #[test]
    fn two_models_do_not_fit_on_one_host() {
        // `M-17`: two links at `localhost` are two models on one GPU, however
        // different their names are.
        let links = links();
        let here = links.get("here").expect("here");
        let also = links.get("also-here").expect("also-here");
        assert_eq!(host_of(here), host_of(also));

        let mut vram = Vram::new();
        vram.claim(here).expect("the first fits");
        let err = vram.claim(also).expect_err("the second does not");
        let text = format!("{err}");
        assert!(text.contains("already holds small"), "{text}");
        assert!(text.contains("not a queue"), "it says why it refuses rather than waits: {text}");

        vram.release(here);
        vram.claim(also).expect("and once released, it fits");
    }

    #[test]
    fn claiming_the_same_model_twice_is_not_a_second_model() {
        let links = links();
        let here = links.get("here").expect("here");
        let mut vram = Vram::new();
        vram.claim(here).expect("first");
        vram.claim(here).expect("the same model is already resident");
        assert_eq!(vram.resident_on(&host_of(here)).len(), 1);
    }

    #[test]
    fn a_host_with_room_for_two_holds_two() {
        let links = links();
        let here = links.get("here").expect("here");
        let mut vram = Vram::new().with_capacity(host_of(here), 2);
        vram.claim(here).expect("first");
        vram.claim(links.get("also-here").expect("also-here")).expect("second");
    }

    #[test]
    fn a_remote_peer_is_a_different_host_from_this_machine() {
        let links = links();
        // The exact keys, not just that they differ — "they differ" is nearly
        // impossible to break, and a test that cannot fail proves nothing.
        assert_eq!(host_of(links.get("rig").expect("rig")), "device:KUR");
        assert_eq!(host_of(links.get("here").expect("here")), "host:localhost:1234");
        let mut vram = Vram::new();
        vram.claim(links.get("here").expect("here")).expect("local");
        vram.claim(links.get("rig").expect("rig")).expect("the rig has its own memory");
    }

    #[test]
    fn throughput_is_a_rolling_mean_per_link() {
        // `M-18`.
        let mut throughput = Throughput::new(3);
        throughput.record("here", Sample { tokens_per_second: 10.0, time_to_first_token_ms: 100.0 });
        throughput.record("here", Sample { tokens_per_second: 20.0, time_to_first_token_ms: 200.0 });
        assert_eq!(throughput.tokens_per_second("here"), Some(15.0));
        assert_eq!(throughput.time_to_first_token_ms("here"), Some(150.0));
        assert_eq!(throughput.tokens_per_second("rig"), None, "never measured");
    }

    #[test]
    fn the_window_forgets_old_samples() {
        let mut throughput = Throughput::new(2);
        for rate in [100.0, 100.0, 10.0] {
            throughput.record("here", Sample { tokens_per_second: rate, time_to_first_token_ms: 0.0 });
        }
        assert_eq!(
            throughput.tokens_per_second("here"),
            Some(55.0),
            "the first sample has fallen out of the window"
        );
    }

    #[test]
    fn an_unmeasured_link_has_no_estimate_rather_than_an_optimistic_one() {
        // A wall-clock budget built on a guess is worse than no budget: it
        // looks like a plan.
        let mut throughput = Throughput::new(5);
        assert_eq!(throughput.estimate("here", 1000), None);

        throughput.record("here", Sample { tokens_per_second: 100.0, time_to_first_token_ms: 500.0 });
        let estimate = throughput.estimate("here", 1000).expect("measured");
        assert_eq!(estimate, Duration::from_millis(10_500), "500ms to start, then 10s of tokens");
    }
}
