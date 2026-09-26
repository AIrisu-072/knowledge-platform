use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use openssl::asn1::Asn1Time;
use openssl::bn::BigNum;
use openssl::ec::{EcGroup, EcKey};
use openssl::hash::MessageDigest;
use openssl::nid::Nid;
use openssl::pkcs7::{Pkcs7, Pkcs7Flags};
use openssl::pkey::PKey;
use openssl::stack::Stack;
use openssl::x509::extension::{BasicConstraints, KeyUsage};
use openssl::x509::{X509, X509NameBuilder};
use xml_sec::policy::{ManifestProcessing, SigningPolicy};
use xml_sec::xmldsig::{
    EcdsaP256SigningKey, KeyInfoWriter, KeyValueInfoWriter, SignContext, UriTypeSet,
    X509CertificateKeyInfoWriter,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const ORIGIN_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/origin";
const SIGNATURE_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/signature";

pub fn detached_cms_with_unrelated_trusted_certificate_first(content: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let unrelated = synthetic_self_signed_ca("DSI ROOT", 1);
    let signer = synthetic_self_signed_ca("DSI ACTUAL CMS SIGNER CERTIFICATE", 20);
    let signer_key = signer.1;
    let signer_certificate = signer.0;

    let mut additional_certificates = Stack::new().expect("CMS extra certificate stack");
    additional_certificates
        .push(X509::from_der(&unrelated.0).expect("unrelated trusted certificate"))
        .expect("push unrelated trusted certificate");
    let pkcs7 = Pkcs7::sign(
        &X509::from_der(&signer_certificate).expect("actual signer certificate"),
        &signer_key,
        &additional_certificates,
        content,
        Pkcs7Flags::DETACHED | Pkcs7Flags::BINARY,
    )
    .expect("synthetic detached CMS signature");
    let mut signature_der = pkcs7.to_der().expect("encode CMS signature");

    // OpenSSL places the SignerInfo certificate first. CMS permits certificate
    // set ordering under BER, so swap the two complete certificate TLVs while
    // retaining all enclosing lengths and fields.
    let signer_offset = signature_der
        .windows(signer_certificate.len())
        .position(|bytes| bytes == signer_certificate)
        .expect("actual signer certificate in CMS DER");
    let unrelated_offset = signature_der
        .windows(unrelated.0.len())
        .position(|bytes| bytes == unrelated.0)
        .expect("unrelated certificate in CMS DER");
    assert_eq!(
        signer_offset + signer_certificate.len(),
        unrelated_offset,
        "CMS certificate choices are adjacent"
    );
    let mut reordered = Vec::with_capacity(signature_der.len());
    reordered.extend_from_slice(&signature_der[..signer_offset]);
    reordered.extend_from_slice(&unrelated.0);
    reordered.extend_from_slice(&signer_certificate);
    reordered.extend_from_slice(&signature_der[unrelated_offset + unrelated.0.len()..]);
    signature_der = reordered;

    let decoded = Pkcs7::from_der(&signature_der).expect("decode CMS signature");
    let certificates = decoded
        .signed()
        .and_then(|signed| signed.certificates())
        .expect("embedded CMS certificates");
    assert_eq!(
        certificates
            .get(0)
            .expect("first embedded certificate")
            .to_der()
            .expect("first certificate DER"),
        unrelated.0,
        "DER ordering places the unrelated trusted certificate first"
    );
    assert!(
        certificates.iter().any(|certificate| {
            certificate
                .to_der()
                .is_ok_and(|der| der == signer_certificate)
        }),
        "actual signer certificate remains embedded"
    );

    (signature_der, unrelated.0)
}

fn synthetic_self_signed_ca(
    common_name: &str,
    serial: u32,
) -> (Vec<u8>, PKey<openssl::pkey::Private>) {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).expect("P-256 group");
    let key = PKey::from_ec_key(EcKey::generate(&group).expect("ephemeral test key"))
        .expect("P-256 private key");
    let mut name = X509NameBuilder::new().expect("test certificate subject");
    name.append_entry_by_text("CN", common_name)
        .expect("test certificate common name");
    let name = name.build();
    let serial = BigNum::from_u32(serial)
        .expect("test serial")
        .to_asn1_integer()
        .expect("test serial integer");
    let mut certificate = X509::builder().expect("test certificate builder");
    certificate.set_version(2).expect("X.509 v3");
    certificate
        .set_serial_number(&serial)
        .expect("set test serial");
    certificate
        .set_subject_name(&name)
        .expect("set test subject");
    certificate.set_issuer_name(&name).expect("set test issuer");
    certificate.set_pubkey(&key).expect("set test public key");
    certificate
        .set_not_before(&Asn1Time::days_from_now(0).expect("test notBefore"))
        .expect("set test notBefore");
    certificate
        .set_not_after(&Asn1Time::days_from_now(365).expect("test notAfter"))
        .expect("set test notAfter");
    certificate
        .append_extension(
            BasicConstraints::new()
                .critical()
                .ca()
                .build()
                .expect("test CA extension"),
        )
        .expect("append test CA extension");
    certificate
        .append_extension(
            KeyUsage::new()
                .critical()
                .digital_signature()
                .key_cert_sign()
                .crl_sign()
                .build()
                .expect("test key usage"),
        )
        .expect("append test key usage");
    certificate
        .sign(&key, MessageDigest::sha256())
        .expect("self-sign test certificate");
    (
        certificate.build().to_der().expect("test certificate DER"),
        key,
    )
}

