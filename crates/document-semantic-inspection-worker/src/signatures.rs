use crate::{WorkerFailure, WorkerFailureCode};
use cms::{content_info::ContentInfo, signed_data::SignedData};
use document_semantic_inspection_core::{DigitalSignatureEvidence, SignatureValidity};
use lopdf::{Dictionary, Document, Object};
use openssl::asn1::Asn1Time;
use openssl::hash::MessageDigest;
use openssl::nid::Nid;
use openssl::pkcs7::{Pkcs7, Pkcs7Flags};
use openssl::stack::Stack;
use openssl::x509::store::X509StoreBuilder;
use openssl::x509::{X509, X509Crl, X509StoreContext};
use quick_xml::events::{BytesStart, Event};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};
use x509_cert::der::{Decode, Encode};
use xml_sec::xmldsig::{
    DefaultKeyResolver, DsigStatus, ReferenceResult, SignatureAlgorithm, UriTypeSet,
    VerificationKey, VerifyContext,
};
use zip::ZipArchive;

#[derive(Debug, thiserror::Error)]
pub enum SignatureError {
    #[error("signature inspection resource limit exceeded")]
    InspectionResourceLimitExceeded,
    #[error("invalid signature result: {0}")]
    InvalidWorkerResult(String),
    #[error("signature extraction failed: {0}")]
    SemanticExtractionFailed(String),
    #[error("unsupported signature construct: {0}")]
    UnsupportedSemanticConstruct(String),
}

