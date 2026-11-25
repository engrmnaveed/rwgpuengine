import re, shutil, sys, pathlib
src = pathlib.Path(sys.argv[1]); out = pathlib.Path(sys.argv[2]); k = int(sys.argv[3])
if out.exists(): shutil.rmtree(out)
shutil.copytree(src, out, ignore=shutil.ignore_patterns('out', 'refs'))
def pair(m):
    return f"{m.group(1)}[{int(float(m.group(2))*k)}, {int(float(m.group(3))*k)}]"
for p in out.rglob('*.toml'):
    t = p.read_text()
    t = re.sub(r'((?:screen_pos|size|offset|tap)\s*=\s*)\[\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*\]', pair, t)
    t = re.sub(r'(scale\s*=\s*)(\d+)', lambda m: f"{m.group(1)}{int(m.group(2))*k}", t)
    if p.name == 'slate.toml':
        t = re.sub(r'width = \d+', f'width = {640*k}', t)
        t = re.sub(r'height = \d+', f'height = {480*k}', t)
        t += f"\n[iso]\ntile_width = {64*k}\ntile_height = {32*k}\nz_height = {32*k}\n"
    # drop [[shot]] blocks (reference images are per resolution)
    t = re.sub(r'\n\[\[shot\]\][^\[]*', '\n', t)
    p.write_text(t)
