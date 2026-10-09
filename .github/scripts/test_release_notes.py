import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('release_notes', Path(__file__).with_name('release-notes.py'))
notes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notes)


class ReleaseNotesTests(unittest.TestCase):
    def test_exact_range_includes_side_branch_and_excludes_base(self):
        with tempfile.TemporaryDirectory() as folder:
            def git(*args):
                return subprocess.check_output(['git', '-C', folder, *args], text=True).strip()
            git('init', '-q', '-b', 'main')
            git('config', 'user.email', 'test@example.com')
            git('config', 'user.name', 'Test')
            git('commit', '-q', '--allow-empty', '-m', 'base')
            base = git('rev-parse', 'HEAD')
            git('checkout', '-q', '-b', 'feature')
            git('commit', '-q', '--allow-empty', '-m', 'feature')
            feature = git('rev-parse', 'HEAD')
            git('checkout', '-q', 'main')
            git('commit', '-q', '--allow-empty', '-m', 'direct')
            git('merge', '-q', '--no-ff', 'feature', '-m', 'merge')
            head = git('rev-parse', 'HEAD')
            real_run = subprocess.run
            with patch.object(notes, 'run', side_effect=lambda *a: git(*a[1:])), \
                 patch.object(notes.subprocess, 'run', side_effect=lambda a, **kw: real_run(
                     ['git', '-C', folder, *a[1:]], **kw)):
                commits = notes.commits_between(base, head)
                self.assertEqual(len(commits), 3)
                self.assertIn(feature, [c['sha'] for c in commits])
                self.assertNotIn(base, [c['sha'] for c in commits])
                self.assertEqual(notes.commits_between(head, head), [])
                with self.assertRaises(subprocess.CalledProcessError):
                    notes.commits_between(head, base)

    def test_collection_deduplicates_prs_and_keeps_direct_commits(self):
        pr = dict(number=12, title='Change', body='![before](https://example.com/b.png)',
                  html_url='https://github.com/a/b/pull/12', merged_at='date',
                  base={'repo': {'full_name': 'a/b'}})
        with patch.object(notes, 'resolve', side_effect=['base', 'head']), \
             patch.object(notes, 'commits_between', return_value=[
                 {'sha': 'one', 'subject': 'first'}, {'sha': 'two', 'subject': 'second'},
                 {'sha': 'three', 'subject': '<img> [direct]'}]), \
             patch.object(notes, 'api', side_effect=[[pr], [pr], [dict(pr, merged_at=None)]]):
            data = notes.collect('a/b', 'v0.0.1', 'HEAD')
        self.assertEqual(len(data['pull_requests']), 1)
        self.assertEqual(data['commits'][2]['prs'], [])
        output = notes.markdown(data)
        self.assertIn('/commit/three', output)
        self.assertIn('/pull/12', output)
        self.assertIn('&lt;img&gt;', output)
        self.assertEqual(data['pull_requests'][0]['body'], pr['body'])

    def test_pagination_keeps_every_page(self):
        with patch.object(notes, 'run', return_value='[[{"number":1}],[{"number":2}]]'):
            self.assertEqual(notes.api('endpoint'), [{'number': 1}, {'number': 2}])

    def test_automatic_baseline_skips_drafts_prereleases_and_nonancestors(self):
        releases = [
            dict(tag_name='draft', draft=True, prerelease=False, published_at=None),
            dict(tag_name='preview', draft=False, prerelease=True, published_at='2026-10-10'),
            dict(tag_name='other-branch', draft=False, prerelease=False, published_at='2026-10-09'),
            dict(tag_name='v0.0.1', draft=False, prerelease=False, published_at='2026-10-07'),
        ]
        with patch.object(notes, 'api', return_value=releases), \
             patch.object(notes, 'resolve', side_effect=lambda ref: ref), \
             patch.object(notes.subprocess, 'run', side_effect=[
                 subprocess.CompletedProcess([], 1), subprocess.CompletedProcess([], 0)]):
            self.assertEqual(notes.previous_release('a/b', 'head'), 'v0.0.1')

    def test_missing_published_tag_fails_instead_of_using_wrong_baseline(self):
        with patch.object(notes, 'api', return_value=[dict(
                tag_name='missing', draft=False, prerelease=False, published_at='2026-10-09')]), \
             patch.object(notes, 'resolve', side_effect=subprocess.CalledProcessError(128, 'git')):
            with self.assertRaises(subprocess.CalledProcessError):
                notes.previous_release('a/b', 'head')
