pub const MAX_JSON_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_MULTIPART_JSON_BYTES: usize = 1024 * 1024;
pub const MAX_MULTIPART_FILE_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_MULTIPART_TOTAL_BYTES: usize = 1024 * 1024 * 1024;
pub const MAX_MULTIPART_PARTS: usize = 64;
pub const MAX_FILENAME_BYTES: usize = 1024;
pub const MAX_MULTIPART_HEADER_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadLimits {
    pub(crate) json_bytes: usize,
    pub(crate) file_bytes: usize,
    pub(crate) total_bytes: usize,
    pub(crate) parts: usize,
    pub(crate) filename_bytes: usize,
    pub(crate) header_bytes: usize,
}

impl UploadLimits {
    pub const PRODUCTION: Self = Self {
        json_bytes: MAX_MULTIPART_JSON_BYTES,
        file_bytes: MAX_MULTIPART_FILE_BYTES,
        total_bytes: MAX_MULTIPART_TOTAL_BYTES,
        parts: MAX_MULTIPART_PARTS,
        filename_bytes: MAX_FILENAME_BYTES,
        header_bytes: MAX_MULTIPART_HEADER_BYTES,
    };

    /// Builds a profile that can only tighten the approved production bounds.
    pub fn tightened(
        json_bytes: usize,
        file_bytes: usize,
        total_bytes: usize,
        parts: usize,
        filename_bytes: usize,
        header_bytes: usize,
    ) -> Result<Self, LimitConfigurationError> {
        let requested = [
            (json_bytes, Self::PRODUCTION.json_bytes),
            (file_bytes, Self::PRODUCTION.file_bytes),
            (total_bytes, Self::PRODUCTION.total_bytes),
            (parts, Self::PRODUCTION.parts),
            (filename_bytes, Self::PRODUCTION.filename_bytes),
            (header_bytes, Self::PRODUCTION.header_bytes),
        ];
        if requested
            .iter()
            .any(|(value, maximum)| *value == 0 || value > maximum)
        {
            return Err(LimitConfigurationError);
        }
        Ok(Self {
            json_bytes,
            file_bytes,
            total_bytes,
            parts,
            filename_bytes,
            header_bytes,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitConfigurationError;
