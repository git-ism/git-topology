use tree_sitter::Language;

#[derive(Debug, Clone, Copy)]
pub enum SupportedLanguage {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Java,
    C,
    Cpp,
    Go,
}

impl SupportedLanguage {
    pub fn tree_sitter_language(&self) -> Language {
        match self {
            SupportedLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
            SupportedLanguage::Python => tree_sitter_python::LANGUAGE.into(),
            SupportedLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            SupportedLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            SupportedLanguage::Java => tree_sitter_java::LANGUAGE.into(),
            SupportedLanguage::C => tree_sitter_c::LANGUAGE.into(),
            SupportedLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            SupportedLanguage::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }
}

pub fn detect_language(file_path: &str) -> Option<SupportedLanguage> {
    let extension = file_path.split('.').next_back()?.to_lowercase();
    match extension.as_str() {
        "rs" => Some(SupportedLanguage::Rust),
        "py" | "pyw" | "pyi" => Some(SupportedLanguage::Python),
        "js" | "mjs" | "cjs" => Some(SupportedLanguage::JavaScript),
        "ts" | "tsx" => Some(SupportedLanguage::TypeScript),
        "java" => Some(SupportedLanguage::Java),
        "c" | "h" => Some(SupportedLanguage::C),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => Some(SupportedLanguage::Cpp),
        "go" => Some(SupportedLanguage::Go),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rust() {
        assert!(matches!(
            detect_language("src/main.rs"),
            Some(SupportedLanguage::Rust)
        ));
    }

    #[test]
    fn detects_python_variants() {
        assert!(matches!(
            detect_language("script.py"),
            Some(SupportedLanguage::Python)
        ));
        assert!(matches!(
            detect_language("types.pyi"),
            Some(SupportedLanguage::Python)
        ));
    }

    #[test]
    fn detects_typescript() {
        assert!(matches!(
            detect_language("app.ts"),
            Some(SupportedLanguage::TypeScript)
        ));
        assert!(matches!(
            detect_language("comp.tsx"),
            Some(SupportedLanguage::TypeScript)
        ));
    }

    #[test]
    fn detects_go() {
        assert!(matches!(
            detect_language("main.go"),
            Some(SupportedLanguage::Go)
        ));
    }

    #[test]
    fn unsupported_returns_none() {
        assert!(detect_language("README.md").is_none());
        assert!(detect_language("data.json").is_none());
        assert!(detect_language("Makefile").is_none());
    }

    #[test]
    fn case_insensitive_extension() {
        assert!(detect_language("main.RS").is_some());
        assert!(detect_language("script.PY").is_some());
    }
}
