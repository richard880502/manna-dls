# Syllable dictionary

`api/data/syllables.txt.gz` is built by `build.py` from **Moby Hyphenator II** by Grady Ward,
public domain by grant from the author (January 2001), distributed as Project Gutenberg eBook
#3204 (`files.zip`, file `mhyph.txt`).

The API uses it to check and correct syllable-division answers the model writes (for example a
「拆音節」 section of a teacher's worksheet template): see `api/src/syllables.rs`.
