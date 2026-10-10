# Terrain material effects

The implementation follows `doc/material-format.txt` in the original
[NewDark 1.27 distribution](https://darkfate.org/view/details/files/projects/thief_2_v1_19/t2_v127.zip)
(`new_dark.zip` inside the download), cross-checked against installed 25AE materials.

- Pass `ani_rate` is milliseconds per frame, defaulting to 250.
- Pass animation defaults to `NORMAL`; `PINGPONG` reverses at the ends.
- A numbered `texture *_ N prefix` sequence starts with `prefix`, followed by
  `prefix_1`, etc. An existing trailing underscore is reused.
- Wave values are normalized to 0–1 before amplitude and bias are applied.
  Phase is measured in cycles; the period is milliseconds.
- UV scroll speeds are texture-coordinate units per second. Optional step
  counts of zero or one mean continuous movement.

Unsupported directives retain the original material. This is a bounded subset,
not a claim of complete NewDark material support.
