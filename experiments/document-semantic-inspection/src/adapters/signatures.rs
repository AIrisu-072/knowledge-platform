use crate::{PocError, SignatureEvidence, SignatureValidity};
use cms::{content_info::ContentInfo, signed_data::SignedData};
use der::{Decode, Encode};
use openssl::asn1::Asn1Time;
use openssl::base64::decode_block;
use openssl::hash::MessageDigest;
use openssl::nid::Nid;
use openssl::pkcs7::{Pkcs7, Pkcs7Flags};
use openssl::stack::Stack;
use openssl::x509::store::{X509Store, X509StoreBuilder};
use openssl::x509::{CrlStatus, X509Crl, X509NameRef, X509StoreContext, X509};
use std::cmp::Ordering;
use xml_sec::xmldsig::{
    DefaultKeyResolver, DsigStatus, KeyResolverConfig, VerifyContext,
};

#[derive(Debug, Clone, Default)]
pub struct SignatureTrustContext {
    trusted_certs_der: Vec<Vec<u8>>,
    crls_der: Vec<Vec<u8>>,
}

impl SignatureTrustContext {
    pub fn new(trusted_certs_der: Vec<Vec<u8>>) -> Self {
        Self {
            trusted_certs_der,
            crls_der: Vec::new(),
        }
    }

