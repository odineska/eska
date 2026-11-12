//! XML messages exchanged with the 1C HTTP debug server.
use std::{fmt::Write as _, io, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use roxmltree::{Document, Node};

use crate::http::Endpoint;

const REQUEST: &str = "http://v8.1c.ru/8.3/debugger/debugRDBGRequestResponse";
const BASE: &str = "http://v8.1c.ru/8.3/debugger/debugBaseData";
const BREAKPOINTS: &str = "http://v8.1c.ru/8.3/debugger/debugBreakpoints";
const AUTO: &str = "http://v8.1c.ru/8.3/debugger/debugAutoAttach";
const CALC: &str = "http://v8.1c.ru/8.3/debugger/debugCalculations";
const RTE: &str = "http://v8.1c.ru/8.3/debugger/debugRTEFilter";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Module {
    pub object: String,
    pub property: String,
    pub extension: String,
}

#[derive(Clone, Debug, Default)]
pub struct Breakpoint {
    pub line: u32,
    pub condition: Option<String>,
    pub hit_count: Option<i32>,
    pub log_expression: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Target {
    pub id: String,
    pub kind: String,
    pub session: String,
    pub user: String,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub module: Option<Module>,
    pub line: u32,
    pub name: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Selector {
    Expression(String),
    Snapshot(String),
    Property(String),
    Index(u32),
}

#[derive(Clone, Copy, Debug)]
pub struct Page {
    pub start: u32,
    pub count: u32,
}

#[derive(Clone, Debug)]
pub struct Value {
    pub name: String,
    pub text: String,
    pub kind: String,
    pub expandable: bool,
    pub indexed: bool,
    pub collection_size: Option<u32>,
    pub error: bool,
}

impl Value {
    pub const fn can_expand(&self) -> bool {
        self.expandable || self.indexed
    }
}

#[derive(Debug)]
pub struct Calculation {
    pub id: String,
    pub value: Value,
    pub children: Vec<Value>,
}

#[derive(Debug)]
pub enum Event {
    Started(Target),
    Exited(String),
    Stack {
        target: String,
        action: StackAction,
        frames: Vec<Frame>,
        message: String,
    },
    Calculated(Calculation),
}

#[derive(Clone, Copy, Debug)]
pub enum StackAction {
    Breakpoint,
    Step,
    Exception,
    Resume,
}

#[derive(Clone, Copy)]
pub enum Step {
    Continue,
    Over,
    In,
    Out,
}

pub struct Server {
    endpoint: Endpoint,
    alias: String,
    id: String,
}

impl Server {
    pub fn new(url: &url::Url, alias: String) -> io::Result<Self> {
        Ok(Self {
            endpoint: Endpoint::resolve(url)?,
            alias,
            id: random_id()?,
        })
    }

    pub fn attach(&self) -> io::Result<()> {
        let response = self.call(
            "attachDebugUI",
            "<options><foregroundAbility>false</foregroundAbility></options>",
        )?;
        if text(document(&response)?.root_element(), "result") == "registered" {
            Ok(())
        } else {
            Err(io::Error::other("debug UI registration rejected"))
        }
    }

    pub fn detach(&self) -> io::Result<()> {
        self.call("detachDebugUI", "").map(|_| ())
    }

    pub fn targets(&self) -> io::Result<Vec<Target>> {
        let response = self.call("getDbgTargets", "")?;
        Ok(children(document(&response)?.root_element(), "id")
            .filter_map(target)
            .collect())
    }

    pub fn auto_attach(&self, types: &[String]) -> io::Result<()> {
        let mut body = String::from("<autoAttachSettings>");
        for kind in types {
            let _ = write!(
                body,
                "<targetType xmlns=\"{AUTO}\">{}</targetType>",
                escape(kind)
            );
        }
        body.push_str("</autoAttachSettings>");
        self.call("setAutoAttachSettings", &body).map(|_| ())
    }

    pub fn attach_target(&self, id: &str) -> io::Result<()> {
        self.call("clearBreakOnNextStatement", "")?;
        self.call(
            "attachDetachDbgTargets",
            &format!(
                "<attach>true</attach><id><id xmlns=\"{BASE}\">{}</id></id>",
                escape(id)
            ),
        )
        .map(|_| ())
    }

    pub fn breakpoints<'a>(
        &self,
        modules: impl IntoIterator<Item = (&'a Module, &'a [Breakpoint])>,
    ) -> io::Result<()> {
        let mut body = String::from("<bpWorkspace>");
        for (module, lines) in modules {
            let kind = if module.extension.is_empty() {
                "ConfigModule"
            } else {
                "ExtensionModule"
            };
            let _ = write!(
                body,
                "<moduleBPInfo xmlns=\"{BREAKPOINTS}\"><id><type xmlns=\"{BASE}\">{kind}</type><extensionName xmlns=\"{BASE}\">{}</extensionName><objectID xmlns=\"{BASE}\">{}</objectID><propertyID xmlns=\"{BASE}\">{}</propertyID></id>",
                escape(&module.extension),
                escape(&module.object),
                escape(&module.property)
            );
            for point in lines {
                let line = point.line;
                let condition = point.condition.as_deref().unwrap_or_default();
                let break_on_condition = point.condition.is_some();
                let break_on_hit_count = point.hit_count.is_some();
                let hit_count = point.hit_count.unwrap_or(1);
                let log_expression = point.log_expression.as_deref().unwrap_or_default();
                let log = point.log_expression.is_some();
                let _ = write!(
                    body,
                    "<bpInfo><line>{line}</line><isActive>true</isActive><breakOnCondition>{break_on_condition}</breakOnCondition><condition>{}</condition><breakOnHitCount>{break_on_hit_count}</breakOnHitCount><hitCountVariant>0</hitCountVariant><hitCount>{hit_count}</hitCount><showOutputMessage>{log}</showOutputMessage><putExpressionResult>{}</putExpressionResult><continueExecution>{log}</continueExecution></bpInfo>",
                    escape(condition),
                    escape(log_expression),
                );
            }
            body.push_str("</moduleBPInfo>");
        }
        body.push_str("</bpWorkspace>");
        self.call("setBreakpoints", &body).map(|_| ())
    }

    pub fn break_on_errors(&self, enabled: bool, condition: Option<&str>) -> io::Result<()> {
        let analyze = enabled && condition.is_some();
        let mut state = format!(
            "<state><stopOnErrors xmlns=\"{RTE}\">{enabled}</stopOnErrors><analyzeErrorStr xmlns=\"{RTE}\">{analyze}</analyzeErrorStr>"
        );
        if let Some(condition) = condition.filter(|_| analyze) {
            let _ = write!(
                state,
                "<strTemplate xmlns=\"{RTE}\"><include>true</include><str>{}</str></strTemplate>",
                escape(condition)
            );
        }
        state.push_str("</state>");
        self.call("setBreakOnRTE", &state).map(|_| ())
    }

    pub fn step(&self, id: &str, step: Step) -> io::Result<Vec<String>> {
        let action = match step {
            Step::Continue => "Continue",
            Step::Over => "Step",
            Step::In => "StepIn",
            Step::Out => "StepOut",
        };
        let response = self.call(
            "step",
            &format!(
                "{}<action>{action}</action><simple>false</simple>",
                target_xml(id)
            ),
        )?;
        if response.is_empty() {
            return Ok(Vec::new());
        }
        Ok(children(document(&response)?.root_element(), "item")
            .filter(|item| text(*item, "state") == "Worked")
            .filter_map(|item| child(item, "targetID"))
            .map(|target| text(target, "id").to_owned())
            .collect())
    }

    pub fn calculate(
        &self,
        target: &str,
        level: usize,
        id: &str,
        path: &[Selector],
        indexed: bool,
        page: Option<Page>,
    ) -> io::Result<Vec<Calculation>> {
        let interface = if indexed { "collection" } else { "context" };
        let mut items = String::new();
        for selector in path {
            let (kind, field, value) = match selector {
                Selector::Expression(text) => ("expression", "expression", escape(text)),
                Selector::Snapshot(address) => (
                    "expression",
                    "expression",
                    escape(&snapshot_expression(&format!("\"{address}\""))),
                ),
                Selector::Property(text) => ("property", "property", escape(text)),
                Selector::Index(index) => ("index", "index", index.to_string()),
            };
            let _ = write!(
                items,
                "<calcItem><itemType>{kind}</itemType><{field}>{value}</{field}></calcItem>"
            );
        }
        let command = if path.is_empty() {
            "evalLocalVariables"
        } else {
            "evalExpr"
        };
        let page = page.map_or_else(String::new, |page| {
            format!(
                "<startIndex>{}</startIndex><pageSize>{}</pageSize>",
                page.start, page.count
            )
        });
        let response = self.call(command, &format!(
            "<calcWaitingTime>100</calcWaitingTime>{}<expr><stackLevel xmlns=\"{CALC}\">{level}</stackLevel><srcCalcInfo xmlns=\"{CALC}\"><expressionResultID>{id}</expressionResultID>{items}<interfaces>{interface}</interfaces>{page}</srcCalcInfo><presOptions xmlns=\"{CALC}\"><maxTextSize>16384</maxTextSize></presOptions></expr>", target_xml(target)))?;
        if response.is_empty() {
            return Ok(Vec::new());
        }
        Ok(children(document(&response)?.root_element(), "result")
            .filter_map(calculation)
            .collect())
    }

    pub fn poll(&self) -> io::Result<Vec<Event>> {
        let response = match self.endpoint.post(
            "rdbg",
            &format!("cmd=pingDebugUIParams&dbgui={}", self.id),
            "",
            Duration::from_secs(2),
        ) {
            Ok(response) => response,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                // The server may hold an idle event request. Probe its independent
                // liveness endpoint before treating an idle socket as a failed session.
                self.endpoint
                    .post("rdbgTest", "cmd=test", "", Duration::from_millis(700))?;
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };
        if response.is_empty() {
            return Ok(Vec::new());
        }
        Ok(children(document(&response)?.root_element(), "result")
            .filter_map(event)
            .collect())
    }

    fn call(&self, command: &str, fields: &str) -> io::Result<Vec<u8>> {
        let mut request = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?><request><infoBaseAlias xmlns=\"{REQUEST}\">{}</infoBaseAlias><idOfDebuggerUI xmlns=\"{REQUEST}\">{}</idOfDebuggerUI>",
            escape(&self.alias),
            self.id
        );
        let namespace = format!(" xmlns=\"{REQUEST}\"");
        // Put a default namespace on each top-level field without changing the envelope.
        let fields = format!("<fields>{fields}</fields>");
        let parsed = Document::parse(&fields).map_err(|_| invalid())?;
        for node in parsed.root_element().children().filter(Node::is_element) {
            let field = &fields[node.range()];
            let start = 1 + node.tag_name().name().len();
            request.push_str(&field[..start]);
            request.push_str(&namespace);
            request.push_str(&field[start..]);
        }
        request.push_str("</request>");
        self.endpoint
            .post("rdbg", &format!("cmd={command}"), &request, REQUEST_TIMEOUT)
    }
}

pub fn random_id() -> io::Result<String> {
    let mut data = [0u8; 16];
    getrandom::fill(&mut data).map_err(|_| io::Error::other("random generator unavailable"))?;
    data[6] = (data[6] & 0x0f) | 0x40;
    data[8] = (data[8] & 0x3f) | 0x80;
    let mut result = String::with_capacity(36);
    for (index, byte) in data.into_iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            result.push('-');
        }
        let _ = write!(result, "{byte:02x}");
    }
    Ok(result)
}

