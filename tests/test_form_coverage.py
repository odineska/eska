from copy import deepcopy
import importlib.util
import json
from pathlib import Path
import unittest

MODULE = Path(__file__).with_name('form_coverage.py')
spec = importlib.util.spec_from_file_location('form_coverage', MODULE)
coverage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coverage)

class CoverageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.ledger = json.loads((coverage.ROOT / coverage.REGISTRY).read_text(encoding='utf-8'))

    def check_error(self, data, code):
        with self.assertRaises(coverage.CoverageError) as result:
            coverage.validate(data)
        self.assertEqual(result.exception.code, code)

    def test_current_ledger_keeps_all_kinds_and_does_not_accept_full_profile(self):
        result = coverage.validate(self.ledger)
        self.assertEqual(result['catalogKinds'], 38)
        self.assertFalse(result['fullProfileAccepted'])
        self.assertFalse(result['nativePropertyInventoryComplete'])

    def test_missing_measurement_cannot_become_acceptance(self):
        data = deepcopy(self.ledger)
        row = next(r for r in data['entries'] if r['id'] == 'buttons.text-widgets')
        row['evidence'] = []
        self.check_error(data, 'acceptance_without_evidence')

    def test_boolean_false_does_not_satisfy_zero_mismatch_assertion(self):
        data = deepcopy(self.ledger)
        row = next(r for r in data['entries'] if r['id'] == 'buttons.primary-interiors')
        row['evidence'][0]['assertions'][1]['equals'] = False
        self.check_error(data, 'evidence_assertion_failed')

    def test_nonzero_pixel_tolerance_is_not_accepted(self):
        data = deepcopy(self.ledger)
        data['comparisonPolicy']['pixelTolerance'] = 1
        self.check_error(data, 'comparison_policy_changed')

    def test_catalogue_kind_cannot_disappear_from_ledger(self):
        data = deepcopy(self.ledger)
        data['kinds'].pop()
        self.check_error(data, 'catalog_coverage_incomplete')

    def test_later_scoped_evidence_retains_the_remaining_requirement(self):
        changes = {r['id']: r for r in self.ledger['supersessions']}
        self.assertEqual(changes['input-date-values']['currentEntries'], ['input.default-dates'])
        self.assertIn('formats', changes['input-date-values']['currentRemainder'])
        self.assertEqual(changes['table-context-inputs']['currentEntries'], ['tables.native-context-fixtures'])
        self.assertIn('pixels remain open', changes['table-context-inputs']['currentRemainder'])

if __name__ == '__main__':
    unittest.main()
