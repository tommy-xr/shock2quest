"""Collect an exact git range, linked PRs, and source material for visual curation."""
import argparse
import html
import json
from pathlib import Path
import subprocess


def run(*args):
    return subprocess.check_output(args, text=True).strip()


def api(endpoint):
    pages = json.loads(run('gh', 'api', '--paginate', '--slurp', endpoint))
    return [item for page in pages for item in page]


def resolve(ref):
    return run('git', 'rev-parse', '--verify', '--end-of-options', f'{ref}^{{commit}}')


def commits_between(base, head):
    subprocess.run(['git', 'merge-base', '--is-ancestor', base, head], check=True)
    lines = run('git', 'log', '--reverse', '--topo-order', '--format=%H%x09%s',
                f'{base}..{head}')
    return [dict(zip(('sha', 'subject'), line.split('\t', 1)))
            for line in lines.splitlines()]


def previous_release(repo, head):
    releases = sorted((r for r in api(f'repos/{repo}/releases?per_page=100')
                       if not r['draft'] and not r['prerelease']),
                      key=lambda r: r['published_at'], reverse=True)
    for release in releases:
        # Missing tags indicate an incomplete checkout; fail instead of silently
        # picking an older baseline and including already-released changes.
        base = resolve(release['tag_name'])
        if subprocess.run(['git', 'merge-base', '--is-ancestor', base, head],
                          stdout=subprocess.DEVNULL).returncode == 0:
            return release['tag_name']
    raise ValueError('No published ancestor release; supply --since explicitly')


def collect(repo, since, until):
    base, head = resolve(since), resolve(until)
    commits = commits_between(base, head)
    prs = {}
    for commit in commits:
        associated = api(f'repos/{repo}/commits/{commit["sha"]}/pulls?per_page=100')
        commit['prs'] = []
        for pr in associated:
            if not pr.get('merged_at') or pr['base']['repo']['full_name'] != repo:
                continue
            number = pr['number']
            commit['prs'].append(number)
            prs[number] = {key: pr[key] for key in ('number', 'title', 'body', 'html_url')}
        commit['prs'].sort()
    return dict(repository=repo, since=since, until=until, base_sha=base,
                head_sha=head, commits=commits,
                pull_requests=sorted(prs.values(), key=lambda p: p['number']))


def markdown(data):
    url = f'https://github.com/{data["repository"]}'
    lines = ['## Changelog', '',
             f'Changes since `{data["since"]}` through `{data["head_sha"][:12]}`.', '',
             f'[Full comparison]({url}/compare/{data["base_sha"]}...{data["head_sha"]})', '']
    for commit in data['commits']:
        links = ', '.join(f'[#{n}]({url}/pull/{n})' for n in commit['prs'])
        subject = html.escape(commit['subject'])
        # Keep commit subjects literal even if they contain Markdown syntax.
        for char in '\\`*_[]':
            subject = subject.replace(char, '\\' + char)
        lines.append(f'- {subject} ([{commit["sha"][:8]}]({url}/commit/{commit["sha"]})'
                     + (f'; {links}' if links else '') + ')')
    if not data['commits']:
        lines.append('No new commits.')
    return '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', help='OWNER/REPO; defaults to the current gh repository')
    parser.add_argument('--since', help='Previous release tag; defaults to latest published ancestor')
    parser.add_argument('--until', default='HEAD')
    parser.add_argument('--output', type=Path, default=Path('target/release-notes'))
    args = parser.parse_args()
    repo = args.repo or run('gh', 'repo', 'view', '--json', 'nameWithOwner', '--jq', '.nameWithOwner')
    since = args.since or previous_release(repo, resolve(args.until))
    data = collect(repo, since, args.until)
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'changelog.md').write_text(markdown(data))
    (args.output / 'sources.json').write_text(json.dumps(data, indent=2) + '\n')
    print(f'{len(data["commits"])} commits, {len(data["pull_requests"])} PRs: {args.output}')


if __name__ == '__main__':
    main()
