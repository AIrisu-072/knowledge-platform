# Signature qualification fixtures

All files in this directory are **TEST ONLY synthetic fixtures** generated for Document Semantic Inspection v0 PoC qualification.

- No customer, institution, production, personal, or secret material is present.
- Private signing keys were generation-time inputs only and are intentionally **not committed**.
- Certificates use deterministic test identities under `DSI TEST ROOT CA`; they are not trusted outside this fixture corpus.
- `revoked.crl.der` is caller-supplied offline revocation evidence. Tests must not perform CRL/OCSP network retrieval.
- CMS objects are detached signatures over `content.bin`.
- The malformed/unsupported/tampered vectors are intentionally invalid test material.

- PDF ByteRange vectors are generated in `tests/support/signature_pdf.rs` from fixed TEST ONLY SEC1 key bytes plus the public DER certificate in this directory.
- The fixture builder freezes final PDF layout and ByteRange values **before** generating detached CMS, then embeds CMS only inside the excluded `/Contents` gap.
- Tampered and malformed variants are derived only after valid signing, so failure causes are isolated.
- The private scalar is represented only as an explicit TEST ONLY byte fixture in Rust source; no PEM/private-key artifact is tracked. It must never be reused outside this corpus.
- The certificate validity interval is fixed to 2025-01-01 through 2035-01-01 to avoid host-clock-dependent fixture failure.
