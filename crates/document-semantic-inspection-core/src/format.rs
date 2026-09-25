use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum FormatId {
    Docx,
    Xlsx,
    Xlsm,
    Pptx,
    Pdf,
    Txt,
    Csv,
    Html,
}

impl FormatId {
    pub const REQUIRED_V0: [Self; 8] = [
        Self::Docx,
        Self::Xlsx,
        Self::Xlsm,
        Self::Pptx,
        Self::Pdf,
        Self::Txt,
        Self::Csv,
        Self::Html,
    ];
}