pub fn snapshot_expression(address_expression: &str) -> String {
    format!("ПолучитьИзВременногоХранилища({address_expression})")
}

fn event(node: Node<'_, '_>) -> Option<Event> {
    let id = child(node, "targetID");
    match text(node, "cmdID") {
        "targetStarted" => target(id?).map(Event::Started),
        "targetQuit" => Some(Event::Exited(text(id?, "id").to_owned())),
        "callStackFormed" | "rteProcessing" => Some(Event::Stack {
            target: text(id?, "id").to_owned(),
            action: stack_action(node),
            frames: children(node, "callStack")
                .filter_map(frame)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
            message: child(node, "exception").map_or_else(
                || text(node, "message").to_owned(),
                |exception| text(exception, "descr").to_owned(),
            ),
        }),
        "exprEvaluated" => calculation(child(node, "evalExprResBaseData")?).map(Event::Calculated),
        _ => None,
    }
}

fn stack_action(node: Node<'_, '_>) -> StackAction {
    if text(node, "cmdID") == "rteProcessing" {
        StackAction::Exception
    } else if text(node, "sendMessageOnly") == "true" || text(node, "sendHitCounterOnly") == "true"
    {
        StackAction::Resume
    } else if text(node, "stopByBP") == "true" || text(node, "suspendedByOther") == "true" {
        StackAction::Breakpoint
    } else {
        StackAction::Step
    }
}

