//! L-17: Batch-level parallelism via persistent feature containers.
//!
//! Orchestrates the creation, routing, and cleanup of persistent containers
//! for parallel feature execution. When enabled, each feature runs in its own
//! container with its own worktree, improving isolation and reproducibility.
//!
//! # Configuration
//!
//! Enable in binding.md:
//! ```
//! l17.enabled = true
//! l17.engine = docker  # or podman
//! l17.image = rust:latest
//! ```
//!
//! # Lifecycle
//!
//! 1. **Start of phase D:** Orchestrator creates ContainerPool
//! 2. **Per feature:** Create persistent container, create worktree inside
//! 3. **Per gate:** Route through `docker/podman exec` instead of `run`
//! 4. **Feature completion:** Cleanup container and worktree
//! 5. **End of phase D:** Cleanup all remaining containers

use crate::binding::Binding;
use crate::container_pool::ContainerPool;
use crate::error::{Error, Result};
use crate::runtime::Runtime;
use std::path::Path;

/// L-17 orchestration configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub engine: String,
    pub image: String,
}

impl Config {
    /// Read L-17 configuration from binding.
    pub fn from_binding(binding: &Binding) -> Result<Config> {
        let enabled = binding
            .get("l17.enabled")
            .map(|v| v.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        if !enabled {
            return Ok(Config {
                enabled: false,
                engine: String::new(),
                image: String::new(),
            });
        }

        let engine = binding
            .get("l17.engine")
            .unwrap_or("docker")
            .to_string();

        let image = binding.get("l17.image").map_err(|_| {
            Error::unbound(
                "l17.image",
                "L-17 enabled but l17.image not configured. Set l17.image = rust:latest (or your image)",
            )
        })?;

        Ok(Config {
            enabled: true,
            engine,
            image: image.to_string(),
        })
    }

    /// Create a container pool for this run.
    pub fn pool(&self) -> ContainerPool {
        ContainerPool::new(self.engine.clone(), self.image.clone())
    }
}

/// Manages feature execution within persistent containers.
pub struct Orchestrator {
    config: Config,
    pool: Option<ContainerPool>,
}

impl Orchestrator {
    /// Create a new orchestrator from binding configuration.
    pub fn from_binding(binding: &Binding) -> Result<Orchestrator> {
        let config = Config::from_binding(binding)?;
        let pool = if config.enabled {
            Some(config.pool())
        } else {
            None
        };

        Ok(Orchestrator { config, pool })
    }

    /// Whether L-17 is enabled for this run.
    #[allow(dead_code)]
    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Create a persistent container for a feature.
    ///
    /// Returns the container ID and mount point if L-17 is enabled,
    /// or None if L-17 is disabled (single-threaded execution).
    #[allow(dead_code)]
    pub fn create_container(
        &mut self,
        feature_id: &str,
        workspace: &Path,
    ) -> Result<Option<(String, String)>> {
        if let Some(pool) = &mut self.pool {
            let (id, mount) = pool.create(feature_id, workspace)?;
            Ok(Some((id, mount)))
        } else {
            Ok(None)
        }
    }

    /// Get the runtime for a feature's gates.
    ///
    /// If L-17 is enabled and a container exists for this feature, returns
    /// `PersistentContainer`. Otherwise returns `Host` (or whatever the
    /// binding configured).
    #[allow(dead_code)]
    pub fn runtime_for_feature(
        &self,
        feature_id: &str,
        default_runtime: &Runtime,
    ) -> Runtime {
        if let Some(pool) = &self.pool {
            if let Some(container) = pool.get(feature_id) {
                return Runtime::PersistentContainer {
                    id: container.id.clone(),
                    engine: self.config.engine.clone(),
                    mount: container.workspace_mount.clone(),
                };
            }
        }
        default_runtime.clone()
    }

    /// Clean up a feature's container when done.
    #[allow(dead_code)]
    pub fn cleanup_feature(&mut self, feature_id: &str) -> Result<()> {
        if let Some(pool) = &mut self.pool {
            pool.cleanup(feature_id)?;
        }
        Ok(())
    }

    /// Clean up all containers (typically at end of batch).
    pub fn cleanup_all(&mut self) -> Result<()> {
        if let Some(pool) = &mut self.pool {
            pool.cleanup_all()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_pool_creates_from_engine_and_image() {
        let config = Config {
            enabled: true,
            engine: "docker".to_string(),
            image: "rust:latest".to_string(),
        };
        let pool = config.pool();
        assert_eq!(pool.active().len(), 0, "new pool starts empty");
    }

    #[test]
    fn orchestrator_has_no_pool_when_disabled() {
        let orch = Orchestrator {
            config: Config {
                enabled: false,
                engine: String::new(),
                image: String::new(),
            },
            pool: None,
        };
        assert!(!orch.enabled());
        let runtime = orch.runtime_for_feature("feat", &Runtime::Host);
        assert_eq!(runtime, Runtime::Host, "disabled mode returns default runtime");
    }

    #[test]
    fn orchestrator_returns_host_runtime_when_container_not_created() {
        let orch = Orchestrator {
            config: Config {
                enabled: true,
                engine: "docker".to_string(),
                image: "rust:latest".to_string(),
            },
            pool: Some(crate::container_pool::ContainerPool::new(
                "docker".to_string(),
                "rust:latest".to_string(),
            )),
        };
        // Feature "feat" has no container in the pool yet
        let runtime = orch.runtime_for_feature("feat", &Runtime::Host);
        // Should fall back to default runtime (Host)
        assert_eq!(runtime, Runtime::Host);
    }
}
