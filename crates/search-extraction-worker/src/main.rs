//! One-shot Search extraction worker. Reader dispatch is installed by P1-I03–I05.

use std::{
    fs::File,
    io::{self, Read, Write},
    os::fd::FromRawFd,
    path::Path,
    process::exit,
};

use search_core::knowledge_unit::{
    ArchiveProfilePlan, ExtractionProfileDefinitionV1, ExtractionProfileId, FormatId,
};
use search_extraction_core::{
    ReaderFailure, RetryableFailureCode, WorkerResponse, decode_request, encode_response,
};
use sha2::{Digest, Sha256};

const MAX_REQUEST_BYTES: u64 = 16_777_216;
const MAX_RAW_BYTES: u64 = 268_435_456;

fn main() {
    match run() {
        Ok(()) => {}
        Err(Failure::Raw) => {
            eprintln!("SEARCH_RAW_BINDING_MISMATCH");
            exit(76);
        }
        Err(Failure::Native) => {
            eprintln!("SEARCH_NATIVE_PIN_MISMATCH");
            exit(77);
        }
        Err(Failure::Sandbox) => {
            eprintln!("SEARCH_SANDBOX_UNAVAILABLE");
            exit(78);
        }
        Err(Failure::Protocol) => exit(79),
    }
}

enum Failure {
    Raw,
    Native,
    Sandbox,
    Protocol,
}

fn run() -> Result<(), Failure> {
    let mut wire = Vec::new();
    io::stdin()
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut wire)
        .map_err(|_| Failure::Protocol)?;
    if wire.len() as u64 > MAX_REQUEST_BYTES {
        return Err(Failure::Protocol);
    }
    let request = decode_request(&wire).map_err(|_| Failure::Protocol)?;

    if std::env::var("SEARCH_INPUT_FD").as_deref() != Ok("3") {
        return Err(Failure::Protocol);
    }
    let input = unsafe { File::from_raw_fd(3) };
    let mut raw = Vec::new();
    input
        .take(MAX_RAW_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|_| Failure::Raw)?;
    if raw.len() as u64 > MAX_RAW_BYTES || raw.len() as u64 != request.expected_raw.size_bytes {
        return Err(Failure::Raw);
    }
    let observed: [u8; 32] = Sha256::digest(&raw).into();
    if observed != request.expected_raw.sha256 {
        return Err(Failure::Raw);
    }

    if request.format == FormatId::Zip {
        let leaves = archive_leaf_chains(&request.profile_bytes)?;
        let plan = ArchiveProfilePlan::decode(&request.profile_bytes, leaves)
            .map_err(|_| Failure::Protocol)?;
        if ExtractionProfileId::for_archive(&plan).map_err(|_| Failure::Protocol)?
            != request.profile
        {
            return Err(Failure::Protocol);
        }
        let mut pdfium_pin = None;
        for node in &plan.nodes {
            if node.definition.format == FormatId::Pdf {
                let pin = node
                    .definition
                    .native_binary_sha256
                    .ok_or(Failure::Native)?;
                if pdfium_pin.is_some_and(|expected| expected != pin) {
                    return Err(Failure::Native);
                }
                pdfium_pin = Some(pin);
            }
        }
        if let Some(pin) = pdfium_pin {
            verify_pdfium_native(pin)?;
        }
    } else {
        let definition = ExtractionProfileDefinitionV1::decode(&request.profile_bytes)
            .map_err(|_| Failure::Protocol)?;
        if definition.format != request.format
            || ExtractionProfileId::for_definition(&definition).map_err(|_| Failure::Protocol)?
                != request.profile
        {
            return Err(Failure::Protocol);
        }
        if request.format == FormatId::Pdf {
            verify_pdfium_native(definition.native_binary_sha256.ok_or(Failure::Native)?)?;
        }
    }

    document_sandbox_runner::seal_worker_sandbox().map_err(|_| Failure::Sandbox)?;
    // Format readers are registered in later implementation tasks. No successful
    // report can be emitted while they are absent.
    let response = WorkerResponse::Failure(ReaderFailure::Retryable(
        RetryableFailureCode::WorkerUnavailable,
    ));
    let encoded = encode_response(&response).map_err(|_| Failure::Protocol)?;
    io::stdout()
        .write_all(&encoded)
        .map_err(|_| Failure::Protocol)
}

fn archive_leaf_chains(bytes: &[u8]) -> Result<Vec<Vec<String>>, Failure> {
    const PREFIX: &[u8] = b"extraction-profile:archive:v2\0";
    let mut input = bytes.strip_prefix(PREFIX).ok_or(Failure::Protocol)?;
    let count = read_u32(&mut input)?;
    if count == 0 || count > 20_001 {
        return Err(Failure::Protocol);
    }
    let mut leaves = Vec::new();
    for _ in 0..count {
        let mut node = read_frame(&mut input)?;
        let mut chain = read_frame(&mut node)?;
        let members = read_u32(&mut chain)?;
        if members > 3 {
            return Err(Failure::Protocol);
        }
        let mut path = Vec::new();
        for _ in 0..members {
            path.push(
                std::str::from_utf8(read_frame(&mut chain)?)
                    .map_err(|_| Failure::Protocol)?
                    .to_owned(),
            );
        }
        if !chain.is_empty() {
            return Err(Failure::Protocol);
        }
        let _parser_id = read_frame(&mut node)?;
        let definition = ExtractionProfileDefinitionV1::decode(read_frame(&mut node)?)
            .map_err(|_| Failure::Protocol)?;
        if !node.is_empty() {
            return Err(Failure::Protocol);
        }
        if definition.format != FormatId::Zip {
            leaves.push(path);
        }
    }
    if !input.is_empty() {
        return Err(Failure::Protocol);
    }
    Ok(leaves)
}

fn read_u32(input: &mut &[u8]) -> Result<usize, Failure> {
    let bytes = input.get(..4).ok_or(Failure::Protocol)?;
    let value = u32::from_be_bytes(bytes.try_into().map_err(|_| Failure::Protocol)?) as usize;
    *input = &input[4..];
    Ok(value)
}

fn read_frame<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], Failure> {
    let size = read_u32(input)?;
    if size > input.len() {
        return Err(Failure::Protocol);
    }
    let (bytes, remaining) = input.split_at(size);
    *input = remaining;
    Ok(bytes)
}

fn verify_pdfium_native(expected: [u8; 32]) -> Result<(), Failure> {
    let directory = std::env::var_os("PDFIUM_DYNAMIC_LIB_PATH").ok_or(Failure::Native)?;
    let mut native =
        File::open(Path::new(&directory).join("libpdfium.so")).map_err(|_| Failure::Native)?;
    if !native
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= 268_435_456)
    {
        return Err(Failure::Native);
    }
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = native.read(&mut buffer).map_err(|_| Failure::Native)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let observed: [u8; 32] = digest.finalize().into();
    if observed != expected {
        return Err(Failure::Native);
    }
    Ok(())
}
