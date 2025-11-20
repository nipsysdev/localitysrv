//! Test to verify that localitysrv uses a single node for all operations
//! instead of creating temporary nodes for each upload.

use codex_bindings::{CodexConfig, LogLevel, UploadOptions};
use localitysrv::models::storage::{PendingUpload, UploadQueue, UploadStats};
use localitysrv::node::manager::{CodexNodeManager, NodeManagerError};
use localitysrv::services::database::DatabaseService;
use localitysrv::services::node_ops::{NodeOps, NodeOpsError};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;

#[tokio::test]
async fn test_node_manager_single_instance() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary directory for the test
    let temp_dir = TempDir::new()?;
    let data_dir = temp_dir.path().join("codex_data");

    // Create codex config
    let codex_config = CodexConfig::new()
        .log_level(LogLevel::Error)
        .data_dir(&data_dir)
        .storage_quota(100 * 1024 * 1024) // 100MB
        .discovery_port(0); // Use random port

    // Create node manager
    let node_manager = Arc::new(CodexNodeManager::new(codex_config));

    // Start the node
    node_manager.start().await?;

    // Verify node is running
    assert!(node_manager.is_running().await);

    // Test that we can get the node multiple times and it's the same instance
    let node1 = node_manager.get_node().await?;
    let peer_id1 = tokio::task::spawn_blocking(move || node1.peer_id()).await??;

    let node2 = node_manager.get_node().await?;
    let peer_id2 = tokio::task::spawn_blocking(move || node2.peer_id()).await??;

    // Both nodes should have the same peer ID (same instance)
    assert_eq!(peer_id1, peer_id2);

    // Stop the node
    node_manager.stop().await?;

    // Note: We don't call destroy() here because the Arc still has references
    // In a real application, the node would be destroyed when all references are dropped

    Ok(())
}

#[tokio::test]
async fn test_node_ops_uses_managed_node() -> Result<(), Box<dyn std::error::Error>> {
    // This test verifies that NodeOps uses the managed node
    // We can't test actual uploads without a real Codex network,
    // but we can verify the structure is correct

    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");

    // Create a mock database service
    let db_service = Arc::new(DatabaseService::new(&db_path.to_string_lossy()).await?);

    // Create codex config
    let codex_config = CodexConfig::new()
        .log_level(LogLevel::Error)
        .data_dir(temp_dir.path().join("codex_data"))
        .storage_quota(100 * 1024 * 1024)
        .discovery_port(0);

    // Create node manager
    let node_manager = Arc::new(CodexNodeManager::new(codex_config));

    // Create NodeOps with the managed node
    let node_ops =
        NodeOps::new_with_databases(db_service.clone(), db_service, node_manager.clone());

    // Verify NodeOps was created successfully
    let stats = node_ops.get_stats().await;
    assert_eq!(stats.total_uploaded, 0);
    assert_eq!(stats.total_failed, 0);

    Ok(())
}

#[tokio::test]
async fn test_concurrent_uploads_use_same_node() -> Result<(), Box<dyn std::error::Error>> {
    // Test that multiple concurrent uploads would use the same node
    let temp_dir = TempDir::new()?;

    // Create codex config
    let codex_config = CodexConfig::new()
        .log_level(LogLevel::Error)
        .data_dir(temp_dir.path().join("codex_data"))
        .storage_quota(100 * 1024 * 1024)
        .discovery_port(0);

    // Create node manager
    let node_manager = Arc::new(CodexNodeManager::new(codex_config));

    // Start the node
    node_manager.start().await?;

    // Get multiple node references concurrently
    let mut handles = Vec::new();
    for _ in 0..5 {
        let manager = node_manager.clone();
        let handle = tokio::spawn(async move {
            let node = manager.get_node().await?;
            let peer_id = tokio::task::spawn_blocking(move || {
                node.peer_id()
                    .map_err(|e| NodeManagerError::NodeOperationError(e.to_string()))
            })
            .await
            .map_err(|e| NodeManagerError::ThreadSafetyError(e.to_string()))??;
            Ok::<String, NodeManagerError>(peer_id)
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    let mut peer_ids = Vec::new();
    for handle in handles {
        let peer_id = handle.await??;
        peer_ids.push(peer_id);
    }

    // All peer IDs should be the same (same node instance)
    let first_peer_id = &peer_ids[0];
    for peer_id in &peer_ids[1..] {
        assert_eq!(first_peer_id, peer_id);
    }

    // Stop the node
    node_manager.stop().await?;

    Ok(())
}

#[test]
fn test_upload_queue_functionality() -> Result<(), Box<dyn std::error::Error>> {
    // Test that upload queue still works correctly after our changes
    let mut queue = UploadQueue::new(5, 10);

    // Create test uploads
    let upload1 = PendingUpload::new(
        "US".to_string(),
        12345,
        PathBuf::from("/test/path1.pmtiles"),
    );

    let upload2 = PendingUpload::new(
        "CA".to_string(),
        67890,
        PathBuf::from("/test/path2.pmtiles"),
    );

    // Add uploads to queue
    queue.add_upload(upload1)?;
    queue.add_upload(upload2)?;

    assert!(!queue.is_empty());
    assert!(!queue.is_full());

    // Take batch
    let batch = queue.take_batch();
    assert_eq!(batch.len(), 2);
    assert!(queue.is_empty());

    Ok(())
}

#[test]
fn test_upload_stats_functionality() -> Result<(), Box<dyn std::error::Error>> {
    // Test that upload stats still work correctly after our changes
    let mut stats = UploadStats::new();

    assert_eq!(stats.total_uploaded, 0);
    assert_eq!(stats.total_failed, 0);
    assert_eq!(stats.total_bytes_uploaded, 0);

    // Increment stats
    stats.increment_uploaded(1024);
    stats.increment_uploaded(2048);
    stats.increment_failed();

    assert_eq!(stats.total_uploaded, 2);
    assert_eq!(stats.total_failed, 1);
    assert_eq!(stats.total_bytes_uploaded, 3072);

    Ok(())
}