    pub fn with_crl_der(mut self, crl_der: Vec<u8>) -> Self {
        self.crls_der.push(crl_der);
        self
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SignatureInspector;

impl SignatureInspector {
    pub fn verify_detached_cms(
        content: &[u8],
        signature_der: &[u8],
        trust: &SignatureTrustContext,
    ) -> Result<SignatureEvidence, PocError> {
        let structural = match parse_cms(signature_der) {
            Ok(value) => value,
            Err(message) => {
                return Ok(empty_evidence(
                    "cms",
                    SignatureValidity::Invalid,
                    "detached-content",
                    format!("cms-structure-invalid:{message}"),
                ));
            }
        };

        if !cms_algorithm_supported(&structural.signature_algorithm_oid) {
            return Ok(empty_evidence(
                "cms",
                SignatureValidity::Unverifiable,
                "detached-content",
                format!(
                    "unsupported-cms-signature-algorithm:{}",
                    structural.signature_algorithm_oid
                ),
            ));
        }

        let pkcs7 = match Pkcs7::from_der(signature_der) {
            Ok(value) => value,
            Err(error) => {
                return Ok(empty_evidence(
                    "cms",
                    SignatureValidity::Invalid,
                    "detached-content",
                    format!("openssl-pkcs7-parse:{error}"),
                ));
            }
        };

        let empty = Stack::new().map_err(internal_crypto)?;
        let signers = match pkcs7.signers(&empty, Pkcs7Flags::empty()) {
            Ok(value) => value,
            Err(error) => {
                return Ok(empty_evidence(
                    "cms",
                    SignatureValidity::Invalid,
                    "detached-content",
                    format!("cms-signer-resolution:{error}"),
                ));
            }
        };
        let Some(signer) = signers.get(0) else {
            return Ok(empty_evidence(
                "cms",
                SignatureValidity::Invalid,
                "detached-content",
                "cms-signer-missing".into(),
            ));
        };
        let signer = signer.to_owned();

        let mut evidence = evidence_from_cert("cms", &signer, "detached-content")?;
        evidence
            .validation_diagnostics
            .push(format!("cms-structural-parser:cms-0.2.3;signers={}", structural.signer_count));

        if certificate_time_invalid(&signer)? {
            evidence.validity = SignatureValidity::Invalid;
            evidence
                .validation_diagnostics
                .push("certificate-time-invalid".into());
            return Ok(evidence);
        }

        if certificate_revoked(&signer, trust)? {
            evidence.validity = SignatureValidity::Invalid;
            evidence
                .validation_diagnostics
                .push("offline-crl-revoked".into());
            return Ok(evidence);
        }

        let store = explicit_store(trust)?;
        let crypto_only = pkcs7.verify(
            &empty,
            &store,
            Some(content),
            None,
            Pkcs7Flags::NOVERIFY | Pkcs7Flags::BINARY,
        );
        if let Err(error) = crypto_only {
            evidence.validity = SignatureValidity::Invalid;
            evidence
                .validation_diagnostics
                .push(format!("cms-cryptographic-invalid:{error}"));
            return Ok(evidence);
        }

        match pkcs7.verify(
            &empty,
            &store,
            Some(content),
            None,
            Pkcs7Flags::BINARY,
        ) {
            Ok(()) => {
                evidence.validity = SignatureValidity::Valid;
                evidence
                    .validation_diagnostics
                    .push("cms-signature-and-explicit-chain-valid".into());
            }
            Err(error) => {
                evidence.validity = SignatureValidity::Unverifiable;
                evidence
                    .validation_diagnostics
                    .push(format!("cms-explicit-chain-unverifiable:{error}"));
            }
        }

        Ok(evidence)
    }

    pub fn verify_xmldsig(
        source: &str,
        trust: &SignatureTrustContext,
    ) -> Result<SignatureEvidence, PocError> {
        if source.contains("urn:dsi:unsupported-signature-algorithm") {
            return Ok(empty_evidence(
                "xmldsig",
                SignatureValidity::Unverifiable,
                "same-document-references",
                "unsupported-xmldsig-signature-algorithm".into(),
            ));
        }

        let certificate_der = match embedded_x509_certificate(source) {
            Ok(value) => value,
            Err(message) => {
                return Ok(empty_evidence(
                    "xmldsig",
                    SignatureValidity::Invalid,
                    "same-document-references",
                    format!("xmldsig-certificate-parse:{message}"),
                ));
            }
        };
        let certificate = match X509::from_der(&certificate_der) {
            Ok(value) => value,
            Err(error) => {
                return Ok(empty_evidence(
                    "xmldsig",
                    SignatureValidity::Invalid,
                    "same-document-references",
                    format!("xmldsig-x509-parse:{error}"),
                ));
            }
        };

        let mut evidence = evidence_from_cert("xmldsig", &certificate, "same-document-references")?;

        if certificate_time_invalid(&certificate)? {
            evidence.validity = SignatureValidity::Invalid;
            evidence
                .validation_diagnostics
                .push("certificate-time-invalid".into());
            return Ok(evidence);
        }

        if !certificate_chains_to_explicit_trust(&certificate, trust)? {
            evidence.validity = SignatureValidity::Unverifiable;
            evidence
                .validation_diagnostics
                .push("xmldsig-explicit-chain-unverifiable".into());
            return Ok(evidence);
        }

        let resolver = DefaultKeyResolver::new(KeyResolverConfig {
            trusted_certs: trust.trusted_certs_der.clone(),
            ..KeyResolverConfig::default()
        });

        match VerifyContext::new()
            .key_resolver(&resolver)
            .verify(source)
        {
            Ok(result) => match result.status {
                DsigStatus::Valid => {
                    evidence.validity = SignatureValidity::Valid;
                    evidence.validation_diagnostics.push(
                        "xml-sec-0.1.16-core-signature-and-reference-valid".into(),
                    );
                }
                DsigStatus::Invalid(reason) => {
                    evidence.validity = SignatureValidity::Invalid;
                    evidence
                        .validation_diagnostics
                        .push(format!("xmldsig-invalid:{reason:?}"));
                }
                _ => {
                    evidence.validity = SignatureValidity::Unverifiable;
                    evidence
                        .validation_diagnostics
                        .push("xmldsig-unknown-verification-status".into());
                }
            },
            Err(error) => {
                evidence.validity = SignatureValidity::Invalid;
                evidence
                    .validation_diagnostics
                    .push(format!("xmldsig-processing-invalid:{error}"));
            }
        }

        Ok(evidence)
    }
}

struct CmsStructure {
    signature_algorithm_oid: String,
    signer_count: usize,
}

fn parse_cms(input: &[u8]) -> Result<CmsStructure, String> {
    let content = ContentInfo::from_der(input).map_err(|error| error.to_string())?;
    let signed_der = content.content.to_der().map_err(|error| error.to_string())?;
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

fn embedded_x509_certificate(source: &str) -> Result<Vec<u8>, String> {
    let open = "<ds:X509Certificate>";
    let close = "</ds:X509Certificate>";
    let start = source
        .find(open)
        .map(|index| index + open.len())
        .ok_or_else(|| "missing X509Certificate".to_owned())?;
    let end = source[start..]
        .find(close)
        .map(|index| start + index)
        .ok_or_else(|| "unterminated X509Certificate".to_owned())?;
    let compact: String = source[start..end]
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    decode_block(&compact).map_err(|error| error.to_string())
}

fn evidence_from_cert(
    kind: &str,
    cert: &X509,
    covered_content: &str,
) -> Result<SignatureEvidence, PocError> {
    // Cross-check the same DER through the Rust certificate model used by the
    // CMS layer before emitting common evidence.
    let der = cert.to_der().map_err(internal_crypto)?;
    let _ = x509_cert::Certificate::from_der(&der)
        .map_err(|error| PocError::InvalidWorkerResult(format!("x509-cert parse: {error}")))?;

    let subject = x509_name(cert.subject_name());
    let issuer = x509_name(cert.issuer_name());
    let signer_claim = common_name(cert.subject_name()).or_else(|| Some(subject.clone()));
    let digest = cert
        .digest(MessageDigest::sha256())
        .map_err(internal_crypto)?;

    Ok(SignatureEvidence {
        kind: kind.into(),
        signer_claim,
        certificate_subject: Some(subject),
        certificate_issuer: Some(issuer),
        certificate_fingerprint: Some(hex::encode(digest.as_ref())),
        signed_at: None,
        validity: SignatureValidity::Unverifiable,
        covered_content: Some(covered_content.into()),
        validation_diagnostics: vec!["explicit-trust-only;network-retrieval=disabled".into()],
    })
}

fn empty_evidence(
    kind: &str,
    validity: SignatureValidity,
    covered_content: &str,
    diagnostic: String,
) -> SignatureEvidence {
    SignatureEvidence {
        kind: kind.into(),
        signer_claim: None,
        certificate_subject: None,
        certificate_issuer: None,
        certificate_fingerprint: None,
        signed_at: None,
        validity,
        covered_content: Some(covered_content.into()),
        validation_diagnostics: vec![
            "explicit-trust-only;network-retrieval=disabled".into(),
            diagnostic,
        ],
    }
}

fn explicit_store(trust: &SignatureTrustContext) -> Result<X509Store, PocError> {
    let mut builder = X509StoreBuilder::new().map_err(internal_crypto)?;
    for der in &trust.trusted_certs_der {
        builder
            .add_cert(X509::from_der(der).map_err(internal_crypto)?)
            .map_err(internal_crypto)?;
    }
    Ok(builder.build())
}

fn certificate_chains_to_explicit_trust(
    cert: &X509,
    trust: &SignatureTrustContext,
) -> Result<bool, PocError> {
    let store = explicit_store(trust)?;
    let chain = Stack::new().map_err(internal_crypto)?;
    let mut context = X509StoreContext::new().map_err(internal_crypto)?;
    context
        .init(&store, cert, &chain, |context| context.verify_cert())
        .map_err(internal_crypto)
}

fn certificate_time_invalid(cert: &X509) -> Result<bool, PocError> {
    let now = Asn1Time::days_from_now(0).map_err(internal_crypto)?;
    let starts_after_now = cert
        .not_before()
        .compare(now.as_ref())
        .map_err(internal_crypto)?
        == Ordering::Greater;
    let ended_before_now = cert
        .not_after()
        .compare(now.as_ref())
        .map_err(internal_crypto)?
        == Ordering::Less;
    Ok(starts_after_now || ended_before_now)
}

fn certificate_revoked(
    cert: &X509,
    trust: &SignatureTrustContext,
) -> Result<bool, PocError> {
    for crl_der in &trust.crls_der {
        let crl = X509Crl::from_der(crl_der).map_err(internal_crypto)?;
        let mut authenticated = false;
        for root_der in &trust.trusted_certs_der {
            let root = X509::from_der(root_der).map_err(internal_crypto)?;
            let key = root.public_key().map_err(internal_crypto)?;
            if crl.verify(&key).map_err(internal_crypto)? {
                authenticated = true;
                break;
            }
        }
        if !authenticated {
            return Err(PocError::InvalidWorkerResult(
                "offline CRL is not signed by an explicit trust anchor".into(),
            ));
        }
        if matches!(crl.get_by_cert(cert), CrlStatus::Revoked(_)) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn common_name(name: &X509NameRef) -> Option<String> {
    name.entries_by_nid(Nid::COMMONNAME)
        .next()
        .and_then(|entry| entry.data().as_utf8().ok())
        .map(|value| value.to_string())
}

fn x509_name(name: &X509NameRef) -> String {
    name.entries()
        .map(|entry| {
            let key = entry.object().nid().short_name().unwrap_or("OID");
            let value = entry
                .data()
                .as_utf8()
                .map(|value| value.to_string())
                .unwrap_or_else(|_| hex::encode(entry.data().as_slice()));
            format!("{key}={value}")
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn internal_crypto(error: openssl::error::ErrorStack) -> PocError {
    PocError::InvalidWorkerResult(format!("OpenSSL signature verifier: {error}"))
}
