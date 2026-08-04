#[cfg(test)]
mod l17_integration_tests {
    use crate::container_pool::ContainerPool;
    use crate::process::Spec;
    use crate::runtime::Runtime;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn l17_persistent_container_workflow() {
        // This test demonstrates the L-17 workflow for parallel feature execution.
        // It shows (conceptually) how persistent containers work, though the
        // actual container operations are mocked/simplified for testing.

        // Step 1: Initialize container pool for the batch
        let pool = ContainerPool::new("docker".to_string(), "rust:latest".to_string());
        assert_eq!(pool.active().len(), 0);

        // Step 2: For a parallel feature, create a persistent container
        // In real usage: let (id, mount) = pool.create("feature-123", &workspace)?;
        // Here we verify the pool structure:
        let feature_id = "feature-123";
        assert!(!pool.has_container(feature_id));

        // Step 3: Use the Runtime::PersistentContainer variant
        // This is what gates would use when a feature runs in a container
        let runtime = Runtime::PersistentContainer {
            id: "container-abc".to_string(),
            engine: "docker".to_string(),
            mount: "/w".to_string(),
        };

        // Step 4: Verify the runtime wraps commands correctly
        let spec = Spec::new(
            "cargo test --workspace".to_string(),
            PathBuf::from("/w"),
            Duration::from_secs(300),
        );
        let wrapped = runtime.wrap(&spec).expect("wrap");

        // Commands should use `docker exec` instead of `docker run`
        assert!(wrapped.command.contains("docker exec"));
        assert!(wrapped.command.contains("container-abc"));
        assert!(wrapped.command.ends_with("cargo test --workspace"));
        assert!(!wrapped.command.contains("--rm"), "persistent containers don't have --rm");
    }

    #[test]
    fn persistent_container_runtime_display() {
        let runtime = Runtime::PersistentContainer {
            id: "container-xyz".to_string(),
            engine: "podman".to_string(),
            mount: "/w".to_string(),
        };

        let text = format!("{}", runtime);
        assert!(text.contains("persistent"));
        assert!(text.contains("container-xyz"));
        assert!(text.contains("podman"));
    }

    #[test]
    fn persistent_container_vs_ephemeral_gate_container() {
        // Demonstrate the difference between ephemeral and persistent containers

        let ephemeral = Runtime::Container {
            image: "rust:latest".to_string(),
            engine: "docker".to_string(),
        };

        let persistent = Runtime::PersistentContainer {
            id: "container-123".to_string(),
            engine: "docker".to_string(),
            mount: "/w".to_string(),
        };

        let spec = Spec::new(
            "cargo test".to_string(),
            PathBuf::from("/workspace"),
            Duration::from_secs(300),
        );

        let ephemeral_wrapped = ephemeral.wrap(&spec).expect("wrap ephemeral");
        let persistent_wrapped = persistent.wrap(&spec).expect("wrap persistent");

        // Ephemeral uses `docker run --rm`
        assert!(ephemeral_wrapped.command.contains("docker run"));
        assert!(ephemeral_wrapped.command.contains("--rm"));
        assert!(ephemeral_wrapped.command.contains("--network none"));

        // Persistent uses `docker exec` (no --rm, no network isolation)
        assert!(persistent_wrapped.command.contains("docker exec"));
        assert!(!persistent_wrapped.command.contains("--rm"));
        assert!(!persistent_wrapped.command.contains("--network none"));
    }

    #[test]
    fn container_pool_starts_empty() {
        let pool = ContainerPool::new("docker".to_string(), "rust:latest".to_string());
        // Pool starts with no containers
        assert_eq!(pool.active().len(), 0);

        // In a real scenario, containers would be created via pool.create(),
        // which requires docker/podman to be available. These unit tests
        // verify the data structures and control flow without requiring that.
    }
}
