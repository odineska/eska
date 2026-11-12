#![cfg(unix)]

#[path = "../../../tests/support/mod.rs"]
mod support;

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;
use support::TestDir;

fn apply(root: &Path, lang: &str, args: &[&str]) -> (Output, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_eska-build"))
        .current_dir(root)
        .env("TMPDIR", root)
        .args(["--lang", lang, "apply", "--format", "json"])
        .args(args)
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap();
    (output, value)
}

fn prepare(root: &Path) -> PathBuf {
    for name in [".eska", "src", "test-base"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    fs::write(root.join("test-base/1Cv8.1CD"), "").unwrap();
    fs::write(root.join("eska.toml"), "[project]\ntype = 'configuration'").unwrap();
    fs::write(root.join("src/Configuration.xml"), "<configuration/>").unwrap();
    let module = root.join("src/Module.bsl");
    fs::write(&module, "A = 1;").unwrap();
    let platform = root.join("ibcmd");
    fs::write(
        &platform,
        r#"#!/bin/sh
case "$1:$2:$3" in
  --version::) printf '8.3.27.2342\n' ;;
  infobase:config:generation-id)
    if test -f edit-after-staging; then printf 'A = 3;' > src/Module.bsl; fi
    if test -f generation-unavailable; then exit 3; fi
    if test -f generation-override; then
      cat generation-override
    else
      printf '0123456789012345678901234567890123456789\n'
    fi ;;
  infobase:config:apply)
    if test -f pause-apply; then
      touch apply-ready
      exec sleep 30
    fi
    test ! -f fail-apply ;;
  infobase:config:import)
    if test "$4" = files; then
      dir=$(dirname "$0")
      cat > "$dir/import-files"
      if test -f "$dir/edit-after-staging"; then
        find . -type f | LC_ALL=C sort > "$dir/staged-files"
        cat Module.bsl > "$dir/staged-module"
      fi
    fi ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&platform, fs::Permissions::from_mode(0o700)).unwrap();
    let path = serde_json::to_string(&platform).unwrap();
    fs::write(
        root.join(".eska/settings_local.toml"),
        format!(
            "[test.infobase]\nkind='file'\npath='test-base'\n[test.runner]\nkind='local'\nversion='8.3.27.2342'\nibcmd={path}\n"
        ),
    )
    .unwrap();
    module
}

fn initialize(root: &Path) {
    let (_, initial) = apply(root, "en-US", &["--initialize"]);
    let token = initial["plan"]["confirmation"].as_str().unwrap();
    assert!(
        apply(root, "en-US", &["--initialize", "--confirm", token])
            .0
            .status
            .success()
    );
}

#[test]
fn apply_stages_only_the_reviewed_module_and_imports_its_frozen_bytes() {
    let root = TestDir::new();
    let root = &root.0;
    let module = prepare(root);
    fs::write(
        root.join("src/Configuration.xml"),
        r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses"><Configuration><Properties><DefaultLanguage>Language.Russian</DefaultLanguage></Properties><ChildObjects><Language>Russian</Language></ChildObjects></Configuration></MetaDataObject>"#,
    )
    .unwrap();
    fs::write(root.join("src/Other.bsl"), "B = 1;").unwrap();
    initialize(root);
    fs::write(&module, "A = 2;").unwrap();
    let (_, preview) = apply(root, "en-US", &[]);
    let token = preview["plan"]["confirmation"].as_str().unwrap();
    assert_eq!(
        preview["plan"]["import_files"],
        serde_json::json!(["Module.bsl"])
    );

    fs::write(root.join("edit-after-staging"), "").unwrap();
    let (output, applied) = apply(root, "ru-RU", &["--confirm", token]);
    assert!(output.status.success(), "{applied}");
    assert_eq!(
        fs::read_to_string(root.join("staged-files")).unwrap(),
        "./Module.bsl\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("staged-module")).unwrap(),
        "A = 2;"
    );
    assert_eq!(fs::read_to_string(&module).unwrap(), "A = 3;");
    fs::remove_file(root.join("edit-after-staging")).unwrap();

    let (_, next) = apply(root, "en-US", &[]);
    assert_eq!(
        next["plan"]["changes"],
        serde_json::json!({"Module.bsl":"modified"})
    );
    assert_ne!(next["plan"]["confirmation"], token);
}

