use super::*;
use crate::tools::outline::grammar::SupportedLanguage;

#[test]
fn test_queries_compile_for_all_languages() {
    let languages = [
        SupportedLanguage::Rust,
        SupportedLanguage::TypeScript,
        SupportedLanguage::Tsx,
        SupportedLanguage::JavaScript,
        SupportedLanguage::Python,
        SupportedLanguage::Go,
    ];

    for lang in languages {
        let res = query_for_language(lang);
        assert!(res.is_ok(), "Query compilation failed for {lang:?}: {:?}", res.err());
    }
}
