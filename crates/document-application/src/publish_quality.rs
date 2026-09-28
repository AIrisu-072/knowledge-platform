use document_semantic_inspection_core::SignatureValidity;
use tokio::io::AsyncReadExt;

use crate::{ApplicationError, FileStorage, versioning_preflight::PreparedManifest};

pub(crate) async fn check_publish_quality<F: FileStorage>(
    prepared: &PreparedManifest,
    storage: &F,
) -> Result<(), ApplicationError> {
    for item in prepared.items() {
        let response = item.inspection().response();
        if response
            .editorial_provenance
            .tracked_changes
            .iter()
            .any(|change| change.unresolved)
        {
            return Err(ApplicationError::PublishQualityRejected(
                "unresolved tracked changes".to_owned(),
            ));
        }
        if !response.editorial_provenance.comments.is_empty() {
            return Err(ApplicationError::PublishQualityRejected(
                "embedded comments".to_owned(),
            ));
        }
        if response.digital_signature_evidence.iter().any(|signature| {
            matches!(
                signature.cryptographic_validity,
                SignatureValidity::Invalid | SignatureValidity::Unverifiable
            )
        }) {
            return Err(ApplicationError::PublishQualityRejected(
                "invalid or unverifiable signature".to_owned(),
            ));
        }
        let mut reader = storage.open(item.file().storage_key()).await?;
        let mut probe = [0_u8; 1];
        reader
            .read(&mut probe)
            .await
            .map_err(|_| ApplicationError::StorageUnavailable)?;
    }
    Ok(())
}
