"""Builds api/data/syllables.txt.gz from Moby Hyphenator II (public domain, Project Gutenberg #3204).

Usage: python3 build.py path/to/mhyph.txt path/to/api/data/syllables.txt.gz

Keeps lowercase single words only (no proper nouns, phrases or hyphenated compounds). Each line
is one word: its syllable splits joined by "-", several splits (heteronyms such as rec-ord /
re-cord) separated by a space.
"""

import gzip
import re
import sys

src, dst = sys.argv[1], sys.argv[2]
entries: dict[str, list[str]] = {}
for line in open(src, "rb").read().replace(b"\r", b"").split(b"\n"):
    if not line:
        continue
    parts = line.decode("latin-1").split("\xa5")  # the hyphenation mark is byte 165
    key = "".join(parts)
    if not re.fullmatch(r"[a-z]+", key):
        continue
    split = "-".join(parts)
    entries.setdefault(key, [])
    if split not in entries[key]:
        entries[key].append(split)
text = "".join(" ".join(v) + "\n" for v in entries.values())
with open(dst, "wb") as f:
    f.write(gzip.compress(text.encode(), 9, mtime=0))
print(f"{len(entries)} words")
