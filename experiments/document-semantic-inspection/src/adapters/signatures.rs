use crate::{PocError, SignatureEvidence, SignatureValidity};
use cms::{content_info::ContentInfo, signed_data::SignedData};
use der::{Decode, Encode};
use openssl::asn1::Asn1Time;
use openssl::hash::MessageDigest;
use openssl::nid::Nid;
use openssl::pkcs7::{Pkcs7, Pkcs7Flags};
use openssl::stack::Stack;
use openssl::x509::store::X509StoreBuilder;
use openssl::x509::{X509, X509Crl, X509StoreContext};
use quick_xml::events::Event;
use std::cmp::Ordering;
use xml_sec::xmldsig::{DefaultKeyResolver, DsigStatus, VerifyContext};

#[derive(Debug, Clone, Default)]
pub struct SignatureTrustContext {
    trusted_certificates_der: Vec<Vec<u8>>,
    crls_der: Vec<Vec<u8>>,
}

impl SignatureTrustContext {
    pub fn new(trusted_certificates_der: Vec<Vec<u8>>) -> Self {
        Self {
            trusted_certificates_der,
            crls_der: Vec::new(),
        }
    }

    pub fn with_crl_der(mut self, crl_der: Vec<u8>) -> Self {
        self.crls_der.push(crl_der);
        self
    }
}

pub struct SignatureInspector;

impl SignatureInspector {
    pub fn verify_detached_cms(
        content: &[u8],
        signature_der: &[u8],
        trust: &SignatureTrustContext,
    ) -> Result<SignatureEvidence, PocError> {
        let structural = match parse_cms_structure(signature_der) {
            Ok(value) => value,
            Err(message) => {
                return Ok(evidence(
                    "cms",
                    SignatureValidity::Invalid,
                    None,
                    "detached-content",
                    vec![
                        "cms-parser=cms-0.2.3".into(),
                        "crypto=openssl-0.10.81".into(),
                        format!("cms-structure-invalid:{message}"),
                    ],
                ));
            }
        };

        let mut diagnostics = vec![
            "cms-parser=cms-0.2.3".to_owned(),
            "crypto=openssl-0.10.81".to_owned(),
            format!(
                "cms-signers={};signature-algorithm={}",
                structural.signer_count, structural.signature_algorithm_oid
            ),
        ];

        if !cms_algorithm_supported(&structural.signature_algorithm_oid) {
            diagnostics.push(format!(
                "unsupported-cms-signature-algorithm:{}",
                structural.signature_algorithm_oid
            ));
            return Ok(evidence(
                "cms",
                SignatureValidity::Unverifiable,
                None,
                "detached-content",
                diagnostics,
            ));
        }

        let pkcs7 = match Pkcs7::from_der(signature_der) {
            Ok(value) => value,
            Err(error) => {
                diagnostics.push(format!("malformed-cms:{error}"));
                return Ok(evidence(
                    "cms",
                    SignatureValidity::Invalid,
                    None,
                    "detached-content",
                    diagnostics,
                ));
            }
        };

        let signer = first_embedded_certificate(&pkcs7);
        let metadata = signer.as_ref().map(certificate_metadata).transpose()?;

        let empty_certs = Stack::<X509>::new()
            .map_err(|error| PocError::InvalidWorkerResult(format!("OpenSSL stack: {error}")))?;
        let empty_store = X509StoreBuilder::new()
            .map_err(|error| PocError::InvalidWorkerResult(format!("OpenSSL store: {error}")))?
            .build();

        let crypto = pkcs7.verify(
            &empty_certs,
            &empty_store,
            Some(content),
            None,
            Pkcs7Flags::NOVERIFY | Pkcs7Flags::BINARY,
        );

        if let Err(error) = crypto {
            let detail = error.to_string();
            diagnostics.push(format!("cms-cryptographic-verification:{detail}"));
            let validity = if unsupported_crypto_error(&detail) {
                SignatureValidity::Unverifiable
            } else {
                SignatureValidity::Invalid
            };
            return Ok(evidence_from_metadata(
                "cms",
                validity,
                metadata,
                "detached-content",
                diagnostics,
            ));
        }

        diagnostics.push("cms-cryptographic-verification=valid".into());

        let validity = match signer {
            Some(ref signer) => {
                evaluate_certificate_policy(signer, embedded_certificates(&pkcs7), trust, &mut diagnostics)?
            }
            None => {
                diagnostics.push("signer-certificate=missing".into());
                SignatureValidity::Unverifiable
            }
        };

        Ok(evidence_from_metadata(
            "cms",
            validity,
            metadata,
            "detached-content",
            diagnostics,
        ))
    }

