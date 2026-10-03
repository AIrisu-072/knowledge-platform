use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest, Sha256};

use super::profile::lower_hex;
use super::{
    ContentPartRef, ExtractionProfileId, NativeLocator, ResourceVersionRef, UnitCodecError, is_nfc,
    validate_logical_path, write_frame,
};

const PREFIX: &[u8] = b"knowledge-unit:v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitId([u8; 32]);

impl UnitId {
    pub fn derive(
        version: &ResourceVersionRef,
        part: &ContentPartRef,
        profile: &ExtractionProfileId,
        locator: &NativeLocator,
        ordinal: u32,
    ) -> Result<Self, UnitCodecError> {
        if version.source_native_version.is_empty()
            || !is_nfc(&version.source_native_version)
            || part.source_native_part_id.is_empty()
            || !is_nfc(&part.source_native_part_id)
        {
            return Err(UnitCodecError::Invalid("native version or part ID"));
        }
        validate_logical_path(&part.logical_path)?;
        let mut input = PREFIX.to_vec();
        write_frame(&mut input, version.source_id.as_uuid().as_bytes())?;
        write_frame(&mut input, version.resource_id.as_uuid().as_bytes())?;
        write_frame(&mut input, version.source_native_version.as_bytes())?;
        write_frame(&mut input, part.source_native_part_id.as_bytes())?;
        write_frame(&mut input, part.logical_path.as_bytes())?;
        write_frame(&mut input, &part.ordinal.to_be_bytes())?;
        write_frame(&mut input, profile.as_str().as_bytes())?;
        write_frame(&mut input, &locator.encode()?)?;
        write_frame(&mut input, &ordinal.to_be_bytes())?;
        Ok(Self(Sha256::digest(&input).into()))
    }

    pub fn parse(value: &str) -> Result<Self, UnitCodecError> {
        let Some(hex) = value.strip_prefix("ku1:") else {
            return Err(UnitCodecError::Invalid("UnitId prefix"));
        };
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(UnitCodecError::Invalid("UnitId hex"));
        }
        let mut bytes = [0_u8; 32];
        for (index, pair) in hex.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let digit = |byte: u8| {
                if byte.is_ascii_digit() {
                    byte - b'0'
                } else {
                    byte - b'a' + 10
                }
            };
            bytes[index] = (digit(pair[0]) << 4) | digit(pair[1]);
        }
        Ok(Self(bytes))
    }
}

impl std::fmt::Display for UnitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ku1:{}", lower_hex(&self.0))
    }
}

impl Serialize for UnitId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for UnitId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}