#[test]
fn run_rejects_a_test_infobase_that_changed_after_apply() {
    let root = TestDir::new();
    let root = &root.0;
    prepare(root);
    initialize(root);
    let (_, unchanged) = apply(root, "en-US", &[]);
    let launch = &unchanged["plan"]["launch"];
    assert_eq!(launch["project_dir"], root.to_str().unwrap());
    let expected = launch["fingerprint"].as_str().unwrap();
    let client = root.join("1cv8c");
    fs::write(
        &client,
        "#!/bin/sh\nprintf '%s\\n' \"$2\" \"$3\" > \"$(dirname \"$0\")/launched\"\n",
    )
    .unwrap();
    fs::set_permissions(&client, fs::Permissions::from_mode(0o700)).unwrap();
    let run = |lang| {
        Command::new(env!("CARGO_BIN_EXE_eska-build"))
            .current_dir(root)
            .args([
                "--lang",
                lang,
                "run",
                "--format",
                "json",
                "--expected-infobase",
                expected,
            ])
            .output()
            .unwrap()
    };
    let output = run("en-US");
    assert!(output.status.success(), "{output:?}");
    let marker = root.join("launched");
    assert_eq!(
        fs::read_to_string(&marker).unwrap(),
        format!("/F\n{}\n", root.join("test-base").display())
    );
    fs::remove_file(&marker).unwrap();
    fs::create_dir(root.join("other-base")).unwrap();
    fs::write(root.join("other-base/1Cv8.1CD"), "").unwrap();
    let settings = root.join(".eska/settings_local.toml");
    let original = fs::read_to_string(&settings).unwrap();
    fs::write(settings, original.replace("test-base", "other-base")).unwrap();
    for lang in ["en-US", "ru-RU"] {
        let output = run(lang);
        assert!(!output.status.success());
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["error"]["code"], "launch_target_changed");
    }
    assert!(!marker.exists());
}

fn prepare_extensions(root: &Path) -> (PathBuf, PathBuf) {
    let base = root.join("base");
    let one = root.join("one");
    let two = root.join("two");
    fs::create_dir(&base).unwrap();
    prepare(&base);
    fs::write(
        base.join("eska.toml"),
        "[project]\nname='base'\ntype='configuration'\n",
    )
    .unwrap();
    fs::write(
        base.join("src/Configuration.xml"),
        r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses"><Configuration uuid="eec8818c-9361-478d-b14f-01d342006de0"><Properties><Name>Base</Name></Properties></Configuration></MetaDataObject>"#,
    )
    .unwrap();
    fs::write(
        root.join("eska.toml"),
        "[workspace.members.probe]\nconfiguration='base'\nextensions=[{path='one', depends_on=['two']}, {path='two'}]\n",
    )
    .unwrap();
    for (member, id, name, uuid) in [
        (&one, "one", "One", "eec8818c-9361-478d-b14f-01d342006de1"),
        (&two, "two", "Two", "eec8818c-9361-478d-b14f-01d342006de2"),
    ] {
        fs::create_dir_all(member.join("src")).unwrap();
        fs::write(
            member.join("eska.toml"),
            format!("[project]\nname='{id}'\ntype='extension'\n"),
        )
        .unwrap();
        fs::write(
            member.join("src/Configuration.xml"),
            format!(
                r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses"><Configuration uuid="{uuid}"><Properties><Name>{name}</Name><ConfigurationExtensionPurpose>Customization</ConfigurationExtensionPurpose></Properties></Configuration></MetaDataObject>"#
            ),
        )
        .unwrap();
        fs::write(member.join("src/Module.bsl"), "A = 1;").unwrap();
        fs::copy(
            member.join("src/Configuration.xml"),
            base.join(format!("installed-root-{name}.xml")),
        )
        .unwrap();
        fs::write(
            base.join(format!("installed-{name}.xml")),
            format!(
                r#"<ConfigDumpInfo xmlns="http://v8.1c.ru/8.3/xcf/dumpinfo"><ConfigVersions><Metadata name="Configuration.{name}" id="{uuid}"/></ConfigVersions></ConfigDumpInfo>"#
            ),
        )
        .unwrap();
    }
    fs::write(
        base.join("ibcmd"),
        r#"#!/bin/sh
dir=$(dirname "$0")
name=base
standalone=false
case "$1" in
    --pid=*) standalone=true; shift ;;
esac
for arg in "$@"; do
    case "$arg" in
        --extension=*) name=${arg#--extension=} ;;
        --out=*) out=${arg#--out=} ;;
    esac
done
case "$1:$2:$3" in
    --version::) printf '8.3.27.2342\n' ;;
    infobase:config:generation-id) cat "$dir/generation-$name" ;;
    infobase:config:export)
        case "$standalone:$4:$5" in
            true:objects:Configuration) cp "$dir/installed-root-$name.xml" "$out/Configuration.xml" ;;
            true:info:*) exit 0 ;;
            false:info:*) cp "$dir/installed-$name.xml" "$out/ConfigDumpInfo.xml" ;;
            *) exit 3 ;;
        esac ;;
    infobase:config:import|infobase:config:apply)
        printf '%s:%s\n' "$3" "$name" >> "$dir/writes"
        if test "$3" = apply; then
            if test -f "$dir/fail-$name"; then exit 4; fi
            if test -f "$dir/advance-generations"; then
                tr 0123456789abcdef 123456789abcdef0 < "$dir/generation-$name" > "$dir/next-generation"
                mv "$dir/next-generation" "$dir/generation-$name"
            fi
        fi
        if test "$4" = files; then cat > "$dir/import-files"; fi ;;
    *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    for (name, digit) in [("base", "0"), ("One", "1"), ("Two", "2")] {
        fs::write(base.join(format!("generation-{name}")), digit.repeat(40)).unwrap();
    }
    (base, one)
}

