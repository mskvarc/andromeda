//! Sending a built request through the pre-send guard.

use crate::FederationService;
use andromeda_outbound_http::endpoint_refusal::EndpointRefusal;
use thiserror::Error;

/// Why a guarded send produced no response.
#[derive(Debug, Error)]
pub(crate) enum GuardedSendError {
    /// The guard refused the endpoint; nothing was sent.
    #[error(transparent)]
    Refused(#[from] EndpointRefusal),
    /// The request could not be built or sent.
    #[error(transparent)]
    Send(#[from] reqwest::Error),
}

impl FederationService {
    /// Send `request` unless the guard refuses its endpoint.
    ///
    /// Every request to a Context Source goes out through here, so the guard
    /// cannot be skipped for one kind of forwarding.
    pub(crate) async fn send_guarded(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response, GuardedSendError> {
        let request = request.build()?;
        self.guard.check(request.url()).await?;
        Ok(self.http_client.execute(request).await?)
    }
}
