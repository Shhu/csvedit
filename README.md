# CSV Edit

A simple CSV editor, in English and French. Written in Rust with
[egui](https://github.com/emilk/egui).

- Opens files from Excel and others: comma, semicolon, tab or pipe
  separated (or any delimiter, even several characters long like Poliris'
  `!#`), UTF-8 (with or without BOM), Windows-1252 or UTF-16, found
  automatically; Excel's `sep=;` line is understood.
- Before opening, an import window with a live preview: character set,
  starting line, one or more delimiters, merged delimiters, trimmed spaces,
  quote character, header row.
- Saves them the way they were written (delimiter, quoting, encoding, line
  endings, lines skipped at the top), or converts them from the status bar.
  Saving writes a new file and then replaces the old one, so a failure
  halfway leaves it whole; if reading left something out (blank lines,
  trimmed spaces, unreadable bytes), saving over the file asks first.
- Edit cells, insert and delete rows and columns, sort, find (matches light
  up, Enter goes to the next one), undo and redo, copy and paste with
  spreadsheets.
- An inspector shows the selected cell's whole text, its `<BR>` tags as
  line breaks, and facts about its column; long texts are edited there.
- Shows only the columns wanted, in the order wanted: type "3, 157" or
  "10-20" in the column chooser, or tick them in its list.
- Columns named A B C, 1 2 3 or 0 1 2 (right click on a column header).
- Dark and light themes (Help › Theme), three row heights.
- A single executable on Windows, a Flatpak on Linux.

## Using it

| Action | Keys |
| --- | --- |
| Move | Arrows, Tab, Home, End, Page Up/Down |
| Edit the cell | Type, or Enter / F2 / double-click |
| Validate | Enter (moves down, adds a row at the end), Tab (moves right) |
| Cancel the typing | Escape |
| Clear the cell | Delete |
| Copy, cut, paste | Ctrl+C, Ctrl+X, Ctrl+V (tab-separated blocks too) |
| Insert a row below, delete the row | Ctrl++, Ctrl+- |
| Find, next match, previous match | Ctrl+F, Enter, Shift+Enter |
| Show or hide the inspector | Ctrl+I |
| Choose the columns shown | Ctrl+K |
| Apply the text typed in the inspector | Ctrl+Enter |
| Undo, redo | Ctrl+Z, Ctrl+Shift+Z or Ctrl+Y |

A file can also be opened by dropping it on the window (on Linux, in X11
sessions only for now: winit doesn't receive dropped files on Wayland), or
given on the command line. In the Flatpak, files outside Documents,
Downloads and Desktop are opened from the command line with
`flatpak run --file-forwarding io.github.Shhu.CsvEdit @@ file.csv @@`.

Very large files are refused when they would make more than 30 million
cells.

## Building

CSV Edit needs a recent stable Rust toolchain (edition 2024).

```sh
cargo run --release
cargo test
```

Packaging scripts:

- Windows, from WSL: `packaging/windows/build.sh` (a single `csvedit.exe`,
  with the C runtime linked in).
- Linux: `packaging/flatpak/build.sh` (a Flatpak bundle, built offline from
  source).

## Contributing

- Code, comments and commit messages in English.
- Every text shown to users goes through `t!` (`src/i18n.rs`), with its key in
  both `locales/en.ftl` and `locales/fr.ftl` (same keys, same order); the
  tests check that every key is translated and used.
- `cargo fmt`, `cargo clippy --all-targets` and `cargo test` must pass. The
  ignored `capture` test renders a screenshot (options at the end of
  `src/app/mod.rs`).
- After any change to `Cargo.lock`, regenerate
  `packaging/flatpak/cargo-sources.json` (see `packaging/flatpak/build.sh`)
  and `THIRD-PARTY-LICENSES.txt` (`python3 packaging/licenses.py`).

## License

CSV Edit is licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE)
or the [MIT license](LICENSE-MIT), at your option. Unless you explicitly state
otherwise, any contribution intentionally submitted for inclusion in CSV Edit
shall be dual licensed as above, without any additional terms or conditions.

Third-party works embedded in the binaries are listed in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md); the licenses of the Rust
crates, with their texts, are in
[THIRD-PARTY-LICENSES.txt](THIRD-PARTY-LICENSES.txt), shown in the app
(Help › About › Third-Party Licenses) and attached to every release.
