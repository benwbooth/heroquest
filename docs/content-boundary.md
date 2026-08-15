# Content boundary

The codebase may implement game procedures, numeric statistics, dice behavior,
turn structure, line-of-sight rules, and a compatible board coordinate system.
It may also load a user's local scans at runtime.

The repository must not contain copied box art, logos, board scans, miniature
scans, card faces, rulebook prose, quest narration, official map images, or a
downloaded third-party scan collection. `assets/local/` is ignored so a user
can build a private content pack from official public PDFs or scans of a set
they own without accidentally publishing those files.

Quest JSON checked into the repository should use original names and prose
unless there is explicit permission to redistribute the official content.
Official quest layouts can instead be generated into `assets/local/quests/` by
an importer and loaded with the same schema as the included demo.
