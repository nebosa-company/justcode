//! Where a command actually executes (`T-9`), and the platform details that are
//! tested rather than assumed (`N-4`).
//!
//! **The gate commands come from the binding; the runtime decides where they
//! run.** That split is the requirement. A binding that said
//! `gate.test = wsl -d Ubuntu -- cargo test` would work exactly once, on one
//! machine, and would stop being a description of *what green means* and become
//! a description of one developer's setup.
//!
//! So `gate.test` stays `cargo test --workspace` everywhere, and the runtime is
//! configured separately — which also means the same binding can be checked on
//! the host and in a container and produce comparable transcripts.
//!
//! ## `N-4` is a test file, not a paragraph
//!
//! *Path handling, line endings and shell quoting are tested on Windows, not
//! assumed.* Windows is the primary platform for this project, which means it
//! is the one where the assumptions are least likely to be examined — everyone
//! has a mental model of POSIX and nobody has one of `cmd`'s re-parsing rules.
//! The tests at the bottom of this file are the requirement.

use std::fmt;
use std::path::Path;

use crate::error::{Error, Result};
use crate::process::Spec;

/// Where gate commands execute (`T-9`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Runtime {
    /// This machine, as this user. The default, and the only one that needs no
    /// configuration.
    #[default]
    Host,
    /// A WSL2 distribution. The working directory is translated, because a
    /// Windows path means nothing inside the distribution.
    Wsl2 { distro: String },
    /// A container. The workspace is mounted; nothing else is.
    Container { image: String, engine: String },
}

impl fmt::Display for Runtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Runtime::Host => f.write_str("host"),
            Runtime::Wsl2 { distro } => write!(f, "wsl2:{distro}"),
            Runtime::Container { image, engine } => write!(f, "{engine}:{image}"),
        }
    }
}

impl Runtime {
    /// Read `runtime = host | wsl2:<distro> | container:<image>` from the
    /// binding, with `runtime.engine` choosing docker or podman.
    pub fn from_entries(entries: &[(String, String)]) -> Result<Runtime> {
        let engine = entries
            .iter()
            .find(|(key, _)| key == "runtime.engine")
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| "docker".to_string());

        let Some((_, value)) = entries.iter().find(|(key, _)| key == "runtime") else {
            return Ok(Runtime::Host);
        };
        match value.split_once(':') {
            None if value == "host" => Ok(Runtime::Host),
            Some(("wsl2", distro)) if !distro.is_empty() => {
                Ok(Runtime::Wsl2 { distro: distro.to_string() })
            }
            Some(("container", image)) if !image.is_empty() => {
                Ok(Runtime::Container { image: image.to_string(), engine })
            }
            _ => Err(Error::unbound(
                "runtime",
                format!(
                    "`{value}` is not a runtime. Use `host`, `wsl2:<distro>` or \
                     `container:<image>`"
                ),
            )),
        }
    }

    /// Whether a transcript from this runtime is comparable with one from
    /// another. It is not: a gate that is green in a container and red on the
    /// host has told you something, and averaging the two would hide it.
    pub fn is_host(&self) -> bool {
        matches!(self, Runtime::Host)
    }

    /// Wrap a gate command so it runs in this runtime.
    ///
    /// The command itself is **never rewritten** — it is the binding's, and the
    /// binding is what says what green means. Only its surroundings change.
    pub fn wrap(&self, spec: &Spec) -> Result<Spec> {
        match self {
            Runtime::Host => Ok(spec.clone()),
            Runtime::Wsl2 { distro } => {
                let cwd = wsl_path(&spec.cwd).ok_or_else(|| {
                    Error::unbound(
                        "runtime",
                        format!(
                            "cannot translate {} into a WSL path — a Windows path means \
                             nothing inside the distribution",
                            spec.cwd.display()
                        ),
                    )
                })?;
                // `--cd` rather than a `cd &&`: the shell inside would re-parse
                // the path, and a workspace under `C:\Program Files` would then
                // be two arguments.
                Ok(Spec::new(
                    format!("wsl -d {distro} --cd {cwd} -- {}", spec.command),
                    &spec.cwd,
                    spec.timeout,
                )
                .with_env(spec.env.clone()))
            }
            Runtime::Container { image, engine } => {
                let mount = spec.cwd.display().to_string();
                Ok(Spec::new(
                    // `--rm` because a container per gate that is never removed
                    // is a disk that fills up over a weekend. No network by
                    // default (`N-6`): a flaky connection must not manufacture
                    // a red.
                    format!(
                        "{engine} run --rm --network none -v \"{mount}\":/w -w /w {image} \
                         {}",
                        spec.command
                    ),
                    &spec.cwd,
                    spec.timeout,
                )
                .with_env(spec.env.clone()))
            }
        }
    }
}