    pub fn verify_pdf_byte_range(
        pdf: &[u8],
        trust: &SignatureTrustContext,
    ) -> Result<SignatureEvidence, PocError> {
        let (covered, cms_der, coverage) = match parse_pdf_byte_range_signature(pdf) {
            Ok(value) => value,
            Err(reason) => {
                return Ok(evidence(
                    "pdf-cms",
                    SignatureValidity::Invalid,
                    None,
                    "pdf-byte-range:invalid",
                    vec![
                        "explicit-trust-only;network-retrieval=disabled".into(),
                        format!("pdf-byte-range-invalid:{reason}"),
                    ],
                ));
            }
        };

        let mut evidence = Self::verify_detached_cms(&covered, &cms_der, trust)?;
        evidence.kind = "pdf-cms".into();
        evidence.covered_content = Some(coverage);
        evidence
            .validation_diagnostics
            .push("pdf-byte-range-structure-valid".into());
        Ok(evidence)
    }

    pub fn verify_xmldsig(
        xml: &str,
        trust: &SignatureTrustContext,
    ) -> Result<SignatureEvidence, PocError> {
        let mut diagnostics = vec!["xmldsig=xml-sec-0.1.16".to_owned()];
        let cert_der = extract_x509_certificate(xml)?;
        let cert = cert_der
            .as_deref()
            .and_then(|bytes| X509::from_der(bytes).ok());
        let metadata = cert.as_ref().map(certificate_metadata).transpose()?;

        let resolver = DefaultKeyResolver::default();
        let verification = VerifyContext::new().key_resolver(&resolver).verify(xml);

        match verification {
            Ok(result) => match result.status {
                DsigStatus::Valid => {
                    diagnostics.push("xmldsig-cryptographic-verification=valid".into());
                }
                DsigStatus::Invalid(reason) => {
                    diagnostics.push(format!("xmldsig-invalid:{reason:?}"));
                    return Ok(evidence_from_metadata(
                        "xmldsig",
                        SignatureValidity::Invalid,
                        metadata,
                        "same-document-references",
                        diagnostics,
                    ));
                }
                other => {
                    diagnostics.push(format!("xmldsig-unverifiable:{other:?}"));
                    return Ok(evidence_from_metadata(
                        "xmldsig",
                        SignatureValidity::Unverifiable,
                        metadata,
                        "same-document-references",
                        diagnostics,
                    ));
                }
            },
            Err(error) => {
                let detail = error.to_string();
                diagnostics.push(format!("xmldsig-error:{detail}"));
                let validity = if unsupported_crypto_error(&detail) {
                    SignatureValidity::Unverifiable
                } else {
                    SignatureValidity::Invalid
                };
                return Ok(evidence_from_metadata(
                    "xmldsig",
                    validity,
                    metadata,
                    "same-document-references",
                    diagnostics,
                ));
            }
        }

        let validity = match cert {
            Some(ref cert) => evaluate_certificate_policy(cert, Vec::new(), trust, &mut diagnostics)?,
            None => {
                diagnostics.push("embedded-x509-certificate=missing-or-invalid".into());
                SignatureValidity::Unverifiable
            }
        };

        Ok(evidence_from_metadata(
            "xmldsig",
            validity,
            metadata,
            "same-document-references",
            diagnostics,
        ))
    }
}

#[derive(Debug, Clone)]
struct CmsStructure {
    signature_algorithm_oid: String,
    signer_count: usize,
}

fn parse_cms_structure(input: &[u8]) -> Result<CmsStructure, String> {
    let content = ContentInfo::from_der(input).map_err(|error| error.to_string())?;
    let signed_der = content
        .content
        .to_der()
        .map_err(|error| error.to_string())?;
    let signed = SignedData::from_der(&signed_der).map_err(|error| error.to_string())?;
    let signer = signed
        .signer_infos
        .0
        .iter()
        .next()
        .ok_or_else(|| "signedData has no signerInfo".to_owned())?;

    Ok(CmsStructure {
        signature_algorithm_oid: signer.signature_algorithm.oid.to_string(),
        signer_count: signed.signer_infos.0.len(),
    })
}

fn cms_algorithm_supported(oid: &str) -> bool {
    matches!(
        oid,
        "1.2.840.10045.4.3.2"
            | "1.2.840.113549.1.1.11"
            | "1.2.840.113549.1.1.1"
            | "1.3.101.112"
    )
}

#[derive(Debug, Clone)]
struct CertificateMetadata {
    signer_claim: Option<String>,
    certificate_subject: Option<String>,
    certificate_issuer: Option<String>,
    certificate_fingerprint: Option<String>,
}

