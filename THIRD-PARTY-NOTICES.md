# Third-party notices

CSV Edit's binary embeds the following works, under their own licenses.

## Fonts

The interface fonts (assets/fonts) are Inter, by The Inter Project
Authors, and JetBrains Mono, by The JetBrains Mono Project Authors, both
under the SIL Open Font License 1.1: see assets/fonts/Inter-OFL.txt and
assets/fonts/JetBrainsMono-OFL.txt.

As fallback, the binary also embeds egui's fonts (`epaint_default_fonts`):
Ubuntu Light (Ubuntu Font Licence 1.0), Hack (MIT, with the Bitstream Vera
license), Noto Emoji and emoji-icon-font (SIL Open Font License 1.1).

The icons are Phosphor Icons (https://phosphoricons.com), under the MIT
license, through the `egui-phosphor` crate.

## Rust crates

CSV Edit is built with many Rust crates, each under its own license (MIT,
Apache-2.0, BSD, zlib, Unicode-3.0 and similar permissive licenses, choosing
the permissive option where a crate offers several); see Cargo.lock for the
list.
