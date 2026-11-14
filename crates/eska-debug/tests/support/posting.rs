//! Real posting callbacks exercise `ServerEmulation`, nested calls and the return to the client.
use super::{Dap, Path, PathBuf, Value, env, fs, json};

fn editor_line(path: &Path, statement: &str) -> u32 {
    // VS Code recognizes CRLF, LF and lone CR independently of the OS.
    let text = fs::read_to_string(path)
        .unwrap()
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    u32::try_from(
        text.lines()
            .position(|line| line.trim() == statement)
            .unwrap()
            + 1,
    )
    .unwrap()
}

fn start(language: &str, statements: &[&str]) -> (Dap, PathBuf, PathBuf) {
    let root = PathBuf::from(env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap());
    let source = root.join("src/Documents/PostingProbe/Ext/ObjectModule.bsl");
    let mut dap = Dap::start_language(&root, "test", "launch", language);
    dap.success("initialize", json!({"adapterID":"eska"}));
    dap.success("launch", json!({}));
    points(&mut dap, &source, statements);
    dap.success("configurationDone", json!({}));
    (dap, root, source)
}

fn points(dap: &mut Dap, source: &Path, statements: &[&str]) {
    let points: Vec<_> = statements
        .iter()
        .map(|line| json!({"line":editor_line(source, line)}))
        .collect();
    let result = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":points}),
    );
    for (actual, expected) in result["breakpoints"].as_array().unwrap().iter().zip(points) {
        assert_eq!(actual["verified"], true, "{actual}");
        assert_eq!(actual["line"], expected["line"]);
    }
}

fn stopped(
    dap: &mut Dap,
    source: &Path,
    statement: &str,
    reason: &str,
    depth: usize,
) -> (u64, Value) {
    let event = dap.stop_event();
    assert_eq!(event["reason"], reason, "{event}");
    let thread = event["threadId"].as_u64().unwrap();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let frames = stack["stackFrames"].as_array().unwrap();
    assert_eq!(frames.len(), depth, "{stack}");
    let actual = Path::new(frames[0]["source"]["path"].as_str().unwrap());
    assert_eq!(
        actual,
        eska_build_runtime::native_path(&source.canonicalize().unwrap()).as_ref(),
        "{stack}"
    );
    assert_eq!(frames[0]["line"], editor_line(source, statement), "{stack}");
    (thread, frames[0]["id"].clone())
}

#[test]
#[ignore = "requires the owned posting fixture with LF, CRLF, CR or mixed endings"]
fn native_posting_step_over_reaches_handler_end_and_client() {
    for language in ["ru-RU", "en-US"] {
        let (mut dap, root, source) = start(language, &["Сумма = 1;"]);
        let (server, _) = stopped(&mut dap, &source, "Сумма = 1;", "breakpoint", 2);
        for statement in [
            "Сумма = Увеличить(Сумма);",
            "Для Индекс = 1 По 3 Цикл",
            "Сумма = Сумма + Индекс;",
            "КонецЦикла;",
            "Для Индекс = 1 По 3 Цикл",
            "Сумма = Сумма + Индекс;",
            "КонецЦикла;",
            "Для Индекс = 1 По 3 Цикл",
            "Сумма = Сумма + Индекс;",
            "КонецЦикла;",
            "Для Индекс = 1 По 3 Цикл",
        ] {
            dap.success("next", json!({"threadId":server}));
            assert_eq!(stopped(&mut dap, &source, statement, "step", 2).0, server);
        }
        dap.success("next", json!({"threadId":server}));
        stopped(
            &mut dap,
            &source,
            "Сумма = Увеличить(Сумма + 0);",
            "step",
            2,
        );
        dap.success("next", json!({"threadId":server}));
        let (_, frame) = stopped(&mut dap, &source, "Сообщить(Сумма);", "step", 2);
        assert_eq!(
            dap.success("evaluate", json!({"frameId":frame,"expression":"Сумма"}))["result"],
            "9"
        );
        dap.success("next", json!({"threadId":server}));
        stopped(&mut dap, &source, "КонецПроцедуры", "step", 2);
        let caller = root.join("src/CommonModules/DebugPosting/Ext/Module.bsl");
        for statement in [
            "Документ.Записать(РежимЗаписиДокумента.Проведение);",
            "Результат = \"Posted\";",
            "Возврат Результат;",
            "КонецФункции",
        ] {
            dap.success("next", json!({"threadId":server}));
            stopped(&mut dap, &caller, statement, "step", 1);
        }
        dap.success("next", json!({"threadId":server}));
        let client = root.join("src/Ext/ManagedApplicationModule.bsl");
        let (client_thread, _) = stopped(&mut dap, &client, "Сообщить(Результат);", "step", 1);
        assert_ne!(client_thread, server);
        dap.shutdown();
    }
}

#[test]
#[ignore = "requires the owned posting fixture with LF, CRLF, CR or mixed endings"]
fn native_posting_steps_into_and_out_then_hits_last_instruction() {
    for language in ["ru-RU", "en-US"] {
        let (mut dap, _, source) = start(language, &["Сумма = Увеличить(Сумма);"]);
        let (thread, _) = stopped(
            &mut dap,
            &source,
            "Сумма = Увеличить(Сумма);",
            "breakpoint",
            2,
        );
        dap.success("stepIn", json!({"threadId":thread}));
        stopped(&mut dap, &source, "Результат = Значение + 1;", "step", 3);
        dap.success("next", json!({"threadId":thread}));
        let (_, frame) = stopped(&mut dap, &source, "Возврат Результат;", "step", 3);
        assert_eq!(
            dap.success(
                "evaluate",
                json!({"frameId":frame,"expression":"Результат"})
            )["result"],
            "2"
        );
        points(&mut dap, &source, &["Сообщить(Сумма);"]);
        dap.success("stepOut", json!({"threadId":thread}));
        stopped(&mut dap, &source, "Сумма = Увеличить(Сумма);", "step", 2);
        dap.success("continue", json!({"threadId":thread}));
        let (_, frame) = stopped(&mut dap, &source, "Сообщить(Сумма);", "breakpoint", 2);
        assert_eq!(
            dap.success("evaluate", json!({"frameId":frame,"expression":"Сумма"}))["result"],
            "9"
        );
        dap.shutdown();
    }
}

#[test]
#[ignore = "requires the owned posting fixture with LF, CRLF, CR or mixed endings"]
fn native_posting_last_instruction_breakpoint_without_stepping() {
    for language in ["ru-RU", "en-US"] {
        let (mut dap, _, source) = start(language, &["Сообщить(Сумма);"]);
        let (_, frame) = stopped(&mut dap, &source, "Сообщить(Сумма);", "breakpoint", 2);
        assert_eq!(
            dap.success("evaluate", json!({"frameId":frame,"expression":"Сумма"}))["result"],
            "9"
        );
        dap.shutdown();
    }
}
