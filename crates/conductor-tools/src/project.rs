//! Project auto-detection, the `conductor.toml` project configuration file and
//! reusable setup recipes for New Project.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("{0}")]
    Invalid(String),
}

impl ProjectError {
    fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        ProjectError::Io {
            path: path.into(),
            source,
        }
    }
}

type CoreResult<T> = Result<T, ProjectError>;
use ProjectError as CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Cargo,
    Npm,
    Pnpm,
    Yarn,
    Gradle,
    Maven,
    Python,
    CMake,
    Godot,
    Unity,
    Unreal,
    Docker,
    Go,
    DotNet,
}

impl ProjectKind {
    pub fn label(self) -> &'static str {
        match self {
            ProjectKind::Cargo => "Rust (Cargo)",
            ProjectKind::Npm => "Node (npm)",
            ProjectKind::Pnpm => "Node (pnpm)",
            ProjectKind::Yarn => "Node (yarn)",
            ProjectKind::Gradle => "Gradle",
            ProjectKind::Maven => "Maven",
            ProjectKind::Python => "Python",
            ProjectKind::CMake => "CMake",
            ProjectKind::Godot => "Godot",
            ProjectKind::Unity => "Unity",
            ProjectKind::Unreal => "Unreal",
            ProjectKind::Docker => "Docker",
            ProjectKind::Go => "Go",
            ProjectKind::DotNet => ".NET",
        }
    }

    /// Conventional verification commands for this kind of project.
    pub fn default_test_commands(self) -> Vec<&'static str> {
        match self {
            ProjectKind::Cargo => vec!["cargo test"],
            ProjectKind::Npm => vec!["npm test"],
            ProjectKind::Pnpm => vec!["pnpm test"],
            ProjectKind::Yarn => vec!["yarn test"],
            ProjectKind::Gradle => vec!["gradle test"],
            ProjectKind::Maven => vec!["mvn test"],
            ProjectKind::Python => vec!["pytest"],
            ProjectKind::Go => vec!["go test ./..."],
            ProjectKind::DotNet => vec!["dotnet test"],
            ProjectKind::CMake => vec!["cmake --build build"],
            _ => vec![],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Detection {
    pub kinds: Vec<ProjectKind>,
    pub is_git: bool,
    pub has_submodules: bool,
    pub is_monorepo: bool,
    pub has_conductor_toml: bool,
    pub languages: Vec<String>,
    pub suggested_test_commands: Vec<String>,
}

/// Detect project type by looking for well-known marker files. Only the root
/// and one level of children are inspected, so this is cheap on huge repos.
pub fn detect(root: &Path) -> Detection {
    let mut d = Detection {
        is_git: root.join(".git").exists(),
        has_submodules: root.join(".gitmodules").exists(),
        has_conductor_toml: root.join("conductor.toml").exists(),
        ..Default::default()
    };
    fn add_kind(kinds: &mut Vec<ProjectKind>, k: ProjectKind) {
        if !kinds.contains(&k) {
            kinds.push(k);
        }
    }
    let mut kinds: Vec<ProjectKind> = Vec::new();
    let mut add = |k: ProjectKind| add_kind(&mut kinds, k);
    let markers: &[(&str, ProjectKind)] = &[
        ("Cargo.toml", ProjectKind::Cargo),
        ("pnpm-lock.yaml", ProjectKind::Pnpm),
        ("yarn.lock", ProjectKind::Yarn),
        ("build.gradle", ProjectKind::Gradle),
        ("build.gradle.kts", ProjectKind::Gradle),
        ("pom.xml", ProjectKind::Maven),
        ("pyproject.toml", ProjectKind::Python),
        ("requirements.txt", ProjectKind::Python),
        ("setup.py", ProjectKind::Python),
        ("CMakeLists.txt", ProjectKind::CMake),
        ("project.godot", ProjectKind::Godot),
        ("Dockerfile", ProjectKind::Docker),
        ("docker-compose.yml", ProjectKind::Docker),
        ("compose.yaml", ProjectKind::Docker),
        ("go.mod", ProjectKind::Go),
    ];
    for (file, kind) in markers {
        if root.join(file).exists() {
            add(*kind);
        }
    }
    let has_pnpm_or_yarn = root.join("pnpm-lock.yaml").exists() || root.join("yarn.lock").exists();
    if root.join("package.json").exists() && !has_pnpm_or_yarn {
        add(ProjectKind::Npm);
    }
    if root.join("Assets").is_dir() && root.join("ProjectSettings").is_dir() {
        add(ProjectKind::Unity);
    }
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".uproject") {
                add(ProjectKind::Unreal);
            }
            if name.ends_with(".sln") || name.ends_with(".csproj") {
                add(ProjectKind::DotNet);
            }
        }
    }
    d.kinds = kinds;
    // Monorepo heuristics
    if root.join("pnpm-workspace.yaml").exists()
        || root.join("lerna.json").exists()
        || root.join("nx.json").exists()
        || root.join("turbo.json").exists()
    {
        d.is_monorepo = true;
    }
    if let Ok(text) = std::fs::read_to_string(root.join("Cargo.toml")) {
        if text.contains("[workspace]") {
            d.is_monorepo = true;
        }
    }
    let mut langs = Vec::new();
    for k in &d.kinds {
        let l = match k {
            ProjectKind::Cargo => "Rust",
            ProjectKind::Npm | ProjectKind::Pnpm | ProjectKind::Yarn => "JavaScript/TypeScript",
            ProjectKind::Gradle | ProjectKind::Maven => "Java/Kotlin",
            ProjectKind::Python => "Python",
            ProjectKind::CMake => "C/C++",
            ProjectKind::Godot => "GDScript",
            ProjectKind::Unity | ProjectKind::DotNet => "C#",
            ProjectKind::Unreal => "C++",
            ProjectKind::Go => "Go",
            ProjectKind::Docker => continue,
        };
        if !langs.contains(&l.to_string()) {
            langs.push(l.to_string());
        }
    }
    d.languages = langs;
    d.suggested_test_commands = d
        .kinds
        .iter()
        .flat_map(|k| k.default_test_commands())
        .map(String::from)
        .collect();
    d
}

