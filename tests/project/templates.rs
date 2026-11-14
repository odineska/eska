use std::{fs, process::Command};

use crate::backend::project::{ProjectType, discovery::discover, templates::Template};

use crate::support::TestDir as Fixture;

#[test]
fn all_built_ins_materialize_into_projects_accepted_by_discovery_and_cli() {
    let fixture = Fixture::new();
    for (index, project_type) in [
        ProjectType::Configuration,
        ProjectType::Extension,
        ProjectType::Processing,
        ProjectType::Report,
    ]
    .into_iter()
    .enumerate()
    {
        let root = fixture.0.join(format!("project-{index} каталог"));
        fs::create_dir(&root).expect("create project directory");
        let template = Template::built_in(project_type).expect("render built-in");

        // Only the test writes the plan. Production creation, collision handling
        // and rollback belong to the next milestone's creation workflow.
        for directory in template.directories() {
            fs::create_dir(root.join(directory)).expect("create source directory");
        }
        for file in template.files() {
            fs::write(root.join(file.path()), file.contents()).expect("write template file");
        }

        assert_eq!(fs::read_dir(&root).expect("read project root").count(), 4);
        assert_eq!(
            fs::read(root.join(".gitattributes")).expect("read attributes"),
            include_bytes!("../../crates/eska-project-core/assets/project/.gitattributes")
        );
        assert_eq!(
            fs::read(root.join(".gitignore")).expect("read ignore rules"),
            include_bytes!("../../crates/eska-project-core/assets/project/.gitignore")
        );
        assert_eq!(
            fs::read_dir(root.join("src"))
                .expect("read sources")
                .count(),
            1
        );
        assert_eq!(
            fs::read(root.join("src/.gitkeep")).expect("read placeholder"),
            b""
        );
        for start in [&root, &root.join("src")] {
            let project = discover(start).expect("discover generated project");
            assert_eq!(project.root(), root);
            assert_eq!(project.source(), root.join("src"));
            assert_eq!(project.configuration().project_type(), project_type);

            for locale in ["ru", "en"] {
                let output = Command::new(env!("CARGO_BIN_EXE_eska"))
                    .current_dir(start)
                    .env_remove("ESKA_LANG")
                    .args(["--lang", locale])
                    .output()
                    .expect("validate generated project with CLI");
                assert_eq!(output.status.code(), Some(0), "{output:?}");
                assert!(output.stdout.is_empty(), "{output:?}");
                assert!(output.stderr.is_empty(), "{output:?}");
            }
        }
    }
}

#[test]
fn git_checkout_uses_shared_line_endings_and_preserves_binary_bytes() {
    let fixture = Fixture::new();
    for autocrlf in ["false", "true", "input"] {
        let root = fixture.0.join(autocrlf);
        fs::create_dir(&root).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .current_dir(&root)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", fixture.0.join("no-global-config"))
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_CONFIG_COUNT")
                .args([
                    "-c",
                    &format!("core.autocrlf={autocrlf}"),
                    "-c",
                    "core.eol=crlf",
                ])
                .args(args)
                .output()
                .expect("run Git for the cross-platform checkout matrix");
            assert!(output.status.success(), "{autocrlf} {args:?}: {output:?}");
            output.stdout
        };
        git(&["init", "--quiet"]);
        fs::write(
            root.join(".gitattributes"),
            include_bytes!("../../crates/eska-project-core/assets/project/.gitattributes"),
        )
        .unwrap();
        for case in CHECKOUT_CASES {
            fs::write(root.join(case.path), case.input).unwrap();
        }
        git(&["add", "."]);
        for case in CHECKOUT_CASES {
            assert_eq!(
                git(&["show", &format!(":{}", case.path)]),
                case.index,
                "{autocrlf}: {}",
                case.path
            );
            fs::remove_file(root.join(case.path)).unwrap();
        }
        git(&["checkout-index", "--all"]);
        for case in CHECKOUT_CASES {
            assert_eq!(
                fs::read(root.join(case.path)).unwrap(),
                case.checkout,
                "{autocrlf}: {}",
                case.path
            );
        }
        git(&["diff", "--exit-code"]);
    }
}

struct CheckoutCase {
    path: &'static str,
    input: &'static [u8],
    index: &'static [u8],
    checkout: &'static [u8],
}

const CHECKOUT_CASES: &[CheckoutCase] = &[
    CheckoutCase {
        path: "Module.bsl",
        input: "Сообщить(1);\r\nСообщить(2);\n".as_bytes(),
        index: "Сообщить(1);\nСообщить(2);\n".as_bytes(),
        checkout: "Сообщить(1);\nСообщить(2);\n".as_bytes(),
    },
    CheckoutCase {
        path: "Configuration.xml",
        input: b"\xef\xbb\xbf<Properties>\r\n</Properties>\r\n",
        index: b"\xef\xbb\xbf<Properties>\n</Properties>\n",
        checkout: b"\xef\xbb\xbf<Properties>\n</Properties>\n",
    },
    CheckoutCase {
        path: "eska.toml",
        input: b"[project]\r\n",
        index: b"[project]\n",
        checkout: b"[project]\n",
    },
    CheckoutCase {
        path: "run.sh",
        input: b"#!/bin/sh\r\necho test\r\n",
        index: b"#!/bin/sh\necho test\n",
        checkout: b"#!/bin/sh\necho test\n",
    },
    CheckoutCase {
        path: "run.cmd",
        input: b"@echo off\necho test\n",
        index: b"@echo off\necho test\n",
        checkout: b"@echo off\r\necho test\r\n",
    },
    CheckoutCase {
        path: "run.bat",
        input: b"@echo off\r\n",
        index: b"@echo off\n",
        checkout: b"@echo off\r\n",
    },
    CheckoutCase {
        path: "configuration.cf",
        input: b"native\r\npayload\r\n",
        index: b"native\r\npayload\r\n",
        checkout: b"native\r\npayload\r\n",
    },
    CheckoutCase {
        path: "payload.dat",
        input: b"\0native\r\npayload",
        index: b"\0native\r\npayload",
        checkout: b"\0native\r\npayload",
    },
];