fn evidence(
    kind: &str,
    validity: SignatureValidity,
    metadata: Option<CertificateMetadata>,
    covered_content: &str,
    validation_diagnostics: Vec<String>,
) -> SignatureEvidence {
    evidence_from_metadata(kind, validity, metadata, covered_content, validation_diagnostics)
}

fn evidence_from_metadata(
    kind: &str,
    validity: SignatureValidity,
    metadata: Option<CertificateMetadata>,
    covered_content: &str,
    validation_diagnostics: Vec<String>,
) -> SignatureEvidence {
    SignatureEvidence {
        kind: kind.to_owned(),
        validity,
        signer_claim: metadata.as_ref().and_then(|value| value.signer_claim.clone()),
        certificate_subject: metadata
            .as_ref()
            .and_then(|value| value.certificate_subject.clone()),
        certificate_issuer: metadata
            .as_ref()
            .and_then(|value| value.certificate_issuer.clone()),
        certificate_fingerprint: metadata
            .as_ref()
            .and_then(|value| value.certificate_fingerprint.clone()),
        signed_at: None,
        covered_content: Some(covered_content.to_owned()),
        validation_diagnostics,
    }
}

fn first_embedded_certificate(pkcs7: &Pkcs7) -> Option<X509> {
    pkcs7
        .signed()
        .and_then(|signed| signed.certificates())
        .and_then(|certificates| certificates.get(0))
        .map(ToOwned::to_owned)
}

fn embedded_certificates(pkcs7: &Pkcs7) -> Vec<X509> {
    pkcs7
        .signed()
        .and_then(|signed| signed.certificates())
        .map(|certificates| certificates.iter().map(ToOwned::to_owned).collect())
        .unwrap_or_default()
}

fn certificate_metadata(cert: &X509) -> Result<CertificateMetadata, PocError> {
    let certificate_der = cert
        .to_der()
        .map_err(|error| PocError::InvalidWorkerResult(format!("certificate DER: {error}")))?;
    let _rust_model = x509_cert::Certificate::from_der(&certificate_der)
        .map_err(|error| PocError::InvalidWorkerResult(format!("x509-cert parse: {error}")))?;

    let subject = name_common_name(cert, false);
    let issuer = name_common_name(cert, true);
    let fingerprint = cert
        .digest(MessageDigest::sha256())
        .map_err(|error| PocError::InvalidWorkerResult(format!("certificate digest: {error}")))?;

    Ok(CertificateMetadata {
        signer_claim: subject.clone(),
        certificate_subject: subject,
        certificate_issuer: issuer,
        certificate_fingerprint: Some(hex::encode(fingerprint)),
    })
}

fn name_common_name(cert: &X509, issuer: bool) -> Option<String> {
    let name = if issuer {
        cert.issuer_name()
    } else {
        cert.subject_name()
    };
    name.entries_by_nid(Nid::COMMONNAME)
        .next()
        .and_then(|entry| entry.data().as_utf8().ok())
        .map(|value| value.to_string())
}

