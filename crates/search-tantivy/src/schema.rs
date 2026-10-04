use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions,
};

pub(crate) const ANALYZER_VERSION: &str = "tantivy-default-0.26.2";
pub(crate) const LEXICAL_SCHEMA_VERSION: &str = "schema-1";

#[derive(Clone, Copy)]
pub(crate) struct LexicalFields {
    pub resource_ref: Field,
    pub kind: Field,
    pub canonical_exact: Field,
    pub aliases_exact: Field,
    pub title_exact: Field,
    pub canonical_name: Field,
    pub aliases: Field,
    pub title: Field,
    pub high_signal_text: Field,
    pub body: Field,
}

impl LexicalFields {
    pub fn ranked(self) -> [(&'static str, Field, bool); 8] {
        [
            ("canonical_name", self.canonical_exact, true),
            ("aliases", self.aliases_exact, true),
            ("title", self.title_exact, true),
            ("canonical_name", self.canonical_name, false),
            ("aliases", self.aliases, false),
            ("title", self.title, false),
            ("high_signal_text", self.high_signal_text, false),
            ("body", self.body, false),
        ]
    }
}

pub(crate) fn normalize_exact(text: &str) -> String {
    text.trim().to_lowercase()
}

pub(crate) const fn kind_token(kind: search_core::resource::ResourceKind) -> &'static str {
    use search_core::resource::ResourceKind;
    match kind {
        ResourceKind::Knowledge => "knowledge",
        ResourceKind::Document => "document",
        ResourceKind::FolderPlacement => "folder_placement",
        ResourceKind::Semantic => "semantic",
        ResourceKind::Capability => "capability",
        ResourceKind::AgentSkill => "agent_skill",
        ResourceKind::Workflow => "workflow",
        ResourceKind::Policy => "policy",
    }
}

pub(crate) fn lexical_schema() -> (Schema, LexicalFields) {
    let mut builder = Schema::builder();
    let resource_ref = builder.add_text_field("resource_ref", STRING | STORED);
    let indexed = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer("default")
            .set_index_option(IndexRecordOption::WithFreqsAndPositions),
    );
    let fields = LexicalFields {
        resource_ref,
        kind: builder.add_text_field("kind", STRING),
        canonical_exact: builder.add_text_field("canonical_exact", STRING),
        aliases_exact: builder.add_text_field("aliases_exact", STRING),
        title_exact: builder.add_text_field("title_exact", STRING),
        canonical_name: builder.add_text_field("canonical_name", indexed.clone()),
        aliases: builder.add_text_field("aliases", indexed.clone()),
        title: builder.add_text_field("title", indexed.clone()),
        high_signal_text: builder.add_text_field("high_signal_text", indexed.clone()),
        body: builder.add_text_field("body", indexed),
    };
    (builder.build(), fields)
}
