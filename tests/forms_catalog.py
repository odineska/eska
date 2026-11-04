#!/usr/bin/env python3
"""Audit Designer XML form vocabulary against installed platform help and optional sources."""

import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import re
import xml.etree.ElementTree as ET

FORM = "http://v8.1c.ru/8.3/xcf/logform"

# Help type identifiers are candidates for Designer properties, not an XML schema.
BASE_TYPES = {
    'root': 'ClientApplicationForm', 'button': 'FormButton', 'field': 'FormField',
    'group': 'FormGroup', 'decoration': 'FormDecoration', 'table': 'FormTable',
    'addition': 'FormItemAddition',
}
FIELD_EXTENSIONS = {
    'CalendarField': 'Form field extension for a calendar field',
    'ChartField': 'Form field extension for a chart field',
    'CheckBoxField': 'Form field extension for a check box field',
    'DendrogramField': 'Form field extension for a dendrogram field',
    'FormattedDocumentField': 'Form field extension for a formatted document',
    'GanttChartField': 'Form field extension for a Gantt chart field',
    'GeographicalSchemaField': 'Form field extension for a geographical schema field',
    'GraphicalSchemaField': 'Form field extension for a graphical schema field',
    'HTMLDocumentField': 'Form extension for a HTML document field',
    'InputField': 'Form field extension for a text box',
    'LabelField': 'Form field extension for a label field',
    'PDFDocumentField': 'Form extension for a PDF document field',
    'PeriodField': 'Form field extension for a period field',
    'PictureField': 'Form field extension for a picture field',
    'PlannerField': 'Form field extension for a planner',
    'ProgressBarField': 'Form field extension for a progress bar field',
    'RadioButtonField': 'Form field extension for a radio button field',
    'SpreadSheetDocumentField': 'Form field extension for a spreadsheet document field',
    'TextDocumentField': 'Form field extension for a text document',
    'TrackBarField': 'Form field extension for a track bar field',
}
OTHER_EXTENSIONS = {
    'ButtonGroup': 'Form group extension for a button group',
    'ColumnGroup': 'Form group extension for a group of columns',
    'CommandBar': 'Form group extension for a command bar',
    'Page': 'Form group extension for a page',
    'Pages': 'Form group extension for pages',
    'Popup': 'Form group extension for a popup',
    'UsualGroup': 'Form group extension for a usual group',
    'LabelDecoration': 'Form decoration extension for a label',
    'PictureDecoration': 'Form decoration extension for a picture',
    'SearchControlAddition': 'Form item addition extension for search control',
    'SearchStringAddition': 'Form item addition extension for search string',
    'ViewStatusAddition': 'Form item addition extension for view status',
}


def help_enumeration(root, name):
    """Read identifiers, never copy proprietary documentation text into the report."""
    folders = list(root.glob(f"objects/*/*/{name}/properties"))
    if len(folders) != 1:
        raise ValueError(f"expected one {name} enumeration, found {len(folders)}")
    return sorted(re.sub(r"\d+\.html$", "", path.name)
                  for path in folders[0].glob("*.html"))


def runtime_bindings(elements):
    """Keep common and type-specific surfaces separate, including unresolved auxiliaries."""
    bindings = {}
    for element in elements:
        tag, family = element['xmlTag'], element['family']
        if tag in bindings:
            raise ValueError(f'duplicate XML kind: {tag}')
        if family == 'auxiliary':
            if tag == 'AutoCommandBar':
                types = ['FormGroup']
            elif tag == 'ExtendedTooltip':
                types = ['FormDecoration']
            else:
                raise ValueError(f'unknown auxiliary kind: {tag}')
        else:
            if family not in BASE_TYPES:
                raise ValueError(f'unknown family: {family}')
            types = [BASE_TYPES[family]]
        if family == 'field':
            if tag not in FIELD_EXTENSIONS:
                raise ValueError(f'unknown field extension: {tag}')
            types.append(FIELD_EXTENSIONS[tag])
        elif tag in OTHER_EXTENSIONS:
            types.append(OTHER_EXTENSIONS[tag])
        bindings[tag] = types
    return bindings


def english_alias(text):
    """Read the machine alias, including dynamic members and identifiers ending in digits."""
    aliases = re.findall(r'\{"en",\s*\d+,\s*\d+,\s*"",\s*"([^"]*)"\}', text)
    if len(aliases) != 1:
        raise ValueError('invalid_help_english_alias')
    return aliases[0]


def help_properties(root, name):
    """Retain identifiers and file hashes; never retain documentation prose."""
    folders = list(root.glob(f'objects/catalog1649/*/{name}/properties'))
    if len(folders) != 1:
        raise ValueError(f'expected one runtime type {name}, found {len(folders)}')
    files, dynamic, names = [], [], set()
    for path in sorted(folders[0].glob('*.html')):
        alias_file = path.with_suffix('.st')
        alias_bytes = alias_file.read_bytes()
        alias = english_alias(alias_bytes.decode('utf-8-sig'))
        definition = {'name': alias, 'file': path.name,
                      'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                      'aliasFile': alias_file.name,
                      'aliasSha256': hashlib.sha256(alias_bytes).hexdigest()}
        if re.fullmatch(r'<[^<>]+>', alias):
            dynamic.append(definition)
            continue
        if not re.fullmatch(r'[A-Za-z_][A-Za-z_0-9]*', alias) or alias in names:
            raise ValueError(f'invalid or duplicate property identifier: {path.name}')
        names.add(alias)
        files.append(definition)
    return {'source': folders[0].relative_to(root).as_posix(),
            'properties': files, 'dynamicMembers': dynamic}


