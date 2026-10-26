## Menus

menu-file = File
menu-edit = Edit
menu-table = Table
menu-help = Help
menu-language = Language
menu-theme = Theme
menu-density = Row Height
theme-system = Like the System
theme-dark = Dark
theme-light = Light
density-compact = Compact
density-normal = Normal
density-comfortable = Comfortable

## Commands

command-new = New
command-open = Open…
command-save = Save
command-save-as = Save As…
command-quit = Quit
command-undo = Undo
command-redo = Redo
command-find = Find
command-inspector = Inspector
command-insert-row-above = Insert Row Above
command-insert-row-below = Insert Row Below
command-delete-row = Delete Row
command-insert-col-left = Insert Column Left
command-insert-col-right = Insert Column Right
command-delete-col = Delete Column
command-sort-ascending = Sort A → Z
command-sort-descending = Sort Z → A
command-about = About CSV Edit
header-row = First Row Is Header

## Window

title-minimize = Minimize
title-maximize = Maximize
title-restore = Restore Down
title-close = Close

## Grid

find-placeholder = Search…
find-count = { $count ->
    [one] { $count } match
   *[other] { $count } matches
}
new-row = New row

## Columns

cell-reference = R{ $row } C{ $col }
column-labels = Column names
column-hide = Hide This Column
column-only = Show Only This Column
columns-show-all = Show All Columns
columns-choose = Choose Columns…
columns-title = Columns shown
columns-help = Numbers, letters or ranges, in the order wanted.
columns-example = 3, 157, 10-20
columns-invalid = No column "{ $item }".
columns-filter = Filter by name
columns-shown = { $shown } of { $count } columns shown

## Status bar

table-size = { $rows ->
    [one] { $rows } row
   *[other] { $rows } rows
} × { $cols ->
    [one] { $cols } column
   *[other] { $cols } columns
}
selection-one = 1 cell selected
find-results = { $count ->
    [one] { $count } match
   *[other] { $count } matches
} for "{ $text }"
delimiter = Delimiter
delimiter-comma = Comma
delimiter-semicolon = Semicolon
delimiter-tab = Tab
delimiter-pipe = Pipe
delimiter-space = Space
delimiter-other = Other
encoding-utf8-bom = UTF-8 with BOM

## Inspector

inspector-cell = Cell
inspector-close = Close the inspector
inspector-column = Column { $letter }
inspector-chars = { $count ->
    [one] character
   *[other] characters
}
inspector-words = { $count ->
    [one] word
   *[other] words
}
inspector-brs = { $count ->
    [one] BR tag
   *[other] BR tags
}
inspector-edit = Edit
inspector-apply = Apply
inspector-copy = Copy
inspector-values = Values
inspector-empty = Empty cells
inspector-longest = Longest text
inspector-longest-value = { $count } chars

## Files

untitled = Untitled
untitled-file = untitled.csv
filter-csv = CSV files
filter-all = All files
error-open = Couldn't open { $file }: { $error }
error-save = Couldn't save { $file }: { $error }
error-encoding = the character "{ $character }" can't be written in Windows-1252. Choose UTF-8 in the status bar.

## Import

import-title = Import { $file }
import-data-rows = { $count ->
    [one] { $count } data row
   *[other] { $count } data rows
}
import-columns = { $count ->
    [one] { $count } column
   *[other] { $count } columns
}
import-header-line = header on line { $line }
import-encoding = Character set
import-start-line = From line
import-quote = String delimiter
import-quote-double = Double quotes
import-quote-single = Single quotes
import-quote-none = None
import-separators = Separators
import-other = Other
import-other-hint = Text between the fields, one or more characters (!# in Poliris files)
import-header-hint = Makes line 1 the titles
import-merge = Merge delimiters
import-merge-hint = Skips empty fields
import-trim = Trim spaces
import-trim-hint = At both ends of cells
import-preview = Preview
import-columns-shown = Columns A to { $last } of { $count }
import-no-separator = No active separator: a single column
import-detected = Detected automatically: { $delimiter }, { $encoding }, { $endings } line endings
import-open = Import

## Dialogs

save-changes-title = Save changes?
save-changes-text = { $file } has unsaved changes.
save-changes-save = Save
save-changes-discard = Don't Save
cancel = Cancel
ok = OK
about-text = A simple editor for CSV files.
about-license = Free software under the MIT or Apache-2.0 license.
