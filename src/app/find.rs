//! Finding text in the table. The matches follow edits that set cells or
//! insert or remove rows; anything else, or a new text to find, starts a
//! search that runs a slice at a time between frames.

use std::time::{Duration, Instant};

use crate::table::{Edit, Table};

/// What an edit did, for the caches that follow edits instead of starting
/// over.
#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// Rows inserted or removed (at most once, first), then cells set
    /// (row, column, as the table stands after the edit).
    Rows {
        shift: Option<Shift>,
        cells: Vec<(usize, usize)>,
    },
    /// Columns or the order of rows changed: start over.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Shift {
    /// `count` rows inserted before row `at`.
    Insert(usize, usize),
    /// `count` rows removed from row `at` on.
    Remove(usize, usize),
}

impl Change {
    /// What `edit`, about to be applied, does.
    pub(super) fn of(edit: &Edit) -> Change {
        let mut shift = None;
        let mut cells = Vec::new();
        if collect(edit, &mut shift, &mut cells) {
            Change::Rows { shift, cells }
        } else {
            Change::Other
        }
    }

    /// The cells set, when that is all the edit does.
    pub(super) fn cells_only(&self) -> Option<&[(usize, usize)]> {
        match self {
            Change::Rows { shift: None, cells } => Some(cells),
            _ => None,
        }
    }
}

/// Adds what `edit` does to `shift` and `cells`; false when it does more
/// than can be followed (a cell set before rows move would be at another
/// place afterwards).
fn collect(edit: &Edit, shift: &mut Option<Shift>, cells: &mut Vec<(usize, usize)>) -> bool {
    match edit {
        Edit::SetCells(set) => {
            cells.extend(set.iter().map(|&(r, c, _)| (r, c)));
            true
        }
        Edit::SetBlock(top, cols, values) => {
            cells.extend(
                values
                    .iter()
                    .enumerate()
                    .flat_map(|(i, line)| cols[..line.len()].iter().map(move |&c| (top + i, c))),
            );
            true
        }
        Edit::InsertRows(at, rows) if shift.is_none() && cells.is_empty() => {
            *shift = Some(Shift::Insert(*at, rows.len()));
            true
        }
        Edit::RemoveRows(at, count) if shift.is_none() && cells.is_empty() => {
            *shift = Some(Shift::Remove(*at, *count));
            true
        }
        Edit::Batch(edits) => edits.iter().all(|e| collect(e, shift, cells)),
        _ => false,
    }
}

/// Time a search takes per frame before going on at the next one.
pub(super) const SLICE: Duration = Duration::from_millis(8);

/// Inserted rows holding more cells than this are searched in slices
/// rather than at once.
const SYNC_MAX: usize = 100_000;

/// Each time this many cells were searched, the time spent is checked.
const CHECK_EVERY: usize = 4096;

/// Where the text being found was found.
#[derive(Default)]
pub(super) struct Matches {
    /// (generation, needle, columns shown) they are found for.
    key: Option<(u64, String, Option<Vec<usize>>)>,
    /// Rows searched so far; those from here on are not yet.
    done: usize,
    /// (row, column) in the order the cells are shown: by row, then by
    /// shown column. `u32` halves the memory: a table holds far fewer
    /// rows or columns (`table::MAX_CELLS`).
    cells: Vec<(u32, u32)>,
    /// The columns searched, in the order shown.
    cols: Vec<usize>,
    /// Each column's place among those shown (`usize::MAX`: hidden), to
    /// find a cell in `cells`.
    position: Vec<usize>,
}

impl Matches {
    /// The matches found so far.
    pub(super) fn cells(&self) -> &[(u32, u32)] {
        &self.cells
    }

    /// Forgets them: the next search starts over.
    #[cfg(test)]
    pub(super) fn clear(&mut self) {
        self.key = None;
    }

    /// Where `cell` goes in `cells`: (row, place of its column).
    fn order(&self, (row, col): (u32, u32)) -> (u32, usize) {
        (
            row,
            self.position
                .get(col as usize)
                .copied()
                .unwrap_or(usize::MAX),
        )
    }