fn apply_application(root: &Path, lang: &str, args: &[&str]) -> (Output, Value) {
    let mut command = vec!["--application", "probe"];
    command.extend_from_slice(args);
    apply(root, lang, &command)
}

#[test]
fn apply_application_orders_components_and_resumes_after_partial_success() {
    let root = TestDir::new();
    let root = &root.0;
    let (base, one) = prepare_extensions(root);
    let two = root.join("two");
    fs::write(base.join("advance-generations"), "").unwrap();
    let writes = base.join("writes");
    let (output, prod) = apply_application(root, "ru-RU", &["--profile", "prod", "--initialize"]);
    assert!(!output.status.success());
    assert_eq!(prod["error"]["code"], "forbidden_action");
    assert!(!writes.exists());
    let (_, blocked) = apply_application(root, "en-US", &[]);
    assert!(blocked["plan"]["confirmation"].is_null());
    assert_eq!(
        blocked["plan"]["components"][0]["review"]["reason"],
        "baseline_required"
    );

    let (output, initial) = apply_application(root, "en-US", &["--initialize"]);
    assert!(output.status.success(), "{initial}");
    assert_eq!(
        initial,
        apply_application(root, "ru-RU", &["--initialize"]).1
    );
    let token = initial["plan"]["confirmation"].as_str().unwrap();
    let (output, applied) = apply_application(root, "en-US", &["--initialize", "--confirm", token]);
    assert!(output.status.success(), "{applied}");
    assert_eq!(
        fs::read_to_string(&writes).unwrap(),
        "import:base\napply:base\nimport:Two\napply:Two\nimport:One\napply:One\n"
    );
    let (_, unchanged) = apply_application(root, "en-US", &["--initialize"]);
    assert!(unchanged["plan"]["confirmation"].is_null());
    assert_eq!(
        unchanged["plan"]["components"][1]["plan"]["mode"],
        "unchanged"
    );

    fs::remove_file(&writes).unwrap();
    for member in [&base, &one, &two] {
        fs::write(member.join("src/Module.bsl"), "A = 2;").unwrap();
    }
    let (_, preview) = apply_application(root, "en-US", &[]);
    let token = preview["plan"]["confirmation"].as_str().unwrap();
    fs::write(one.join("src/Module.bsl"), "A = 3;").unwrap();
    let (output, stale) = apply_application(root, "en-US", &["--confirm", token]);
    assert!(!output.status.success());
    assert_eq!(stale["error"]["code"], "stale_preview");
    assert!(!writes.exists());

    let failure = base.join("fail-Two");
    fs::write(&failure, "").unwrap();
    let (_, preview) = apply_application(root, "en-US", &[]);
    let token = preview["plan"]["confirmation"].as_str().unwrap();
    let (output, failed) = apply_application(root, "en-US", &["--confirm", token]);
    assert!(!output.status.success());
    assert_eq!(failed["error"]["code"], "apply_failed");
    assert_eq!(failed["error"]["component"], "two");
    assert_eq!(
        failed["outcomes"],
        serde_json::json!([
            {"member":"base","status":"applied"},
            {"member":"two","status":"failed"},
            {"member":"one","status":"not_started"}
        ])
    );

    fs::remove_file(failure).unwrap();
    let (_, blocked) = apply_application(root, "en-US", &[]);
    assert_eq!(
        blocked["plan"]["components"][1]["review"]["reason"],
        "incomplete"
    );
    assert!(blocked["plan"]["confirmation"].is_null());
    let (_, recovery) = apply_application(root, "en-US", &["--initialize"]);
    let components = &recovery["plan"]["components"];
    assert_eq!(components[0]["plan"]["mode"], "unchanged");
    assert_eq!(components[1]["plan"]["mode"], "initialize");
    assert_eq!(components[2]["plan"]["mode"], "incremental");
    let token = recovery["plan"]["confirmation"].as_str().unwrap();
    let (output, resumed) = apply_application(root, "en-US", &["--initialize", "--confirm", token]);
    assert!(output.status.success(), "{resumed}");
    assert_eq!(
        fs::read_to_string(writes).unwrap(),
        "import:base\napply:base\nimport:Two\napply:Two\nimport:Two\napply:Two\nimport:One\napply:One\n"
    );
}

