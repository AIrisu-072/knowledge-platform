//! Frozen finite resource profile for Document Semantic Inspection v0.

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionResourceProfile {
    pub cpu_seconds: u64,
    pub output_file_blocks: u64,
    pub address_space_kib: u64,
    pub wall_timeout_ms: u64,
    pub input_bytes: u64,
    pub ooxml_total_uncompressed_bytes: u64,
    pub ooxml_entry_bytes: u64,
    pub archive_entries: u64,
    pub xml_depth: u64,
    pub xml_nodes: u64,
    pub sheets: u64,
    pub cells: u64,
    pub slides: u64,
    pub shapes: u64,
    pub images: u64,
    pub decoded_pixels: u64,
    pub embedded_objects: u64,
    pub vba_modules: u64,
    pub vba_source_bytes: u64,
    pub structured_result_bytes: u64,
    pub stderr_bytes: u64,
    pub temp_disk_bytes: u64,
    pub child_processes: u64,

    // Format-specific limits already qualified by the PoC. These are retained
    // explicitly rather than widened to the generic OOXML ceiling.
    pub docx_archive_entries: u64,
    pub docx_entry_bytes: u64,
    pub docx_total_uncompressed_bytes: u64,
    pub docx_xml_depth: u64,
    pub spreadsheet_archive_entries: u64,
    pub spreadsheet_total_uncompressed_bytes: u64,
    pub pptx_archive_entries: u64,
    pub pptx_total_uncompressed_bytes: u64,
    pub pptx_xml_depth: u64,
    pub pdf_decompressed_stream_bytes: u64,
}

impl ProductionResourceProfile {
    pub const DSI_V0: Self = Self {
        cpu_seconds: 8,
        output_file_blocks: 2_048,
        address_space_kib: 2_097_152,
        wall_timeout_ms: 10_000,

        // New production ceilings. They are intentionally conservative within
        // the 2 GiB address-space envelope and fail closed above the boundary.
        input_bytes: 256 * MIB,
        ooxml_total_uncompressed_bytes: 512 * MIB,
        ooxml_entry_bytes: 64 * MIB,
        archive_entries: 20_000,
        xml_depth: 256,
        xml_nodes: 2_000_000,
        sheets: 1_024,
        cells: 1_000_000,
        slides: 4_096,
        shapes: 100_000,
        images: 4_096,
        decoded_pixels: 64 * 1024 * 1024,
        embedded_objects: 1_024,
        vba_modules: 1_024,
        vba_source_bytes: 16 * MIB,
        structured_result_bytes: 16 * MIB,
        stderr_bytes: 1 * MIB,
        temp_disk_bytes: 1 * GIB,
        child_processes: 0,

        docx_archive_entries: 256,
        docx_entry_bytes: 8 * MIB,
        docx_total_uncompressed_bytes: 32 * MIB,
        docx_xml_depth: 64,
        spreadsheet_archive_entries: 20_000,
        spreadsheet_total_uncompressed_bytes: 512 * MIB,
        pptx_archive_entries: 20_000,
        pptx_total_uncompressed_bytes: 512 * MIB,
        pptx_xml_depth: 256,
        pdf_decompressed_stream_bytes: 64 * MIB,
    };

    pub const fn limit(self, class: ResourceClass) -> u64 {
        match class {
            ResourceClass::CpuSeconds => self.cpu_seconds,
            ResourceClass::OutputFileBlocks => self.output_file_blocks,
            ResourceClass::AddressSpaceKib => self.address_space_kib,
            ResourceClass::WallTimeoutMs => self.wall_timeout_ms,
            ResourceClass::InputBytes => self.input_bytes,
            ResourceClass::OoxmlTotalUncompressedBytes => self.ooxml_total_uncompressed_bytes,
            ResourceClass::OoxmlEntryBytes => self.ooxml_entry_bytes,
            ResourceClass::ArchiveEntries => self.archive_entries,
            ResourceClass::XmlDepth => self.xml_depth,
            ResourceClass::XmlNodes => self.xml_nodes,
            ResourceClass::Sheets => self.sheets,
            ResourceClass::Cells => self.cells,
            ResourceClass::Slides => self.slides,
            ResourceClass::Shapes => self.shapes,
            ResourceClass::Images => self.images,
            ResourceClass::DecodedPixels => self.decoded_pixels,
            ResourceClass::EmbeddedObjects => self.embedded_objects,
            ResourceClass::VbaModules => self.vba_modules,
            ResourceClass::VbaSourceBytes => self.vba_source_bytes,
            ResourceClass::StructuredResultBytes => self.structured_result_bytes,
            ResourceClass::StderrBytes => self.stderr_bytes,
            ResourceClass::TempDiskBytes => self.temp_disk_bytes,
            ResourceClass::ChildProcesses => self.child_processes,
        }
    }