    fn find(&self, cell: (u32, u32)) -> Result<usize, usize> {
        let key = self.order(cell);
        self.cells.binary_search_by(|&c| self.order(c).cmp(&key))
    }

    pub(super) fn contains(&self, row: usize, col: usize) -> bool {
        let cell = (row as u32, col as u32);
        self.order(cell).1 != usize::MAX && self.find(cell).is_ok()
    }

    /// Searches `table` for `needle` (lowercase) in the columns shown
    /// (`shown`, or all of them), for `generation` of the table. Without a
    /// `budget` the search is finished now; with one, it stops after that
    /// long and goes on at the next call. Returns whether it did anything.
    pub(super) fn search(
        &mut self,
        table: &Table,
        generation: u64,
        needle: &str,
        shown: Option<&Vec<usize>>,
        budget: Option<Duration>,
    ) -> bool {
        let started = Instant::now();
        let mut worked = false;
        let up_to_date = self
            .key
            .as_ref()
            .is_some_and(|(g, n, s)| *g == generation && n == needle && s.as_ref() == shown);
        if !up_to_date {
            worked = true;
            // Typing on: the cells holding the longer text are among those
            // that held the shorter one.
            let narrowing = self.key.as_ref().is_some_and(|(g, old, s)| {
                *g == generation
                    && s.as_ref() == shown
                    && !old.is_empty()
                    && old.is_ascii() == needle.is_ascii()
                    && needle.contains(old.as_str())
            });
            if narrowing {
                self.cells.retain(|&(r, c)| {
                    super::contains_ignoring_case(table.cell(r as usize, c as usize), needle)
                });
            } else {
                self.cols = shown
                    .cloned()
                    .unwrap_or_else(|| (0..table.col_count()).collect());
                self.position = vec![usize::MAX; table.col_count()];
                for (i, &c) in self.cols.iter().enumerate() {
                    self.position[c] = i;
                }
                self.cells = Vec::new();
                self.done = 0;
            }
            self.key = Some((generation, needle.to_owned(), shown.cloned()));
        }
        let rows = table.rows();
        if needle.is_empty() {
            self.cells = Vec::new();
            self.done = rows.len();
            return worked;
        }
        if self.done < rows.len() {
            worked = true;
            let mut since_check = 0;
            while self.done < rows.len() {
                let row = &rows[self.done];
                for &c in &self.cols {
                    if super::contains_ignoring_case(&row[c], needle) {
                        self.cells.push((self.done as u32, c as u32));
                    }
                }
                self.done += 1;
                since_check += self.cols.len();
                if since_check >= CHECK_EVERY {
                    since_check = 0;
                    if budget.is_some_and(|b| started.elapsed() >= b) {
                        break;
                    }
                }
            }
        }
        if worked {
            if self.done == rows.len() {
                self.cells.shrink_to_fit();
            }
            crate::diag::record("find", started.elapsed());
        }
        worked
    }

    /// Whether the search went through the whole table.
    #[cfg(test)]
    pub(super) fn finished(&self, table: &Table) -> bool {
        self.done >= table.row_count()
    }

