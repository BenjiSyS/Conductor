//! Lightweight symbol and import extraction.
//!
//! Regex-based on purpose: it is fast, has no native dependencies, works on
//! partially broken code, and is good enough for relevance scoring. It does
//! not try to be a compiler.

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Java,
    Kotlin,
    CSharp,
    Cpp,
    C,
    GdScript,
    Svelte,
    Vue,
    Html,
    Css,
    Markdown,
    Toml,
    Json,
    Yaml,
    Shell,
    Other,
}

impl Lang {
    pub fn from_path(path: &str) -> Lang {
        let lower = path.to_ascii_lowercase();
        let ext = lower.rsplit('.').next().unwrap_or("");
        match ext {
            "rs" => Lang::Rust,
            "ts" | "tsx" | "mts" | "cts" => Lang::TypeScript,
            "js" | "jsx" | "mjs" | "cjs" => Lang::JavaScript,
            "py" | "pyi" => Lang::Python,
            "go" => Lang::Go,
            "java" => Lang::Java,
            "kt" | "kts" => Lang::Kotlin,
            "cs" => Lang::CSharp,
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Lang::Cpp,
            "c" | "h" => Lang::C,
            "gd" => Lang::GdScript,
            "svelte" => Lang::Svelte,
            "vue" => Lang::Vue,
            "html" | "htm" => Lang::Html,
            "css" | "scss" | "less" => Lang::Css,
            "md" | "mdx" => Lang::Markdown,
            "toml" => Lang::Toml,
            "json" | "jsonc" => Lang::Json,
            "yml" | "yaml" => Lang::Yaml,
            "sh" | "bash" | "zsh" | "ps1" | "psm1" | "bat" | "cmd" => Lang::Shell,
            _ => Lang::Other,
        }
    }

