pub mod languages;
pub mod parser;

use anyhow::Result;

#[derive(Debug, Clone)]
pub struct CodeChunk {
    pub text: String,
    pub start_line: usize,
    pub end_line: usize,
}

pub fn chunk_code(text: &str, file_path: Option<&str>) -> Result<Vec<CodeChunk>> {
    if let Some(path) = file_path {
        if let Some(language) = languages::detect_language(path) {
            return parser::parse_with_tree_sitter(text, language);
        }
    }

    Ok(vec![CodeChunk {
        text: text.to_string(),
        start_line: 0,
        end_line: text.lines().count(),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_unsupported_language_returns_whole_file() {
        let text = "hello world\nfoo bar\n";
        let chunks = chunk_code(text, Some("file.txt")).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, text);
        assert_eq!(chunks[0].start_line, 0);
    }

    #[test]
    fn chunk_no_path_returns_whole_file() {
        let text = "fn main() {}";
        let chunks = chunk_code(text, None).unwrap();
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn chunk_rust_finds_functions() {
        let code = "fn foo() { let x = 1; }\nfn bar() { let y = 2; }\n";
        let chunks = chunk_code(code, Some("src/main.rs")).unwrap();
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn chunk_python_finds_functions() {
        let code = "def foo():\n    pass\n\ndef bar():\n    pass\n";
        let chunks = chunk_code(code, Some("main.py")).unwrap();
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn chunk_lines_are_ordered() {
        let code = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let chunks = chunk_code(code, Some("lib.rs")).unwrap();
        let starts: Vec<usize> = chunks.iter().map(|c| c.start_line).collect();
        let mut sorted = starts.clone();
        sorted.sort();
        assert_eq!(starts, sorted);
    }
}
