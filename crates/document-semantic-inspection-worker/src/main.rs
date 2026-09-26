use std::{
    ffi::OsStr,
    fs::File,
    io::{Read, stderr, stdin, stdout},
    process::exit,
};

#[cfg(target_os = "linux")]
use document_semantic_inspection_worker::{PdfAdapter, decode_request_bounded};
use document_semantic_inspection_worker::{
    SignatureTrustContext, WorkerFailure, WorkerFailureCode, open_inherited_input,
    run_worker_shell_with_signature_trust, write_failure,
};
use openssl::x509::{X509, X509Crl};
use serde::Deserialize;

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;
const MAX_TRUST_BUNDLE_BYTES: usize = 1024 * 1024;
const MAX_TRUST_DER_BYTES: usize = 256 * 1024;
const MAX_TRUST_CERTIFICATES: usize = 32;
const MAX_TRUST_CRLS: usize = 32;
const SANDBOX_REQUIRED_ENV: &str = "DSI_SANDBOX_REQUIRED";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignatureTrustBundle {
    trusted_certificates_der: Vec<Vec<u8>>,
    crls_der: Vec<Vec<u8>>,
}

fn main() {
    let sandbox_required = match sandbox_required() {
        Ok(required) => required,
        Err(failure) => exit_with_failure(failure, 69),
    };

    let input_fd = match std::env::var("DSI_INPUT_FD")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|fd| *fd >= 0)
    {
        Some(fd) => fd,
        None => {
            let failure = WorkerFailure::new(
                WorkerFailureCode::ExtractorUnavailable,
                "DSI_INPUT_FD must identify the inherited read-only input descriptor",
            );
            let mut stderr = stderr().lock();
            write_failure(&mut stderr, &failure);
            exit(69);
        }
    };

    let mut input = match open_inherited_input(input_fd) {
        Ok(input) => input,
        Err(failure) => {
            let mut stderr = stderr().lock();
            write_failure(&mut stderr, &failure);
            exit(69);
        }
    };

    let signature_trust = match read_signature_trust(input_fd) {
        Ok(trust) => trust,
        Err(failure) => {
            let mut stderr = stderr().lock();
            write_failure(&mut stderr, &failure);
            exit(65);
        }
    };

    let mut request_bytes = Vec::with_capacity(MAX_REQUEST_BYTES.min(8 * 1024));
    if let Err(error) = stdin()
        .lock()
        .take((MAX_REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut request_bytes)
    {
        let failure = WorkerFailure::new(
            WorkerFailureCode::MalformedRequest,
            format!("failed to read worker request: {error}"),
        );
        let mut stderr = stderr().lock();
        write_failure(&mut stderr, &failure);
        exit(65);
    }

    if sandbox_required && let Err(failure) = prepare_and_seal_worker(&input, &request_bytes) {
        let code = if failure.code() == WorkerFailureCode::MalformedRequest {
            65
        } else {
            69
        };
        exit_with_failure(failure, code);
    }

    let mut stdout = stdout().lock();
    let mut stderr = stderr().lock();
    let code = run_worker_shell_with_signature_trust(
        &request_bytes,
        &mut input,
        &mut stdout,
        &mut stderr,
        MAX_REQUEST_BYTES,
        MAX_INPUT_BYTES,
        &signature_trust,
    );
    exit(code);
}

fn sandbox_required() -> Result<bool, WorkerFailure> {
    match std::env::var_os(SANDBOX_REQUIRED_ENV) {
        None => Ok(false),
        Some(value) if value.as_os_str() == OsStr::new("1") => Ok(true),
        Some(_) => Err(WorkerFailure::new(
            WorkerFailureCode::ExtractorUnavailable,
            "mandatory worker sandbox policy is invalid",
        )),
    }
}

#[cfg(target_os = "linux")]
fn prepare_and_seal_worker(input: &File, request_bytes: &[u8]) -> Result<(), WorkerFailure> {
    let request = decode_request_bounded(request_bytes, MAX_REQUEST_BYTES)
        .map_err(|_| invalid_preseal_request())?;

    if is_declared_pdf(&request.declared_media_type) && has_pdf_header(input)? {
        PdfAdapter::warm_up_native_runtime().map_err(|_| {
            WorkerFailure::new(
                WorkerFailureCode::ExtractorUnavailable,
                "mandatory worker native runtime initialization failed",
            )
        })?;
    }

    document_semantic_inspection_runner::seal_worker_sandbox().map_err(|_| {
        WorkerFailure::new(
            WorkerFailureCode::ExtractorUnavailable,
            "mandatory worker sandbox setup failed",
        )
    })
}