    pub fn is_code(self) -> bool {
        !matches!(
            self,
            Lang::Markdown
                | Lang::Toml
                | Lang::Json
                | Lang::Yaml
                | Lang::Other
                | Lang::Html
                | Lang::Css
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Type,
    Trait,
    Const,
    Module,
    Heading,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// 1-based line number.
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outline {
    pub symbols: Vec<Symbol>,
    /// Raw import specifiers (module paths, relative files, crate names).
    pub imports: Vec<String>,
}

struct LangRules {
    symbols: Vec<(Regex, SymbolKind)>,
    imports: Vec<Regex>,
}

fn rules(lang: Lang) -> Option<&'static LangRules> {
    static RUST: OnceLock<LangRules> = OnceLock::new();
    static TS: OnceLock<LangRules> = OnceLock::new();
    static PY: OnceLock<LangRules> = OnceLock::new();
    static GO: OnceLock<LangRules> = OnceLock::new();
    static JAVA: OnceLock<LangRules> = OnceLock::new();
    static CS: OnceLock<LangRules> = OnceLock::new();
    static CPP: OnceLock<LangRules> = OnceLock::new();
    static GD: OnceLock<LangRules> = OnceLock::new();
    static MD: OnceLock<LangRules> = OnceLock::new();
    let re = |s: &str| Regex::new(s).expect("valid symbol regex");
    Some(match lang {
        Lang::Rust => RUST.get_or_init(|| LangRules {
            symbols: vec![
                (re(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:const\s+)?(?:unsafe\s+)?(?:extern\s+\S+\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Function),
                (re(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|union|type)\s+([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Type),
                (re(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Trait),
                (re(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const|static)\s+([A-Z_][A-Z0-9_]*)\s*:"), SymbolKind::Const),
                (re(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Module),
            ],
            imports: vec![re(r"(?m)^\s*(?:pub\s+)?use\s+([A-Za-z_][A-Za-z0-9_:]*)"), re(r"(?m)^\s*(?:pub\s+)?mod\s+([a-z_][a-z0-9_]*)\s*;")],
        }),
        Lang::TypeScript | Lang::JavaScript | Lang::Svelte | Lang::Vue => TS.get_or_init(|| LangRules {
            symbols: vec![
                (re(r"(?m)^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*([A-Za-z_$][A-Za-z0-9_$]*)"), SymbolKind::Function),
                (re(r"(?m)^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*(?::[^=]+)?=\s*(?:async\s*)?(?:\([^)]*\)|[A-Za-z_$][A-Za-z0-9_$]*)\s*(?::[^=]+)?=>"), SymbolKind::Function),
                (re(r"(?m)^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][A-Za-z0-9_$]*)"), SymbolKind::Type),
                (re(r"(?m)^\s*(?:export\s+)?(?:interface|type|enum)\s+([A-Za-z_$][A-Za-z0-9_$]*)"), SymbolKind::Type),
                (re(r"(?m)^\s*export\s+(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)"), SymbolKind::Const),
            ],
            imports: vec![
                re(r#"(?m)^\s*import\s+(?:[^'"]*\s+from\s+)?['"]([^'"]+)['"]"#),
                re(r#"(?m)require\(\s*['"]([^'"]+)['"]\s*\)"#),
                re(r#"(?m)^\s*export\s+[^'"]*\s+from\s+['"]([^'"]+)['"]"#),
            ],
        }),
        Lang::Python => PY.get_or_init(|| LangRules {
            symbols: vec![
                (re(r"(?m)^\s*(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Function),
                (re(r"(?m)^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Type),
                (re(r"(?m)^([A-Z][A-Z0-9_]+)\s*="), SymbolKind::Const),
            ],
            imports: vec![re(r"(?m)^\s*from\s+([\w.]+)\s+import"), re(r"(?m)^\s*import\s+([\w.]+)")],
        }),
        Lang::Go => GO.get_or_init(|| LangRules {
            symbols: vec![
                (re(r"(?m)^func\s+(?:\([^)]*\)\s*)?([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Function),
                (re(r"(?m)^type\s+([A-Za-z_][A-Za-z0-9_]*)\s+(?:struct|interface)"), SymbolKind::Type),
            ],
            imports: vec![re(r#"(?m)^\s*(?:import\s+)?(?:[A-Za-z_]+\s+)?"([^"]+)"\s*$"#)],
        }),
        Lang::Java | Lang::Kotlin => JAVA.get_or_init(|| LangRules {
            symbols: vec![
                (re(r"(?m)^\s*(?:public|private|protected|internal|abstract|final|open|data|sealed|static|\s)*\s*(?:class|interface|enum|record|object)\s+([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Type),
                (re(r"(?m)^\s*(?:public|private|protected|static|final|synchronized|abstract|override|suspend|\s)+[\w<>\[\],\s]*?\s+([a-z][A-Za-z0-9_]*)\s*\([^;{]*\)\s*(?:throws [\w., ]+)?\s*\{"), SymbolKind::Function),
                (re(r"(?m)^\s*(?:override\s+|suspend\s+|private\s+|public\s+)*fun\s+(?:<[^>]*>\s*)?([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Function),
            ],
            imports: vec![re(r"(?m)^\s*import\s+(?:static\s+)?([\w.]+)")],
        }),
        Lang::CSharp => CS.get_or_init(|| LangRules {
            symbols: vec![
                (re(r"(?m)^\s*(?:public|private|protected|internal|abstract|sealed|static|partial|\s)*\s*(?:class|interface|enum|struct|record)\s+([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Type),
                (re(r"(?m)^\s*(?:public|private|protected|internal|static|virtual|override|async|\s)+[\w<>\[\],?\s]*?\s+([A-Z][A-Za-z0-9_]*)\s*\([^;]*\)\s*(?:\{|=>|$)"), SymbolKind::Function),
            ],
            imports: vec![re(r"(?m)^\s*using\s+(?:static\s+)?([\w.]+)\s*;")],
        }),
        Lang::Cpp | Lang::C => CPP.get_or_init(|| LangRules {
            symbols: vec![
                (re(r"(?m)^(?:class|struct)\s+([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Type),
                (re(r"(?m)^[A-Za-z_][\w:<>\*&\s]*?\s+\**([A-Za-z_][A-Za-z0-9_:]*)\s*\([^;]*\)\s*(?:const\s*)?\{"), SymbolKind::Function),
            ],
            imports: vec![re(r#"(?m)^\s*#\s*include\s+[<"]([^>"]+)[>"]"#)],
        }),
        Lang::GdScript => GD.get_or_init(|| LangRules {
            symbols: vec![
                (re(r"(?m)^func\s+([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Function),
                (re(r"(?m)^class_name\s+([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Type),
                (re(r"(?m)^class\s+([A-Za-z_][A-Za-z0-9_]*)"), SymbolKind::Type),
            ],
            imports: vec![re(r#"(?m)(?:preload|load)\(\s*"([^"]+)"\s*\)"#)],
        }),
        Lang::Markdown => MD.get_or_init(|| LangRules {
            symbols: vec![(re(r"(?m)^#{1,3}\s+(.+?)\s*#*\s*$"), SymbolKind::Heading)],
            imports: vec![],
        }),
        _ => return None,
    })
}

/// Extract symbols and imports for a file.
pub fn outline(lang: Lang, text: &str) -> Outline {
    let Some(r) = rules(lang) else {
        return Outline::default();
    };
    // Precompute line starts once for fast offset -> line mapping.
    let mut line_starts = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }
    let line_of = |off: usize| match line_starts.binary_search(&off) {
        Ok(i) => i + 1,
        Err(i) => i,
    };
    let mut symbols = Vec::new();
    for (re, kind) in &r.symbols {
        for c in re.captures_iter(text) {
            if let Some(m) = c.get(1) {
                let name = m.as_str().trim().to_string();
                if name.is_empty() || is_keyword(&name) {
                    continue;
                }
                symbols.push(Symbol {
                    name,
                    kind: *kind,
                    line: line_of(m.start()),
                });
            }
        }
    }
    symbols.sort_by_key(|s| s.line);
    symbols.dedup_by(|a, b| a.name == b.name && a.line == b.line);
    let mut imports = Vec::new();
    for re in &r.imports {
        for c in re.captures_iter(text) {
            if let Some(m) = c.get(1) {
                let s = m.as_str().to_string();
                if !imports.contains(&s) {
                    imports.push(s);
                }
            }
        }
    }
    Outline { symbols, imports }
}

fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "if" | "for"
            | "while"
            | "switch"
            | "return"
            | "catch"
            | "new"
            | "else"
            | "match"
            | "main_loop"
    )
}

/// Split identifiers into lowercase words: `parseHttpRequest` ->
/// [parse, http, request]; `load_config` -> [load, config].
pub fn words(ident: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = ident.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur).to_lowercase());
            }
            continue;
        }
        let boundary = c.is_uppercase()
            && !cur.is_empty()
            && (chars
                .get(i.wrapping_sub(1))
                .is_some_and(|p| p.is_lowercase() || p.is_ascii_digit())
                || chars.get(i + 1).is_some_and(|n| n.is_lowercase()));
        if boundary {
            out.push(std::mem::take(&mut cur).to_lowercase());
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur.to_lowercase());
    }
    out.retain(|w| w.len() > 1);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_outline() {
        let src = "use std::io;\nmod net;\npub struct Server {}\nimpl Server {\n    pub async fn handle_request(&self) {}\n}\npub trait Handler {}\nconst MAX_CONN: usize = 4;\n";
        let o = outline(Lang::Rust, src);
        let names: Vec<_> = o.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Server"));
        assert!(names.contains(&"handle_request"));
        assert!(names.contains(&"Handler"));
        assert!(names.contains(&"MAX_CONN"));
        assert!(o.imports.contains(&"std::io".to_string()));
        assert!(o.imports.contains(&"net".to_string()));
        let h = o
            .symbols
            .iter()
            .find(|s| s.name == "handle_request")
            .unwrap();
        assert_eq!(h.line, 5);
    }

    #[test]
    fn ts_outline() {
        let src = "import { x } from './util';\nexport const login = async (u: string) => {};\nexport class AuthService {}\nexport interface User {}\nfunction helper() {}\n";
        let o = outline(Lang::TypeScript, src);
        let names: Vec<_> = o.symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["login", "AuthService", "User", "helper"]);
        assert_eq!(o.imports, vec!["./util"]);
    }

    #[test]
    fn python_outline() {
        let o = outline(
            Lang::Python,
            "from app.db import x\nimport os\nclass Repo:\n    def save(self):\n        pass\n",
        );
        assert_eq!(o.symbols.len(), 2);
        assert_eq!(o.imports, vec!["app.db", "os"]);
    }

    #[test]
    fn word_split() {
        assert_eq!(words("parseHttpRequest"), vec!["parse", "http", "request"]);
        assert_eq!(words("load_config"), vec!["load", "config"]);
        assert_eq!(words("HTTPServer"), vec!["http", "server"]);
    }
}