    /// Follows `change`, just applied to `table`, which took it from
    /// `before` to `after`; the matches start over at the next search when
    /// they were not up to date or cannot follow it.
    pub(super) fn follow(&mut self, before: u64, after: u64, change: &Change, table: &Table) {
        let Some((generation, needle, _)) = &self.key else {
            return;
        };
        let Change::Rows { shift, cells } = change else {
            return;
        };
        if *generation != before {
            return;
        }
        let needle = needle.clone();
        if needle.is_empty() {
            // Nothing is found, whatever the edit: only the rows move.
            self.done = table.row_count();
            if let Some(key) = &mut self.key {
                key.0 = after;
            }
            return;
        }
        match *shift {
            Some(Shift::Insert(at, count)) => {
                let (at32, count32) = (at as u32, count as u32);
                let split = self.cells.partition_point(|&(r, _)| r < at32);
                for cell in &mut self.cells[split..] {
                    cell.0 += count32;
                }
                if self.done >= at {
                    if count * self.cols.len() > SYNC_MAX {
                        // Searched again from there, a slice at a time.
                        self.cells.truncate(split);
                        self.done = at;
                    } else {
                        let mut found = Vec::new();
                        for r in at..at + count {
                            for &c in &self.cols {
                                if super::contains_ignoring_case(table.cell(r, c), &needle) {
                                    found.push((r as u32, c as u32));
                                }
                            }
                        }
                        self.cells.splice(split..split, found);
                        self.done += count;
                    }
                }
            }
            Some(Shift::Remove(at, count)) => {
                let (at32, end32) = (at as u32, (at + count) as u32);
                let from = self.cells.partition_point(|&(r, _)| r < at32);
                let to = self.cells.partition_point(|&(r, _)| r < end32);
                self.cells.drain(from..to);
                for cell in &mut self.cells[from..] {
                    cell.0 -= end32 - at32;
                }
                if self.done >= at + count {
                    self.done -= count;
                } else {
                    self.done = self.done.min(at);
                }
            }
            None => {}
        }
        self.retest(table, &needle, cells);
        if let Some(key) = &mut self.key {
            key.0 = after;
        }
    }

