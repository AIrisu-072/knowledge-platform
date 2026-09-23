mod support;
use document_semantic_inspection_poc::{ErrorCode,FixtureCase,FixtureManifest,PdfAdapter,run_case};
fn manifest()->FixtureManifest{FixtureManifest::from_path(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/manifest.json")).expect("manifest")}
fn case(id:&str)->FixtureCase{manifest().cases.into_iter().find(|c|c.id==id).unwrap_or_else(||panic!("missing {id}"))}
fn fp(id:&str)->[u8;32]{run_case(&case(id),&PdfAdapter).unwrap_or_else(|e|panic!("{id}: {e}")).semantic_fingerprint}
fn same(id:&str){assert_eq!(fp("pdf/base"),fp(id),"{id}")} fn different(id:&str){assert_ne!(fp("pdf/base"),fp(id),"{id}")}
fn error(id:&str,code:ErrorCode){let e=run_case(&case(id),&PdfAdapter).unwrap_err();assert_eq!(e.code(),code,"{id}: {e}")}
#[test] fn raw_pdf_fixture_builder_is_parser_independent(){let b=support::pdf_fixture::minimal_text_pdf("hello","one");let n=support::pdf_fixture::minimal_text_pdf("hello","two");assert!(b.starts_with(b"%PDF-"));assert_ne!(b,n);assert!(support::pdf_fixture::with_broken_startxref(b).starts_with(b"%PDF-"));}
#[test] fn producer_and_object_number_noise_is_invariant(){same("pdf/object-id-producer-noise");}
#[test] fn reader_visible_pdf_semantics_change_identity(){for id in ["pdf/text-change","pdf/page-order-change","pdf/link-change","pdf/annotation-change","pdf/form-value-change","pdf/image-change"]{different(id);}}
#[test] fn scan_only_and_broken_or_encrypted_inputs_fail_closed(){error("pdf/scan-only",ErrorCode::RequiresOcr);error("pdf/broken-xref",ErrorCode::SemanticExtractionFailed);error("pdf/encrypted",ErrorCode::EncryptedContentUnsupported);}
#[test] fn required_dual_engine_disagreement_is_not_resolved_by_preference(){error("pdf/parser-disagreement",ErrorCode::ParserDisagreement);}
