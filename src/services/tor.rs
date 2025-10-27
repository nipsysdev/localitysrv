use crate::AppState;
use anyhow::Result;
use arti_client::{TorClient, TorClientConfig};
use axum::Router;
use futures::StreamExt;
use hyper::{body::Incoming, Request};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server;
use safelog::{sensitive, DisplayRedacted as _};
use std::sync::Arc;
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::{config::OnionServiceConfigBuilder, StreamRequest};
use tor_proto::client::stream::IncomingStreamRequest;
use tower::Service;
use tracing::{debug, error, info, warn};

pub struct TorServiceManager {
    app: Router,
    app_state: AppState,
    shutdown_signal: Arc<tokio::sync::Notify>,
}

impl TorServiceManager {
    pub fn new(
        app: Router,
        app_state: AppState,
        shutdown_signal: Arc<tokio::sync::Notify>,
    ) -> Self {
        Self {
            app,
            app_state,
            shutdown_signal,
        }
    }

    pub async fn run_with_retry(&self, onion_address_tx: tokio::sync::oneshot::Sender<String>) {
        let mut retry_count = 0;
        let max_retries = 5;
        let base_delay = std::time::Duration::from_secs(5);
        let mut onion_address_tx = Some(onion_address_tx);

        loop {
            let result = self.run_tor_service(onion_address_tx.take()).await;

            match result {
                Ok(_) => {
                    info!("Tor hidden service stopped successfully");
                    break;
                }
                Err(e) if retry_count >= max_retries => {
                    error!(
                        "Max retries ({}) reached for Tor hidden service: {}. Giving up.",
                        max_retries, e
                    );
                    break;
                }
                Err(e) => {
                    retry_count += 1;
                    let delay = base_delay * retry_count;
                    warn!(
                        "Tor hidden service failed (attempt {}/{}): {}. Restarting in {:?}...",
                        retry_count, max_retries, e, delay
                    );

                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    async fn run_tor_service(
        &self,
        onion_address_tx: Option<tokio::sync::oneshot::Sender<String>>,
    ) -> Result<()> {
        info!("Starting Tor hidden service...");

        let config = TorClientConfig::default();

        info!("Bootstrapping Tor client...");
        let client = TorClient::create_bootstrapped(config).await?;
        info!("Tor client bootstrapped successfully");

        let svc_cfg = OnionServiceConfigBuilder::default()
            .nickname("localitysrv".parse().unwrap())
            .build()?;

        info!("Launching onion service...");
        let (service, request_stream) = client.launch_onion_service(svc_cfg)?;
        let onion_address = service
            .onion_address()
            .unwrap()
            .display_unredacted()
            .to_string();

        {
            let mut config = self.app_state.config.lock().await;
            config.onion_address = Some(onion_address.clone());
        }

        info!("Tor hidden service launched at: {}", onion_address);

        if let Some(tx) = onion_address_tx {
            let _ = tx.send(onion_address.clone());
        }

        info!("Waiting for Tor hidden service to be fully reachable...");
        let mut status_events = service.status_events();

        while let Some(status) = status_events.next().await {
            if status.state().is_fully_reachable() {
                info!(
                    "✓ Tor hidden service is now fully reachable at http://{}",
                    onion_address
                );
                break;
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            status_events = service.status_events();
        }

        let stream_requests = tor_hsservice::handle_rend_requests(request_stream);
        tokio::pin!(stream_requests);

        loop {
            tokio::select! {
                biased;
                _ = self.shutdown_signal.notified() => {
                    info!("Tor hidden service shutting down...");
                    drop(service);
                    return Ok(());
                }
                Some(stream_request) = stream_requests.next() => {
                    let app_clone = self.app.clone();

                    tokio::spawn(async move {
                        let request = stream_request.request().clone();
                        if let Err(err) = handle_stream_request(stream_request, app_clone).await {
                            warn!("Error serving connection {:?}: {}. Arti will handle recovery.", sensitive(request), err);
                        };
                    });
                }
            }
        }
    }
}

async fn handle_stream_request(stream_request: StreamRequest, app: Router) -> Result<()> {
    debug!("Handling new stream request");

    match stream_request.request() {
        IncomingStreamRequest::Begin(begin) if begin.port() == 80 => {
            debug!("Accepting stream request on port 80");

            match stream_request.accept(Connected::new_empty()).await {
                Ok(onion_service_stream) => {
                    debug!("Stream accepted successfully");
                    let io = TokioIo::new(onion_service_stream);

                    let hyper_service =
                        hyper::service::service_fn(move |request: Request<Incoming>| {
                            app.clone().call(request)
                        });

                    match server::conn::auto::Builder::new(TokioExecutor::new())
                        .serve_connection(io, hyper_service)
                        .await
                    {
                        Ok(_) => {
                            debug!("Connection served successfully");
                            Ok(())
                        }
                        Err(e) => {
                            warn!("Connection error: {}. Arti will handle recovery.", e);
                            Err(anyhow::anyhow!(e))
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to accept stream: {}. Arti will handle recovery.", e);
                    Err(anyhow::anyhow!(e))
                }
            }
        }
        _ => {
            debug!("Rejecting stream request on non-port 80");
            stream_request.shutdown_circuit()?;
            Ok(())
        }
    }
}