impl From<SignatureError> for WorkerFailure {
    fn from(error: SignatureError) -> Self {
        let code = match error {
            SignatureError::InspectionResourceLimitExceeded => {
                WorkerFailureCode::InspectionResourceLimitExceeded
            }
            SignatureError::InvalidWorkerResult(_) => WorkerFailureCode::InvalidWorkerResult,
            SignatureError::SemanticExtractionFailed(_) => {
                WorkerFailureCode::SemanticExtractionFailed
            }
            SignatureError::UnsupportedSemanticConstruct(_) => {
                WorkerFailureCode::UnsupportedSemanticConstruct
            }
        };
        WorkerFailure::new(code, error.to_string())
    }
}

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
    ) -> Result<DigitalSignatureEvidence, SignatureError> {
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
        if structural.signer_count != 1 {
            diagnostics.push("cms-signer-count=unsupported".into());
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

        let empty_certs = Stack::<X509>::new().map_err(|error| {
            SignatureError::InvalidWorkerResult(format!("OpenSSL stack: {error}"))
        })?;
        let signer = match pkcs7.signers(&empty_certs, Pkcs7Flags::empty()) {
            Ok(signers) if signers.len() == 1 => signers.get(0).map(ToOwned::to_owned),
            Ok(_) => None,
            Err(error) => {
                diagnostics.push(format!("cms-signer-certificate-resolution:{error}"));
                None
            }
        };
        let metadata = signer.as_ref().map(certificate_metadata).transpose()?;
        let empty_store = X509StoreBuilder::new()
            .map_err(|error| {
                SignatureError::InvalidWorkerResult(format!("OpenSSL store: {error}"))
            })?
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
            Some(ref signer) => evaluate_certificate_policy(
                signer,
                embedded_certificates(&pkcs7),
                trust,
                &mut diagnostics,
            )?,
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
    ) -> Result<DigitalSignatureEvidence, SignatureError> {
        Ok(Self::inspect_pdf_signatures(pdf, trust)?
            .into_iter()
            .next()
            .unwrap_or_else(|| pdf_invalid_evidence("missing-signature-field")))
    }

    /// Finds PDF signature dictionaries only through the AcroForm field tree.
    pub fn inspect_pdf_signatures(
        pdf: &[u8],
        trust: &SignatureTrustContext,
    ) -> Result<Vec<DigitalSignatureEvidence>, SignatureError> {
        let document = Document::load_mem(pdf).map_err(|error| {
            SignatureError::SemanticExtractionFailed(format!("PDF signature parse: {error}"))
        })?;
        let catalog = document.catalog().map_err(|error| {
            SignatureError::SemanticExtractionFailed(format!("PDF catalog: {error}"))
        })?;
        let Ok(acroform) = catalog.get(b"AcroForm") else {
            return Ok(Vec::new());
        };
        let acroform = document
            .dereference(acroform)
            .map_err(|error| {
                SignatureError::SemanticExtractionFailed(format!("PDF AcroForm: {error}"))
            })?
            .1
            .as_dict()
            .map_err(|error| {
                SignatureError::SemanticExtractionFailed(format!("PDF AcroForm: {error}"))
            })?;
        let fields = acroform.get(b"Fields").map_err(|error| {
            SignatureError::SemanticExtractionFailed(format!("PDF AcroForm fields: {error}"))
        })?;
        let fields = document
            .dereference(fields)
            .map_err(|error| {
                SignatureError::SemanticExtractionFailed(format!("PDF AcroForm fields: {error}"))
            })?
            .1
            .as_array()
            .map_err(|error| {
                SignatureError::SemanticExtractionFailed(format!("PDF AcroForm fields: {error}"))
            })?;
        let mut evidence = Vec::new();
        for field in fields {
            inspect_pdf_field(&document, field, pdf, trust, 0, &mut evidence)?;
        }
        Ok(evidence)
    }

    fn verify_pdf_signature_dict(
        pdf: &[u8],
        signature: &Dictionary,
        trust: &SignatureTrustContext,
    ) -> Result<DigitalSignatureEvidence, SignatureError> {
        let (covered, cms_der, coverage) = match parse_pdf_byte_range_signature(pdf, signature) {
            Ok(value) => value,
            Err(reason) => {
                return Ok(pdf_invalid_evidence(reason));
            }
        };

        let mut evidence = Self::verify_detached_cms(&covered, &cms_der, trust)?;
        evidence.signature_type = "pdf-cms".into();
        evidence.covered_content = vec![coverage];
        evidence
            .validation_diagnostics
            .push("pdf-byte-range-structure-valid".into());
        Ok(evidence)
    }

    pub fn verify_ooxml_package(
        package: &[u8],
        trust: &SignatureTrustContext,
    ) -> Result<Vec<DigitalSignatureEvidence>, SignatureError> {
        const ORIGIN_REL: &str =
            "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/origin";
        const SIGNATURE_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/signature";

        let mut archive = ZipArchive::new(Cursor::new(package)).map_err(|error| {
            SignatureError::SemanticExtractionFailed(format!("OOXML signature package: {error}"))
        })?;
        if archive.len() > 20_000 {
            return Err(SignatureError::InspectionResourceLimitExceeded);
        }

        let has_signature_parts = (0..archive.len()).any(|index| {
            archive.by_index(index).ok().is_some_and(|file| {
                file.name().starts_with("_xmlsignatures/") && file.name().ends_with(".xml")
            })
        });

        let root_rels = match read_zip_text(&mut archive, "_rels/.rels")? {
            Some(value) => value,
            None if has_signature_parts => {
                return Ok(vec![ooxml_invalid_evidence(
                    "signature-parts-present-without-package-relationships",
                )]);
            }
            None => return Ok(Vec::new()),
        };

        let origins = relationship_targets(&root_rels, ORIGIN_REL)?;
        if origins.is_empty() {
            return if has_signature_parts {
                Ok(vec![ooxml_invalid_evidence(
                    "signature-parts-present-without-origin-relationship",
                )])
            } else {
                Ok(Vec::new())
            };
        }
        if origins.len() != 1 {
            return Ok(vec![ooxml_invalid_evidence(
                "multiple-digital-signature-origins",
            )]);
        }

        let origin = resolve_opc_target("", &origins[0])?;
        let origin_rels = relationship_part_name(&origin)?;
        let Some(origin_rels_xml) = read_zip_text(&mut archive, &origin_rels)? else {
            return Ok(vec![ooxml_invalid_evidence(
                "digital-signature-origin-relationships-missing",
            )]);
        };

        let mut signature_targets = relationship_targets(&origin_rels_xml, SIGNATURE_REL)?;
        signature_targets.sort();
        signature_targets.dedup();
        if signature_targets.is_empty() {
            return Ok(vec![ooxml_invalid_evidence(
                "digital-signature-origin-has-no-signatures",
            )]);
        }

        let mut evidence = Vec::with_capacity(signature_targets.len());
        for target in signature_targets {
            let part = resolve_opc_target(&origin, &target)?;
            let Some(xml) = read_zip_text(&mut archive, &part)? else {
                evidence.push(ooxml_invalid_evidence(
                    "digital-signature-relationship-target-missing",
                ));
                continue;
            };
            let references = match ooxml_manifest_references(&xml) {
                Ok(references) => references,
                Err(reason) => {
                    evidence.push(ooxml_unverifiable_evidence(&reason));
                    continue;
                }
            };
            let payloads = match ooxml_manifest_payloads(&mut archive, &references) {
                Ok(payloads) => payloads,
                Err(reason) => {
                    evidence.push(ooxml_unverifiable_evidence(&reason));
                    continue;
                }
            };
            let (mut item, manifest_results) = verify_xmldsig_inner(&xml, trust, Some(&payloads))?;
            item.signature_type = "ooxml-xmldsig".into();
            item.covered_content.clear();
            item.validation_diagnostics
                .push("opc-digital-signature-relationship-chain=valid".into());
            if manifest_results.len() != references.len() || references.is_empty() {
                if item.cryptographic_validity == SignatureValidity::Valid {
                    item.cryptographic_validity = SignatureValidity::Unverifiable;
                }
                item.validation_diagnostics
                    .push("opc-package-part-coverage=no-authenticated-manifest-references".into());
            } else if manifest_results
                .iter()
                .zip(&references)
                .any(|(result, reference)| {
                    result.uri != reference.uri || !matches!(result.status, DsigStatus::Valid)
                })
            {
                item.cryptographic_validity = SignatureValidity::Invalid;
                item.validation_diagnostics
                    .push("opc-package-part-digest=mismatch".into());
            } else if item.cryptographic_validity == SignatureValidity::Valid {
                item.covered_content = references
                    .iter()
                    .map(|reference| reference.part.clone())
                    .collect();
                item.validation_diagnostics
                    .push("opc-package-part-digest=valid".into());
            }
            evidence.push(item);
        }
        Ok(evidence)
    }

    pub fn verify_xmldsig(
        xml: &str,
        trust: &SignatureTrustContext,
    ) -> Result<DigitalSignatureEvidence, SignatureError> {
        verify_xmldsig_inner(xml, trust, None).map(|(evidence, _)| evidence)
    }
}

