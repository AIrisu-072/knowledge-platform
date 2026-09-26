use std::{
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
};

use crate::{WorkerFailure, WorkerFailureCode};

// The workbook package profile already limits each expanded OOXML part to 64 MiB.
// Reuse that ceiling for the VBA CFB container and for its encoded streams.
const MAX_VBA_PROJECT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CFB_STREAM_BYTES: u64 = MAX_VBA_PROJECT_BYTES;
const MAX_CFB_STREAM_TOTAL_BYTES: u64 = MAX_VBA_PROJECT_BYTES;

/// Performs bounded structural checks before the VBA project is passed to `ovba`.
///
/// This validates the CFB structure, requires the VBA directory stream, and reads
/// every stream through a fixed-size buffer while enforcing the existing 64 MiB
/// per-part/total ceilings. Decoded `.dir` and module-source bounds, module count,
/// and strict `.dir` consumption must also be enforced by the hardened `ovba`
/// parser. Stock `ovba 0.7.1` does not expose the `.dir` parser's consumed offset,
/// so this preflight alone cannot prove release-mode rejection of trailing decoded
/// `.dir` data.
pub(super) fn preflight_vba_project(vba_bytes: &[u8]) -> Result<(), WorkerFailure> {
    let input_size = u64::try_from(vba_bytes.len()).map_err(|_| resource_limit())?;
    if input_size > MAX_VBA_PROJECT_BYTES {
        return Err(resource_limit());
    }

    let mut container =
        cfb::CompoundFile::open_strict(Cursor::new(vba_bytes)).map_err(|_| malformed_project())?;
    let sector_size =
        u64::try_from(container.version().sector_len()).map_err(|_| malformed_project())?;
    if input_size % sector_size != 0 {
        return Err(malformed_project());
    }

    let mut total_stream_bytes = 0_u64;
    let mut vba_dir_count = 0_u8;
    let maximum_streams = usize::try_from(input_size / 128 + 1).map_err(|_| resource_limit())?;
    let mut streams = Vec::<(PathBuf, u64, bool)>::new();

    for entry in container
        .read_storage("/VBA")
        .map_err(|_| malformed_project())?
    {
        if !entry.is_stream() {
            continue;
        }

        let declared_size = entry.len();
        if declared_size > MAX_CFB_STREAM_BYTES {
            return Err(resource_limit());
        }
        total_stream_bytes = total_stream_bytes
            .checked_add(declared_size)
            .filter(|total| *total <= MAX_CFB_STREAM_TOTAL_BYTES)
            .ok_or_else(resource_limit)?;

        let is_dir = is_vba_dir_path(entry.path());
        if is_dir {
            vba_dir_count = vba_dir_count.checked_add(1).ok_or_else(malformed_project)?;
            if vba_dir_count > 1 || declared_size < 3 {
                return Err(malformed_project());
            }
        }

        if streams.len() >= maximum_streams {
            return Err(malformed_project());
        }
        streams.try_reserve(1).map_err(|_| resource_limit())?;
        streams.push((entry.path().to_path_buf(), declared_size, is_dir));
    }

    if vba_dir_count != 1 {
        return Err(malformed_project());
    }

    for (path, declared_size, is_dir) in streams {
        let stream = container
            .open_stream(path)
            .map_err(|_| malformed_project())?;
        verify_stream_read(stream, declared_size, is_dir)?;
    }

    Ok(())
}

fn verify_stream_read<R: Read>(
    reader: R,
    expected_size: u64,
    is_dir: bool,
) -> Result<(), WorkerFailure> {
    if expected_size > MAX_CFB_STREAM_BYTES {
        return Err(resource_limit());
    }
    let read_limit = expected_size.checked_add(1).ok_or_else(resource_limit)?;
    let mut bounded = reader.take(read_limit);
    let mut buffer = [0_u8; 8 * 1024];
    let mut actual_size = 0_u64;
    let mut first_byte = None;

    loop {
        let bytes_read = bounded.read(&mut buffer).map_err(|_| malformed_project())?;
        if bytes_read == 0 {
            break;
        }
        if first_byte.is_none() {
            first_byte = Some(buffer[0]);
        }
        actual_size = actual_size
            .checked_add(bytes_read as u64)
            .ok_or_else(resource_limit)?;
        if actual_size > expected_size {
            return Err(malformed_project());
        }
    }

    if actual_size != expected_size {
        return Err(malformed_project());
    }
    if is_dir && first_byte != Some(0x01) {
        return Err(malformed_project());
    }

    Ok(())
}

fn is_vba_dir_path(path: &Path) -> bool {
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::RootDir)) {
        return false;
    }
    let Some(Component::Normal(storage)) = components.next() else {
        return false;
    };
    let Some(Component::Normal(stream)) = components.next() else {
        return false;
    };
    components.next().is_none()
        && storage.to_string_lossy().eq_ignore_ascii_case("VBA")
        && stream.to_string_lossy().eq_ignore_ascii_case("dir")
}

fn malformed_project() -> WorkerFailure {
    WorkerFailure::new(
        WorkerFailureCode::SemanticExtractionFailed,
        "VBA project could not be inspected safely",
    )
}

fn resource_limit() -> WorkerFailure {
    WorkerFailure::new(
        WorkerFailureCode::InspectionResourceLimitExceeded,
        "VBA project exceeds a configured inspection limit",
    )
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Write};

    use crate::WorkerFailureCode;

    use super::{preflight_vba_project, verify_stream_read};

    #[test]
    fn rejects_non_cfb_bytes_without_echoing_input() {
        let raw = b"private-vba-source-marker";

        let failure = preflight_vba_project(raw).expect_err("non-CFB input must fail closed");

        assert_eq!(failure.code(), WorkerFailureCode::SemanticExtractionFailed);
        assert!(!failure.message().contains("private-vba-source-marker"));
    }

    #[test]
    fn rejects_a_stream_that_exceeds_its_cfb_declared_length() {
        let failure = verify_stream_read(Cursor::new([1_u8, 2, 3]), 2, false)
            .expect_err("an overlong stream must fail closed");

        assert_eq!(failure.code(), WorkerFailureCode::SemanticExtractionFailed);
    }

    #[test]
    fn accepts_the_qualified_calamine_vba_seed() {
        let workbook = include_bytes!(
            "../../../../experiments/document-semantic-inspection/fixtures/xlsm/calamine-vba.xlsm"
        );
        let mut archive = zip::ZipArchive::new(Cursor::new(workbook))
            .expect("qualified fixture should be a valid OOXML package");
        let mut vba_project = Vec::new();
        archive
            .by_name("xl/vbaProject.bin")
            .expect("qualified fixture should contain a VBA project")
            .read_to_end(&mut vba_project)
            .expect("VBA project entry should be readable");

        preflight_vba_project(&vba_project).expect("qualified VBA seed should pass preflight");
    }

    #[test]
    fn accepts_a_well_formed_cfb_with_a_bounded_dir_stream() {
        let mut container = cfb::CompoundFile::create(Cursor::new(Vec::new()))
            .expect("CFB container should be created");
        container
            .create_storage("/VBA")
            .expect("VBA storage should be created");
        container
            .create_stream("/VBA/dir")
            .expect("dir stream should be created")
            .write_all(&[0x01, 0x02, 0x30])
            .expect("dir stream should be written");
        container.flush().expect("CFB container should flush");
        let bytes = container.into_inner().into_inner();

        preflight_vba_project(&bytes).expect("bounded CFB structure should pass preflight");
    }
}