#[test]
fn apply_extension_uses_the_base_profile_and_revalidates_native_identity_and_dependencies() {
    let root = TestDir::new();
    let (base, one) = prepare_extensions(&root.0);
    let (output, initial) = apply(&one, "en-US", &["--initialize"]);
    assert!(output.status.success(), "{initial}");
    assert_eq!(apply(&one, "ru-RU", &["--initialize"]).1, initial);
    assert_eq!(initial["plan"]["extension"]["name"], "One");
    assert_eq!(
        initial["plan"]["launch"]["project_dir"],
        base.to_str().unwrap()
    );
    assert_eq!(
        initial["plan"]["extension"]["uuid"],
        "eec8818c-9361-478d-b14f-01d342006de1"
    );
    let inspection = &initial["plan"]["inspection"];
    assert_eq!(inspection["configuration_generation"], "0".repeat(40));
    assert_eq!(inspection["dependency_generations"]["Two"], "2".repeat(40));
    let token = initial["plan"]["confirmation"].as_str().unwrap();
    let (output, wrong_target) = apply(
        &root.0.join("two"),
        "en-US",
        &["--initialize", "--confirm", token],
    );
    assert!(!output.status.success());
    assert_eq!(wrong_target["error"]["code"], "stale_preview");

    fs::write(base.join("generation-Two"), "3".repeat(40)).unwrap();
    let (output, stale) = apply(&one, "en-US", &["--initialize", "--confirm", token]);
    assert!(!output.status.success());
    assert_eq!(stale["error"]["code"], "stale_preview");
    assert!(!base.join("writes").exists());
    initialize(&one);
    assert!(!one.join(".eska").exists());

    fs::write(one.join("src/Module.bsl"), "A = 2;").unwrap();
    let (_, incremental) = apply(&one, "en-US", &[]);
    assert_eq!(incremental["plan"]["mode"], "incremental");
    assert_eq!(incremental["plan"]["import_files"][0], "Module.bsl");
    let token = incremental["plan"]["confirmation"].as_str().unwrap();
    let descriptor = base.join("installed-Two.xml");
    let verified = fs::read_to_string(&descriptor).unwrap();
    fs::write(&descriptor, verified.replace("6de2", "6de3")).unwrap();
    let (output, mismatch) = apply(&one, "en-US", &["--confirm", token]);
    assert!(!output.status.success());
    assert_eq!(mismatch["error"]["code"], "extension_identity_mismatch");
    let writes = base.join("writes");
    assert_eq!(
        fs::read_to_string(&writes).unwrap(),
        "import:One\napply:One\n"
    );
    fs::write(&descriptor, verified).unwrap();
    assert!(
        apply(&one, "en-US", &["--confirm", token])
            .0
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(writes).unwrap(),
        "import:One\napply:One\nimport:One\napply:One\n"
    );
}

#[test]
fn apply_extension_checks_the_standalone_configuration_object() {
    let root = TestDir::new();
    let (base, one) = prepare_extensions(&root.0);
    let settings = base.join(".eska/settings_local.toml");
    let original = fs::read_to_string(&settings).unwrap();
    fs::write(
        settings,
        original.replace(
            "kind='file'\npath='test-base'",
            "kind='standalone'\nurl='http://127.0.0.1:8314'\nname='eska-test'\nadmin_pid=1",
        ),
    )
    .unwrap();
    let (output, plan) = apply(&one, "en-US", &["--initialize"]);
    assert!(output.status.success(), "{plan}");
    let token = plan["plan"]["confirmation"].as_str().unwrap();

    let installed = base.join("installed-root-One.xml");
    let descriptor = fs::read_to_string(&installed).unwrap();
    fs::write(
        &installed,
        descriptor.replace("<Name>One</Name>", "<Name>Wrong</Name>"),
    )
    .unwrap();
    let (output, mismatch) = apply(&one, "en-US", &["--initialize", "--confirm", token]);
    assert!(!output.status.success());
    assert_eq!(mismatch["error"]["code"], "extension_identity_mismatch");
    assert!(!base.join("writes").exists());

    fs::write(installed, descriptor).unwrap();
    assert!(
        apply(&one, "en-US", &["--initialize", "--confirm", token])
            .0
            .status
            .success()
    );
}

