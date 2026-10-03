from pathlib import Path
import unittest

from verify_gitleaks_scope import checked_self_test

BASELINE = '[tasks."security:secrets:self-test"]\nrun = "known scanner self-test"\n'


class SelfTestBindingTests(unittest.TestCase):
    def test_unchanged_self_test_is_returned(self):
        self.assertEqual(checked_self_test(BASELINE, BASELINE, Path('/verified/gitleaks')), 'known scanner self-test')

    def test_replacement_with_true_is_rejected(self):
        changed = BASELINE.replace('known scanner self-test', 'true')
        with self.assertRaises(ValueError):
            checked_self_test(changed, BASELINE, Path('/verified/gitleaks'))

    def test_renamed_verified_binary_is_rejected(self):
        with self.assertRaises(ValueError):
            checked_self_test(BASELINE, BASELINE, Path('/verified/renamed'))

    def test_unrelated_task_does_not_change_self_test_identity(self):
        current = BASELINE + '\n[tasks.unrelated]\nrun = "unused"\n'
        self.assertEqual(checked_self_test(current, BASELINE, Path('/verified/gitleaks')), 'known scanner self-test')


if __name__ == '__main__':
    unittest.main()