fn target(node: Node<'_, '_>) -> Option<Target> {
    Some(Target {
        id: child(node, "id")?.text()?.to_owned(),
        kind: text(node, "targetType").to_owned(),
        session: text(node, "seanceNo").to_owned(),
        user: decoded(node, "userName"),
    })
}

fn frame(node: Node<'_, '_>) -> Option<Frame> {
    Some(Frame {
        module: child(node, "moduleID").and_then(|node| {
            if matches!(text(node, "type"), "" | "ConfigModule" | "ExtensionModule") {
                Some(Module {
                    object: child(node, "objectID")?.text()?.to_ascii_lowercase(),
                    property: child(node, "propertyID")?.text()?.to_ascii_lowercase(),
                    extension: text(node, "extensionName").to_owned(),
                })
            } else {
                None
            }
        }),
        line: text(node, "lineNo").parse().ok()?,
        name: decoded(node, "presentation"),
    })
}

fn calculation(node: Node<'_, '_>) -> Option<Calculation> {
    let id = child(node, "expressionResultID")?.text()?.to_owned();
    let mut result = value(
        child(node, "resultValueInfo"),
        decoded(node, "localVariableName"),
    );
    result.error =
        text(node, "errorOccurred") == "true" || text(node, "evalResultState") == "withErrors";
    if result.error {
        result.text = decoded(node, "exceptionStr");
    }
    let mut values = Vec::new();
    for result in children(node, "calculationResult") {
        let mut collection_index = 0;
        for property in result.children().filter(Node::is_element) {
            let name = match property.tag_name().name() {
                "valueOfContextPropInfo" => child(property, "propInfo")
                    .map_or_else(String::new, |info| text(info, "propName").to_owned()),
                "valueOfCollectionInfo" | "valueOfEnumInfo" => {
                    let index = collection_index;
                    collection_index += 1;
                    index.to_string()
                }
                _ => continue,
            };
            values.push(value(child(property, "valueInfo"), name));
        }
    }
    Some(Calculation {
        id,
        value: result,
        children: values,
    })
}