fn verify_xmldsig_inner(
    xml: &str,
    trust: &SignatureTrustContext,
    ooxml_payloads: Option<&HashMap<String, Vec<u8>>>,
) -> Result<(DigitalSignatureEvidence, Vec<ReferenceResult>), SignatureError> {
    let mut diagnostics = vec!["xmldsig=xml-sec-0.1.16".to_owned()];
    if xml.len() > 8 * 1024 * 1024 {
        return Err(SignatureError::InspectionResourceLimitExceeded);
    }
    if ooxml_payloads.is_none() && has_external_xmldsig_reference(xml) {
        diagnostics.push("xmldsig-external-reference=blocked".into());
        return Ok((
            evidence(
                "xmldsig",
                SignatureValidity::Unverifiable,
                None,
                "same-document-references",
                diagnostics,
            ),
            Vec::new(),
        ));
    }
    let cert_der = extract_x509_certificate(xml)?;
    let cert = cert_der
        .as_deref()
        .and_then(|bytes| X509::from_der(bytes).ok());
    let metadata = cert.as_ref().map(certificate_metadata).transpose()?;

    let resolver = DefaultKeyResolver::default();
    let bound_key = if let Some(ref cert) = cert {
        let algorithm = match signature_algorithm(xml) {
            Ok(algorithm) => algorithm,
            Err(reason) => {
                diagnostics.push(format!("xmldsig-signature-method:{reason}"));
                let validity = if reason == "unsupported-algorithm" {
                    SignatureValidity::Unverifiable
                } else {
                    SignatureValidity::Invalid
                };
                return Ok((
                    evidence_from_metadata(
                        "xmldsig",
                        validity,
                        metadata,
                        "same-document-references",
                        diagnostics,
                    ),
                    Vec::new(),
                ));
            }
        };
        let public_key_bytes = cert
            .public_key()
            .and_then(|key| key.public_key_to_der())
            .map_err(|error| {
                SignatureError::InvalidWorkerResult(format!("XML signer public key: {error}"))
            })?;
        Some(VerificationKey {
            algorithm,
            public_key_bytes,
            certificate_der: cert_der.clone(),
            name: None,
        })
    } else {
        None
    };
    let mut context = VerifyContext::new();
    context = if let Some(ref key) = bound_key {
        context.key(key)
    } else {
        context.key_resolver(&resolver)
    };
    if let Some(payloads) = ooxml_payloads {
        context = context
            .allowed_uri_types(UriTypeSet::ALL)
            .external_resources(payloads)
            .process_manifests(true);
    }
    let verification = context.verify(xml);

    let manifest_references = match verification {
        Ok(result) => {
            let manifest_references = result.manifest_references;
            match result.status {
                DsigStatus::Valid => {
                    diagnostics.push("xmldsig-cryptographic-verification=valid".into());
                }
                DsigStatus::Invalid(reason) => {
                    diagnostics.push(format!("xmldsig-invalid:{reason:?}"));
                    return Ok((
                        evidence_from_metadata(
                            "xmldsig",
                            SignatureValidity::Invalid,
                            metadata,
                            "same-document-references",
                            diagnostics,
                        ),
                        manifest_references,
                    ));
                }
                other => {
                    diagnostics.push(format!("xmldsig-unverifiable:{other:?}"));
                    return Ok((
                        evidence_from_metadata(
                            "xmldsig",
                            SignatureValidity::Unverifiable,
                            metadata,
                            "same-document-references",
                            diagnostics,
                        ),
                        manifest_references,
                    ));
                }
            }
            manifest_references
        }
        Err(error) => {
            let detail = error.to_string();
            diagnostics.push(format!("xmldsig-error:{detail}"));
            let validity = if unsupported_crypto_error(&detail) {
                SignatureValidity::Unverifiable
            } else {
                SignatureValidity::Invalid
            };
            return Ok((
                evidence_from_metadata(
                    "xmldsig",
                    validity,
                    metadata,
                    "same-document-references",
                    diagnostics,
                ),
                Vec::new(),
            ));
        }
    };

    let validity = match cert {
        Some(ref cert) => evaluate_certificate_policy(cert, Vec::new(), trust, &mut diagnostics)?,
        None => {
            diagnostics.push("embedded-x509-certificate=missing-or-invalid".into());
            SignatureValidity::Unverifiable
        }
    };

    Ok((
        evidence_from_metadata(
            "xmldsig",
            validity,
            metadata,
            "same-document-references",
            diagnostics,
        ),
        manifest_references,
    ))
}

