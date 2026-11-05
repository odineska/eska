#!/usr/bin/env python3
"""Validate the current form evidence ledger; never infer whole-profile acceptance."""
from collections import Counter
from hashlib import sha256
import json
from pathlib import Path, PurePosixPath
import sys
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
REGISTRY = Path('tests/fixtures/forms/coverage.json')
LEVELS = {'semantics', 'geometry', 'widget_pixels', 'form_pixels', 'behavior'}
STATUSES = {'unmeasured', 'reproduced_mismatch', 'in_progress', 'accepted_scope', 'deferred'}

class CoverageError(ValueError):
    def __init__(self, code, **context):
        self.code, self.context = code, context
        super().__init__(code)

def require(condition, code, **context):
    if not condition:
        raise CoverageError(code, **context)

def local(root, name):
    path = PurePosixPath(name)
    require(not path.is_absolute() and '..' not in path.parts and '\\' not in name, 'invalid_relative_path', path=name)
    result = root / name
    require(result.exists(), 'missing_reference', path=name)
    return result

def pointer(document, path):
    value = document
    if path == '':
        return value
    require(path.startswith('/'), 'invalid_json_pointer', pointer=path)
    try:
        for token in path[1:].split('/'):
            token = token.replace('~1', '/').replace('~0', '~')
            value = value[int(token)] if isinstance(value, list) else value[token]
    except (KeyError, IndexError, ValueError, TypeError):
        raise CoverageError('missing_evidence_value', pointer=path) from None
    return value

def validate(data, root=ROOT, explorer=None):
    require(data.get('schemaVersion') == 1, 'unsupported_schema')
    catalog = json.loads(local(root, data['catalog']).read_text(encoding='utf-8'))
    expected = {r['xmlTag'] for r in catalog['elements']}
    require(len(expected) == 38, 'catalog_changed_requires_review')
    kind_rows = data['kinds']
    require(len(kind_rows) == len(expected) and {r['kind'] for r in kind_rows} == expected, 'catalog_coverage_incomplete')
    require(data['comparisonPolicy']['pixelTolerance'] == 0, 'comparison_policy_changed')
    profiles = {r['id']: r for r in data['profiles']}
    axes = {r['id'] for r in data['requiredAxes']}
    entries = data['entries']
    ids = {r['id'] for r in entries}
    require(len(ids) == len(entries), 'duplicate_scope_id')
    local(root, data['roadmap'])
    for row in kind_rows:
        require(row['requiredAxes'] and set(row['requiredAxes']) <= axes, 'unknown_axis', kind=row['kind'])
        require(set(row['scopes']) <= ids and row['scopes'], 'unknown_kind_scope', kind=row['kind'])
        example = ET.parse(local(root, row['nativeExample']))
        require(any(e.tag.rsplit('}', 1)[-1] == row['kind'] for e in example.iter()), 'example_missing_kind', kind=row['kind'])
        for id in row['scopes']:
            require(row['kind'] in next(e for e in entries if e['id'] == id)['kinds'], 'scope_kind_mismatch', id=id)
    for row in entries:
        id = row['id']
        require(row['status'] in STATUSES and row['level'] in LEVELS, 'invalid_scope_state', id=id)
        require(row['kinds'] and set(row['kinds']) <= expected, 'unknown_kind', id=id)
        require(row['axes'] and set(row['axes']) <= axes and row['profile'] in profiles, 'unknown_profile_or_axis', id=id)
        require(row['remaining'] and row['nextAction'], 'missing_remainder', id=id)
        for name in row['fixtures']:
            local(root, name)
        driver = row['driver']
        if driver:
            require(driver['repository'] in {'eska', 'eska-vscode-explorer'}, 'unknown_driver_repository', id=id)
            require(driver.get('caseIds') and driver.get('path'), 'missing_case_identity', id=id)
            if driver['repository'] == 'eska':
                local(root, driver['path'])
            elif explorer:
                local(explorer, driver['path'])
        if row['status'] == 'accepted_scope':
            require(row['evidence'] and row['lastConfirmation'] and driver, 'acceptance_without_evidence', id=id)
            require(row['lastConfirmation'] in [e['report'] for e in row['evidence']], 'unbound_confirmation', id=id)
            require(any(e['assertions'] for e in row['evidence']), 'acceptance_without_assertions', id=id)
        if row['status'] == 'unmeasured':
            require(not row['evidence'] and row['lastConfirmation'] is None, 'unmeasured_has_confirmation', id=id)
        if row['status'] == 'deferred':
            local(root, row['decision'])
            require(profiles[row['profile']].get('status') == 'deferred', 'invalid_deferral', id=id)
        for evidence in row['evidence']:
            file = local(root, evidence['report'])
            require(sha256(file.read_bytes()).hexdigest() == evidence['sha256'], 'evidence_changed', id=id, path=evidence['report'])
            document = json.loads(file.read_text(encoding='utf-8'))
            for assertion in evidence['assertions']:
                value = pointer(document, assertion['pointer'])
                require(type(value) is type(assertion['equals']) and value == assertion['equals'], 'evidence_assertion_failed', id=id, pointer=assertion['pointer'])
    for row in data['supersessions']:
        local(root, row['historicalReport'])
        require(set(row['currentEntries']) <= ids and row['currentRemainder'], 'invalid_supersession', id=row['id'])
    for name in data['historicalReports']:
        local(root, name)
    for group in data['nextGroups']:
        require(set(group['entries']) <= ids, 'unknown_next_scope', id=group['id'])
    return {'schemaVersion': 1, 'valid': True, 'fullProfileAccepted': False,
            'catalogKinds': len(expected), 'nativePropertyInventoryComplete': data['nativePropertyInventoryComplete'],
            'scopeCountsByStatus': dict(Counter(r['status'] for r in entries)),
            'scopeCountsByLevel': dict(Counter(r['level'] for r in entries)),
            'explorerDriversChecked': explorer is not None,
            'historicalReports': len(data['historicalReports']), 'supersededHistoricalGaps': len(data['supersessions']),
            'nextGroups': [{**g, 'nextActions': [next(r for r in entries if r['id'] == id)['nextAction'] for id in g['entries']]} for g in data['nextGroups']]}

def main(args):
    explorer = ROOT.parent / 'eska-vscode-explorer'
    if args == ['--help']:
        return {'usage': ['python3 tests/form_coverage.py', 'python3 tests/form_coverage.py --explorer-root <path>'], 'output': 'json'}
    if args:
        require(len(args) == 2 and args[0] == '--explorer-root', 'invalid_arguments')
        explorer = Path(args[1]).resolve()
        require(explorer.exists(), 'missing_explorer_root', path=str(explorer))
    return validate(json.loads((ROOT / REGISTRY).read_text(encoding='utf-8')), explorer=explorer if explorer.exists() else None)

if __name__ == '__main__':
    sys.stdout.reconfigure(encoding='utf-8')
    try:
        print(json.dumps(main(sys.argv[1:]), ensure_ascii=False, indent=2))
    except CoverageError as error:
        print(json.dumps({'valid': False, 'error': {'code': error.code, **error.context}}, ensure_ascii=False))
        sys.exit(1)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(json.dumps({'valid': False, 'error': {'code': 'invalid_registry', 'kind': type(error).__name__}}))
        sys.exit(1)
