import importlib.util
import json
from pathlib import Path
import unittest

MODULE = Path(__file__).with_name('forms_catalog.py')
spec = importlib.util.spec_from_file_location('forms_catalog', MODULE)
catalogue = importlib.util.module_from_spec(spec)
spec.loader.exec_module(catalogue)


class PropertyCandidatesTests(unittest.TestCase):
    def test_machine_alias_preserves_digits_and_dynamic_member_placeholder(self):
        self.assertEqual(catalogue.english_alias('{0,{"en",0,0,"","Value3D"}}'), 'Value3D')
        self.assertEqual(catalogue.english_alias('{0,{"en",0,0,"","<Attribute name>"}}'), '<Attribute name>')

    def test_missing_or_ambiguous_machine_alias_is_rejected(self):
        for text in ['', '{"ru",0,0,"","Имя"}', '{"en",0,0,"","One"}{"en",0,0,"","Two"}']:
            with self.assertRaisesRegex(ValueError, 'invalid_help_english_alias'):
                catalogue.english_alias(text)

    def test_all_38_kinds_have_separate_common_and_specific_candidates(self):
        elements = json.loads(MODULE.with_name('fixtures').joinpath('forms/catalog.json').read_text())['elements']
        bindings = catalogue.runtime_bindings(elements)
        self.assertEqual(len(bindings), 38)
        self.assertEqual(bindings['InputField'], ['FormField', 'Form field extension for a text box'])
        self.assertEqual(bindings['PictureField'], ['FormField', 'Form field extension for a picture field'])
        self.assertEqual(bindings['PictureDecoration'], ['FormDecoration', 'Form decoration extension for a picture'])
        self.assertEqual(bindings['ContextMenu'], ['FormGroup'])

    def test_unknown_field_cannot_silently_lose_its_specific_surface(self):
        with self.assertRaisesRegex(ValueError, 'unknown field extension'):
            catalogue.runtime_bindings([{'xmlTag': 'NewField', 'family': 'field'}])

    def test_duplicate_kind_is_not_collapsed(self):
        row = {'xmlTag': 'Button', 'family': 'button'}
        with self.assertRaisesRegex(ValueError, 'duplicate XML kind'):
            catalogue.runtime_bindings([row, row])

    def test_runtime_candidates_do_not_establish_designer_applicability(self):
        rows = [{'xmlTag': 'InputField', 'family': 'field'}]
        types = {'FormField': {'properties': [{'name': 'Title'}, {'name': 'ToolTip'}]},
                 'Form field extension for a text box': {'properties': [{'name': 'MultiLine'}]}}
        observed = {'InputField': [f'{{{catalogue.FORM}}}Title', f'{{{catalogue.FORM}}}Tooltip', '{urn:other}MultiLine']}
        row, = catalogue.property_candidates(rows, types, observed)
        self.assertEqual(row['runtimePropertyNames'], ['MultiLine', 'Title', 'ToolTip'])
        self.assertEqual(row['observedXmlProperties'], ['Title', 'Tooltip'])
        self.assertEqual(row['runtimeNamesNotObservedAsXml'], ['MultiLine', 'ToolTip'])
        self.assertEqual(row['xmlNamesNotDeclaredByTheseRuntimeTypes'], ['Tooltip'])
        self.assertFalse(row['designerApplicabilityVerified'])
        self.assertFalse(row['stateCombinationMatrixComplete'])

    def test_absent_corpus_does_not_hide_undocumented_candidates(self):
        row, = catalogue.property_candidates([{'xmlTag': 'Button', 'family': 'button'}],
                                             {'FormButton': {'properties': [{'name': 'Picture'}]}})
        self.assertEqual(row['observedXmlProperties'], [])
        self.assertEqual(row['runtimeNamesNotObservedAsXml'], ['Picture'])

    def test_recorded_candidates_keep_all_kinds_and_do_not_claim_designer_acceptance(self):
        root = MODULE.parent.parent
        report = json.loads((root / 'docs/measurements/t152-property-candidates-linux.json').read_text())
        reference = json.loads((root / 'docs/measurements/t152-form-catalog.json').read_text())
        catalog = json.loads((root / 'tests/fixtures/forms/catalog.json').read_text())
        self.assertEqual(report['helpSha256'], reference['helpSha256'])
        candidates = report['propertyCandidates']
        self.assertFalse(candidates['nativePropertyInventoryComplete'])
        self.assertFalse(candidates['visualParityVerified'])
        self.assertEqual({row['xmlTag'] for row in candidates['elements']},
                         {row['xmlTag'] for row in catalog['elements']})
        self.assertEqual(report['nativeEnumerations']['FormButtonType'],
                         ['CommandBarButton', 'CommandBarHyperlink', 'Hyperlink', 'UsualButton'])
        for row in candidates['elements']:
            self.assertFalse(row['designerApplicabilityVerified'])
            self.assertFalse(row['stateCombinationMatrixComplete'])
        form = candidates['runtimeTypes']['ClientApplicationForm']
        self.assertNotIn('prop', {value['name'] for value in form['properties']})
        self.assertEqual([value['name'] for value in form['dynamicMembers']], ['<Attribute name>'])


if __name__ == '__main__':
    unittest.main()