fn has_external_xmldsig_reference(xml: &str) -> bool {
    let mut reader = quick_xml::Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event)) => {
                for attribute in event.attributes() {
                    let Ok(attribute) = attribute else {
                        return true;
                    };
                    if attribute.key.as_ref() == "xml:base" {
                        return true;
                    }
                    if event.local_name().as_ref() == "Reference"
                        && attribute.key.local_name().as_ref() == "URI"
                        && !attribute.value.is_empty()
                        && !attribute.value.starts_with('#')
                    {
                        return true;
                    }
                }
            }
            Ok(Event::DocType(_)) => return true,
            Ok(Event::Eof) | Err(_) => return false,
            Ok(_) => {}
        }
    }
}

fn signature_algorithm(xml: &str) -> Result<SignatureAlgorithm, &'static str> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<String> = Vec::new();
    let mut algorithm = None;
    loop {
        let event = reader.read_event().map_err(|_| "malformed-signature-xml")?;
        let (start, empty) = match &event {
            Event::Start(start) => (Some(start), false),
            Event::Empty(start) => (Some(start), true),
            _ => (None, false),
        };
        if let Some(start) = start {
            let name = start.local_name().as_ref().to_owned();
            if name == "SignatureMethod" && stack.last().map(String::as_str) == Some("SignedInfo") {
                if algorithm.is_some() {
                    return Err("duplicate-signature-method");
                }
                let uri = xml_attribute(start, "Algorithm")
                    .map_err(|_| "malformed-signature-method")?
                    .ok_or("missing-signature-algorithm")?;
                algorithm =
                    Some(SignatureAlgorithm::from_uri(&uri).ok_or("unsupported-algorithm")?);
            }
            if !empty {
                stack.push(name.to_owned());
            }
        }
        match event {
            Event::End(_) => {
                if stack.pop().is_none() {
                    return Err("mismatched-signature-xml");
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    algorithm.ok_or("missing-signature-method")
}

#[derive(Debug, Clone)]
struct CmsStructure {
    signature_algorithm_oid: String,
    signer_count: usize,
}

fn ooxml_invalid_evidence(reason: &str) -> DigitalSignatureEvidence {
    evidence(
        "ooxml-xmldsig",
        SignatureValidity::Invalid,
        None,
        "ooxml-signature-package",
        vec![
            "explicit-trust-only;network-retrieval=disabled".into(),
            format!("ooxml-signature-invalid:{reason}"),
        ],
    )
}

fn ooxml_unverifiable_evidence(reason: &str) -> DigitalSignatureEvidence {
    evidence(
        "ooxml-xmldsig",
        SignatureValidity::Unverifiable,
        None,
        "ooxml-signature-package",
        vec![
            "explicit-trust-only;network-retrieval=disabled".into(),
            format!("opc-package-part-coverage=unverifiable:{reason}"),
        ],
    )
}

#[derive(Debug, Clone)]
struct OpcManifestReference {
    uri: String,
    part: String,
}

/// Only the raw package-part profile is admitted here. Unsupported OPC
/// relationship transforms remain explicit Unverifiable evidence.
fn ooxml_manifest_references(xml: &str) -> Result<Vec<OpcManifestReference>, String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<String> = Vec::new();
    let mut references = Vec::new();
    loop {
        let event = reader.read_event().map_err(|error| error.to_string())?;
        let (start, empty) = match &event {
            Event::Start(start) => (Some(start), false),
            Event::Empty(start) => (Some(start), true),
            _ => (None, false),
        };
        if let Some(start) = start {
            let name = start.local_name().as_ref().to_owned();
            for attribute in start.attributes() {
                let attribute = attribute.map_err(|error| error.to_string())?;
                if attribute.key.as_ref() == "xml:base" {
                    return Err("xml-base-disallowed".into());
                }
            }
            if name == "Reference" {
                let uri = xml_attribute(start, "URI")
                    .map_err(|error| error.to_string())?
                    .ok_or("reference-uri-missing")?;
                match stack.last().map(String::as_str) {
                    Some("SignedInfo") if uri.starts_with('#') => {}
                    Some("SignedInfo") => return Err("signed-info-external-reference".into()),
                    Some("Manifest") => {
                        let (part, _) = parse_opc_part_uri(&uri)?;
                        references.push(OpcManifestReference { uri, part });
                        if references.len() > 64 {
                            return Err("too-many-manifest-references".into());
                        }
                    }
                    _ => return Err("reference-outside-signed-info-or-manifest".into()),
                }
            }
            if name == "Transform" && stack.iter().any(|item| item == "Manifest") {
                return Err("unsupported-opc-manifest-transform".into());
            }
            if name == "DigestMethod" && stack.iter().any(|item| item == "Manifest") {
                let algorithm = xml_attribute(start, "Algorithm")
                    .map_err(|error| error.to_string())?
                    .ok_or("manifest-digest-method-missing")?;
                if algorithm != "http://www.w3.org/2001/04/xmlenc#sha256" {
                    return Err("unsupported-opc-manifest-digest".into());
                }
            }
            if !empty {
                stack.push(name);
            }
        }
        match event {
            Event::End(end) => {
                let name = end.local_name().as_ref().to_owned();
                if stack.pop().as_deref() != Some(name.as_str()) {
                    return Err("mismatched-signature-xml-element".into());
                }
            }
            Event::DocType(_) => return Err("signature-doctype-disallowed".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err("unterminated-signature-xml".into());
    }
    Ok(references)
}

fn parse_opc_part_uri(uri: &str) -> Result<(String, String), String> {
    let (path, content_type) = uri
        .split_once("?ContentType=")
        .ok_or("opc-part-uri-missing-content-type")?;
    if content_type.is_empty() || content_type.contains(['?', '#', '%', '\\']) {
        return Err("invalid-opc-content-type-uri".into());
    }
    let part = path
        .strip_prefix('/')
        .ok_or("opc-part-uri-not-package-absolute")?;
    if !safe_opc_part_path(part) {
        return Err("unsafe-opc-part-uri".into());
    }
    Ok((part.to_owned(), content_type.to_owned()))
}

fn safe_opc_part_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['%', '?', '#', '\\', ':'])
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
        && path.bytes().all(|byte| byte.is_ascii_graphic())
}

fn ooxml_manifest_payloads(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    references: &[OpcManifestReference],
) -> Result<HashMap<String, Vec<u8>>, String> {
    if archive.len() > 20_000 {
        return Err("opc-package-entry-limit".into());
    }
    let mut names = HashSet::new();
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|error| error.to_string())?
            .name()
            .to_owned();
        if !names.insert(name.clone()) || !safe_opc_part_path(name.trim_end_matches('/')) {
            return Err("ambiguous-opc-part-index".into());
        }
    }
    let content_types_xml = read_zip_text(archive, "[Content_Types].xml")
        .map_err(|error| error.to_string())?
        .ok_or("opc-content-types-missing")?;
    let content_types = OpcContentTypes::parse(&content_types_xml)?;
    let mut payloads = HashMap::new();
    let mut total = 0usize;
    for reference in references {
        let (_, claimed_content_type) = parse_opc_part_uri(&reference.uri)?;
        if content_types.for_part(&reference.part).as_deref() != Some(&claimed_content_type) {
            return Err("opc-part-content-type-mismatch".into());
        }
        if payloads.contains_key(&reference.uri) {
            return Err("duplicate-opc-manifest-reference".into());
        }
        let mut file = archive
            .by_name(&reference.part)
            .map_err(|_| "opc-manifest-part-missing")?;
        if file.size() > 8 * 1024 * 1024 {
            return Err("opc-manifest-part-size-limit".into());
        }
        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.by_ref()
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("opc-manifest-part-size-limit".into());
        }
        total = total
            .checked_add(bytes.len())
            .ok_or("opc-manifest-total-size-limit")?;
        if total > 32 * 1024 * 1024 {
            return Err("opc-manifest-total-size-limit".into());
        }
        payloads.insert(reference.uri.clone(), bytes);
    }
    Ok(payloads)
}