fn evaluate_certificate_policy(
    signer: &X509,
    embedded: Vec<X509>,
    trust: &SignatureTrustContext,
    diagnostics: &mut Vec<String>,
) -> Result<SignatureValidity, PocError> {
    let now = Asn1Time::days_from_now(0)
        .map_err(|error| PocError::InvalidWorkerResult(format!("verification time: {error}")))?;

    if signer
        .not_before()
        .compare(&now)
        .map_err(|error| PocError::InvalidWorkerResult(format!("notBefore compare: {error}")))?
        == Ordering::Greater
    {
        diagnostics.push("certificate=not-yet-valid".into());
        return Ok(SignatureValidity::Invalid);
    }
    if signer
        .not_after()
        .compare(&now)
        .map_err(|error| PocError::InvalidWorkerResult(format!("notAfter compare: {error}")))?
        == Ordering::Less
    {
        diagnostics.push("certificate=expired".into());
        return Ok(SignatureValidity::Invalid);
    }

    match certificate_revocation_status(signer, trust, diagnostics)? {
        RevocationStatus::Revoked => return Ok(SignatureValidity::Invalid),
        RevocationStatus::Unverifiable => return Ok(SignatureValidity::Unverifiable),
        RevocationStatus::NotRevoked => {}
    }

    if trust.trusted_certificates_der.is_empty() {
        diagnostics.push("certificate-chain=no-trust-anchor".into());
        return Ok(SignatureValidity::Unverifiable);
    }

    let mut store_builder = X509StoreBuilder::new()
        .map_err(|error| PocError::InvalidWorkerResult(format!("X509 store: {error}")))?;
    for root_der in &trust.trusted_certificates_der {
        let root = X509::from_der(root_der).map_err(|error| {
            PocError::InvalidWorkerResult(format!("trusted certificate DER: {error}"))
        })?;
        store_builder
            .add_cert(root)
            .map_err(|error| PocError::InvalidWorkerResult(format!("add trust anchor: {error}")))?;
    }
    let store = store_builder.build();

    let mut chain = Stack::<X509>::new()
        .map_err(|error| PocError::InvalidWorkerResult(format!("X509 chain stack: {error}")))?;
    for certificate in embedded {
        if certificate.to_der().ok() == signer.to_der().ok() {
            continue;
        }
        chain
            .push(certificate)
            .map_err(|error| PocError::InvalidWorkerResult(format!("X509 chain push: {error}")))?;
    }

    let mut context = X509StoreContext::new()
        .map_err(|error| PocError::InvalidWorkerResult(format!("X509 store context: {error}")))?;
    let verified = context
        .init(&store, signer, &chain, |ctx| ctx.verify_cert())
        .map_err(|error| PocError::InvalidWorkerResult(format!("X509 path verify: {error}")))?;

    if verified {
        diagnostics.push("certificate-chain=valid".into());
        Ok(SignatureValidity::Valid)
    } else {
        diagnostics.push(format!("certificate-chain=unverifiable:{:?}", context.error()));
        Ok(SignatureValidity::Unverifiable)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RevocationStatus {
    NotRevoked,
    Revoked,
    Unverifiable,
}

fn certificate_revocation_status(
    signer: &X509,
    trust: &SignatureTrustContext,
    diagnostics: &mut Vec<String>,
) -> Result<RevocationStatus, PocError> {
    if trust.crls_der.is_empty() {
        return Ok(RevocationStatus::NotRevoked);
    }

    let signer_serial = signer
        .serial_number()
        .to_bn()
        .map_err(|error| PocError::InvalidWorkerResult(format!("signer serial: {error}")))?
        .to_vec();

    for crl_der in &trust.crls_der {
        let crl = X509Crl::from_der(crl_der)
            .map_err(|error| PocError::InvalidWorkerResult(format!("CRL DER: {error}")))?;

        let mut authenticated = false;
        for root_der in &trust.trusted_certificates_der {
            let root = X509::from_der(root_der).map_err(|error| {
                PocError::InvalidWorkerResult(format!("trusted certificate DER: {error}"))
            })?;
            let public_key = root.public_key().map_err(|error| {
                PocError::InvalidWorkerResult(format!("CRL issuer public key: {error}"))
            })?;
            if crl.verify(&public_key).map_err(|error| {
                PocError::InvalidWorkerResult(format!("CRL signature verification: {error}"))
            })? {
                authenticated = true;
                break;
            }
        }

        if !authenticated {
            diagnostics.push("offline-crl=unverifiable-signature".into());
            return Ok(RevocationStatus::Unverifiable);
        }

        if let Some(revoked) = crl.get_revoked() {
            for entry in revoked {
                let serial = entry
                    .serial_number()
                    .to_bn()
                    .map_err(|error| {
                        PocError::InvalidWorkerResult(format!("CRL serial: {error}"))
                    })?
                    .to_vec();
                if serial == signer_serial {
                    diagnostics.push("certificate=revoked-offline-crl".into());
                    return Ok(RevocationStatus::Revoked);
                }
            }
        }
    }

    diagnostics.push("offline-crl=authenticated-checked-not-revoked".into());
    Ok(RevocationStatus::NotRevoked)
}

fn parse_pdf_byte_range_signature(
    pdf: &[u8],
) -> Result<(Vec<u8>, Vec<u8>, String), &'static str> {
    const BYTE_RANGE: &[u8] = b"/ByteRange";
    const CONTENTS: &[u8] = b"/Contents";

    let byte_range_offset = find_subslice(pdf, BYTE_RANGE).ok_or("missing-byte-range")?;
    if find_subslice(&pdf[byte_range_offset + BYTE_RANGE.len()..], BYTE_RANGE).is_some() {
        return Err("multiple-byte-ranges");
    }

    let after_marker = &pdf[byte_range_offset + BYTE_RANGE.len()..];
    let open_relative = after_marker
        .iter()
        .position(|byte| *byte == b'[')
        .ok_or("missing-byte-range-array")?;
    let after_open = &after_marker[open_relative + 1..];
    let close_relative = after_open
        .iter()
        .position(|byte| *byte == b']')
        .ok_or("unterminated-byte-range-array")?;
    let array = &after_open[..close_relative];
    let text = std::str::from_utf8(array).map_err(|_| "non-ascii-byte-range")?;
    let numbers = text
        .split_ascii_whitespace()
        .map(|value| value.parse::<usize>().map_err(|_| "invalid-byte-range-number"))
        .collect::<Result<Vec<_>, _>>()?;
    if numbers.len() != 4 {
        return Err("byte-range-arity");
    }

    let first_start = numbers[0];
    let first_len = numbers[1];
    let second_start = numbers[2];
    let second_len = numbers[3];
    if first_start != 0 || first_len == 0 {
        return Err("unsupported-first-range");
    }

    let first_end = first_start
        .checked_add(first_len)
        .ok_or("byte-range-overflow")?;
    let second_end = second_start
        .checked_add(second_len)
        .ok_or("byte-range-overflow")?;
    if first_end >= second_start || second_end > pdf.len() {
        return Err("overlapping-or-out-of-bounds-byte-range");
    }

    let gap = &pdf[first_end..second_start];
    if gap.len() < 3 || gap.first() != Some(&b'<') || gap.last() != Some(&b'>') {
        return Err("byte-range-gap-is-not-contents");
    }
    let prefix_start = first_end.saturating_sub(256);
    if find_subslice(&pdf[prefix_start..first_end], CONTENTS).is_none() {
        return Err("contents-marker-not-adjacent");
    }

    let hex_text = gap[1..gap.len() - 1]
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    if hex_text.is_empty() || hex_text.len() % 2 != 0 {
        return Err("invalid-contents-hex-length");
    }
    let hex_text =
        std::str::from_utf8(&hex_text).map_err(|_| "non-ascii-contents-hex")?;
    let padded_der = hex::decode(hex_text).map_err(|_| "invalid-contents-hex")?;
    let der_len = der_sequence_total_len(&padded_der)?;
    if padded_der[der_len..].iter().any(|byte| *byte != 0) {
        return Err("nonzero-contents-padding");
    }
    let cms_der = padded_der[..der_len].to_vec();

    let mut covered = Vec::with_capacity(first_len + second_len);
    covered.extend_from_slice(&pdf[first_start..first_end]);
    covered.extend_from_slice(&pdf[second_start..second_end]);

    Ok((
        covered,
        cms_der,
        format!(
            "pdf-byte-range:{}+{};{}+{}",
            first_start, first_len, second_start, second_len
        ),
    ))
}

fn der_sequence_total_len(input: &[u8]) -> Result<usize, &'static str> {
    if input.len() < 2 || input[0] != 0x30 {
        return Err("contents-is-not-der-sequence");
    }

    let length_octet = input[1];
    let (header_len, payload_len) = if length_octet & 0x80 == 0 {
        (2usize, usize::from(length_octet))
    } else {
        let octets = usize::from(length_octet & 0x7f);
        if octets == 0 || octets > std::mem::size_of::<usize>() || input.len() < 2 + octets {
            return Err("invalid-der-length");
        }
        let mut payload_len = 0usize;
        for byte in &input[2..2 + octets] {
            payload_len = payload_len
                .checked_mul(256)
                .and_then(|value| value.checked_add(usize::from(*byte)))
                .ok_or("der-length-overflow")?;
        }
        (2 + octets, payload_len)
    };

    let total = header_len
        .checked_add(payload_len)
        .ok_or("der-length-overflow")?;
    if total > input.len() {
        return Err("truncated-der-contents");
    }
    Ok(total)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn extract_x509_certificate(xml: &str) -> Result<Option<Vec<u8>>, PocError> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut inside_certificate = false;
    let mut encoded = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.local_name().as_ref() == "X509Certificate" => {
                inside_certificate = true;
            }
            Ok(Event::End(event)) if event.local_name().as_ref() == "X509Certificate" => {
                break;
            }
            Ok(Event::Text(text)) if inside_certificate => {
                let decoded = String::from_utf8_lossy(text.as_ref());
                encoded.push_str(decoded.as_ref());
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(PocError::SemanticExtractionFailed(format!(
                    "XML signature parse: {error}"
                )));
            }
        }
    }

    let compact: String = encoded.chars().filter(|character| !character.is_whitespace()).collect();
    if compact.is_empty() {
        return Ok(None);
    }

    openssl::base64::decode_block(&compact)
        .map(Some)
        .map_err(|error| PocError::SemanticExtractionFailed(format!("X509Certificate base64: {error}")))
}

fn unsupported_crypto_error(detail: &str) -> bool {
    let detail = detail.to_ascii_lowercase();
    detail.contains("unsupported")
        || detail.contains("unknown algorithm")
        || detail.contains("unknown digest")
        || detail.contains("algorithm not supported")
}
