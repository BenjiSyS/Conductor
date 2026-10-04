//! UI themes.
//!
//! Themes are pure data (`theme.json`): a whitelist of design tokens with
//! validated values. No CSS, no scripts, no URLs — so a shared theme cannot
//! execute code or exfiltrate data. The UI applies tokens as CSS custom
//! properties.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::package::{Installed, PackageDir, PackageError};

/// Every token a theme may set. Unknown tokens are rejected so typos surface
/// immediately and themes stay forward-compatible.
pub const TOKENS: &[(&str, TokenKind)] = &[
    ("bg", TokenKind::Color),
    ("bg-elevated", TokenKind::Color),
    ("bg-sunken", TokenKind::Color),
    ("surface", TokenKind::Color),
    ("surface-hover", TokenKind::Color),
    ("border", TokenKind::Color),
    ("border-strong", TokenKind::Color),
    ("text", TokenKind::Color),
    ("text-muted", TokenKind::Color),
    ("text-faint", TokenKind::Color),
    ("accent", TokenKind::Color),
    ("accent-text", TokenKind::Color),
    ("accent-soft", TokenKind::Color),
    ("success", TokenKind::Color),
    ("warning", TokenKind::Color),
    ("danger", TokenKind::Color),
    ("focus", TokenKind::Color),
    ("code-bg", TokenKind::Color),
    ("code-text", TokenKind::Color),
    ("agent-running", TokenKind::Color),
    ("agent-idle", TokenKind::Color),
    ("font-ui", TokenKind::Font),
    ("font-mono", TokenKind::Font),
    ("font-size", TokenKind::Length),
    ("radius-sm", TokenKind::Length),
    ("radius", TokenKind::Length),
    ("radius-lg", TokenKind::Length),
    ("space-unit", TokenKind::Length),
    ("border-width", TokenKind::Length),
    ("ease", TokenKind::Easing),
    ("duration", TokenKind::Duration),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Color,
    Font,
    Length,
    Easing,
    Duration,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeManifest {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// Tokens for light mode.
    #[serde(default)]
    pub light: BTreeMap<String, String>,
    /// Tokens for dark mode.
    #[serde(default)]
    pub dark: BTreeMap<String, String>,
    #[serde(default)]
    pub repository: Option<String>,
}

impl ThemeManifest {
    pub fn load(dir: &Path) -> Result<Self, PackageError> {
        let text = std::fs::read_to_string(dir.join("theme.json"))?;
        let m: ThemeManifest =
            serde_json::from_str(&text).map_err(|e| PackageError::Invalid(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<(), PackageError> {
        if self.light.is_empty() && self.dark.is_empty() {
            return Err(PackageError::Invalid("theme defines no tokens".into()));
        }
        for (mode, map) in [("light", &self.light), ("dark", &self.dark)] {
            for (k, v) in map {
                let kind = TOKENS
                    .iter()
                    .find(|(n, _)| n == k)
                    .map(|(_, kind)| *kind)
                    .ok_or_else(|| {
                        PackageError::Invalid(format!("unknown token '{k}' in {mode}"))
                    })?;
                validate_value(kind, v)
                    .map_err(|why| PackageError::Invalid(format!("{mode}.{k}: {why}")))?;
            }
        }
        Ok(())
    }

    /// CSS custom properties for one mode.
    pub fn css_vars(&self, dark: bool) -> String {
        let map = if dark && !self.dark.is_empty() {
            &self.dark
        } else {
            &self.light
        };
        map.iter()
            .map(|(k, v)| format!("--{k}: {v};"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn validate_value(kind: TokenKind, v: &str) -> Result<(), String> {
    let v = v.trim();
    if v.len() > 120 {
        return Err("value too long".into());
    }
    let lower = v.to_ascii_lowercase();
    for bad in [
        "url(",
        "expression(",
        "javascript:",
        "@import",
        ";",
        "{",
        "}",
        "<",
        ">",
        "\\",
        "/*",
    ] {
        if lower.contains(bad) {
            return Err(format!("'{bad}' is not allowed"));
        }
    }
    let ok = match kind {
        TokenKind::Color => is_color(&lower),
        TokenKind::Font => v
            .chars()
            .all(|c| c.is_alphanumeric() || " ,-_'\"".contains(c)),
        TokenKind::Length => is_length(&lower),
        TokenKind::Easing => {
            matches!(
                lower.as_str(),
                "linear" | "ease" | "ease-in" | "ease-out" | "ease-in-out"
            ) || (lower.starts_with("cubic-bezier(")
                && lower.ends_with(')')
                && lower[13..lower.len() - 1]
                    .split(',')
                    .all(|n| n.trim().parse::<f32>().is_ok()))
        }
        TokenKind::Duration => lower
            .strip_suffix("ms")
            .is_some_and(|n| n.parse::<u32>().is_ok_and(|n| n <= 2000)),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("invalid value '{v}'"))
    }
}

fn is_color(v: &str) -> bool {
    if let Some(h) = v.strip_prefix('#') {
        return matches!(h.len(), 3 | 4 | 6 | 8) && h.chars().all(|c| c.is_ascii_hexdigit());
    }
    for f in ["rgb(", "rgba(", "hsl(", "hsla(", "oklch(", "color-mix("] {
        if let Some(inner) = v.strip_prefix(f).and_then(|r| r.strip_suffix(')')) {
            return inner
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || " ,.%/-#()".contains(c));
        }
    }
    v == "transparent" || v == "currentcolor"
}

fn is_length(v: &str) -> bool {
    for unit in ["px", "rem", "em", "%"] {
        if let Some(n) = v.strip_suffix(unit) {
            return n.parse::<f32>().is_ok_and(|n| (0.0..=200.0).contains(&n));
        }
    }
    v == "0"
}

pub struct Themes {
    dir: PackageDir,
}

impl Themes {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            dir: PackageDir::new(root, "theme.json"),
        }
    }

    fn validate(p: &Path) -> Result<(String, String), PackageError> {
        let m = ThemeManifest::load(p)?;
        // Themes may only contain data files.
        for e in std::fs::read_dir(p)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().to_lowercase();
            let allowed = name.ends_with(".json")
                || name.ends_with(".md")
                || name.ends_with(".png")
                || name.ends_with(".txt")
                || name == "license";
            if !allowed {
                return Err(PackageError::Invalid(format!(
                    "themes may not contain '{name}'"
                )));
            }
        }
        Ok((m.name, m.version))
    }

    pub fn install_dir(&self, src: &Path) -> Result<Installed, PackageError> {
        self.dir
            .install_dir(src, &format!("local:{}", src.display()), &Self::validate)
    }

    pub async fn install_git(
        &self,
        url: &str,
        rev: Option<&str>,
    ) -> Result<Installed, PackageError> {
        self.dir.install_git(url, rev, &Self::validate).await
    }

    pub fn remove(&self, name: &str) -> Result<(), PackageError> {
        self.dir.remove(name)
    }

    pub fn list(&self) -> Vec<ThemeManifest> {
        self.dir
            .installed_names()
            .iter()
            .filter_map(|n| ThemeManifest::load(&self.dir.path_of(n)).ok())
            .collect()
    }

    /// Write a starter theme folder (for "create theme").
    pub fn scaffold(dest: &Path, name: &str) -> Result<(), PackageError> {
        std::fs::create_dir_all(dest)?;
        let mut light = BTreeMap::new();
        let mut dark = BTreeMap::new();
        for (k, l, d) in [
            ("bg", "#fbfaf8", "#151517"),
            ("text", "#1d1d1f", "#ececee"),
            ("accent", "#3d5afe", "#8c9eff"),
            ("border", "#e6e3df", "#2a2a2e"),
        ] {
            light.insert(k.to_string(), l.to_string());
            dark.insert(k.to_string(), d.to_string());
        }
        let m = ThemeManifest {
            name: name.into(),
            version: "0.1.0".into(),
            author: String::new(),
            description: "My Conductor theme".into(),
            light,
            dark,
            repository: None,
        };
        std::fs::write(
            dest.join("theme.json"),
            serde_json::to_string_pretty(&m).map_err(|e| PackageError::Invalid(e.to_string()))?,
        )?;
        std::fs::write(dest.join("README.md"), format!("# {name}\n\nA Conductor theme. Edit `theme.json`, then install it from Settings → Appearance → Install theme.\n"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_install_and_css() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("ocean");
        Themes::scaffold(&src, "ocean").unwrap();
        let t = Themes::new(d.path().join("themes"));
        t.install_dir(&src).unwrap();
        let m = &t.list()[0];
        assert!(m.css_vars(true).contains("--bg: #151517;"));
        assert!(m.css_vars(false).contains("--accent: #3d5afe;"));
    }

    #[test]
    fn malicious_themes_rejected() {
        let mk = |light: &[(&str, &str)]| ThemeManifest {
            name: "x".into(),
            version: "1".into(),
            author: String::new(),
            description: String::new(),
            light: light
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            dark: BTreeMap::new(),
            repository: None,
        };
        assert!(mk(&[("bg", "url(https://evil/x.png)")]).validate().is_err());
        assert!(mk(&[("bg", "#fff; } body { display:none")])
            .validate()
            .is_err());
        assert!(mk(&[("font-ui", "Inter\"; @import 'x'")])
            .validate()
            .is_err());
        assert!(mk(&[("evil-token", "#fff")]).validate().is_err());
        assert!(mk(&[("radius", "9999px")]).validate().is_err());
        assert!(mk(&[("duration", "60000ms")]).validate().is_err());
        assert!(mk(&[
            ("bg", "rgb(10, 20, 30)"),
            ("ease", "cubic-bezier(0.2, 0, 0, 1)"),
            ("radius", "8px"),
            ("duration", "160ms"),
            ("font-ui", "Inter, system-ui")
        ])
        .validate()
        .is_ok());

        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("evil");
        Themes::scaffold(&src, "evil").unwrap();
        std::fs::write(src.join("payload.js"), "alert(1)").unwrap();
        assert!(Themes::new(d.path().join("t")).install_dir(&src).is_err());
    }
}