#[derive(Default)]
struct OpcContentTypes {
    defaults: HashMap<String, String>,
    overrides: HashMap<String, String>,
}

impl OpcContentTypes {
    fn parse(xml: &str) -> Result<Self, String> {
        let mut reader = quick_xml::Reader::from_str(xml);
        let mut result = Self::default();
        loop {
            match reader.read_event().map_err(|error| error.to_string())? {
                Event::Start(event) | Event::Empty(event) => match event.local_name().as_ref() {
                    "Default" => {
                        let extension = xml_attribute(&event, "Extension")
                            .map_err(|error| error.to_string())?
                            .ok_or("content-type-default-extension-missing")?;
                        let content_type = xml_attribute(&event, "ContentType")
                            .map_err(|error| error.to_string())?
                            .ok_or("content-type-default-value-missing")?;
                        if result.defaults.insert(extension, content_type).is_some() {
                            return Err("duplicate-opc-default-content-type".into());
                        }
                    }
                    "Override" => {
                        let part = xml_attribute(&event, "PartName")
                            .map_err(|error| error.to_string())?
                            .ok_or("content-type-override-part-missing")?;
                        let part = part
                            .strip_prefix('/')
                            .ok_or("content-type-override-not-absolute")?;
                        if !safe_opc_part_path(part) {
                            return Err("unsafe-opc-content-type-override".into());
                        }
                        let content_type = xml_attribute(&event, "ContentType")
                            .map_err(|error| error.to_string())?
                            .ok_or("content-type-override-value-missing")?;
                        if result
                            .overrides
                            .insert(part.to_owned(), content_type)
                            .is_some()
                        {
                            return Err("duplicate-opc-override-content-type".into());
                        }
                    }
                    _ => {}
                },
                Event::DocType(_) => return Err("content-types-doctype-disallowed".into()),
                Event::Eof => break,
                _ => {}
            }
        }
        Ok(result)
    }

