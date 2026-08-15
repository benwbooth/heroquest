# Original US Game System scan pack

The private runtime pack at `assets/local/editions/original-us/` is the
original North American/US Game System edition. It must not be confused with
the original UK edition or the 2021 remake.

The local source archive is named `HQ Game System US.rar`. Its SHA-256 is:

```text
a7d3ae6c71c8de3c517dd6667d7ab4a8cce3046e80e96ad2ef470f5ef8ec74db
```

The archive passed a complete RAR integrity test. It contains 16 PDFs:

- Box (2 pages)
- Cards (16 sheets)
- Character Sheet (1 page)
- Gameboard (1 page, native scan 14340 by 11376 pixels at 600 dpi)
- Identification Guide and Armory (1 page)
- Instruction Booklet, booklet-print and computer-view variants
- Quest Book, booklet-print and computer-view variants
- Tile Sheets (4 pages)
- Four poster files
- Quest-pack order form and survey form

The import retains all source PDFs and produces local-only 300 dpi derivatives.
The board derivative is 7170 by 5688 pixels. There are 19 computer-view quest
pages, 13 computer-view instruction pages, 16 card sheets, and 4 tile sheets.
Box, character-sheet, armory, poster, order-form, and survey derivatives are
also retained.

The renderer selects this edition by default when it is installed. The board
is live at runtime, with `board-calibration.json` preserving the full printed
logo border while mapping pieces to the 26 by 19 grid. Character Cards,
Character Sheets, spell hands, the Armory, Zargon's decks, Monster references,
and Quest Book pages now appear as in-world surfaces. Run
`tools/extract-original-us-tabletop-art.sh` to create the compact `tabletop/`
runtime cache; direct interaction with those surfaces remains incomplete.
The archive does not contain standalone miniature, furniture, or dice scans and
does not provide 3D meshes.

The two sides of the Information Screen are embedded in tile-sheet pages 1 and
2. `tools/extract-original-us-screen.sh` creates private transparent textures
for the renderer under `screen/`; these derivatives stay inside the gitignored
edition pack.

`tools/extract-original-us-components.sh` similarly reproduces compact scan
crops under `components/`. The renderer instances those faces over the matching
thin marker and assembled-component GLBs, including both one- and two-square
blocked markers, rather than replacing the original print with procedural art.
