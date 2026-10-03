use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest, Sha256};

use super::{
    FormatId, Reader, UnitCodecError, is_nfc, validate_archive_member, write_frame, write_u32,
};

const PROFILE_PREFIX: &[u8] = b"extraction-profile:v1\0";
const ARCHIVE_PREFIX: &[u8] = b"extraction-profile:archive:v2\0";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExtractionProfileId(String);

impl ExtractionProfileId {
    pub fn parse(value: &str) -> Result<Self, UnitCodecError> {
        let Some(hex) = value.strip_prefix("sha256:") else {
            return Err(UnitCodecError::Invalid("profile ID prefix"));
        };
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(UnitCodecError::Invalid("profile ID hex"));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn for_definition(
        definition: &ExtractionProfileDefinitionV1,
    ) -> Result<Self, UnitCodecError> {
        Ok(Self(format!(
            "sha256:{}",
            lower_hex(&Sha256::digest(definition.encode()?))
        )))
    }

    pub fn for_archive(plan: &ArchiveProfilePlan) -> Result<Self, UnitCodecError> {
        Ok(Self(format!(
            "sha256:{}",
            lower_hex(&Sha256::digest(plan.encode()?))
        )))
    }
}

impl std::fmt::Display for ExtractionProfileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ExtractionProfileId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ExtractionProfileId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}

pub(super) fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("String formatting");
    }
    output
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BudgetKey {
    InputBytes,
    ZipEntries,
    ZipEntryBytes,
    ZipTotalBytes,
    ZipDepth,
    Units,
    UnitUtf8Bytes,
    WorkerOutputBytes,
    XmlDepth,
    XmlNodes,
    PdfPages,
    PdfOperations,
    HtmlNodes,
    CsvRecords,
    CsvFieldBytes,
}

impl BudgetKey {
    pub const ALL: [Self; 15] = [
        Self::InputBytes,
        Self::ZipEntries,
        Self::ZipEntryBytes,
        Self::ZipTotalBytes,
        Self::ZipDepth,
        Self::Units,
        Self::UnitUtf8Bytes,
        Self::WorkerOutputBytes,
        Self::XmlDepth,
        Self::XmlNodes,
        Self::PdfPages,
        Self::PdfOperations,
        Self::HtmlNodes,
        Self::CsvRecords,
        Self::CsvFieldBytes,
    ];

