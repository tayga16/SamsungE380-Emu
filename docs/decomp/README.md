# Decompilation notes

This directory contains small, reviewable descriptions produced during the
E830 investigation.  They are not firmware exports and are not intended to
reconstruct or redistribute Samsung binaries.

Included material may contain:

- manually written pseudocode for a path that was verified by traces;
- curated addresses and descriptive labels used by the emulator investigation;
- instructions for reproducing local analysis with a user-supplied image.

Raw CLA/TFS/NSF images, complete DEF/MAP files, IDA/Ghidra databases, generated
assembly listings and RAM/NAND captures must remain outside Git.  The repository
tools accept those files locally without copying their contents into the tree.

## Current slice

[`hkbd-menu-input.md`](hkbd-menu-input.md) records the late-menu keypad path.
It narrows the inert-arrow problem to code after the common HKBD queue function;
it does not claim that the queue consumer or active-screen dispatch is decoded.

[`e830-hkbd-symbols.csv`](e830-hkbd-symbols.csv) is a compact set of labels for
that slice.  Labels ending in `_site` identify an observed instruction or call
site rather than a proven function entry.

## Local symbol extraction

`tools/extract_def_symbols.py` reads a local Samsung DEF file and prints only
matching address symbols.  For example:

```powershell
python .\tools\extract_def_symbols.py .\firmware\E830XEGG1.def `
  --prefix a_hkbd_ --format csv
```

The input file is never modified.  Redirect output only when the selected
symbols are safe and useful to publish.