    /// Searches `cells` again, set to new text in `table`.
    fn retest(&mut self, table: &Table, needle: &str, cells: &[(usize, usize)]) {
        let mut touched: Vec<(u32, u32)> = cells
            .iter()
            .filter(|&&(r, c)| r < self.done && self.position.get(c) != Some(&usize::MAX))
            .map(|&(r, c)| (r as u32, c as u32))
            .collect();
        let hit = |&(r, c): &(u32, u32)| {
            super::contains_ignoring_case(table.cell(r as usize, c as usize), needle)
        };
        if touched.len() <= 64 {
            for cell in touched {
                match (self.find(cell), hit(&cell)) {
                    (Ok(i), false) => {
                        self.cells.remove(i);
                    }
                    (Err(i), true) => self.cells.insert(i, cell),
                    _ => {}
                }
            }
            return;
        }
        // Many cells (a paste): merged in one pass.
        touched.sort_unstable_by_key(|&c| self.order(c));
        touched.dedup();
        let old = std::mem::take(&mut self.cells);
        let mut merged = Vec::with_capacity(old.len() + touched.len());
        let mut old = old.into_iter().peekable();
        for cell in touched {
            let key = self.order(cell);
            while let Some(&o) = old.peek() {
                let o_key = self.order(o);
                if o_key > key {
                    break;
                }
                old.next();
                if o_key < key {
                    merged.push(o);
                }
            }
            if hit(&cell) {
                merged.push(cell);
            }
        }
        merged.extend(old);
        self.cells = merged;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Cell;

    fn table(rows: &[&[&str]]) -> Table {
        Table::from_rows(
            rows.iter()
                .map(|row| row.iter().map(|&t| Cell::from(t)).collect())
                .collect(),
        )
    }

    #[test]
    fn tells_what_an_edit_does() {
        let set = |r, c| Edit::SetCells(vec![(r, c, Cell::from("x"))]);
        assert_eq!(
            Change::of(&Edit::Batch(vec![set(1, 0), set(2, 1)])),
            Change::Rows {
                shift: None,
                cells: vec![(1, 0), (2, 1)]
            }
        );
        assert_eq!(
            Change::of(&Edit::Batch(vec![Edit::RemoveRows(3, 2), set(1, 0)])),
            Change::Rows {
                shift: Some(Shift::Remove(3, 2)),
                cells: vec![(1, 0)]
            }
        );
        // Cells set before rows move, two moves, columns, a sort.
        for edit in [
            Edit::Batch(vec![set(1, 0), Edit::RemoveRows(0, 1)]),
            Edit::Batch(vec![Edit::RemoveRows(0, 1), Edit::InsertRows(0, vec![])]),
            Edit::RemoveCols(0, 1),
            Edit::Reorder(0, vec![1, 0]),
        ] {
            assert_eq!(Change::of(&edit), Change::Other);
        }
    }

    /// Applies `edit` to `t` and to `m`, then checks `m` against a fresh
    /// search.
    fn follows(t: &mut Table, m: &mut Matches, generation: &mut u64, edit: Edit) {
        let change = Change::of(&edit);
        t.apply(edit);
        m.follow(*generation, *generation + 1, &change, t);
        *generation += 1;
        assert!(!m.search(t, *generation, "a", None, None), "followed");
        let mut fresh = Matches::default();
        fresh.search(t, *generation, "a", None, None);
        assert_eq!(m.cells(), fresh.cells());
    }

    #[test]
    fn follows_rows_inserted_and_removed() {
        let mut t = table(&[&["a", "b"], &["b", "a"], &["a", "a"], &["b", "b"]]);
        let mut m = Matches::default();
        let mut g = 0;
        m.search(&t, g, "a", None, None);
        assert_eq!(m.cells(), [(0, 0), (1, 1), (2, 0), (2, 1)]);
        let rows = vec![vec![Cell::from("ka"), Cell::from("z")]];
        follows(&mut t, &mut m, &mut g, Edit::InsertRows(1, rows));
        assert_eq!(m.cells(), [(0, 0), (1, 0), (2, 1), (3, 0), (3, 1)]);
        follows(&mut t, &mut m, &mut g, Edit::RemoveRows(0, 2));
        follows(
            &mut t,
            &mut m,
            &mut g,
            Edit::Batch(vec![
                Edit::InsertRows(3, vec![vec![Cell::default(), Cell::default()]]),
                Edit::SetCells(vec![(3, 1, Cell::from("A")), (0, 1, Cell::from("x"))]),
            ]),
        );
        // A large paste is merged.
        let cells = (0..t.row_count())
            .flat_map(|r| (0..2).map(move |c| (r, c, Cell::from(if c == 0 { "b" } else { "a" }))))
            .chain((0..100).map(|_| (0, 0, Cell::from("a"))))
            .collect();
        follows(&mut t, &mut m, &mut g, Edit::SetCells(cells));
    }

    #[test]
    fn searches_a_slice_at_a_time() {
        let rows: Vec<Vec<Cell>> = (0..20_000)
            .map(|i| vec![Cell::from(if i % 3 == 0 { "abc" } else { "xyz" })])
            .collect();
        let mut t = Table::from_rows(rows);
        let mut m = Matches::default();
        // No time at all: one check's worth of cells per call.
        assert!(m.search(&t, 0, "b", None, Some(Duration::ZERO)));
        assert!(!m.finished(&t));
        let searched = m.cells().len();
        // An edit in the part searched and one in the rest.
        let edit = Edit::SetCells(vec![(1, 0, Cell::from("b")), (19_999, 0, Cell::from("b"))]);
        let change = Change::of(&edit);
        t.apply(edit);
        m.follow(0, 1, &change, &t);
        assert_eq!(m.cells().len(), searched + 1);
        assert!(m.search(&t, 1, "b", None, None));
        assert!(m.finished(&t));
        let mut fresh = Matches::default();
        fresh.search(&t, 1, "b", None, None);
        assert_eq!(m.cells(), fresh.cells());
        assert_eq!(m.cells().len(), 6_667 + 2);
    }

    #[test]
    fn nothing_to_find_finds_nothing_after_edits() {
        let mut t = table(&[&["a"], &["b"]]);
        let mut m = Matches::default();
        m.search(&t, 0, "", None, None);
        let edit = Edit::Batch(vec![
            Edit::InsertRows(1, vec![vec![Cell::from("c")]]),
            Edit::SetCells(vec![(0, 0, Cell::from("x"))]),
        ]);
        let change = Change::of(&edit);
        t.apply(edit);
        m.follow(0, 1, &change, &t);
        assert!(m.cells().is_empty());
        assert!(!m.contains(0, 0));
        assert!(m.finished(&t));
    }
}