    fn for_part(&self, part: &str) -> Option<String> {
        self.overrides.get(part).cloned().or_else(|| {
            part.rsplit_once('.')
                .and_then(|(_, extension)| self.defaults.get(extension))
                .cloned()
        })
    }
}

fn read_zip_text(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<String>, SignatureError> {
    let mut file = match archive.by_name(name) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => {
            return Err(SignatureError::SemanticExtractionFailed(format!(
                "OOXML signature part {name}: {error}"
            )));
        }
    };
    if file.size() > 8 * 1024 * 1024 {
        return Err(SignatureError::InspectionResourceLimitExceeded);
    }
    let mut bytes = Vec::with_capacity(file.size() as usize);
    file.by_ref()
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            SignatureError::SemanticExtractionFailed(format!(
                "OOXML signature part {name} read: {error}"
            ))
        })?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(SignatureError::InspectionResourceLimitExceeded);
    }
    String::from_utf8(bytes).map(Some).map_err(|_| {
        SignatureError::SemanticExtractionFailed(format!(
            "OOXML signature part {name} is not UTF-8 XML"
        ))
    })
}

fn relationship_targets(xml: &str, relationship_type: &str) -> Result<Vec<String>, SignatureError> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut targets = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event.local_name().as_ref() == "Relationship" =>
            {
                let kind = xml_attribute(&event, "Type")?;
                if kind.as_deref() == Some(relationship_type) {
                    let target = xml_attribute(&event, "Target")?.ok_or_else(|| {
                        SignatureError::SemanticExtractionFailed(
                            "digital-signature relationship missing Target".into(),
                        )
                    })?;
                    if xml_attribute(&event, "TargetMode")?
                        .is_some_and(|value| value.eq_ignore_ascii_case("External"))
                    {
                        return Err(SignatureError::UnsupportedSemanticConstruct(
                            "external OOXML digital-signature relationship".into(),
                        ));
                    }
                    targets.push(target);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(SignatureError::SemanticExtractionFailed(format!(
                    "OOXML signature relationships XML: {error}"
                )));
            }
        }
    }
    Ok(targets)
}

fn xml_attribute(event: &BytesStart<'_>, name: &str) -> Result<Option<String>, SignatureError> {
    for attribute in event.attributes() {
        let attribute = attribute.map_err(|error| {
            SignatureError::SemanticExtractionFailed(format!(
                "OOXML signature relationship attribute: {error}"
            ))
        })?;
        if attribute.key.local_name().as_ref() == name {
            return Ok(Some(attribute.value.as_ref().to_owned()));
        }
    }
    Ok(None)
}

fn relationship_part_name(source: &str) -> Result<String, SignatureError> {
    let (dir, file) = source.rsplit_once('/').unwrap_or(("", source));
    if file.is_empty() {
        return Err(SignatureError::SemanticExtractionFailed(
            "empty OOXML digital-signature origin part".into(),
        ));
    }
    Ok(if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    })
}

fn resolve_opc_target(source: &str, target: &str) -> Result<String, SignatureError> {
    if target.is_empty() || target.starts_with('/') || target.contains('\\') {
        return Err(SignatureError::SemanticExtractionFailed(format!(
            "unsafe OOXML digital-signature target {target:?}"
        )));
    }
    let mut segments: Vec<&str> = source
        .rsplit_once('/')
        .map(|(dir, _)| {
            dir.split('/')
                .filter(|segment| !segment.is_empty())
                .collect()
        })
        .unwrap_or_default();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(SignatureError::SemanticExtractionFailed(format!(
                        "OOXML digital-signature target escapes package root: {target:?}"
                    )));
                }
            }
            value => segments.push(value),
        }
    }
    Ok(segments.join("/"))
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
        "1.2.840.10045.4.3.2" | "1.2.840.113549.1.1.11" | "1.2.840.113549.1.1.1" | "1.3.101.112"
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
) -> DigitalSignatureEvidence {
    evidence_from_metadata(
        kind,
        validity,
        metadata,
        covered_content,
        validation_diagnostics,
    )
}

fn evidence_from_metadata(
    kind: &str,
    validity: SignatureValidity,
    metadata: Option<CertificateMetadata>,
    covered_content: &str,
    validation_diagnostics: Vec<String>,
) -> DigitalSignatureEvidence {
    DigitalSignatureEvidence {
        signature_type: kind.to_owned(),
        cryptographic_validity: validity,
        signer_claim: metadata
            .as_ref()
            .and_then(|value| value.signer_claim.clone()),
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
        covered_content: vec![covered_content.to_owned()],
        validation_diagnostics,
    }
}

fn embedded_certificates(pkcs7: &Pkcs7) -> Vec<X509> {
    pkcs7
        .signed()
        .and_then(|signed| signed.certificates())
        .map(|certificates| certificates.iter().map(ToOwned::to_owned).collect())
        .unwrap_or_default()
}

