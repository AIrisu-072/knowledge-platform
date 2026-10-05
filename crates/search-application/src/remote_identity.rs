//! P4-06: stable Source-local identity of a remote Resource.
//!
//! The ResourceId and candidate ID are derived by the application from the
//! trusted tenant, the server-owned SourceId, the registered provider kind and
//! the provider's bounded native ID. A provider never chooses a ResourceId,
//! candidate ID or SourceId.

use search_core::id::{ResourceId, SourceId};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::SearchError;
use crate::retrieval::OpaqueNativeId;
use crate::scoped::TenantId;

const NAMESPACE: &[u8] = b"search-remote-resource:v1";

fn frame(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

/// Length-framed, versioned SHA-256 of `(tenant, source, provider kind,
/// native ID)`, laid out as an RFC 9562 version 8 UUID.
pub fn remote_resource_id(
    tenant: &TenantId,
    source: SourceId,
    provider_kind: &str,
    native_id: &OpaqueNativeId,
) -> Result<ResourceId, SearchError> {
    let native = native_id.as_str();
    if provider_kind.is_empty()
        || provider_kind.len() > 128
        || native.is_empty()
        || native.len() > 512
        || source.as_uuid().is_nil()
    {
        return Err(SearchError::InvalidRequest(
            "remote identity is out of bounds".into(),
        ));
    }
    let mut hasher = Sha256::new();
    frame(&mut hasher, NAMESPACE);
    frame(&mut hasher, tenant.as_str().as_bytes());
    frame(&mut hasher, source.as_uuid().as_bytes());
    frame(&mut hasher, provider_kind.as_bytes());
    frame(&mut hasher, native.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(ResourceId::from_uuid(Uuid::from_bytes(bytes)))
}

/// The candidate ID used by every retriever for this Source-local Resource.
pub fn remote_candidate_id(source: SourceId, resource: ResourceId) -> String {
    format!("{}:{}", source.as_uuid(), resource.as_uuid())
}
