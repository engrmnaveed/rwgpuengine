# Mutation fuzz: break example files at random and check that slate never
# panics, hangs or prints non-JSON under --json. Deterministic (seeded).
import json, pathlib, random, re, shutil, subprocess, sys, collections
slate, repo, work, n = sys.argv[1], pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3]), int(sys.argv[4])
rng = random.Random(20261125)
examples = ['iso_garden', 'coin_vault', 'tiny_town']
VALUES = ['-1', '0', '999999999999', '1e308', '-0.0', '""', '"ground"', '"self"', 'true', 'false', '[]', '[[1,2],[3]]',
          '{}', '[0, 0]', '[-5, 99999]', '"#ggg"', '"\\u0000"', '9223372036854775807', '[1, 1, 1, 1]', '0.5', '"é😀"', '4097', '1000001']
def mutate(text):
    lines = text.split('\n')
    kind = rng.randrange(9)
    i = rng.randrange(len(lines)) if lines else 0
    if kind == 0 and lines: del lines[i]
    elif kind == 1 and lines: lines.insert(i, lines[rng.randrange(len(lines))])
    elif kind == 2 and lines:
        m = re.match(r'^(\s*[\w.]+\s*=\s*)(.*)$', lines[i])
        if m: lines[i] = m.group(1) + rng.choice(VALUES)
    elif kind == 3 and lines:
        j = rng.randrange(len(lines)); lines[i], lines[j] = lines[j], lines[i]
    elif kind == 4 and lines and lines[i]:
        k = rng.randrange(len(lines[i])); lines[i] = lines[i][:k] + lines[i][k+1:]
    elif kind == 5: lines = lines[:i]
    elif kind == 6 and lines:
        lines[i] = re.sub(r'-?\d+', lambda m: rng.choice(['-1', '0', '99999', '2147483648', '-2147483649', '1000000']), lines[i], count=1)
    elif kind == 7 and lines:
        names = re.findall(r'"([a-z_]+)"', text)
        if names: lines[i] = re.sub(r'"[a-z_]+"', '"' + rng.choice(names) + '"', lines[i], count=1)
    else:
        lines.insert(i, rng.choice(['[[entity]]', '[[connect]]', '[ground]', 'footprint = [4096, 4096]', 'cells = [[0,0],[0,0]]',
                                    'counts = { n = "house" }', 'when = [{ reason = "funds" }]', 'actions = []', 'frames = 1000000']))
    return '\n'.join(lines)
stats = collections.Counter(); bad = []
for it in range(n):
    ex = rng.choice(examples)
    dst = work / 'fz'
    if dst.exists(): shutil.rmtree(dst)
    shutil.copytree(repo / 'examples' / ex, dst, ignore=shutil.ignore_patterns('out'))
    files = sorted(p for p in dst.rglob('*.toml'))
    for _ in range(rng.choice([1, 1, 1, 2, 3])):
        f = rng.choice(files); f.write_text(mutate(f.read_text()))
    for cmd in (['check'], ['test'], ['run', '--frames', '200'], ['shot', '--report'], ['inspect', '--events']):
        try:
            p = subprocess.run([slate, *cmd, '--json'], cwd=dst, capture_output=True, timeout=20)
        except subprocess.TimeoutExpired:
            stats['TIMEOUT'] += 1; bad.append((it, ex, cmd, 'timeout')); shutil.copytree(dst, work / f'bad_{it}_{cmd[0]}', dirs_exist_ok=True); continue
        stats[f'exit {p.returncode}'] += 1
        ok_json = True
        try: json.loads(p.stdout)
        except Exception: ok_json = False
        if p.returncode not in (0, 1) or not ok_json or b'panicked' in p.stderr:
            bad.append((it, ex, cmd, p.returncode, p.stderr[-300:].decode(errors='replace')))
            shutil.copytree(dst, work / f'bad_{it}_{cmd[0]}', dirs_exist_ok=True)
print(dict(stats)); print('bad:', len(bad))
seen = set()
for b in bad:
    key = re.sub(r'\d+', 'N', str(b[4]) if len(b) > 4 else 'timeout')[:160]
    if key in seen: continue
    seen.add(key); print(b)