/// `conductor.toml` — committed project configuration. Never contains secrets.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub project: ProjectMeta,
    pub models: ModelPrefs,
    pub verify: VerifyConfig,
    pub context: ContextRules,
    pub permissions: PermissionRules,
    pub mcp: Vec<String>,
    pub skills: Vec<String>,
    pub instructions: InstructionRefs,
    pub env: EnvRequirements,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectMeta {
    pub name: Option<String>,
    pub description: Option<String>,
    pub platforms: Vec<String>,
    pub performance: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelPrefs {
    pub default_combo: Option<String>,
    pub default_model: Option<String>,
    pub preferred_providers: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VerifyConfig {
    /// Commands that make up the Test Gate.
    pub commands: Vec<String>,
    pub startup: Vec<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextRules {
    pub ignore: Vec<String>,
    pub always_include: Vec<String>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionRules {
    pub level: Option<String>,
    pub deny_paths: Vec<String>,
    pub allow_commands: Vec<String>,
    pub deny_commands: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InstructionRefs {
    pub files: Vec<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnvRequirements {
    pub tools: BTreeMap<String, String>,
}

impl ProjectConfig {
    pub fn load(root: &Path) -> CoreResult<Option<Self>> {
        let p = root.join("conductor.toml");
        match std::fs::read_to_string(&p) {
            Ok(text) => {
                let cfg: ProjectConfig = toml::from_str(&text).map_err(|e| CoreError::Parse {
                    path: p.clone(),
                    message: e.to_string(),
                })?;
                cfg.validate()?;
                Ok(Some(cfg))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(CoreError::io(p, e)),
        }
    }

    /// Reject configs that look like they embed secrets.
    pub fn validate(&self) -> CoreResult<()> {
        let text = toml::to_string(self).unwrap_or_default();
        let lowered = text.to_lowercase();
        for needle in ["api_key", "apikey", "password", "secret_key", "sk-", "ghp_"] {
            if lowered.contains(needle) {
                return Err(CoreError::Invalid(format!(
                    "conductor.toml appears to contain a secret ('{needle}'). Store secrets in Settings → Providers instead."
                )));
            }
        }
        Ok(())
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

/// Reusable setup recipe for New Project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub description: String,
    pub files: Vec<(String, String)>,
    pub test_commands: Vec<String>,
    pub env_tools: Vec<String>,
    pub recommended_combo: String,
}

pub fn builtin_recipes() -> Vec<Recipe> {
    vec![
        Recipe {
            id: "empty".into(),
            name: "Empty folder".into(),
            description: "Just a folder with Git.".into(),
            files: vec![("README.md".into(), "# {name}\n".into())],
            test_commands: vec![],
            env_tools: vec!["git".into()],
            recommended_combo: "balanced".into(),
        },
        Recipe {
            id: "rust-desktop".into(),
            name: "Rust app".into(),
            description: "Cargo binary crate.".into(),
            files: vec![
                (
                    "Cargo.toml".into(),
                    "[package]\nname = \"{slug}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n".into(),
                ),
                ("src/main.rs".into(), "fn main() {\n    println!(\"Hello from {name}\");\n}\n".into()),
                (".gitignore".into(), "/target\n".into()),
            ],
            test_commands: vec!["cargo test".into(), "cargo fmt --check".into()],
            env_tools: vec!["cargo".into(), "git".into()],
            recommended_combo: "coding".into(),
        },
        Recipe {
            id: "threejs-game".into(),
            name: "Three.js game".into(),
            description: "Vite + Three.js web game.".into(),
            files: vec![
                (
                    "package.json".into(),
                    "{\n  \"name\": \"{slug}\",\n  \"private\": true,\n  \"type\": \"module\",\n  \"scripts\": { \"dev\": \"vite\", \"build\": \"vite build\" },\n  \"dependencies\": { \"three\": \"latest\" },\n  \"devDependencies\": { \"vite\": \"latest\" }\n}\n".into(),
                ),
                (
                    "index.html".into(),
                    "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{name}</title></head>\n<body style=\"margin:0\"><script type=\"module\" src=\"/main.js\"></script></body></html>\n".into(),
                ),
                (
                    "main.js".into(),
                    "import * as THREE from 'three';\nconst scene = new THREE.Scene();\nconst camera = new THREE.PerspectiveCamera(70, innerWidth / innerHeight, 0.1, 100);\nconst renderer = new THREE.WebGLRenderer({ antialias: true });\nrenderer.setSize(innerWidth, innerHeight);\ndocument.body.appendChild(renderer.domElement);\nconst cube = new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshNormalMaterial());\nscene.add(cube);\ncamera.position.z = 3;\nrenderer.setAnimationLoop(() => { cube.rotation.y += 0.01; renderer.render(scene, camera); });\n".into(),
                ),
                (".gitignore".into(), "node_modules\ndist\n".into()),
            ],
            test_commands: vec!["npm run build".into()],
            env_tools: vec!["node".into(), "npm".into(), "git".into()],
            recommended_combo: "ui_design".into(),
        },
        Recipe {
            id: "godot-web".into(),
            name: "Godot web game".into(),
            description: "Godot 4 project skeleton.".into(),
            files: vec![
                (
                    "project.godot".into(),
                    "config_version=5\n\n[application]\nconfig/name=\"{name}\"\n".into(),
                ),
                (".gitignore".into(), ".godot/\n".into()),
            ],
            test_commands: vec![],
            env_tools: vec!["godot".into(), "git".into()],
            recommended_combo: "coding".into(),
        },
        Recipe {
            id: "python".into(),
            name: "Python package".into(),
            description: "pyproject + pytest.".into(),
            files: vec![
                (
                    "pyproject.toml".into(),
                    "[project]\nname = \"{slug}\"\nversion = \"0.1.0\"\nrequires-python = \">=3.10\"\n".into(),
                ),
                ("tests/test_basic.py".into(), "def test_ok():\n    assert True\n".into()),
                (".gitignore".into(), "__pycache__/\n.venv/\n".into()),
            ],
            test_commands: vec!["pytest".into()],
            env_tools: vec!["python".into(), "git".into()],
            recommended_combo: "coding".into(),
        },
    ]
}

/// Materialise a recipe into an (empty or new) folder.
pub fn apply_recipe(recipe: &Recipe, root: &Path, name: &str) -> CoreResult<Vec<PathBuf>> {
    std::fs::create_dir_all(root).map_err(|e| CoreError::io(root, e))?;
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let slug = if slug.is_empty() {
        "project".to_string()
    } else {
        slug
    };
    let mut written = Vec::new();
    for (rel, content) in &recipe.files {
        if rel.contains("..") || Path::new(rel).is_absolute() {
            return Err(CoreError::Invalid(format!(
                "recipe path escapes project: {rel}"
            )));
        }
        let p = root.join(rel);
        if p.exists() {
            // Never overwrite user files.
            continue;
        }
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
        }
        let body = content.replace("{name}", name).replace("{slug}", &slug);
        std::fs::write(&p, body).map_err(|e| CoreError::io(&p, e))?;
        written.push(p);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_cargo_workspace_and_git() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("Cargo.toml"), "[workspace]\nmembers=[]\n").unwrap();
        std::fs::create_dir(d.path().join(".git")).unwrap();
        std::fs::write(d.path().join("package.json"), "{}").unwrap();
        let det = detect(d.path());
        assert!(det.kinds.contains(&ProjectKind::Cargo));
        assert!(det.kinds.contains(&ProjectKind::Npm));
        assert!(det.is_git);
        assert!(det.is_monorepo);
        assert!(det
            .suggested_test_commands
            .contains(&"cargo test".to_string()));
    }

    #[test]
    fn pnpm_wins_over_npm() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("package.json"), "{}").unwrap();
        std::fs::write(d.path().join("pnpm-lock.yaml"), "").unwrap();
        let det = detect(d.path());
        assert!(det.kinds.contains(&ProjectKind::Pnpm));
        assert!(!det.kinds.contains(&ProjectKind::Npm));
    }

    #[test]
    fn conductor_toml_parse_and_secret_rejection() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("conductor.toml"),
            "[project]\nname='x'\nplatforms=['windows']\n[verify]\ncommands=['cargo test']\n",
        )
        .unwrap();
        let cfg = ProjectConfig::load(d.path()).unwrap().unwrap();
        assert_eq!(cfg.verify.commands, vec!["cargo test"]);
        std::fs::write(
            d.path().join("conductor.toml"),
            "[instructions]\ntext='api_key = sk-abc'\n",
        )
        .unwrap();
        assert!(ProjectConfig::load(d.path()).is_err());
    }

    #[test]
    fn recipe_never_overwrites() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("README.md"), "mine").unwrap();
        let r = &builtin_recipes()[0];
        apply_recipe(r, d.path(), "Demo").unwrap();
        assert_eq!(
            std::fs::read_to_string(d.path().join("README.md")).unwrap(),
            "mine"
        );
        let rust = builtin_recipes()
            .into_iter()
            .find(|r| r.id == "rust-desktop")
            .unwrap();
        apply_recipe(&rust, d.path(), "My App").unwrap();
        let cargo = std::fs::read_to_string(d.path().join("Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"my-app\""));
    }
}
