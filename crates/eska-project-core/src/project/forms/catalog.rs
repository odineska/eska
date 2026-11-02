//! Designer XML names are case-sensitive and distinct from runtime enumeration spellings.

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ElementFamily {
    Root,
    Group,
    Field,
    Button,
    Table,
    Decoration,
    Auxiliary,
    Addition,
}

macro_rules! element_kinds {
    ($( $kind:ident => ($name:literal, $family:ident) ),+ $(,)?) => {
        /// Known XML names for the 8.3.27 form profile; recognition does not assert visual parity.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
        pub enum ElementKind {
            $( #[serde(rename = $name)] $kind, )+
        }

        impl ElementKind {
            pub const ALL: &'static [Self] = &[$(Self::$kind),+];

            #[must_use]
            pub fn from_xml_name(name: &str) -> Option<Self> {
                match name { $( $name => Some(Self::$kind), )+ _ => None }
            }

            #[must_use]
            pub const fn xml_name(self) -> &'static str {
                match self { $( Self::$kind => $name, )+ }
            }

            #[must_use]
            pub const fn family(self) -> ElementFamily {
                match self { $( Self::$kind => ElementFamily::$family, )+ }
            }
        }
    };
}

element_kinds! {
    Form => ("Form", Root),
    UsualGroup => ("UsualGroup", Group),
    ColumnGroup => ("ColumnGroup", Group),
    ButtonGroup => ("ButtonGroup", Group),
    CommandBar => ("CommandBar", Group),
    ContextMenu => ("ContextMenu", Auxiliary),
    Page => ("Page", Group),
    Pages => ("Pages", Group),
    Popup => ("Popup", Group),
    Button => ("Button", Button),
    Table => ("Table", Table),
    LabelDecoration => ("LabelDecoration", Decoration),
    PictureDecoration => ("PictureDecoration", Decoration),
    AutoCommandBar => ("AutoCommandBar", Auxiliary),
    ExtendedTooltip => ("ExtendedTooltip", Auxiliary),
    SearchControlAddition => ("SearchControlAddition", Addition),
    SearchStringAddition => ("SearchStringAddition", Addition),
    ViewStatusAddition => ("ViewStatusAddition", Addition),
    CalendarField => ("CalendarField", Field),
    ChartField => ("ChartField", Field),
    CheckBoxField => ("CheckBoxField", Field),
    DendrogramField => ("DendrogramField", Field),
    FormattedDocumentField => ("FormattedDocumentField", Field),
    GanttChartField => ("GanttChartField", Field),
    GeographicalSchemaField => ("GeographicalSchemaField", Field),
    GraphicalSchemaField => ("GraphicalSchemaField", Field),
    HtmlDocumentField => ("HTMLDocumentField", Field),
    InputField => ("InputField", Field),
    LabelField => ("LabelField", Field),
    PdfDocumentField => ("PDFDocumentField", Field),
    PeriodField => ("PeriodField", Field),
    PictureField => ("PictureField", Field),
    PlannerField => ("PlannerField", Field),
    ProgressBarField => ("ProgressBarField", Field),
    RadioButtonField => ("RadioButtonField", Field),
    SpreadsheetDocumentField => ("SpreadSheetDocumentField", Field),
    TextDocumentField => ("TextDocumentField", Field),
    TrackBarField => ("TrackBarField", Field),
}