pub fn add_ooxml_signature(package: &[u8], signature_xml: &[u8]) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(package)).expect("input OOXML zip");
    let mut entries = Vec::new();

    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("OOXML entry");
        let name = file.name().to_owned();
        if name.starts_with("_xmlsignatures/") {
            continue;
        }
        let mut data = Vec::new();
        file.read_to_end(&mut data).expect("read OOXML entry");

        let data = match name.as_str() {
            "[Content_Types].xml" => insert_before(
                data,
                b"</Types>",
                "<Override PartName=\"/_xmlsignatures/origin.sigs\" ContentType=\"application/vnd.openxmlformats-package.digital-signature-origin\"/><Override PartName=\"/_xmlsignatures/sig1.xml\" ContentType=\"application/vnd.openxmlformats-package.digital-signature-xmlsignature+xml\"/>".to_string()
                .as_bytes(),
            ),
            "_rels/.rels" => insert_before(
                data,
                b"</Relationships>",
                format!(
                    "<Relationship Id=\"rIdDsiSignatureOrigin\" Type=\"{ORIGIN_REL}\" Target=\"_xmlsignatures/origin.sigs\"/>"
                )
                .as_bytes(),
            ),
            _ => data,
        };
        entries.push((name, data));
    }

    entries.push((
        "_xmlsignatures/origin.sigs".to_owned(),
        b"<SignatureOrigin xmlns=\"http://schemas.openxmlformats.org/package/2006/digital-signature\"/>".to_vec(),
    ));
    entries.push((
        "_xmlsignatures/_rels/origin.sigs.rels".to_owned(),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdSignature1\" Type=\"{SIGNATURE_REL}\" Target=\"sig1.xml\"/></Relationships>"
        )
        .into_bytes(),
    ));
    entries.push(("_xmlsignatures/sig1.xml".to_owned(), signature_xml.to_vec()));

    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, data) in entries {
        writer
            .start_file(name, options)
            .expect("start OOXML output entry");
        writer.write_all(&data).expect("write OOXML output entry");
    }
    writer
        .finish()
        .expect("finish OOXML output zip")
        .into_inner()
}

pub struct TestXmlSigner {
    signing_key: EcdsaP256SigningKey,
    certificate_der: Vec<u8>,
    key_info: X509CertificateKeyInfoWriter,
}

impl TestXmlSigner {
    pub fn new() -> Self {
        let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).expect("P-256 group");
        let ec_key = EcKey::generate(&group).expect("ephemeral test signing key");
        let private_key = PKey::from_ec_key(ec_key).expect("P-256 private key");

        let mut name = X509NameBuilder::new().expect("test certificate subject");
        name.append_entry_by_text("CN", "DSI SYNTHETIC OOXML TEST SIGNER")
            .expect("test certificate common name");
        let name = name.build();

        let serial = BigNum::from_u32(1)
            .expect("test serial")
            .to_asn1_integer()
            .expect("test serial integer");
        let mut certificate = X509::builder().expect("test certificate builder");
        certificate.set_version(2).expect("X.509 v3");
        certificate
            .set_serial_number(&serial)
            .expect("test serial number");
        certificate
            .set_subject_name(&name)
            .expect("test subject name");
        certificate
            .set_issuer_name(&name)
            .expect("test issuer name");
        certificate
            .set_pubkey(&private_key)
            .expect("test certificate public key");
        certificate
            .set_not_before(&Asn1Time::days_from_now(0).expect("test notBefore"))
            .expect("set test notBefore");
        certificate
            .set_not_after(&Asn1Time::days_from_now(365).expect("test notAfter"))
            .expect("set test notAfter");
        certificate
            .append_extension(
                BasicConstraints::new()
                    .critical()
                    .ca()
                    .build()
                    .expect("test CA extension"),
            )
            .expect("append test CA extension");
        certificate
            .append_extension(
                KeyUsage::new()
                    .critical()
                    .digital_signature()
                    .key_cert_sign()
                    .crl_sign()
                    .build()
                    .expect("test key usage"),
            )
            .expect("append test key usage");
        certificate
            .sign(&private_key, MessageDigest::sha256())
            .expect("self-sign test certificate");
        let certificate_der = certificate.build().to_der().expect("test certificate DER");

