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


def help_enumeration(root, name):
    """Read identifiers, never copy proprietary documentation text into the report."""
    folders = list(root.glob(f"objects/*/*/{name}/properties"))
    if len(folders) != 1:
        raise ValueError(f"expected one {name} enumeration, found {len(folders)}")
    return sorted(re.sub(r"\d+\.html$", "", path.name)
                  for path in folders[0].glob("*.html"))


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
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