#[test]
fn apply_extension_imports_only_an_existing_role_descriptor_without_resources() {
    let root = TestDir::new();
    let (base, one) = prepare_extensions(&root.0);
    let roles = one.join("src/Roles");
    fs::create_dir(&roles).unwrap();
    let role = roles.join("One_DefaultRole.xml");
    let descriptor = r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses"><Role uuid="97bcf1fc-5482-4ff0-b651-876c772bd338"><Properties><Name>One_DefaultRole</Name><Comment/></Properties></Role></MetaDataObject>"#;
    fs::write(&role, descriptor).unwrap();
    initialize(&one);

    let edited = descriptor.replace("<Comment/>", "<Comment>Updated</Comment>");
    fs::write(&role, &edited).unwrap();
    let (output, incremental) = apply(&one, "en-US", &[]);
    assert!(output.status.success(), "{incremental}");
    assert_eq!(incremental["plan"]["mode"], "incremental");
    assert_eq!(
        incremental["plan"]["import_files"],
        serde_json::json!(["Roles/One_DefaultRole.xml"])
    );
    let token = incremental["plan"]["confirmation"].as_str().unwrap();
    assert!(
        apply(&one, "en-US", &["--confirm", token])
            .0
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(base.join("import-files")).unwrap(),
        "Roles/One_DefaultRole.xml\n"
    );
    fs::write(&role, edited.replace("72bd338", "72bd339")).unwrap();
    let (_, replaced) = apply(&one, "en-US", &[]);
    assert_eq!(replaced["plan"]["mode"], "unsupported");
    assert!(replaced["plan"]["confirmation"].is_null());

    fs::write(&role, &edited).unwrap();
    let resources = roles.join("One_DefaultRole/Ext");
    fs::create_dir_all(&resources).unwrap();
    fs::write(resources.join("Rights.xml"), "<Rights/>").unwrap();
    initialize(&one);
    fs::write(&role, descriptor).unwrap();
    let (_, with_resources) = apply(&one, "en-US", &[]);
    assert_eq!(with_resources["plan"]["mode"], "unsupported");
}

#[test]
fn apply_proves_structural_identity_and_keeps_unrelated_root_xml_out_of_the_plan() {
    let root = TestDir::new();
    let root = &root.0;
    prepare(root);
    let source = root.join("src");
    let languages = source.join("Languages");
    fs::create_dir(&languages).unwrap();
    let default = "9830bf6c-f182-4be1-9d98-642d2985cbca";
    let new = "1130bf6c-f182-4be1-9d98-642d2985cbca";
    let language = |name, uuid| {
        format!(
            r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses"><Language uuid="{uuid}"><Properties><Name>{name}</Name></Properties></Language></MetaDataObject>"#
        )
    };
    let configuration = |name, children| {
        format!(
            r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses"><Configuration uuid="{default}"><Properties><Name>{name}</Name><DefaultLanguage>Language.Default</DefaultLanguage></Properties><ChildObjects><Language>Default</Language>{children}</ChildObjects></Configuration></MetaDataObject>"#
        )
    };
    let main = source.join("Configuration.xml");
    fs::write(languages.join("Default.xml"), language("Default", default)).unwrap();
    fs::write(&main, configuration("Base", "")).unwrap();
    initialize(root);

    let added = languages.join("Added.xml");
    fs::write(&added, language("Added", default)).unwrap();
    fs::write(&main, configuration("Base", "<Language>Added</Language>")).unwrap();
    let (_, preview) = apply(root, "en-US", &[]);
    assert_eq!(preview["plan"]["mode"], "unsupported");

    fs::write(&added, language("Added", new)).unwrap();
    fs::write(&main, configuration("Other", "<Language>Added</Language>")).unwrap();
    let (_, preview) = apply(root, "en-US", &[]);
    assert_eq!(preview["plan"]["mode"], "unsupported");
    fs::write(&main, configuration("Base", "<Language>Added</Language>")).unwrap();

    for (operation, name, previous, expected) in [
        ("added", "Added", None, "Languages/Added.xml"),
        ("renamed", "Renamed", Some("Added"), "Languages/Renamed.xml"),
        ("removed", "Renamed", None, "Languages/Default.xml"),
    ] {
        if operation == "renamed" {
            fs::remove_file(&added).unwrap();
            fs::write(languages.join("Renamed.xml"), language("Renamed", new)).unwrap();
            fs::write(&main, configuration("Base", "<Language>Renamed</Language>")).unwrap();
        } else if operation == "removed" {
            fs::remove_file(languages.join("Renamed.xml")).unwrap();
            fs::write(&main, configuration("Base", "")).unwrap();
        }
        let (out, preview) = apply(root, "en-US", &[]);
        assert!(out.status.success());
        assert_eq!(preview, apply(root, "ru-RU", &[]).1);
        let plan = &preview["plan"];
        assert_eq!(plan["mode"], "incremental");
        assert_eq!(
            plan["import_files"],
            serde_json::json!(["Configuration.xml", expected])
        );
        let group = &plan["object_changes"][0];
        assert_eq!(group["kind"], "language");
        assert_eq!(group["operation"], operation);
        assert_eq!(group["name"], name);
        assert_eq!(group["previous_name"], serde_json::json!(previous));
        let token = plan["confirmation"].as_str().unwrap();
        assert!(
            apply(root, "ru-RU", &["--confirm", token])
                .0
                .status
                .success()
        );
        assert_eq!(apply(root, "en-US", &[]).1["plan"]["mode"], "unchanged");
    }
}