def property_candidates(elements, types, observed=None):
    """A runtime-only name is a gap to investigate, not a supported XML property."""
    rows = []
    for tag, bindings in sorted(runtime_bindings(elements).items()):
        declared = {item['name'] for name in bindings for item in types[name]['properties']}
        xml = set((observed or {}).get(tag, []))
        xml = {name.removeprefix(f'{{{FORM}}}') for name in xml
               if name.startswith(f'{{{FORM}}}')}
        rows.append({'xmlTag': tag, 'runtimeTypeCandidates': bindings,
                     'runtimePropertyNames': sorted(declared),
                     'observedXmlProperties': sorted(xml),
                     'runtimeNamesNotObservedAsXml': sorted(declared - xml),
                     'xmlNamesNotDeclaredByTheseRuntimeTypes': sorted(xml - declared),
                     'designerApplicabilityVerified': False,
                     'stateCombinationMatrixComplete': False})
    return rows


def audit_property_candidates(root, elements, observed=None):
    bindings = runtime_bindings(elements)
    names = {name for values in bindings.values() for name in values}
    # These owner/data-dependent additions must not be applied to every form/table.
    context = {}
    for prefix in ['Client application form extension for ', 'Form table extension for ']:
        for folder in sorted(root.glob(f'objects/catalog1649/*/{prefix}*/properties')):
            name = folder.parent.name
            context[name] = help_properties(root, name)
    types = {name: help_properties(root, name) for name in sorted(names)}
    return {
        'nativePropertyInventoryComplete': False,
        'visualParityVerified': False,
        'scope': 'Runtime help candidates and observed XML names only; not Designer XML applicability or rendering acceptance.',
        'runtimeTypes': types,
        'contextDependentTypes': context,
        'elements': property_candidates(elements, types, observed),
        'unresolved': [
            'Designer-only properties, defaults and applicable combinations need Configurator verification.',
            'Runtime properties include data, structural and runtime-only members; names alone do not establish a visual property.',
            'AutoCommandBar, ContextMenu and ExtendedTooltip need subtype/applicability verification.',
            'Owner/data-dependent extensions are listed separately; they are not unconditional form/table properties.',
            'Different casing and spellings are not silently treated as aliases.',
        ],
    }


def audit_sources(root, kinds):
    """Keep source directories read-only and report only names, counts and property shapes."""
    counts, properties, unknown = Counter(), defaultdict(set), Counter()
    forms = 0
    for path in sorted(root.rglob("Form.xml")):
        form = ET.parse(path).getroot()
        if form.tag != f"{{{FORM}}}Form":
            continue
        forms += 1
        for node in form.iter():
            name = node.tag.removeprefix(f"{{{FORM}}}")
            if name in kinds:
                counts[name] += 1
                properties[name].update(child.tag for child in node)
            elif node.get("id") is not None and node.get("name") is not None:
                if name not in {"Attribute", "Column", "Command", "Parameter"}:
                    unknown[name] += 1
    return {"forms": forms, "elements": dict(sorted(counts.items())),
            "properties": {key: sorted(value) for key, value in sorted(properties.items())},
            "unknownIdentityNodes": dict(sorted(unknown.items()))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--help-root", type=Path, required=True,
                        help="read-only directory extracted from installed shcntx_ru.hbk")
    parser.add_argument("--source-root", type=Path,
                        help="optional read-only directory of Designer XML forms")
    parser.add_argument("--help-archive", type=Path,
                        help="optional original HBK to identify the installed reference")
    parser.add_argument('--properties', action='store_true',
                        help='runtime property candidates / кандидаты свойств без приёмки')
    args = parser.parse_args()
    catalog = json.loads((Path(__file__).parent / "fixtures/forms/catalog.json").read_text())
    tags = [item["xmlTag"] for item in catalog["elements"]]
    if len(tags) != len(set(tags)):
        raise ValueError("duplicate catalog XML tag")
    enums = {}
    for name in ["FormFieldType", "FormGroupType"]:
        actual = help_enumeration(args.help_root, name)
        expected = sorted(item["platformValue"] for item in catalog["elements"]
                          if item.get("platformEnum") == name)
        if actual != expected:
            raise ValueError(f"{name}: missing={set(actual)-set(expected)}, extra={set(expected)-set(actual)}")
        enums[name] = actual
    report = {"schemaVersion": 1, "platform": catalog["platform"],
              "catalogElements": len(tags), "nativeEnumerations": enums,
              "visualParityVerified": False}
    if args.help_archive:
        report["helpSha256"] = hashlib.sha256(args.help_archive.read_bytes()).hexdigest()
    if args.source_root:
        report["sources"] = audit_sources(args.source_root, set(tags))
    if args.properties:
        report['propertyCandidates'] = audit_property_candidates(
            args.help_root, catalog['elements'], report.get('sources', {}).get('properties'))
        for name in ['FormButtonType', 'FormDecorationType', 'FormItemAdditionType']:
            report['nativeEnumerations'][name] = help_enumeration(args.help_root, name)
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