fn value(info: Option<Node<'_, '_>>, name: String) -> Value {
    Value {
        name,
        text: info.map_or_else(String::new, |node| decoded(node, "pres")),
        kind: info.map_or_else(String::new, |node| text(node, "typeName").to_owned()),
        expandable: info.is_some_and(|node| text(node, "isExpandable") == "true"),
        indexed: info.is_some_and(|node| text(node, "isIIndexedCollectionRO") == "true"),
        collection_size: info.and_then(|node| text(node, "collectionSize").parse().ok()),
        error: false,
    }
}

fn document(bytes: &[u8]) -> io::Result<Document<'_>> {
    let utf8 = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    Document::parse(utf8.trim_start_matches('\u{feff}')).map_err(|_| invalid())
}

fn children<'a, 'i>(node: Node<'a, 'i>, name: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(move |node| node.has_tag_name(name))
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &'static str) -> Option<Node<'a, 'i>> {
    children(node, name).next()
}

fn text<'a>(node: Node<'a, '_>, name: &'static str) -> &'a str {
    child(node, name)
        .and_then(|node| node.text())
        .unwrap_or_default()
}

fn decoded(node: Node<'_, '_>, name: &'static str) -> String {
    let raw = text(node, name);
    if raw.is_empty() {
        return String::new();
    }
    let bytes = STANDARD
        .decode(raw.split_ascii_whitespace().collect::<String>())
        .unwrap_or_else(|_| raw.as_bytes().to_vec());
    String::from_utf8_lossy(&bytes).into_owned()
}

fn target_xml(target: &str) -> String {
    format!(
        "<targetID><id xmlns=\"{BASE}\">{}</id></targetID>",
        escape(target)
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\r', "&#13;")
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid debugger XML")
}