fn certificate_metadata(cert: &X509) -> Result<CertificateMetadata, SignatureError> {
    let certificate_der = cert.to_der().map_err(|error| {
        SignatureError::InvalidWorkerResult(format!("certificate DER: {error}"))
    })?;
    let _rust_model = x509_cert::Certificate::from_der(&certificate_der).map_err(|error| {
        SignatureError::InvalidWorkerResult(format!("x509-cert parse: {error}"))
    })?;

    let subject = name_common_name(cert, false);
    let issuer = name_common_name(cert, true);
    let fingerprint = cert.digest(MessageDigest::sha256()).map_err(|error| {
        SignatureError::InvalidWorkerResult(format!("certificate digest: {error}"))
    })?;

    Ok(CertificateMetadata {
        signer_claim: subject.clone(),
        certificate_subject: subject,
        certificate_issuer: issuer,
        certificate_fingerprint: Some(
            fingerprint
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        ),
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
        .and_then(|entry| entry.data().to_string().ok())
}

fn evaluate_certificate_policy(
    signer: &X509,
    embedded: Vec<X509>,
    trust: &SignatureTrustContext,
    diagnostics: &mut Vec<String>,
) -> Result<SignatureValidity, SignatureError> {
    let now = Asn1Time::days_from_now(0).map_err(|error| {
        SignatureError::InvalidWorkerResult(format!("verification time: {error}"))
    })?;

    if signer.not_before().compare(&now).map_err(|error| {
        SignatureError::InvalidWorkerResult(format!("notBefore compare: {error}"))
    })? == Ordering::Greater
    {
        diagnostics.push("certificate=not-yet-valid".into());
        return Ok(SignatureValidity::Invalid);
    }
    if signer.not_after().compare(&now).map_err(|error| {
        SignatureError::InvalidWorkerResult(format!("notAfter compare: {error}"))
    })? == Ordering::Less
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
        .map_err(|error| SignatureError::InvalidWorkerResult(format!("X509 store: {error}")))?;
    for root_der in &trust.trusted_certificates_der {
        let root = X509::from_der(root_der).map_err(|error| {
            SignatureError::InvalidWorkerResult(format!("trusted certificate DER: {error}"))
        })?;
        store_builder.add_cert(root).map_err(|error| {
            SignatureError::InvalidWorkerResult(format!("add trust anchor: {error}"))
        })?;
    }
    let store = store_builder.build();

    let mut chain = Stack::<X509>::new().map_err(|error| {
        SignatureError::InvalidWorkerResult(format!("X509 chain stack: {error}"))
    })?;
    for certificate in embedded {
        if certificate.to_der().ok() == signer.to_der().ok() {
            continue;
        }
        chain.push(certificate).map_err(|error| {
            SignatureError::InvalidWorkerResult(format!("X509 chain push: {error}"))
        })?;
    }

    let mut context = X509StoreContext::new().map_err(|error| {
        SignatureError::InvalidWorkerResult(format!("X509 store context: {error}"))
    })?;
    let verified = context
        .init(&store, signer, &chain, |ctx| ctx.verify_cert())
        .map_err(|error| {
            SignatureError::InvalidWorkerResult(format!("X509 path verify: {error}"))
        })?;

    if verified {
        diagnostics.push("certificate-chain=valid".into());
        Ok(SignatureValidity::Valid)
    } else {
        diagnostics.push(format!(
            "certificate-chain=unverifiable:{:?}",
            context.error()
        ));
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
) -> Result<RevocationStatus, SignatureError> {
    if trust.crls_der.is_empty() {
        return Ok(RevocationStatus::NotRevoked);
    }

    let signer_serial = signer
        .serial_number()
        .to_bn()
        .map_err(|error| SignatureError::InvalidWorkerResult(format!("signer serial: {error}")))?
        .to_vec();

    for crl_der in &trust.crls_der {
        let crl = X509Crl::from_der(crl_der)
            .map_err(|error| SignatureError::InvalidWorkerResult(format!("CRL DER: {error}")))?;

        let mut authenticated = false;
        for root_der in &trust.trusted_certificates_der {
            let root = X509::from_der(root_der).map_err(|error| {
                SignatureError::InvalidWorkerResult(format!("trusted certificate DER: {error}"))
            })?;
            let public_key = root.public_key().map_err(|error| {
                SignatureError::InvalidWorkerResult(format!("CRL issuer public key: {error}"))
            })?;
            if crl.verify(&public_key).map_err(|error| {
                SignatureError::InvalidWorkerResult(format!("CRL signature verification: {error}"))
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
                        SignatureError::InvalidWorkerResult(format!("CRL serial: {error}"))
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

fn pdf_invalid_evidence(reason: &str) -> DigitalSignatureEvidence {
    evidence(
        "pdf-cms",
        SignatureValidity::Invalid,
        None,
        "pdf-byte-range:invalid",
        vec![
            "explicit-trust-only;network-retrieval=disabled".into(),
            format!("pdf-byte-range-invalid:{reason}"),
        ],
    )
}

fn inspect_pdf_field(
    document: &Document,
    field: &Object,
    pdf: &[u8],
    trust: &SignatureTrustContext,
    depth: usize,
    evidence: &mut Vec<DigitalSignatureEvidence>,
) -> Result<(), SignatureError> {
    if depth > 64 {
        return Err(SignatureError::InspectionResourceLimitExceeded);
    }
    let field = document
        .dereference(field)
        .map_err(|error| {
            SignatureError::SemanticExtractionFailed(format!("PDF signature field: {error}"))
        })?
        .1
        .as_dict()
        .map_err(|error| {
            SignatureError::SemanticExtractionFailed(format!("PDF signature field: {error}"))
        })?;
    let is_signature =
        field.get(b"FT").ok().and_then(|value| value.as_name().ok()) == Some(b"Sig".as_slice());
    if is_signature && let Ok(value) = field.get(b"V") {
        let signature = document
            .dereference(value)
            .map_err(|error| {
                SignatureError::SemanticExtractionFailed(format!(
                    "PDF signature dictionary: {error}"
                ))
            })?
            .1
            .as_dict()
            .map_err(|error| {
                SignatureError::SemanticExtractionFailed(format!(
                    "PDF signature dictionary: {error}"
                ))
            })?;
        evidence.push(SignatureInspector::verify_pdf_signature_dict(
            pdf, signature, trust,
        )?);
    }
    if let Ok(kids) = field.get(b"Kids") {
        let kids = document
            .dereference(kids)
            .map_err(|error| {
                SignatureError::SemanticExtractionFailed(format!(
                    "PDF signature child fields: {error}"
                ))
            })?
            .1
            .as_array()
            .map_err(|error| {
                SignatureError::SemanticExtractionFailed(format!(
                    "PDF signature child fields: {error}"
                ))
            })?;
        for child in kids {
            inspect_pdf_field(document, child, pdf, trust, depth + 1, evidence)?;
        }
    }
    Ok(())
}

fn parse_pdf_byte_range_signature(
    pdf: &[u8],
    signature: &Dictionary,
) -> Result<(Vec<u8>, Vec<u8>, String), &'static str> {
    let byte_range = signature
        .get(b"ByteRange")
        .map_err(|_| "missing-byte-range")?
        .as_array()
        .map_err(|_| "invalid-byte-range-array")?;
    if byte_range.len() != 4 {
        return Err("byte-range-arity");
    }
    let numbers = byte_range
        .iter()
        .map(|value| {
            usize::try_from(value.as_i64().map_err(|_| "invalid-byte-range-number")?)
                .map_err(|_| "invalid-byte-range-number")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (first_start, first_len, second_start, second_len) =
        (numbers[0], numbers[1], numbers[2], numbers[3]);
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
    let hex_text = gap[1..gap.len() - 1]
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    let padded_der = decode_hex(&hex_text)?;
    let parsed_contents = signature
        .get(b"Contents")
        .map_err(|_| "missing-contents")?
        .as_str()
        .map_err(|_| "invalid-contents")?;
    if padded_der.as_slice() != parsed_contents {
        return Err("byte-range-gap-does-not-match-contents");
    }
    let der_len = der_sequence_total_len(&padded_der)?;
    if padded_der[der_len..].iter().any(|byte| *byte != 0) {
        return Err("nonzero-contents-padding");
    }
    let mut covered = Vec::with_capacity(first_len + second_len);
    covered.extend_from_slice(&pdf[first_start..first_end]);
    covered.extend_from_slice(&pdf[second_start..second_end]);
    Ok((
        covered,
        padded_der[..der_len].to_vec(),
        format!(
            "pdf-byte-range:{}+{};{}+{}",
            first_start, first_len, second_start, second_len
        ),
    ))
}

fn decode_hex(input: &[u8]) -> Result<Vec<u8>, &'static str> {
    if input.is_empty() || !input.len().is_multiple_of(2) {
        return Err("invalid-contents-hex-length");
    }
    input
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .ok_or("invalid-contents-hex")?;
            let low = (pair[1] as char)
                .to_digit(16)
                .ok_or("invalid-contents-hex")?;
            Ok(((high << 4) | low) as u8)
        })
        .collect()
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

fn extract_x509_certificate(xml: &str) -> Result<Option<Vec<u8>>, SignatureError> {
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
                encoded.push_str(text.as_ref());
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                return Err(SignatureError::SemanticExtractionFailed(format!(
                    "XML signature parse: {error}"
                )));
            }
        }
    }

    let compact: String = encoded
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    if compact.is_empty() {
        return Ok(None);
    }

    openssl::base64::decode_block(&compact)
        .map(Some)
        .map_err(|error| {
            SignatureError::SemanticExtractionFailed(format!("X509Certificate base64: {error}"))
        })
}

fn unsupported_crypto_error(detail: &str) -> bool {
    let detail = detail.to_ascii_lowercase();
    detail.contains("unsupported")
        || detail.contains("unknown algorithm")
        || detail.contains("unknown digest")
        || detail.contains("algorithm not supported")
}
