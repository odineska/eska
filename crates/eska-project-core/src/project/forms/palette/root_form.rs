//! Designer 8.3.27 root palettes for common and non-hierarchical catalog forms.

use super::{
    PaletteCategory, PaletteDefinition, PaletteEditor, PlatformPropertyValue, SettingSource,
};
use crate::project::{
    forms::{EffectiveElement, FormPreview, appearance, presentation, references},
    metadata_model::MetadataKind,
};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Context {
    Ordinary,
    CatalogObject,
}

impl Context {
    pub(super) fn new(preview: &FormPreview) -> Option<Self> {
        let owner = preview.form_owner.as_ref()?;
        match (owner.kind, preview.form.main_attribute()) {
            (MetadataKind::CommonForm | MetadataKind::Catalog, None) => Some(Self::Ordinary),
            (MetadataKind::Catalog, Some(attribute)) => {
                let field_type = presentation::definition_type(attribute);
                let [kind] = field_type.types.as_slice() else {
                    return None;
                };
                if kind.namespace.as_deref() != Some(references::CONFIG)
                    || kind.name.strip_prefix("CatalogObject.") != Some(owner.name.as_str())
                {
                    return None;
                }
                let catalog = preview
                    .resources
                    .iter()
                    .flat_map(|resource| &resource.descriptor.objects)
                    .find(|object| {
                        object.metadata.kind() == MetadataKind::Catalog
                            && object.metadata.name() == owner.name
                    })?;
                let hierarchy =
                    presentation::metadata_property(catalog, "Hierarchical", &preview.resources)?;
                (appearance::text(&hierarchy.value) == Some("false")).then_some(Self::CatalogObject)
            }
            _ => None,
        }
    }
}

pub(super) fn definitions(
    context: Context,
    element: &EffectiveElement,
) -> impl Iterator<Item = (&'static PaletteDefinition, bool)> {
    let settings_storage = element.scalar("SaveDataInSettings") == Some("UseList");
    FORM.iter()
        .filter(move |definition| match definition.id {
            "SettingsStorage" => settings_storage,
            "UseForFoldersAndItems"
            | "OnReadAtServer"
            | "BeforeWrite"
            | "BeforeWriteAtServer"
            | "OnWriteAtServer"
            | "AfterWriteAtServer"
            | "AfterWrite"
            | "ValueChoice" => context == Context::CatalogObject,
            _ => true,
        })
        .map(|definition| (definition, true))
}

const fn event(id: &'static str) -> PaletteDefinition {
    PaletteDefinition::new(
        id,
        PaletteCategory::Events,
        PaletteEditor::Event,
        PlatformPropertyValue::Empty,
    )
    .with_source(SettingSource::Event)
}