#[cfg(not(target_os = "linux"))]
fn prepare_and_seal_worker(_input: &File, _request_bytes: &[u8]) -> Result<(), WorkerFailure> {
    Err(WorkerFailure::new(
        WorkerFailureCode::ExtractorUnavailable,
        "mandatory worker sandbox is unavailable on this platform",
    ))
}

#[cfg(target_os = "linux")]
fn has_pdf_header(input: &File) -> Result<bool, WorkerFailure> {
    use std::os::unix::fs::FileExt;

    let mut header = [0u8; 5];
    let mut observed = 0;
    while observed < header.len() {
        let read = input
            .read_at(&mut header[observed..], observed as u64)
            .map_err(|_| input_preflight_failure())?;
        if read == 0 {
            break;
        }
        observed += read;
    }
    Ok(observed == header.len() && &header == b"%PDF-")
}

#[cfg(target_os = "linux")]
fn is_declared_pdf(media_type: &str) -> bool {
    media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .eq_ignore_ascii_case("application/pdf")
}

#[cfg(target_os = "linux")]
fn invalid_preseal_request() -> WorkerFailure {
    WorkerFailure::new(
        WorkerFailureCode::MalformedRequest,
        "worker request is invalid",
    )
}

#[cfg(target_os = "linux")]
fn input_preflight_failure() -> WorkerFailure {
    WorkerFailure::new(
        WorkerFailureCode::ExtractorUnavailable,
        "worker input preflight failed",
    )
}

fn exit_with_failure(failure: WorkerFailure, code: i32) -> ! {
    let mut stderr = stderr().lock();
    write_failure(&mut stderr, &failure);
    exit(code);
}

fn read_signature_trust(input_fd: i32) -> Result<SignatureTrustContext, WorkerFailure> {
    let trust_fd = match std::env::var("DSI_TRUST_FD") {
        Ok(value) => value
            .parse::<i32>()
            .ok()
            .filter(|fd| *fd >= 3 && *fd != input_fd)
            .ok_or_else(malformed_trust_bundle)?,
        Err(std::env::VarError::NotPresent) => return Ok(SignatureTrustContext::default()),
        Err(std::env::VarError::NotUnicode(_)) => return Err(malformed_trust_bundle()),
    };

    let mut trust_file = open_inherited_input(trust_fd).map_err(|_| malformed_trust_bundle())?;
    let mut trust_bytes = Vec::with_capacity(MAX_TRUST_BUNDLE_BYTES.min(8 * 1024));
    if (&mut trust_file)
        .take((MAX_TRUST_BUNDLE_BYTES + 1) as u64)
        .read_to_end(&mut trust_bytes)
        .is_err()
    {
        return Err(malformed_trust_bundle());
    }
    if trust_bytes.len() > MAX_TRUST_BUNDLE_BYTES {
        return Err(WorkerFailure::new(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            "inherited trust bundle exceeds its byte bound",
        ));
    }

    let bundle: SignatureTrustBundle =
        serde_json::from_slice(&trust_bytes).map_err(|_| malformed_trust_bundle())?;
    validate_trust_bundle(&bundle)?;

    let mut trust = SignatureTrustContext::new(bundle.trusted_certificates_der);
    for crl_der in bundle.crls_der {
        trust = trust.with_crl_der(crl_der);
    }
    Ok(trust)
}

fn validate_trust_bundle(bundle: &SignatureTrustBundle) -> Result<(), WorkerFailure> {
    if bundle.trusted_certificates_der.len() > MAX_TRUST_CERTIFICATES
        || bundle.crls_der.len() > MAX_TRUST_CRLS
        || bundle.trusted_certificates_der.iter().any(|der| {
            der.is_empty() || der.len() > MAX_TRUST_DER_BYTES || X509::from_der(der).is_err()
        })
        || bundle.crls_der.iter().any(|der| {
            der.is_empty() || der.len() > MAX_TRUST_DER_BYTES || X509Crl::from_der(der).is_err()
        })
    {
        return Err(WorkerFailure::new(
            WorkerFailureCode::InspectionResourceLimitExceeded,
            "inherited trust bundle exceeds its item bound or contains invalid DER",
        ));
    }
    Ok(())
}

fn malformed_trust_bundle() -> WorkerFailure {
    WorkerFailure::new(
        WorkerFailureCode::MalformedRequest,
        "DSI_TRUST_FD must identify a separate bounded DER trust bundle",
    )
}