/// `D:\repos\justcode` → `/mnt/d/repos/justcode`.
///
/// Returns `None` for a path WSL cannot see — a UNC share, a mapped drive that
/// is not a real volume. Refusing is better than producing a path that silently
/// resolves to nothing inside the distribution.
pub fn wsl_path(path: &Path) -> Option<String> {
    let text = path.display().to_string();
    if text.starts_with("\\\\") || text.starts_with("//") {
        return None;
    }
    let mut chars = text.chars();
    let drive = chars.next()?;
    if !drive.is_ascii_alphabetic() || chars.next() != Some(':') {
        // Already a POSIX path — running on Linux, or a relative one.
        return (!text.contains(':')).then(|| text.replace('\\', "/"));
    }
    let rest: String = chars.collect();
    Some(format!("/mnt/{}{}", drive.to_ascii_lowercase(), rest.replace('\\', "/")))
}

/// A path as this platform writes it, for comparing against tool output
/// (`N-4`).
pub fn normalise(path: &Path) -> String {
    // Forward slashes everywhere. Windows accepts them in every API the harness
    // uses, and a journal that mixes separators is one where the same file
    // appears twice in a diff of two runs.
    path.display().to_string().replace('\\', "/")
}

/// Strip a trailing carriage return (`N-4`).
///
/// Git on Windows checks out CRLF by default, so a file the harness reads and a
/// string it compares against differ by one invisible byte — and the assertion
/// that fails says the two are different without showing anything different.
pub fn trim_eol(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

/// Split text into lines with either ending (`N-4`).
pub fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').map(trim_eol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    fn spec(command: &str, cwd: &str) -> Spec {
        Spec::new(command, PathBuf::from(cwd), Duration::from_secs(60))
    }

    #[test]
    fn the_binding_says_what_and_the_runtime_says_where() {
        // `T-9`. A binding holding `wsl -d Ubuntu -- cargo test` works once, on
        // one machine, and stops being a description of what green means.
        let gate = spec("cargo test --workspace", "D:/repos/justcode/crates");

        let host = Runtime::Host.wrap(&gate).expect("host");
        assert_eq!(host.command, "cargo test --workspace", "untouched");

        let wsl = Runtime::Wsl2 { distro: "Ubuntu".into() }.wrap(&gate).expect("wsl");
        assert!(wsl.command.starts_with("wsl -d Ubuntu"), "{}", wsl.command);
        assert!(wsl.command.ends_with("cargo test --workspace"), "the command is not rewritten");

        let container = Runtime::Container { image: "rust:1".into(), engine: "docker".into() }
            .wrap(&gate)
            .expect("container");
        assert!(container.command.contains("--rm"), "{}", container.command);
        assert!(container.command.ends_with("cargo test --workspace"));
    }

    #[test]
    fn a_container_gate_gets_no_network() {
        // `N-6`: a flaky connection must not manufacture a red.
        let wrapped = Runtime::Container { image: "rust:1".into(), engine: "podman".into() }
            .wrap(&spec("cargo test", "/w"))
            .expect("wrap");
        assert!(wrapped.command.contains("--network none"), "{}", wrapped.command);
        assert!(wrapped.command.starts_with("podman run"), "the engine is configurable");
    }

    #[test]
    fn a_windows_path_becomes_a_wsl_path() {
        // `N-4`, tested rather than assumed.
        assert_eq!(
            wsl_path(Path::new("D:\\repos\\justcode")).as_deref(),
            Some("/mnt/d/repos/justcode")
        );
        assert_eq!(wsl_path(Path::new("C:/Users/x")).as_deref(), Some("/mnt/c/Users/x"));
        // A UNC path is refused rather than mangled into one that silently
        // resolves to nothing inside the distribution.
        assert_eq!(wsl_path(Path::new("\\\\server\\share\\x")), None);
    }

    #[test]
    fn a_runtime_that_cannot_see_the_workspace_refuses_rather_than_guesses() {
        let err = Runtime::Wsl2 { distro: "Ubuntu".into() }
            .wrap(&spec("cargo test", "\\\\server\\share\\repo"))
            .expect_err("a UNC path is not visible in WSL");
        assert!(format!("{err}").contains("means nothing inside"), "{err}");
    }

    #[test]
    fn a_path_is_written_one_way_everywhere() {
        // `N-4`. A journal that mixes separators is one where the same file
        // appears twice in a diff of two runs.
        assert_eq!(normalise(Path::new("crates\\perp-core\\src")), "crates/perp-core/src");
        assert_eq!(normalise(Path::new("crates/perp-core/src")), "crates/perp-core/src");
    }

    #[test]
    fn a_carriage_return_does_not_survive_into_a_comparison() {
        // `N-4`. Git on Windows checks out CRLF by default, so a file the
        // harness reads and a string it compares against differ by one
        // invisible byte — and the assertion that fails says they are different
        // without showing anything different.
        assert_eq!(trim_eol("cargo test\r"), "cargo test");
        assert_eq!(trim_eol("cargo test"), "cargo test");

        let crlf = "gate: test\r\nexit 0\r\n";
        let read: Vec<&str> = lines(crlf).collect();
        assert_eq!(read, ["gate: test", "exit 0", ""]);

        let lf = "gate: test\nexit 0\n";
        assert_eq!(lines(crlf).collect::<Vec<_>>(), lines(lf).collect::<Vec<_>>());
    }

    #[test]
    fn a_path_with_a_space_survives_the_container_mount() {
        // `N-4`, shell quoting. `C:\Program Files` is the case everyone's
        // quoting is wrong about, and on Windows it is not hypothetical.
        let wrapped = Runtime::Container { image: "rust:1".into(), engine: "docker".into() }
            .wrap(&spec("cargo test", "C:/Program Files/repo"))
            .expect("wrap");
        assert!(
            wrapped.command.contains("-v \"C:/Program Files/repo\":/w"),
            "the mount must be quoted: {}",
            wrapped.command
        );
    }

    #[test]
    fn the_runtime_comes_out_of_the_binding() {
        let host = Runtime::from_entries(&[]).expect("default");
        assert_eq!(host, Runtime::Host, "no configuration is the host");

        let wsl = Runtime::from_entries(&[("runtime".into(), "wsl2:Ubuntu-24.04".into())])
            .expect("wsl");
        assert_eq!(wsl, Runtime::Wsl2 { distro: "Ubuntu-24.04".into() });

        let container = Runtime::from_entries(&[
            ("runtime".into(), "container:rust:1.97".into()),
            ("runtime.engine".into(), "podman".into()),
        ])
        .expect("container");
        assert_eq!(
            container,
            Runtime::Container { image: "rust:1.97".into(), engine: "podman".into() }
        );
    }

    #[test]
    fn an_unreadable_runtime_names_the_ones_that_exist() {
        let err = Runtime::from_entries(&[("runtime".into(), "vm:something".into())])
            .expect_err("must refuse");
        let text = format!("{err}");
        assert!(text.contains("wsl2:<distro>"), "{text}");
        assert!(text.contains("container:<image>"), "{text}");
    }

    #[test]
    fn transcripts_from_different_runtimes_are_not_interchangeable() {
        // A gate green in a container and red on the host has told you
        // something. The runtime is on the transcript so nobody averages them.
        assert!(Runtime::Host.is_host());
        assert!(!Runtime::Wsl2 { distro: "Ubuntu".into() }.is_host());
        assert_eq!(format!("{}", Runtime::Wsl2 { distro: "Ubuntu".into() }), "wsl2:Ubuntu");
    }
}