        Self {
            signing_key: EcdsaP256SigningKey::from_pkcs8_der(
                &private_key
                    .private_key_to_pkcs8()
                    .expect("PKCS#8 test private key"),
            )
            .expect("XMLDSig test signing key"),
            key_info: X509CertificateKeyInfoWriter::from_der(&certificate_der)
                .expect("XMLDSig test X.509 KeyInfo"),
            certificate_der,
        }
    }

    pub fn certificate_der(&self) -> &[u8] {
        &self.certificate_der
    }

    pub fn sign_package_part(
        &self,
        package: &[u8],
        part_name: &str,
        content_type: &str,
    ) -> Vec<u8> {
        self.sign_package_part_with_writer(package, part_name, content_type, &self.key_info)
    }

    pub fn sign_package_part_with_unrelated_trusted_certificate(
        &self,
        package: &[u8],
        part_name: &str,
        content_type: &str,
        unrelated_trusted_certificate_der: &[u8],
    ) -> Vec<u8> {
        let signed = self.sign_package_part_with_writer(
            package,
            part_name,
            content_type,
            &KeyValueInfoWriter,
        );
        let signature_xml = String::from_utf8(
            read_zip_entry(&signed, "_xmlsignatures/sig1.xml").expect("signature XML part"),
        )
        .expect("signature XML is UTF-8");
        let unrelated_certificate =
            openssl::base64::encode_block(unrelated_trusted_certificate_der);
        let injected_object = format!(
            "<ds:Object><ds:X509Data xmlns:ds=\"http://www.w3.org/2000/09/xmldsig#\"><ds:X509Certificate>{unrelated_certificate}</ds:X509Certificate></ds:X509Data></ds:Object>"
        );
        let injected_xml = signature_xml.replacen(
            "</ds:Signature>",
            &format!("{injected_object}</ds:Signature>"),
            1,
        );
        assert_ne!(injected_xml, signature_xml, "signature node is present");
        replace_zip_entry(&signed, "_xmlsignatures/sig1.xml", injected_xml.as_bytes())
    }

    fn sign_package_part_with_writer(
        &self,
        package: &[u8],
        part_name: &str,
        content_type: &str,
        key_info_writer: &dyn KeyInfoWriter,
    ) -> Vec<u8> {
        let part_bytes = read_zip_entry(package, part_name).expect("signed package part exists");
        let part_uri = format!("/{part_name}?ContentType={content_type}");
        let resources = HashMap::from([(part_uri.clone(), part_bytes)]);
        let template = format!(
            r##"<root xmlns:ds="http://www.w3.org/2000/09/xmldsig#">
  <ds:Signature>
    <ds:SignedInfo>
      <ds:CanonicalizationMethod Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/>
      <ds:SignatureMethod Algorithm="http://www.w3.org/2001/04/xmldsig-more#ecdsa-sha256"/>
      <ds:Reference URI="#manifest">
        <ds:Transforms><ds:Transform Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/></ds:Transforms>
        <ds:DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/>
        <ds:DigestValue></ds:DigestValue>
      </ds:Reference>
    </ds:SignedInfo>
    <ds:SignatureValue></ds:SignatureValue>
    <ds:KeyInfo></ds:KeyInfo>
    <ds:Object><ds:Manifest Id="manifest">
      <ds:Reference URI="{part_uri}">
        <ds:DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/>
        <ds:DigestValue></ds:DigestValue>
      </ds:Reference>
    </ds:Manifest></ds:Object>
  </ds:Signature>
</root>"##
        );
        let mut policy = SigningPolicy::default();
        policy.uris.references = UriTypeSet::ALL;
        policy.manifest_processing = ManifestProcessing::Process;
        let signed_xml = SignContext::new(&self.signing_key)
            .policy(policy)
            .external_resources(&resources)
            .key_info_writer(key_info_writer)
            .sign_template(&template)
            .expect("generate authenticated OPC Manifest");

        add_ooxml_signature(package, signed_xml.as_bytes())
    }
}

pub fn replace_zip_entry(package: &[u8], part_name: &str, replacement: &[u8]) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(package)).expect("input OOXML zip");
    let mut entries = Vec::new();
    let mut replaced = false;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("OOXML entry");
        let name = file.name().to_owned();
        let mut data = Vec::new();
        file.read_to_end(&mut data).expect("read OOXML entry");
        if name == part_name {
            data = replacement.to_vec();
            replaced = true;
        }
        entries.push((name, data));
    }
    assert!(replaced, "OOXML package contains {part_name}");

    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, data) in entries {
        writer
            .start_file(name, options)
            .expect("start OOXML replacement entry");
        writer
            .write_all(&data)
            .expect("write OOXML replacement entry");
    }
    writer
        .finish()
        .expect("finish OOXML replacement zip")
        .into_inner()
}

fn read_zip_entry(package: &[u8], part_name: &str) -> Option<Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(package)).ok()?;
    let mut file = archive.by_name(part_name).ok()?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).ok()?;
    Some(data)
}

fn insert_before(mut source: Vec<u8>, marker: &[u8], insertion: &[u8]) -> Vec<u8> {
    let offset = source
        .windows(marker.len())
        .rposition(|window| window == marker)
        .expect("OOXML marker");
    source.splice(offset..offset, insertion.iter().copied());
    source
}
