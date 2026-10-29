# D-72 scale check: writes a knowledge folder of N Markdown pages for external mode (FR-KN-EXT-*).
#
#   python3 -I scripts/probes/ext-gen-folder.py /tmp/x-big 10000
#
# Six categories (top folders), nested groups up to three levels, a few pages in the root, front
# matter on about a third of the pages, relative links and `[[Title]]` links between pages, and
# bodies of about 150 to 1,500 words (median about 450), like real notes. Deterministic (seed 72).
# The folder must not exist yet.
import os
import random
import sys

root, n = sys.argv[1], int(sys.argv[2])
if os.path.exists(root):
    sys.exit(f"{root} exists already")
rng = random.Random(72)
WORDS = (
    "pump seal valve pressure flow sensor check routine site safety hose fitting gasket torque "
    "rust ownership borrow lifetime trait async tokio thread channel index query cache network "
    "customer portal invoice report pipeline batch schedule deploy release review test fix build "
    "the a of to and in for with on is are be was this that it from as by at an or not"
).split()
CATS = ["Operations", "Learning", "Projects", "Customers", "Engineering", "Admin"]


def sentence():
    w = [rng.choice(WORDS) for _ in range(rng.randint(6, 18))]
    return " ".join(w).capitalize() + "."


# Folders: each category gets 3-6 groups, each with 0-3 subgroups, some with a third level.
folders = []
for c in CATS:
    folders.append(c)
    for g in range(rng.randint(3, 6)):
        gp = f"{c}/Group {g + 1}"
        folders.append(gp)
        for s in range(rng.randint(0, 3)):
            sp = f"{gp}/Sub {s + 1}"
            folders.append(sp)
            if rng.random() < 0.3:
                folders.append(f"{sp}/Deep")

pages = []  # (relative path, title)
for i in range(n):
    folder = "" if i < 20 else rng.choice(folders)
    title = f"Note {i:05d} {rng.choice(WORDS)} {rng.choice(WORDS)}"
    pages.append((f"{folder}/{title}.md" if folder else f"{title}.md", title))

for i, (path, title) in enumerate(pages):
    full = os.path.join(root, path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    out = []
    if rng.random() < 0.35:
        out.append("---")
        out.append("tags: [" + ", ".join(rng.sample(WORDS[:40], rng.randint(1, 3))) + "]")
        if rng.random() < 0.5:
            out.append(f'source: "Meeting notes {rng.randint(1, 300)}"')
        out.append("---")
        out.append("")
    out.append(f"# {title}")
    out.append("")
    words = int(rng.lognormvariate(6.1, 0.55))
    words = max(150, min(1500, words))
    written = 0
    while written < words:
        para = [sentence() for _ in range(rng.randint(2, 6))]
        # Links to other pages: relative, or Obsidian style.
        if rng.random() < 0.5:
            tp, tt = pages[rng.randrange(n)]
            rel = os.path.relpath(tp, os.path.dirname(path) or ".").replace(" ", "%20")
            para.append(f"See [{tt}]({rel}).")
        elif rng.random() < 0.3:
            para.append(f"Also [[{pages[rng.randrange(n)][1]}]].")
        text = " ".join(para)
        written += len(text.split())
        out.append(text)
        out.append("")
    with open(full, "w", encoding="utf-8") as f:
        f.write("\n".join(out))

total = sum(os.path.getsize(os.path.join(dp, f)) for dp, _, fs in os.walk(root) for f in fs)
print(f"{n} pages in {len(folders)} folders, {total / 1e6:.1f} MB")
