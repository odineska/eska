pub mod blank;
pub mod children;
pub mod forms;
pub mod twins;
mod verify;

use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde_json::Value;

use crate::support::TestDir;

pub const MD: &str = "http://v8.1c.ru/8.3/MDClasses";
pub const XR: &str = "http://v8.1c.ru/8.3/xcf/readable";

pub struct Stand {
    pub root: TestDir,
    pub source: PathBuf,
    project: PathBuf,
    cli: PathBuf,
    tool: OsString,
}

impl Stand {
    pub fn from_source(input: &Path) -> Self {
        let stand = Self::prepare();
        copy_tree(input, &stand.source);
        fs::write(
            stand.project.join("eska.toml"),
            "[project]\ntype = \"configuration\"\nsource = \"src\"\n",
        )
        .unwrap();
        stand
    }

    fn prepare() -> Self {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let playground = fs::canonicalize(repo.join("../eska-playground"))
            .expect("repository sibling playground exists");
        let selected = std::env::var_os("ESKA_TEST_ROOT")
            .expect("set ESKA_TEST_ROOT to the repository sibling playground");
        assert_eq!(fs::canonicalize(selected).unwrap(), playground);
        let root = TestDir::new();
        let project = root.0.clone();
        let source = root.0.join("src");
        fs::create_dir(root.0.join("logs")).unwrap();
        let selected_cli = std::env::var_os("ESKA_METADATA_CLI").map_or_else(
            || {
                repo.join(format!(
                    "target/debug/eska-project{}",
                    std::env::consts::EXE_SUFFIX
                ))
            },
            PathBuf::from,
        );
        let tools = root.0.join("native-tools");
        fs::create_dir(&tools).unwrap();
        let cli = tools.join(format!("eska-project{}", std::env::consts::EXE_SUFFIX));
        // A concurrent development build must not change a saved-plan contract mid-run.
        fs::copy(selected_cli, &cli).expect("build eska-project or select ESKA_METADATA_CLI");
        Self {
            root,
            source,
            project,
            cli,
            tool: std::env::var_os("ESKA_NATIVE_IBCMD").expect("select an installed ibcmd"),
        }
    }

    pub fn cli(&self, language: &str, arguments: &[String]) -> Value {
        self.cli_expected(language, arguments, true)
    }

    pub fn cli_error(&self, language: &str, arguments: &[String]) -> Value {
        self.cli_expected(language, arguments, false)
    }

