use std::future::Future;
use std::pin::Pin;

use axum::http::{HeaderMap, HeaderValue};
use document_application::{IdentityResolutionError, VerifiedActorContext};

/// Request metadata supplied to a trusted adapter. Headers are transport input,
/// not verified identity claims; the adapter owns authentication and resolution.
#[derive(Clone)]
pub struct IdentityRequestContext {
    headers: HeaderMap,
}

impl IdentityRequestContext {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            headers: headers.clone(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&HeaderValue> {
        self.headers.get(name)
    }
}

pub trait IdentityAdapter: Send + Sync {
    fn resolve<'a>(
        &'a self,
        request: &'a IdentityRequestContext,
    ) -> Pin<
        Box<dyn Future<Output = Result<VerifiedActorContext, IdentityResolutionError>> + Send + 'a>,
    >;
}