const FORM: &[PaletteDefinition] = {
    use PaletteCategory::{Appearance, Layout, Main, Usage};
    use PaletteDefinition as Row;
    use PaletteEditor::{Boolean, Dialog, Enum, Localized, Number, Reference};
    use PlatformPropertyValue::{Auto, Choice, Empty};
    use SettingSource::Property;

    const CHECKED: PlatformPropertyValue = PlatformPropertyValue::Boolean(true);
    const ZERO: PlatformPropertyValue = PlatformPropertyValue::Number(0);
    const OPEN: PlatformPropertyValue = PlatformPropertyValue::Dialog;
    const DO_NOT_USE: PlatformPropertyValue = Choice("doNotUse");
    &[
        Row::new("Title", Main, Localized, Empty),
        Row::new("AutoTitle", Main, Boolean, CHECKED),
        Row::new("Enabled", Main, Boolean, CHECKED),
        Row::new("Customizable", Main, Boolean, CHECKED),
        Row::new("Group", Main, Enum, Choice("vertical")).with_enum_captions(&[
            ("Vertical", "vertical"),
            ("AlwaysHorizontal", "alwaysHorizontal"),
            ("HorizontalIfPossible", "horizontalIfPossible"),
        ]),
        Row::new("CommandBarLocation", Usage, Enum, Auto).with_enum_captions(&[
            ("Auto", "auto"),
            ("Top", "top"),
            ("Bottom", "bottom"),
            ("None", "none"),
        ]),
        Row::new("MobileDeviceCommandBarContent", Usage, Dialog, OPEN),
        Row::new("CommandSet", Usage, Dialog, OPEN),
        Row::new("WindowOpeningMode", Usage, Enum, Choice("independent")).with_enum_captions(&[
            ("Independent", "independent"),
            ("LockOwnerWindow", "lockOwnerWindow"),
        ]),
        Row::new("EnterKeyBehavior", Usage, Enum, Choice("elementNavigation"))
            .with_enum_captions(&[("DefaultButton", "defaultButton")]),
        Row::new("AutoFillCheck", Usage, Boolean, CHECKED),
        Row::new("AutoURL", Usage, Boolean, CHECKED),
        Row::new("ItemsAndTitlesAlign", Usage, Enum, Auto).with_source(Property("ChildrenAlign")),
        Row::new("ShowTitle", Usage, Boolean, CHECKED),
        Row::new("ShowCloseButton", Usage, Boolean, CHECKED),
        Row::new("ConversationsRepresentation", Usage, Enum, Auto),
        Row::new("CollapseItemsByImportance", Usage, Enum, Auto),
        Row::new("UseForFoldersAndItems", Usage, Enum, Choice("forItem"))
            .with_enum_captions(&[("Items", "forItem")]),
        Row::new("AutoSaveDataInSettings", Usage, Enum, DO_NOT_USE)
            .with_enum_captions(&[("DontUse", "doNotUse"), ("Use", "use")]),
        Row::new("SaveDataInSettings", Usage, Enum, DO_NOT_USE)
            .with_enum_captions(&[("DontUse", "doNotUse"), ("UseList", "useList")]),
        Row::new("SaveWindowSettings", Usage, Boolean, CHECKED),
        Row::new("SettingsStorage", Usage, Reference, Empty),
        Row::new("ScalingMode", Appearance, Enum, Auto).with_enum_captions(&[
            ("Auto", "auto"),
            ("Normal", "standardScaling"),
            ("Compact", "compactScaling"),
        ]),
        Row::new("ConditionalAppearance", Appearance, Dialog, OPEN),
        Row::new("HorizontalSpacing", Appearance, Enum, Auto),
        Row::new("VerticalSpacing", Appearance, Enum, Auto),
        Row::new("ChildItemsHorizontalAlign", Layout, Enum, Auto)
            .with_source(Property("HorizontalAlign")),
        Row::new("ChildItemsVerticalAlign", Layout, Enum, Auto)
            .with_source(Property("VerticalAlign")),
        Row::new("Width", Layout, Number, ZERO),
        Row::new("Height", Layout, Number, ZERO),
        Row::new("Scale", Layout, Number, PlatformPropertyValue::Number(100)),
        Row::new("VerticalScroll", Layout, Enum, Auto)
            .with_enum_captions(&[("useIfNecessary", "useIfNecessary")]),
        event("OnCreateAtServer"),
        event("OnOpen"),
        event("OnReopen"),
        event("BeforeClose"),
        event("OnClose"),
        event("ChoiceProcessing"),
        event("NotificationProcessing"),
        event("ActivationProcessing"),
        event("NewWriteProcessing"),
        event("OnReadAtServer"),
        event("BeforeWrite"),
        event("BeforeWriteAtServer"),
        event("OnWriteAtServer"),
        event("AfterWriteAtServer"),
        event("AfterWrite"),
        event("FillCheckProcessingAtServer"),
        event("ExternalEvent"),
        event("AddInDetachmentOnError"),
        event("OnSaveDataInSettingsAtServer"),
        event("BeforeLoadDataFromSettingsAtServer"),
        event("OnLoadDataFromSettingsAtServer"),
        event("URLProcessing"),
        event("URLGetProcessing"),
        event("URLListGetProcessing"),
        event("NavigationProcessing"),
        event("ValueChoice"),
        event("OnChangeDisplaySettings"),
        event("CollaborationSystemUsersAutoComplete"),
        event("CollaborationSystemUsersChoiceFormGetProcessing"),
        event("OnMainServerAvailabilityChange"),
        event("BeforeReopenFromOtherServer"),
        event("OnReopenFromOtherServer"),
        event("OnClientApplicationSuspend"),
        event("OnClientApplicationResume"),
        event("OnPasteFromClipboard"),
    ]
};