    const fn tag(self) -> u8 {
        match self {
            Self::InputBytes => 1,
            Self::ZipEntries => 2,
            Self::ZipEntryBytes => 3,
            Self::ZipTotalBytes => 4,
            Self::ZipDepth => 5,
            Self::Units => 6,
            Self::UnitUtf8Bytes => 7,
            Self::WorkerOutputBytes => 8,
            Self::XmlDepth => 9,
            Self::XmlNodes => 10,
            Self::PdfPages => 11,
            Self::PdfOperations => 12,
            Self::HtmlNodes => 13,
            Self::CsvRecords => 14,
            Self::CsvFieldBytes => 15,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, UnitCodecError> {
        Self::ALL
            .get(usize::from(tag).wrapping_sub(1))
            .copied()
            .filter(|key| key.tag() == tag)
            .ok_or(UnitCodecError::Invalid("budget tag"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FormatSettings {
    None,
    Text {
        charset: String,
    },
    Csv {
        charset: String,
        delimiter: u8,
        quote: u8,
    },
    Archive {
        member_decoder: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionProfileDefinitionV1 {
    pub format: FormatId,
    pub parser_name: String,
    pub parser_version: String,
    pub parser_build_sha256: [u8; 32],
    pub native_binary_sha256: Option<[u8; 32]>,
    pub scope_revision: u32,
    pub segmentation_revision: u32,
    pub normalization_revision: u32,
    pub locator_revision: u32,
    pub format_settings: FormatSettings,
    pub limits: BTreeMap<BudgetKey, u64>,
}

pub(super) fn valid_ascii_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= u32::MAX as usize
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn canonical_setting(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= u32::MAX as usize
        && is_nfc(value)
        && !value.chars().any(char::is_control)
}

fn read_string(bytes: &[u8]) -> Result<String, UnitCodecError> {
    Ok(std::str::from_utf8(bytes)
        .map_err(|_| UnitCodecError::Invalid("profile UTF-8"))?
        .to_owned())
}

fn read_u32_payload(bytes: &[u8]) -> Result<u32, UnitCodecError> {
    let mut input = Reader::new(bytes);
    let value = input.u32()?;
    input.finish()?;
    Ok(value)
}

impl ExtractionProfileDefinitionV1 {
    fn validate(&self) -> Result<(), UnitCodecError> {
        if !valid_ascii_id(&self.parser_name) || !valid_ascii_id(&self.parser_version) {
            return Err(UnitCodecError::Invalid("parser identifier"));
        }
        if self.format == FormatId::Pdf && self.native_binary_sha256.is_none() {
            return Err(UnitCodecError::Invalid("PDF native pin"));
        }
        if self.normalization_revision != 1 || self.locator_revision != 1 {
            return Err(UnitCodecError::Invalid("profile revision"));
        }
        let settings_valid = match (&self.format, &self.format_settings) {
            (FormatId::Text, FormatSettings::Text { charset }) => canonical_setting(charset),
            (FormatId::Csv, FormatSettings::Csv { charset, .. }) => canonical_setting(charset),
            (FormatId::Zip, FormatSettings::Archive { member_decoder }) => {
                canonical_setting(member_decoder)
            }
            (
                FormatId::Docx
                | FormatId::Xlsx
                | FormatId::Xlsm
                | FormatId::Pptx
                | FormatId::Pdf
                | FormatId::Html,
                FormatSettings::None,
            ) => true,
            _ => false,
        };
        if !settings_valid {
            return Err(UnitCodecError::Invalid("format settings"));
        }
        if self.limits.len() != BudgetKey::ALL.len()
            || BudgetKey::ALL
                .iter()
                .any(|key| !self.limits.contains_key(key))
        {
            return Err(UnitCodecError::Invalid("budget keys"));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, UnitCodecError> {
        self.validate()?;
        let mut output = PROFILE_PREFIX.to_vec();
        write_frame(&mut output, &[self.format.tag()])?;
        write_frame(&mut output, self.parser_name.as_bytes())?;
        write_frame(&mut output, self.parser_version.as_bytes())?;
        write_frame(&mut output, &self.parser_build_sha256)?;
        let mut native = Vec::with_capacity(33);
        match self.native_binary_sha256 {
            None => native.push(0),
            Some(hash) => {
                native.push(1);
                native.extend_from_slice(&hash);
            }
        }
        write_frame(&mut output, &native)?;
        for revision in [
            self.scope_revision,
            self.segmentation_revision,
            self.normalization_revision,
            self.locator_revision,
        ] {
            write_frame(&mut output, &revision.to_be_bytes())?;
        }
        let mut settings = Vec::new();
        match &self.format_settings {
            FormatSettings::None => settings.push(0),
            FormatSettings::Text { charset } => {
                settings.push(1);
                write_frame(&mut settings, charset.as_bytes())?;
            }
            FormatSettings::Csv {
                charset,
                delimiter,
                quote,
            } => {
                settings.push(2);
                write_frame(&mut settings, charset.as_bytes())?;
                settings.extend_from_slice(&[*delimiter, *quote]);
            }
            FormatSettings::Archive { member_decoder } => {
                settings.push(3);
                write_frame(&mut settings, member_decoder.as_bytes())?;
            }
        }
        write_frame(&mut output, &settings)?;
        let mut budgets = Vec::new();
        write_u32(&mut budgets, BudgetKey::ALL.len())?;
        for key in BudgetKey::ALL {
            budgets.push(key.tag());
            budgets.extend_from_slice(&self.limits[&key].to_be_bytes());
        }
        write_frame(&mut output, &budgets)?;
        Ok(output)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, UnitCodecError> {
        let mut input = Reader::new(bytes);
        if input.take(PROFILE_PREFIX.len())? != PROFILE_PREFIX {
            return Err(UnitCodecError::Invalid("profile prefix"));
        }
        let format = {
            let mut field = Reader::new(input.frame()?);
            let value = FormatId::from_tag(field.byte()?)?;
            field.finish()?;
            value
        };
        let parser_name = read_string(input.frame()?)?;
        let parser_version = read_string(input.frame()?)?;
        let parser_build_sha256: [u8; 32] = input
            .frame()?
            .try_into()
            .map_err(|_| UnitCodecError::Invalid("parser hash"))?;
        let native_binary_sha256 = {
            let mut field = Reader::new(input.frame()?);
            let value = match field.byte()? {
                0 => None,
                1 => Some(field.take(32)?.try_into().expect("32 bytes")),
                _ => return Err(UnitCodecError::Invalid("native pin option")),
            };
            field.finish()?;
            value
        };
        let scope_revision = read_u32_payload(input.frame()?)?;
        let segmentation_revision = read_u32_payload(input.frame()?)?;
        let normalization_revision = read_u32_payload(input.frame()?)?;
        let locator_revision = read_u32_payload(input.frame()?)?;
        let format_settings = {
            let mut field = Reader::new(input.frame()?);
            let value = match field.byte()? {
                0 => FormatSettings::None,
                1 => FormatSettings::Text {
                    charset: read_string(field.frame()?)?,
                },
                2 => FormatSettings::Csv {
                    charset: read_string(field.frame()?)?,
                    delimiter: field.byte()?,
                    quote: field.byte()?,
                },
                3 => FormatSettings::Archive {
                    member_decoder: read_string(field.frame()?)?,
                },
                _ => return Err(UnitCodecError::Invalid("settings tag")),
            };
            field.finish()?;
            value
        };
        let limits = {
            let mut field = Reader::new(input.frame()?);
            if field.u32()? != 15 {
                return Err(UnitCodecError::Invalid("budget count"));
            }
            let mut map = BTreeMap::new();
            for expected in BudgetKey::ALL {
                let key = BudgetKey::from_tag(field.byte()?)?;
                if key != expected || map.insert(key, field.u64()?).is_some() {
                    return Err(UnitCodecError::Invalid("budget order"));
                }
            }
            field.finish()?;
            map
        };
        input.finish()?;
        let value = Self {
            format,
            parser_name,
            parser_version,
            parser_build_sha256,
            native_binary_sha256,
            scope_revision,
            segmentation_revision,
            normalization_revision,
            locator_revision,
            format_settings,
            limits,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveReaderNode {
    pub members: Vec<String>,
    pub parser_build_id: String,
    pub definition: ExtractionProfileDefinitionV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveProfilePlan {
    pub nodes: Vec<ArchiveReaderNode>,
    pub used_leaf_chains: Vec<Vec<String>>,
}

impl ArchiveProfilePlan {
    fn validate(&self) -> Result<(), UnitCodecError> {
        if self.nodes.is_empty() || self.used_leaf_chains.is_empty() {
            return Err(UnitCodecError::Invalid("empty archive plan"));
        }
        let mut nodes = BTreeMap::new();
        let mut previous: Option<&[String]> = None;
        for node in &self.nodes {
            if !valid_ascii_id(&node.parser_build_id) {
                return Err(UnitCodecError::Invalid("archive build ID"));
            }
            node.definition.validate()?;
            for member in &node.members {
                validate_archive_member(member)?;
            }
            if previous.is_some_and(|chain| chain >= node.members.as_slice()) {
                return Err(UnitCodecError::Invalid("archive node order"));
            }
            previous = Some(&node.members);
            if nodes
                .insert(node.members.as_slice(), node.definition.format)
                .is_some()
            {
                return Err(UnitCodecError::Invalid("duplicate archive node"));
            }
        }
        if self.nodes[0].members != Vec::<String>::new()
            || self.nodes[0].definition.format != FormatId::Zip
        {
            return Err(UnitCodecError::Invalid("archive root"));
        }
        for node in self.nodes.iter().skip(1) {
            if node.members.is_empty()
                || (0..node.members.len())
                    .any(|length| nodes.get(&node.members[..length]) != Some(&FormatId::Zip))
            {
                return Err(UnitCodecError::Invalid("missing ZIP prefix"));
            }
        }
        let mut leaves = BTreeSet::new();
        for chain in &self.used_leaf_chains {
            if chain.is_empty()
                || !leaves.insert(chain.as_slice())
                || nodes
                    .get(chain.as_slice())
                    .is_none_or(|format| *format == FormatId::Zip)
            {
                return Err(UnitCodecError::Invalid("archive leaf"));
            }
        }
        for node in self.nodes.iter().skip(1) {
            if node.definition.format == FormatId::Zip {
                if !leaves
                    .iter()
                    .any(|leaf| leaf.starts_with(&node.members) && leaf.len() > node.members.len())
                {
                    return Err(UnitCodecError::Invalid("unused ZIP node"));
                }
            } else if !leaves.contains(node.members.as_slice()) {
                return Err(UnitCodecError::Invalid("unused leaf node"));
            }
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, UnitCodecError> {
        self.validate()?;
        let mut output = ARCHIVE_PREFIX.to_vec();
        write_u32(&mut output, self.nodes.len())?;
        for node in &self.nodes {
            let mut chain = Vec::new();
            write_u32(&mut chain, node.members.len())?;
            for member in &node.members {
                write_frame(&mut chain, member.as_bytes())?;
            }
            let mut node_bytes = Vec::new();
            write_frame(&mut node_bytes, &chain)?;
            write_frame(&mut node_bytes, node.parser_build_id.as_bytes())?;
            write_frame(&mut node_bytes, &node.definition.encode()?)?;
            write_frame(&mut output, &node_bytes)?;
        }
        Ok(output)
    }

    pub fn decode(
        bytes: &[u8],
        used_leaf_chains: Vec<Vec<String>>,
    ) -> Result<Self, UnitCodecError> {
        let mut input = Reader::new(bytes);
        if input.take(ARCHIVE_PREFIX.len())? != ARCHIVE_PREFIX {
            return Err(UnitCodecError::Invalid("archive profile prefix"));
        }
        let count = input.u32()? as usize;
        if count == 0 || count > input.remaining() / 4 {
            return Err(UnitCodecError::Invalid("archive node count"));
        }
        let mut nodes = Vec::new();
        for _ in 0..count {
            let mut node = Reader::new(input.frame()?);
            let mut chain = Reader::new(node.frame()?);
            let member_count = chain.u32()? as usize;
            if member_count > chain.remaining() / 4 {
                return Err(UnitCodecError::Invalid("archive chain count"));
            }
            let mut members = Vec::new();
            for _ in 0..member_count {
                let member = read_string(chain.frame()?)?;
                validate_archive_member(&member)?;
                members.push(member);
            }
            chain.finish()?;
            let parser_build_id = read_string(node.frame()?)?;
            let definition = ExtractionProfileDefinitionV1::decode(node.frame()?)?;
            node.finish()?;
            nodes.push(ArchiveReaderNode {
                members,
                parser_build_id,
                definition,
            });
        }
        input.finish()?;
        let plan = Self {
            nodes,
            used_leaf_chains,
        };
        plan.validate()?;
        Ok(plan)
    }
}