    pub const fn check(
        self,
        class: ResourceClass,
        observed: u64,
    ) -> Result<(), ResourceLimitExceeded> {
        let limit = self.limit(class);
        if observed <= limit {
            Ok(())
        } else {
            Err(ResourceLimitExceeded {
                class,
                observed,
                limit,
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceClass {
    CpuSeconds,
    OutputFileBlocks,
    AddressSpaceKib,
    WallTimeoutMs,
    InputBytes,
    OoxmlTotalUncompressedBytes,
    OoxmlEntryBytes,
    ArchiveEntries,
    XmlDepth,
    XmlNodes,
    Sheets,
    Cells,
    Slides,
    Shapes,
    Images,
    DecodedPixels,
    EmbeddedObjects,
    VbaModules,
    VbaSourceBytes,
    StructuredResultBytes,
    StderrBytes,
    TempDiskBytes,
    ChildProcesses,
}

impl ResourceClass {
    pub const ALL: [Self; 23] = [
        Self::CpuSeconds,
        Self::OutputFileBlocks,
        Self::AddressSpaceKib,
        Self::WallTimeoutMs,
        Self::InputBytes,
        Self::OoxmlTotalUncompressedBytes,
        Self::OoxmlEntryBytes,
        Self::ArchiveEntries,
        Self::XmlDepth,
        Self::XmlNodes,
        Self::Sheets,
        Self::Cells,
        Self::Slides,
        Self::Shapes,
        Self::Images,
        Self::DecodedPixels,
        Self::EmbeddedObjects,
        Self::VbaModules,
        Self::VbaSourceBytes,
        Self::StructuredResultBytes,
        Self::StderrBytes,
        Self::TempDiskBytes,
        Self::ChildProcesses,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimitExceeded {
    pub class: ResourceClass,
    pub observed: u64,
    pub limit: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsi_v0_keeps_exact_poc_qualified_limits() {
        let p = ProductionResourceProfile::DSI_V0;
        assert_eq!(p.cpu_seconds, 8);
        assert_eq!(p.output_file_blocks, 2_048);
        assert_eq!(p.address_space_kib, 2_097_152);
        assert_eq!(p.docx_archive_entries, 256);
        assert_eq!(p.docx_entry_bytes, 8 * MIB);
        assert_eq!(p.docx_total_uncompressed_bytes, 32 * MIB);
        assert_eq!(p.docx_xml_depth, 64);
        assert_eq!(p.spreadsheet_archive_entries, 20_000);
        assert_eq!(p.spreadsheet_total_uncompressed_bytes, 512 * MIB);
        assert_eq!(p.pptx_archive_entries, 20_000);
        assert_eq!(p.pptx_total_uncompressed_bytes, 512 * MIB);
        assert_eq!(p.pptx_xml_depth, 256);
        assert_eq!(p.pdf_decompressed_stream_bytes, 64 * MIB);
    }

    #[test]
    fn every_required_resource_class_has_an_explicit_finite_boundary() {
        let p = ProductionResourceProfile::DSI_V0;
        for class in ResourceClass::ALL {
            let limit = p.limit(class);
            if class == ResourceClass::ChildProcesses {
                assert_eq!(limit, 0, "production child-process count must be zero");
            } else {
                assert!(limit > 0, "{class:?} is not bounded");
                assert!(limit < u64::MAX, "{class:?} is effectively unlimited");
            }
        }
    }

    #[test]
    fn synthetic_boundary_cases_accept_exact_limit_and_reject_one_over() {
        let p = ProductionResourceProfile::DSI_V0;
        for class in ResourceClass::ALL {
            let limit = p.limit(class);
            assert_eq!(p.check(class, limit), Ok(()), "{class:?}");
            let observed = limit.checked_add(1).expect("finite resource limit");
            assert_eq!(
                p.check(class, observed),
                Err(ResourceLimitExceeded {
                    class,
                    observed,
                    limit,
                }),
                "{class:?}"
            );
        }
    }

    #[test]
    fn decoded_image_budget_stays_inside_worker_address_space_envelope() {
        let p = ProductionResourceProfile::DSI_V0;
        let rgba_bytes = p.decoded_pixels * 4;
        let address_space_bytes = p.address_space_kib * 1024;
        assert!(rgba_bytes <= 256 * MIB);
        assert!(p.input_bytes + rgba_bytes + p.structured_result_bytes < address_space_bytes);
    }
}
