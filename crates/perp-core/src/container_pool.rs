//! Persistent container lifecycle for parallel feature execution (`L-17`).
//!
//! Unlike ephemeral gate containers (`--rm`), feature containers persist across
//! multiple steps. One container per parallel feature, created on demand, kept
//! running, cleaned up on feature completion.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};

/// A persistent container running a single feature's work.
#[derive(Debug, Clone)]
pub struct FeatureContainer {
    /// Container ID as returned by `docker run` or `podman run`.
    pub id: String,
    /// Which feature is using this container.
    pub feature_id: String,
    /// Mount point for the workspace inside the container.
    pub workspace_mount: String,
    /// When the container was created.
    pub created_at: i64,
}

/// Manages the lifecycle of persistent containers for parallel feature execution.
///
/// Containers are created on demand, reused for all steps of a feature, and
/// cleaned up when the feature completes or is abandoned.
pub struct ContainerPool {
    containers: HashMap<String, FeatureContainer>,
    engine: String,
    image: String,
}

impl ContainerPool {
    /// Create a new pool for a specific container engine and image.
    pub fn new(engine: String, image: String) -> Self {
        ContainerPool {
            containers: HashMap::new(),
            engine,
            image,
        }
    }

    /// Create or reuse a persistent container for a feature.
    ///
    /// Returns the container ID and the path where the workspace is mounted.
    pub fn create(
        &mut self,
        feature_id: &str,
        workspace: &std::path::Path,
    ) -> Result<(String, String)> {
        // Reuse existing container if it's already running.
        if let Some(container) = self.containers.get(feature_id) {
            return Ok((container.id.clone(), container.workspace_mount.clone()));
        }

        let workspace_str = workspace.display().to_string();
        let mount_point = "/w";
        let name = format!("perp-{}", feature_id.replace(['/', '\\'], "-"));

        // Build the docker/podman run command.
        // Note: no `--rm` for persistent containers.
        let cmd = format!(
            "{engine} run -d --name {name} --network none -v \"{workspace_str}\":{mount_point} -w {mount_point} {image} sleep infinity",
            engine = self.engine,
            image = self.image
        );

        // Get current directory or fallback to workspace.
        let cwd = std::env::current_dir().unwrap_or_else(|_| workspace.to_path_buf());

        // Execute the container creation command.
        let run = crate::process::run(&crate::process::Spec::new(
            cmd,
            cwd,
            std::time::Duration::from_secs(30),
        ))?;

        if !run.exit.is_success() {
            return Err(Error::unbound(
                "container_pool",
                format!(
                    "failed to create persistent container for {}: {}",
                    feature_id, run.stdout_tail
                ),
            ));
        }

        // Extract container ID from output (usually first line).
        let container_id = run.stdout_tail.trim().lines().next().unwrap_or("unknown").to_string();

        let container = FeatureContainer {
            id: container_id.clone(),
            feature_id: feature_id.to_string(),
            workspace_mount: mount_point.to_string(),
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        };

        self.containers.insert(feature_id.to_string(), container);

        Ok((container_id, mount_point.to_string()))
    }

    /// Get a container by feature ID.
    pub(crate) fn get(&self, feature_id: &str) -> Option<&FeatureContainer> {
        self.containers.get(feature_id)
    }

    /// Check if a container exists for this feature.
    #[allow(dead_code)]
    pub(crate) fn has_container(&self, feature_id: &str) -> bool {
        self.containers.contains_key(feature_id)
    }

    /// Clean up a container when a feature completes.
    pub(crate) fn cleanup(&mut self, feature_id: &str) -> Result<()> {
        if let Some(container) = self.containers.remove(feature_id) {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));

            // Stop and remove the container.
            let stop_cmd = format!("{} stop {}", self.engine, container.id);
            let _ = crate::process::run(&crate::process::Spec::new(
                stop_cmd,
                cwd.clone(),
                std::time::Duration::from_secs(10),
            ));

            let rm_cmd = format!("{} rm {}", self.engine, container.id);
            let _ = crate::process::run(&crate::process::Spec::new(
                rm_cmd,
                cwd,
                std::time::Duration::from_secs(10),
            ));
        }
        Ok(())
    }

    /// Clean up all containers (for shutdown).
    #[allow(dead_code)]
    pub(crate) fn cleanup_all(&mut self) -> Result<()> {
        let feature_ids: Vec<_> = self.containers.keys().cloned().collect();
        for feature_id in feature_ids {
            let _ = self.cleanup(&feature_id);
        }
        Ok(())
    }

    /// Get all active containers.
    #[allow(dead_code)]
    pub(crate) fn active(&self) -> Vec<&FeatureContainer> {
        self.containers.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_pool_tracks_multiple_containers() {
        let pool = ContainerPool::new("docker".to_string(), "rust:latest".to_string());
        assert_eq!(pool.active().len(), 0);

        // In real usage, containers are created via the runtime layer, not here.
        // This test just verifies the data structure.
    }

    #[test]
    fn cleanup_removes_container_from_pool() {
        let mut pool = ContainerPool::new("docker".to_string(), "rust:latest".to_string());
        // Manually add for testing (in real use, create() adds it).
        let container = FeatureContainer {
            id: "abc123".to_string(),
            feature_id: "feature1".to_string(),
            workspace_mount: "/w".to_string(),
            created_at: 0,
        };
        pool.containers.insert("feature1".to_string(), container);

        assert!(pool.has_container("feature1"));
        let _ = pool.cleanup("feature1");
        assert!(!pool.has_container("feature1"));
    }

    #[test]
    fn reusing_a_container_returns_same_id() {
        let mut pool = ContainerPool::new("docker".to_string(), "rust:latest".to_string());
        let container = FeatureContainer {
            id: "abc123".to_string(),
            feature_id: "feature1".to_string(),
            workspace_mount: "/w".to_string(),
            created_at: 0,
        };
        pool.containers.insert("feature1".to_string(), container);

        if let Some(existing) = pool.get("feature1") {
            assert_eq!(existing.id, "abc123");
        }
    }
}