#[test]
fn apply_imports_existing_metadata_properties_and_their_form_resource() {
    let root = TestDir::new();
    let root = &root.0;
    prepare(root);
    let source = root.join("src");
    let descriptor = "CommonForms/ProbeButtons.xml";
    let form = "CommonForms/ProbeButtons/Ext/Form.xml";
    let original = include_str!(
        "../../../tests/fixtures/forms/native-button-rows/source/CommonForms/ProbeButtons.xml"
    );
    fs::create_dir_all(source.join(form).parent().unwrap()).unwrap();
    fs::write(source.join(descriptor), original).unwrap();
    fs::write(source.join(form), "<Form/>").unwrap();
    initialize(root);
    let edited = original.replace("<Comment/>", "<Comment>Updated</Comment>");
    assert_ne!(edited, original);
    fs::write(source.join(descriptor), &edited).unwrap();
    fs::write(source.join(form), "<Form><Title>Updated</Title></Form>").unwrap();
    let (_, preview) = apply(root, "en-US", &[]);
    assert_eq!(preview["plan"]["mode"], "incremental");
    let token = preview["plan"]["confirmation"].as_str().unwrap();
    let (_, localized) = apply(root, "ru-RU", &[]);
    assert_eq!(preview, localized);
    let list = root.join("import-files");
    assert!(!list.exists());
    assert!(
        apply(root, "en-US", &["--confirm", token])
            .0
            .status
            .success()
    );
    let files = fs::read_to_string(&list).unwrap();
    assert_eq!(
        files.lines().collect::<std::collections::BTreeSet<_>>(),
        [descriptor, form].into_iter().collect()
    );

    let renamed = edited.replace("<Name>ProbeButtons</Name>", "<Name>Another</Name>");
    assert_ne!(renamed, edited);
    fs::write(source.join(descriptor), renamed).unwrap();
    let (_, preview) = apply(root, "en-US", &[]);
    assert_eq!(preview["plan"]["mode"], "unsupported");
    assert!(preview["plan"]["confirmation"].is_null());

    let structure = edited.replace(
        "</CommonForm>",
        "<ChildObjects><Form>NewChild</Form></ChildObjects></CommonForm>",
    );
    assert_ne!(structure, edited);
    fs::write(source.join(descriptor), structure).unwrap();
    let (_, preview) = apply(root, "en-US", &[]);
    assert_eq!(preview["plan"]["mode"], "unsupported");
}

