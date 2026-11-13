//! XML messages exchanged with the 1C HTTP debug server.
use std::{
    fmt::Write as _,
    io,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use roxmltree::{Document, Node};

use crate::http::{Cancellation, Endpoint};

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
    pub origin: ModuleOrigin,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ModuleOrigin {
    Configuration,
    Extension(String),
    External(String),
}

impl Module {
    fn from_xml(node: Node<'_, '_>) -> Option<Self> {
        let extension = text(node, "extensionName");
        let origin = match text(node, "type") {
            "" | "ConfigModule" | "ExtensionModule" if extension.is_empty() => {
                ModuleOrigin::Configuration
            }
            "" | "ConfigModule" | "ExtensionModule" => {
                ModuleOrigin::Extension(extension.to_owned())
            }
            "ExtMDModule" => {
                let url = child(node, "URL")?.text()?.to_owned();
                ModuleOrigin::External(url)
            }
            _ => return None,
        };
        Some(Self {
            object: child(node, "objectID")?.text()?.to_ascii_lowercase(),
            property: child(node, "propertyID")?.text()?.to_ascii_lowercase(),
            origin,
        })
    }

    fn write_xml(&self, body: &mut String) {
        let (kind, qualifier, value) = match &self.origin {
            ModuleOrigin::Configuration => ("ConfigModule", "extensionName", ""),
            ModuleOrigin::Extension(name) => ("ExtensionModule", "extensionName", name.as_str()),
            ModuleOrigin::External(url) => ("ExtMDModule", "URL", url.as_str()),
        };
        let _ = write!(
            body,
            "<id><type xmlns=\"{BASE}\">{kind}</type><{qualifier} xmlns=\"{BASE}\">{}</{qualifier}><objectID xmlns=\"{BASE}\">{}</objectID><propertyID xmlns=\"{BASE}\">{}</propertyID></id>",
            escape(value),
            escape(&self.object),
            escape(&self.property)
        );
    }
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
    Retained(String),
    Property(String),
    Index(u32),
}

#[derive(Clone, Copy, Debug)]
pub struct Page {
    pub start: u32,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum View {
    Context,
    Collection,
    Enumeration,
}

impl View {
    pub const fn paged(self) -> bool {
        !matches!(self, Self::Context)
    }
}

#[derive(Clone, Debug)]
pub struct Value {
    pub name: String,
    pub text: String,
    pub kind: String,
    pub view: Option<View>,
    pub entry_key: Option<String>,
    pub collection_size: Option<u32>,
    pub error: bool,
}

impl Value {
    pub const fn can_expand(&self) -> bool {
        !self.error && self.view.is_some()
    }
}

#[derive(Debug)]
pub struct Calculation {
    pub id: String,
    pub value: Value,
    pub children: Vec<Value>,
}

pub struct CalculationQuery {
    pub id: String,
    pub target: String,
    pub level: usize,
    pub path: Vec<Selector>,
    pub view: View,
    pub page: Option<Page>,
    pub new_expression: Option<String>,
}

const RETAINED_VALUE: &str = "Value";

impl CalculationQuery {
    fn path_xml(&self) -> String {
        let Self {
            id,
            level,
            path,
            view,
            page,
            new_expression,
            ..
        } = self;
        let interface = match (new_expression, view) {
            (Some(_), _) => "none",
            (None, View::Context) => "context",
            (None, View::Collection) => "collection",
            (None, View::Enumeration) => "enum",
        };
        let mut expression_id = String::new();
        let mut items = String::new();
        for selector in path {
            let (kind, field, value) = match selector {
                Selector::Expression(text) => ("expression", "expression", escape(text)),
                Selector::Retained(identity) => {
                    let _ = write!(expression_id, "<expressionID>{identity}</expressionID>");
                    // dbgs retains the first expression under expressionID until execution
                    // resumes. An empty expression can read it only with a following selector.
                    let expression = new_expression.as_ref().map_or_else(String::new, |text| {
                        escape(&format!("Новый Структура(\"{RETAINED_VALUE}\", ({text}))"))
                    });
                    let _ = write!(
                        items,
                        "<calcItem><itemType>expression</itemType><expression>{expression}</expression></calcItem><calcItem><itemType>property</itemType><property>{RETAINED_VALUE}</property></calcItem>"
                    );
                    continue;
                }
                Selector::Property(text) => ("property", "property", escape(text)),
                Selector::Index(index) => ("index", "index", index.to_string()),
            };
            let _ = write!(
                items,
                "<calcItem><itemType>{kind}</itemType><{field}>{value}</{field}></calcItem>"
            );
        }
        let page = page.map_or_else(String::new, |page| {
            format!(
                "<startIndex>{}</startIndex><pageSize>{}</pageSize>",
                page.start, page.count
            )
        });
        format!(
            "<stackLevel xmlns=\"{CALC}\">{level}</stackLevel><srcCalcInfo xmlns=\"{CALC}\">{expression_id}<expressionResultID>{id}</expressionResultID>{items}<interfaces>{interface}</interfaces>{page}</srcCalcInfo>"
        )
    }
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

#[derive(Debug, PartialEq, Eq)]
pub enum Registration {
    Registered,
    InfobaseBusy,
    Rejected,
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

    pub fn attach(&self) -> io::Result<Registration> {
        let response = self.call(
            "attachDebugUI",
            "<options><foregroundAbility>false</foregroundAbility></options>",
        )?;
        Ok(match text(document(&response)?.root_element(), "result") {
            "registered" => Registration::Registered,
            "ibInDebug" => Registration::InfobaseBusy,
            _ => Registration::Rejected,
        })
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
        self.call(
            "attachDetachDbgTargets",
            &format!(
                "<attach>true</attach><id><id xmlns=\"{BASE}\">{}</id></id>",
                escape(id)
            ),
        )
        .map(|_| ())
    }

    pub fn break_on_next_statement(&self, enabled: bool) -> io::Result<()> {
        self.break_on_next_statement_before(enabled, Instant::now() + REQUEST_TIMEOUT, None)
    }

    pub fn break_on_next_statement_before(
        &self,
        enabled: bool,
        deadline: Instant,
        cancellation: Option<&Cancellation>,
    ) -> io::Result<()> {
        let command = if enabled {
            "setBreakOnNextStatement"
        } else {
            "clearBreakOnNextStatement"
        };
        self.call_before(command, "", deadline, cancellation)
            .map(|_| ())
    }

    pub fn breakpoints<'a>(
        &self,
        modules: impl IntoIterator<Item = (&'a Module, &'a [Breakpoint])>,
    ) -> io::Result<()> {
        let mut body = String::from("<bpWorkspace>");
        for (module, lines) in modules {
            let _ = write!(body, "<moduleBPInfo xmlns=\"{BREAKPOINTS}\">");
            module.write_xml(&mut body);
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
        self.step_before(id, step, Instant::now() + REQUEST_TIMEOUT, None)
    }

    pub fn step_before(
        &self,
        id: &str,
        step: Step,
        deadline: Instant,
        cancellation: Option<&Cancellation>,
    ) -> io::Result<Vec<String>> {
        let action = match step {
            Step::Continue => "Continue",
            Step::Over => "Step",
            Step::In => "StepIn",
            Step::Out => "StepOut",
        };
        let response = self.call_before(
            "step",
            &format!(
                "{}<action>{action}</action><simple>false</simple>",
                target_xml(id)
            ),
            deadline,
            cancellation,
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

    pub fn control_step(
        &self,
        id: &str,
        step: Step,
        clear_pause: bool,
        cancellation: &Cancellation,
    ) -> io::Result<Vec<String>> {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        if clear_pause {
            self.break_on_next_statement_before(false, deadline, Some(cancellation))?;
        }
        self.step_before(id, step, deadline, Some(cancellation))
    }

    pub fn calculate(
        &self,
        query: &CalculationQuery,
        deadline: Instant,
        cancellation: Option<&Cancellation>,
    ) -> io::Result<Vec<Calculation>> {
        let command = if query.path.is_empty() {
            "evalLocalVariables"
        } else {
            "evalExpr"
        };
        let fields = format!(
            "<calcWaitingTime>100</calcWaitingTime>{}<expr>{}<presOptions xmlns=\"{CALC}\"><maxTextSize>16384</maxTextSize></presOptions></expr>",
            target_xml(&query.target),
            query.path_xml()
        );
        let response = self.call_before(command, &fields, deadline, cancellation)?;
        if response.is_empty() {
            return Ok(Vec::new());
        }
        Ok(children(document(&response)?.root_element(), "result")
            .filter_map(calculation)
            .collect())
    }

    pub fn modify(
        &self,
        query: &CalculationQuery,
        expression: &str,
        deadline: Instant,
        cancellation: &Cancellation,
    ) -> io::Result<Vec<Calculation>> {
        let fields = format!(
            "{}<modifyDataPath>{}</modifyDataPath><newValueInfo><variant xmlns=\"{CALC}\">expr</variant><valueExpression xmlns=\"{CALC}\">{}</valueExpression></newValueInfo><timeout>1000</timeout>",
            target_xml(&query.target),
            query.path_xml(),
            escape(expression)
        );
        let response = self.call_before("modifyValue", &fields, deadline, Some(cancellation))?;
        let document = document(&response)?;
        let root = document.root_element();
        let result = if text(root, "processed") == "false" {
            let mut failed = value(None, String::new());
            failed.error = true;
            failed.text = decoded(root, "errorDescr");
            Calculation {
                id: query.id.clone(),
                value: failed,
                children: Vec::new(),
            }
        } else {
            child(root, "newValueState")
                .and_then(calculation)
                .ok_or_else(invalid)?
        };
        Ok(vec![result])
    }

    pub fn poll(&self, cancellation: Option<&Cancellation>) -> io::Result<Vec<Event>> {
        // Leave the native long poll open through quiet periods. Closing it early
        // can lose an event that dbgs delivers to that connection as activity resumes.
        let response = match self.endpoint.post(
            "rdbg",
            &format!("cmd=pingDebugUIParams&dbgui={}", self.id),
            "",
            Instant::now() + Duration::from_secs(8),
            cancellation,
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
                self.endpoint.post(
                    "rdbgTest",
                    "cmd=test",
                    "",
                    Instant::now() + Duration::from_millis(700),
                    cancellation,
                )?;
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
        self.call_before(command, fields, Instant::now() + REQUEST_TIMEOUT, None)
    }

    fn call_before(
        &self,
        command: &str,
        fields: &str,
        deadline: Instant,
        cancellation: Option<&Cancellation>,
    ) -> io::Result<Vec<u8>> {
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
        self.endpoint.post(
            "rdbg",
            &format!("cmd={command}"),
            &request,
            deadline,
            cancellation,
        )
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
        module: child(node, "moduleID").and_then(Module::from_xml),
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
            match property.tag_name().name() {
                "valueOfContextPropInfo" => values.push(context_property(property)),
                "valueOfCollectionInfo" | "valueOfEnumInfo" => {
                    let index = collection_index;
                    collection_index += 1;
                    let mut entry = value(child(property, "valueInfo"), index.to_string());
                    if property.has_tag_name("valueOfEnumInfo") {
                        entry.entry_key = children(property, "valueOfContextPropInfo")
                            .map(context_property)
                            .find(|key| !key.error && matches!(key.name.as_str(), "Ключ" | "Key"))
                            .map(|key| key.text);
                    }
                    values.push(entry);
                }
                _ => {}
            }
        }
    }
    Some(Calculation {
        id,
        value: result,
        children: values,
    })
}

fn context_property(property: Node<'_, '_>) -> Value {
    let info = child(property, "propInfo");
    let data = child(property, "valueInfo");
    let name = info.map_or_else(String::new, |info| text(info, "propName").to_owned());
    let mut result = value(data, name);
    if let Some(info) = info
        && (data.is_none() || text(info, "isReaded") == "false")
    {
        result.error = true;
        result.text = decoded(info, "errorStr");
    }
    result
}

fn value(info: Option<Node<'_, '_>>, name: String) -> Value {
    Value {
        name,
        text: info.map_or_else(String::new, |node| decoded(node, "pres")),
        kind: info.map_or_else(String::new, |node| text(node, "typeName").to_owned()),
        view: info.and_then(|node| {
            [
                ("isExpandable", View::Context),
                ("isIIndexedCollectionRO", View::Collection),
                ("isSupportIEnumValue", View::Enumeration),
            ]
            .into_iter()
            .find_map(|(flag, view)| (text(node, flag) == "true").then_some(view))
        }),
        entry_key: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_value_path_keeps_identity_and_omits_source_on_expansion() {
        let mut query = CalculationQuery {
            id: "first-response".into(),
            target: "client".into(),
            level: 1,
            path: vec![Selector::Retained("result-identity".into())],
            view: View::Context,
            page: None,
            new_expression: Some("Probe(\"<&>\")".into()),
        };
        let xml = format!("<expr>{}</expr>", query.path_xml());
        let document = Document::parse(&xml).unwrap();
        let expr = document.root_element();
        let info = child(expr, "srcCalcInfo").unwrap();
        let mut path = children(info, "calcItem");
        assert_eq!(text(expr, "stackLevel"), "1");
        assert_eq!(text(info, "expressionID"), "result-identity");
        assert_eq!(text(info, "expressionResultID"), "first-response");
        assert_eq!(text(info, "interfaces"), "none");
        assert_eq!(
            text(path.next().unwrap(), "expression"),
            "Новый Структура(\"Value\", (Probe(\"<&>\")))"
        );
        assert_eq!(text(path.next().unwrap(), "property"), "Value");
        assert!(path.next().is_none());

        query.id = "next-response".into();
        query.new_expression = None;
        query.path.push(Selector::Property("Data".into()));
        query.view = View::Collection;
        query.page = Some(Page {
            start: 50,
            count: 10,
        });
        let xml = format!("<expr>{}</expr>", query.path_xml());
        let document = Document::parse(&xml).unwrap();
        let info = child(document.root_element(), "srcCalcInfo").unwrap();
        let mut path = children(info, "calcItem");
        assert_eq!(text(info, "expressionID"), "result-identity");
        assert_eq!(text(info, "expressionResultID"), "next-response");
        assert_eq!(text(info, "interfaces"), "collection");
        assert_eq!(text(info, "startIndex"), "50");
        assert_eq!(text(info, "pageSize"), "10");
        assert!(
            child(path.next().unwrap(), "expression")
                .unwrap()
                .text()
                .is_none()
        );
        assert_eq!(text(path.next().unwrap(), "property"), "Value");
        assert_eq!(text(path.next().unwrap(), "property"), "Data");
        assert!(path.next().is_none());
    }

    #[test]
    fn external_module_uses_the_exact_platform_url_for_frames_and_breakpoints() {
        let xml = r"<callStack><lineNo>3</lineNo><moduleID>
            <type>ExtMDModule</type><URL>sfile:///tmp/Проба &amp; тест/Probe.epf</URL>
            <objectID>8FD0D78A-2342-4C52-AC20-D6A5D366DA6D</objectID>
            <propertyID>A637F77F-3840-441D-A1C3-699C8C5CB7E0</propertyID>
            <version>0000000000000000000000000000000000000000</version>
            </moduleID></callStack>";
        let document = Document::parse(xml).unwrap();
        let frame = frame(document.root_element()).unwrap();
        assert_eq!(frame.line, 3);
        let module = frame.module.unwrap();
        assert_eq!(
            module.origin,
            ModuleOrigin::External("sfile:///tmp/Проба & тест/Probe.epf".into())
        );
        let mut breakpoint = String::new();
        module.write_xml(&mut breakpoint);
        assert!(breakpoint.contains("Проба &amp; тест"));
        let document = Document::parse(&breakpoint).unwrap();
        let id = document.root_element();
        assert!(child(id, "extensionName").is_none());
        assert_eq!(Module::from_xml(id), Some(module));
    }

    #[test]
    fn a_failed_context_property_keeps_its_native_message_and_siblings() {
        let xml = r"<result><expressionResultID>id</expressionResultID><calculationResult>
            <valueOfContextPropInfo><propInfo><propName>Unavailable</propName><isReaded>false</isReaded><errorStr>RGVuaWVk</errorStr></propInfo></valueOfContextPropInfo>
            <valueOfContextPropInfo><propInfo><propName>Readable</propName><isReaded>true</isReaded></propInfo><valueInfo><typeName>Number</typeName><pres>NDI=</pres></valueInfo></valueOfContextPropInfo>
            </calculationResult></result>";
        let result = calculation(Document::parse(xml).unwrap().root_element()).unwrap();
        assert!(!result.value.error);
        assert_eq!(result.children[0].name, "Unavailable");
        assert!(result.children[0].error);
        assert_eq!(result.children[0].text, "Denied");
        assert!(!result.children[0].can_expand());
        assert_eq!(result.children[1].text, "42");
    }
}
