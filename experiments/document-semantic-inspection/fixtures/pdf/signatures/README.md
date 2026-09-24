# Signature qualification fixtures

All files in this directory are **TEST ONLY synthetic fixtures** generated for Document Semantic Inspection v0 PoC qualification.

- No customer, institution, production, personal, or secret material is present.
- Private signing keys were generation-time inputs only and are intentionally **not committed**.
- Certificates use deterministic test identities under `DSI TEST ROOT CA`; they are not trusted outside this fixture corpus.
- `revoked.crl.der` is caller-supplied offline revocation evidence. Tests must not perform CRL/OCSP network retrieval.
- CMS objects are detached signatures over `content.bin`.
- The malformed/unsupported/tampered vectors are intentionally invalid test material.