#[test]
fn apply_reviews_and_imports_unchanged_catalog_companions_in_platform_order() {
    let root = TestDir::new();
    let root = &root.0;
    prepare(root);
    let source = root.join("src");
    let catalog = "Catalogs/ProbeOwner.xml";
    let catalog_form = "Catalogs/ProbeOwner/Forms/ProbeObject.xml";
    let catalog_layout = "Catalogs/ProbeOwner/Forms/ProbeObject/Ext/Form.xml";
    let catalog_original = include_str!(
        "../../../tests/fixtures/forms/native-owner-composition/base/Catalogs/ProbeOwner.xml"
    );
    fs::create_dir_all(source.join(catalog_layout).parent().unwrap()).unwrap();
    fs::write(source.join(catalog), catalog_original).unwrap();
    fs::write(
        source.join(catalog_form),
        include_str!(
            "../../../tests/fixtures/forms/native-owner-composition/base/Catalogs/ProbeOwner/Forms/ProbeObject.xml"
        ),
    )
    .unwrap();
    fs::write(source.join(catalog_layout), "<Form/>").unwrap();
    initialize(root);
    let list = root.join("import-files");
    fs::write(
        source.join(catalog),
        catalog_original.replace("<Comment/>", "<Comment>Catalog property</Comment>"),
    )
    .unwrap();
    let (_, preview) = apply(root, "ru-RU", &[]);
    assert_eq!(preview["plan"]["mode"], "incremental");
    assert_eq!(
        preview["plan"]["changes"],
        serde_json::json!({(catalog):"modified"})
    );
    assert_eq!(
        preview["plan"]["import_files"],
        serde_json::json!([catalog, catalog_form, catalog_layout])
    );
    let token = preview["plan"]["confirmation"].as_str().unwrap();
    assert!(
        apply(root, "en-US", &["--confirm", token])
            .0
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(list)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        [catalog, catalog_form, catalog_layout]
    );
}

