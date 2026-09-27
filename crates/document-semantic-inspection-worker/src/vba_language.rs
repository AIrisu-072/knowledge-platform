use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_vba() -> *const ();
}

pub(crate) const VBA_LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_vba) };
