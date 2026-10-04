# P1 minimal Unit plan — Archive part binding clarification

- Autonomous controller decision, 2026-09-30: frozen heterogeneous Archive leaf semantics take precedence over the plan-introduced scalar part binding field.
- For an Archive part, `UnitAuthorityBinding.archive_inner_format=None` means no single leaf format fixed for the whole part. Each Unit must have `archive_inner_format=Some(leaf)` and match its exact member chain to the trusted ArchiveProfilePlan leaf format, inner locator and UnitKind. `Some(leaf)` on the binding remains an optional additional single-format constraint. Non-Archive bindings/Units keep None. No authority field is skipped.
- The composed profile is common to the whole Archive item. Frozen UnitProvenance.parser_build_id denotes the outer reader node, so its scalar equality remains required.
- Original plan is preserved. A same-Part Text plus CSV leaf regression was RED under scalar equality and GREEN under this rule. Full body/raw/current authority qualification remains separate. Independent core reviewer must verify this rule against original frozen amendment.
