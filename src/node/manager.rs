use codex_bindings::callback::with_libcodex_lock;
use codex_bindings::{upload_file, CodexConfig, CodexNode, UploadOptions, UploadResult};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

#[derive(Error, Debug)]
pub enum NodeManagerError {
    #[error("Failed to create Codex node: {0}")]
    NodeCreationFailed(String),
    #[error("Failed to start node: {0}")]
    NodeStartFailed(String),
    #[error("Failed to stop node: {0}")]
    NodeStopFailed(String),
    #[error("Failed to destroy node: {0}")]
    NodeDestroyFailed(String),
    #[error("Thread safety error: {0}")]
    ThreadSafetyError(String),
    #[error("Node is not running")]
    NodeNotRunning,
    #[error("Node operation error: {0}")]
    NodeOperationError(String),
}

pub struct CodexNodeManager {
    node: Arc<Mutex<Option<CodexNode>>>,
    config: CodexConfig,
    is_running: Arc<Mutex<bool>>,
}

impl CodexNodeManager {
    pub fn new(config: CodexConfig) -> Self {
        Self {
            node: Arc::new(Mutex::new(None)),
            config,
            is_running: Arc::new(Mutex::new(false)),
        }
    }

    /// Create and start the Codex node
    pub async fn start(&self) -> Result<(), NodeManagerError> {
        let mut node_guard = self.node.lock().await;
        let mut running_guard = self.is_running.lock().await;

        if *running_guard {
            warn!("Node is already running");
            return Ok(());
        }

        info!("Creating Codex node...");

        // Create and start node directly like the integration test
        let config = self.config.clone();
        let node = tokio::task::spawn_blocking(move || {
            let mut node = CodexNode::new(config)
                .map_err(|e| NodeManagerError::NodeCreationFailed(e.to_string()))?;

            node.start()
                .map_err(|e| NodeManagerError::NodeStartFailed(e.to_string()))?;

            Ok::<CodexNode, NodeManagerError>(node)
        })
        .await
        .map_err(|e| NodeManagerError::ThreadSafetyError(e.to_string()))??;

        *node_guard = Some(node);
        *running_guard = true;

        info!("Codex node started successfully");
        Ok(())
    }

    /// Stop the Codex node
    pub async fn stop(&self) -> Result<(), NodeManagerError> {
        let mut node_guard = self.node.lock().await;
        let mut running_guard = self.is_running.lock().await;

        if !*running_guard {
            warn!("Node is not running");
            return Ok(());
        }

        if let Some(mut node) = node_guard.take() {
            info!("Stopping Codex node...");

            tokio::task::spawn_blocking(move || {
                node.stop()
                    .map_err(|e| NodeManagerError::NodeStopFailed(e.to_string()))?;
                node.destroy()
                    .map_err(|e| NodeManagerError::NodeDestroyFailed(e.to_string()))?;
                Ok::<(), NodeManagerError>(())
            })
            .await
            .map_err(|e| NodeManagerError::ThreadSafetyError(e.to_string()))??;

            info!("Codex node stopped and destroyed");
        }

        *running_guard = false;
        Ok(())
    }

    /// Get node information
    pub async fn get_peer_id(&self) -> Result<String, NodeManagerError> {
        let node = self.get_node().await?;

        tokio::task::spawn_blocking(move || {
            with_libcodex_lock(|| {
                node.peer_id()
                    .map_err(|e| NodeManagerError::NodeOperationError(e.to_string()))
            })
        })
        .await
        .map_err(|e| NodeManagerError::ThreadSafetyError(e.to_string()))?
    }

    /// Get a reference to the managed node for operations
    pub async fn get_node(&self) -> Result<CodexNode, NodeManagerError> {
        let node_guard = self.node.lock().await;
        let running_guard = self.is_running.lock().await;

        if !*running_guard {
            return Err(NodeManagerError::NodeNotRunning);
        }

        node_guard
            .as_ref()
            .cloned()
            .ok_or(NodeManagerError::NodeOperationError(
                "Node not initialized".to_string(),
            ))
    }

    /// Upload a file using the managed node
    pub async fn upload_file(
        &self,
        options: UploadOptions,
    ) -> Result<UploadResult, NodeManagerError> {
        let node = self.get_node().await?;

        upload_file(&node, options)
            .await
            .map_err(|e| NodeManagerError::NodeOperationError(e.to_string()))
    }
}

impl Drop for CodexNodeManager {
    fn drop(&mut self) {
        // Note: This is a synchronous drop, so we can't use async operations here
        // In a real implementation, you might want to ensure proper cleanup
        // by calling stop() explicitly before the manager goes out of scope
        info!("CodexNodeManager being dropped - ensure stop() was called explicitly");
    }
}