    fn cli_expected(&self, language: &str, arguments: &[String], success: bool) -> Value {
        let output = Command::new(&self.cli)
            .current_dir(&self.project)
            .args(["--lang", language, "metadata"])
            .args(arguments)
            .stdin(Stdio::null())
            .output()
            .expect("build eska-project or select ESKA_METADATA_CLI");
        assert_eq!(
            output.status.code(),
            Some(i32::from(!success)),
            "CLI {arguments:?}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty(), "unexpected stderr: {output:?}");
        let response: Value = serde_json::from_slice(&output.stdout).expect("one JSON envelope");
        assert_eq!(response["schema_version"], 1);
        response
    }

    pub fn mutate(&self, label: &str, arguments: &[String]) -> Value {
        let operation = &arguments[0];
        let mut preview = arguments.to_vec();
        preview.extend(strings(&["--dry-run", "--no-input", "--format", "json"]));
        let plan = self.cli("ru-RU", &preview);
        let path = self.root.0.join("logs").join(format!("{label}.json"));
        fs::write(&path, serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
        let mut replay = strings(&[operation, "--plan"]);
        replay.push(path.to_str().unwrap().to_owned());
        replay.extend(strings(&["--no-input", "--format", "json"]));
        let mut reviewed = replay.clone();
        reviewed.push("--dry-run".to_owned());
        assert_eq!(
            plan,
            self.cli("en-US", &reviewed),
            "saved identities and JSON ignore locale"
        );
        self.cli("en-US", &replay);
        plan
    }

    pub fn property(&self, object: &str, name: &str, change: &Value) -> Value {
        let inspected = self.cli(
            "en-US",
            &strings(&["inspect", "--object", object, "--format", "json"]),
        );
        let editing = &inspected["result"]["editing"];
        let field = editing["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| {
                field["path"]
                    .as_array()
                    .is_some_and(|path| path.len() == 1 && path[0]["key"]["name"] == name)
            })
            .unwrap_or_else(|| panic!("missing shared property writer: {object}.{name}"));
        let request = serde_json::json!({"schemaVersion":1,"objectId":object,"snapshot":editing["snapshot"],"contextSnapshot":editing["contextSnapshot"],"path":field["path"],"change":change});
        let path = self.root.0.join("property-edit.json");
        fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
        self.cli(
            "en-US",
            &strings(&["apply", path.to_str().unwrap(), "--format", "json"]),
        );
        field.clone()
    }

    pub fn native(&self, label: &str, arguments: &[OsString]) {
        let result = Command::new(&self.tool)
            .current_dir(&self.root.0)
            .args(arguments)
            .stdin(Stdio::null())
            .output()
            .expect("execute explicitly selected ibcmd");
        let logs = self.root.0.join("logs");
        fs::write(logs.join(format!("{label}.stdout")), &result.stdout).unwrap();
        fs::write(logs.join(format!("{label}.stderr")), &result.stderr).unwrap();
        assert!(
            result.status.success(),
            "native {label}: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }

    pub fn roundtrip(&self, label: &str, create: bool) -> PathBuf {
        self.native_roundtrip(label, create, true)
    }

    pub fn source_roundtrip(&self, label: &str, create: bool) -> PathBuf {
        self.native_roundtrip(label, create, false)
    }

    fn native_roundtrip(&self, label: &str, create: bool, apply: bool) -> PathBuf {
        let data = option("--data", &self.root.0.join("data"));
        if create {
            self.native(
                "create",
                &["infobase".into(), "create".into(), data.clone()],
            );
        }
        self.native(
            &format!("{label}-import"),
            &[
                "infobase".into(),
                "config".into(),
                "import".into(),
                data.clone(),
                self.source.clone().into(),
            ],
        );
        if apply {
            self.native(
                &format!("{label}-apply"),
                &[
                    "infobase".into(),
                    "config".into(),
                    "apply".into(),
                    data.clone(),
                    "--force".into(),
                ],
            );
        }
        self.native(
            &format!("{label}-save"),
            &[
                "infobase".into(),
                "config".into(),
                "save".into(),
                data.clone(),
                self.root.0.join(format!("{label}.cf")).into(),
            ],
        );
        let export = self.root.0.join(format!("export-{label}"));
        self.native(
            &format!("{label}-export"),
            &[
                "infobase".into(),
                "config".into(),
                "export".into(),
                data,
                export.clone().into(),
            ],
        );
        export
    }
}

pub fn bootstrap() {
    let mut stand = Stand::prepare();
    stand.project = stand.root.0.join("new-project");
    stand.source = stand.project.join("src");
    let selected_wrapper = std::env::var_os("ESKA_NATIVE_ESKA").map_or_else(
        || {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("target/debug/eska{}", std::env::consts::EXE_SUFFIX))
        },
        PathBuf::from,
    );
    let wrapper = stand
        .root
        .0
        .join("native-tools")
        .join(format!("eska{}", std::env::consts::EXE_SUFFIX));
    fs::copy(selected_wrapper, &wrapper).expect("build eska or select ESKA_NATIVE_ESKA");
    let output = Command::new(wrapper)
        .current_dir(&stand.root.0)
        .arg("new")
        .arg(&stand.project)
        .args([
            "--type",
            "configuration",
            "--workflow",
            "trunk",
            "--platform-version",
            "",
            "--no-vcs",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "new scaffold: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!stand.source.join("Configuration.xml").exists());
    stand.mutate(
        "bootstrap",
        &strings(&[
            "create",
            "--kind",
            "configuration",
            "--name",
            "NativeBlankConfiguration",
        ]),
    );
    let source = fs::read_to_string(stand.source.join("Configuration.xml")).unwrap();
    let xml = roxmltree::Document::parse(&source).unwrap();
    let contained: Vec<_> = xml
        .descendants()
        .filter(|node| node.has_tag_name((XR, "ObjectId")))
        .filter_map(|node| node.text())
        .collect();
    assert_eq!(contained.len(), 7);
    assert_eq!(
        contained
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        7
    );
    let export = stand.roundtrip("bootstrap", true);
    let canonical = fs::read_to_string(export.join("Configuration.xml")).unwrap();
    let canonical = roxmltree::Document::parse(&canonical).unwrap();
    for id in contained {
        assert!(
            canonical
                .descendants()
                .any(|node| node.has_tag_name((XR, "ObjectId")) && node.text() == Some(id)),
            "platform changed configuration-contained identity"
        );
    }
}

pub fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn option(name: &str, path: &Path) -> OsString {
    let mut result = OsString::from(name);
    result.push("=");
    result.push(path);
    result
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if [".git", ".eska", ".codex-attachments", "settings_local.toml"]
            .iter()
            .any(|name| entry.file_name() == OsStr::new(name))
        {
            continue;
        }
        let path = destination.join(entry.file_name());
        let kind = entry.file_type().unwrap();
        assert!(
            !kind.is_symlink(),
            "native inputs must not contain symlinks"
        );
        if kind.is_dir() {
            copy_tree(&entry.path(), &path);
        } else {
            fs::copy(entry.path(), path).unwrap();
        }
    }
}