#[test]
fn apply_reports_saved_source_and_database_changes_independently() {
    let root = TestDir::new();
    let root = &root.0;
    let module = prepare(root);

    let (output, no_baseline) = apply(root, "en-US", &[]);
    assert!(!output.status.success());
    assert_eq!(no_baseline["error"]["code"], "baseline_required");
    assert_eq!(
        no_baseline["review"]["inspection"]["status"],
        "unknown_baseline"
    );
    assert_eq!(no_baseline["review"]["inspection"]["sources"], "unknown");
    assert!(no_baseline["review"]["inspection"]["database_generation"].is_string());
    assert!(no_baseline["review"]["changes"].is_null());

    let (_, initial) = apply(root, "en-US", &["--initialize"]);
    let token = initial["plan"]["confirmation"].as_str().unwrap();
    assert!(
        apply(root, "en-US", &["--initialize", "--confirm", token])
            .0
            .status
            .success()
    );
    let (_, unchanged) = apply(root, "en-US", &[]);
    assert_eq!(unchanged["plan"]["inspection"]["status"], "up_to_date");
    assert_eq!(
        unchanged["plan"]["inspection"]["baseline_generation"],
        unchanged["plan"]["inspection"]["database_generation"]
    );
    let generation = root.join("generation-override");
    fs::write(&generation, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n").unwrap();
    let (_, external) = apply(root, "en-US", &[]);
    assert_eq!(external["error"]["code"], "baseline_changed");
    assert_eq!(
        external["review"]["inspection"]["status"],
        "database_changed"
    );
    assert_eq!(external["review"]["changes"], serde_json::json!({}));

    let modified = fs::metadata(&module).unwrap().modified().unwrap();
    fs::write(&module, "A = 2;").unwrap();
    fs::File::options()
        .write(true)
        .open(&module)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let (english, both) = apply(root, "en-US", &[]);
    let (russian, localized) = apply(root, "ru-RU", &[]);
    assert!(!english.status.success() && !russian.status.success());
    assert_ne!(english.stderr, russian.stderr);
    assert_eq!(both, localized);
    assert_eq!(both["error"]["code"], "baseline_changed");
    assert_eq!(both["review"]["inspection"]["status"], "both_changed");
    assert_eq!(both["review"]["inspection"]["sources"], "changed");
    assert_eq!(both["review"]["inspection"]["database"], "changed");
    assert_eq!(both["review"]["changes"]["Module.bsl"], "modified");

    fs::remove_file(generation).unwrap();
    let (_, local) = apply(root, "en-US", &[]);
    assert_eq!(local["plan"]["inspection"]["status"], "local_changes");
    assert_eq!(local["plan"]["inspection"]["database"], "unchanged");
    fs::write(root.join("generation-unavailable"), "").unwrap();
    let (_, unavailable) = apply(root, "en-US", &[]);
    assert_eq!(unavailable["error"]["code"], "generation_failed");
    assert_eq!(
        unavailable["review"]["inspection"]["status"],
        "database_unavailable"
    );
    assert_eq!(unavailable["review"]["inspection"]["sources"], "changed");
    assert_eq!(unavailable["review"]["inspection"]["database"], "unknown");
    assert!(unavailable["review"]["inspection"]["database_generation"].is_null());
    assert_eq!(unavailable["review"]["changes"]["Module.bsl"], "modified");
}

#[test]
fn full_import_confirmation_requires_reviewing_a_new_pending_operation() {
    let root = TestDir::new();
    let root = &root.0;
    prepare(root);
    let (_, preview) = apply(root, "en-US", &["--initialize"]);
    let token = preview["plan"]["confirmation"].as_str().unwrap();
    fs::write(root.join("fail-apply"), "").unwrap();
    let (_, failed) = apply(root, "en-US", &["--initialize", "--confirm", token]);
    assert_eq!(failed["error"]["code"], "apply_failed");
    fs::remove_file(root.join("fail-apply")).unwrap();
    let (output, stale) = apply(root, "en-US", &["--initialize", "--confirm", token]);
    assert!(!output.status.success());
    assert_eq!(stale["error"]["code"], "stale_preview");
    let (_, recovery) = apply(root, "en-US", &["--initialize"]);
    assert_eq!(recovery["plan"]["inspection"]["status"], "incomplete");
    assert_ne!(recovery["plan"]["confirmation"], token);
}

#[test]
fn failed_apply_preserves_the_last_snapshot_and_requires_review() {
    let root = TestDir::new();
    let root = &root.0;
    let module = prepare(root);

    let (output, initial) = apply(root, "en-US", &["--initialize"]);
    assert!(output.status.success(), "{output:?}");
    let token = initial["plan"]["confirmation"].as_str().unwrap();
    let (output, _) = apply(root, "ru-RU", &["--initialize", "--confirm", token]);
    assert!(output.status.success(), "{output:?}");
    let phases: Vec<Value> = String::from_utf8(output.stderr)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(phases[0]["phase"], "import");
    assert_eq!(phases[1]["phase"], "apply");

    let snapshot = fs::read_dir(root.join(".eska"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .unwrap();
    let before = fs::read(&snapshot).unwrap();
    fs::write(&module, "A = 2;").unwrap();
    let (_, plan) = apply(root, "en-US", &[]);
    assert_eq!(plan["plan"]["mode"], "incremental");
    let token = plan["plan"]["confirmation"].as_str().unwrap();
    fs::write(root.join("fail-apply"), "").unwrap();
    let (output, failure) = apply(root, "en-US", &["--confirm", token]);
    assert!(!output.status.success());
    assert_eq!(failure["error"]["code"], "apply_failed");
    assert_eq!(fs::read(&snapshot).unwrap(), before);

    let (english, error) = apply(root, "en-US", &[]);
    let (russian, localized_error) = apply(root, "ru-RU", &[]);
    assert!(!english.status.success() && !russian.status.success());
    assert_ne!(english.stderr, russian.stderr);
    assert_eq!(error, localized_error);
    assert_eq!(error["error"]["code"], "incomplete_apply");
    assert_eq!(error["review"]["inspection"]["status"], "incomplete");
    assert_eq!(error["review"]["inspection"]["pending_apply"], true);
    assert_eq!(error["review"]["inspection"]["sources"], "changed");

    fs::remove_file(root.join("fail-apply")).unwrap();
    let (_, plan) = apply(root, "ru-RU", &["--initialize"]);
    let token = plan["plan"]["confirmation"].as_str().unwrap();
    let (output, _) = apply(root, "en-US", &["--initialize", "--confirm", token]);
    assert!(output.status.success(), "{output:?}");
    let (_, unchanged) = apply(root, "ru-RU", &[]);
    assert_eq!(unchanged["plan"]["mode"], "unchanged");

    let applied = fs::read(&snapshot).unwrap();
    fs::write(module, "A = 3;").unwrap();
    fs::write(root.join("pause-apply"), "").unwrap();
    let (_, plan) = apply(root, "en-US", &[]);
    let token = plan["plan"]["confirmation"].as_str().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_eska-build"))
        .current_dir(root)
        .args([
            "--lang",
            "en-US",
            "apply",
            "--format",
            "json",
            "--confirm",
            token,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.join("apply-ready").is_file() {
        if Instant::now() >= deadline || child.try_wait().unwrap().is_some() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the first apply did not reach the mutation phase");
        }
        thread::sleep(Duration::from_millis(25));
    }
    let (_, concurrent) = apply(root, "en-US", &["--confirm", token]);
    let signalled = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    let cancelled = child.wait_with_output().unwrap();
    assert!(signalled.success());
    assert_eq!(concurrent["error"]["code"], "busy");
    assert!(!cancelled.status.success());
    let cancelled: Value = serde_json::from_slice(&cancelled.stdout).unwrap();
    assert_eq!(cancelled["error"]["code"], "cancelled");
    assert_eq!(fs::read(snapshot).unwrap(), applied);
    let (_, pending) = apply(root, "en-US", &[]);
    assert_eq!(pending["error"]["code"], "incomplete_apply");
}
