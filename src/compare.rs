//! Comparing two file listings: which files the two have in common, which
//! of those match byte for byte, and which are on only one side.
//!
//! It works on listings rather than disks, so a disk can be compared with
//! an archive or a T64 as easily as with another disk. Files are paired by
//! name. When a name appears more than once on a side, the first copy on
//! the left pairs with the first on the right, the second with the second,
//! and so on.

use std::collections::HashMap;
use std::fmt;

/// One file in a listing.
#[derive(Clone, Copy, Debug)]
pub struct Item<'a> {
    pub name: &'a str,
    /// The file type's mnemonic (`prg`, `seq`, …), or empty when the format
    /// doesn't record one (LHA, Wraptor); it then matches any type.
    pub kind: &'a str,
    /// The file's bytes, or `None` if they couldn't be read.
    pub data: Option<&'a [u8]>,
}

/// How a paired file differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diff {
    /// The file types differ (e.g. PRG on one side, SEQ on the other).
    pub kind: bool,
    /// The offset of the first byte that differs, `None` when the bytes are
    /// the same (only the type differs). When one file is a prefix of the
    /// other, it's the shorter one's length.
    pub first: Option<usize>,
}

/// What a row of the comparison found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// On both sides, same type and same bytes.
    Same,
    /// On both sides, but different.
    Differs(Diff),
    /// On both sides, but one or both couldn't be read.
    Unreadable,
    /// Only on the left.
    OnlyLeft,
    /// Only on the right.
    OnlyRight,
}

impl Status {
    /// A one-character mark for listings: `=` same, `*` different, `?`
    /// unreadable, `<` only on the left, `>` only on the right.
    pub fn mark(&self) -> &'static str {
        match self {
            Status::Same => "=",
            Status::Differs(_) => "*",
            Status::Unreadable => "?",
            Status::OnlyLeft => "<",
            Status::OnlyRight => ">",
        }
    }

    /// What was found, in words, given each side's file type (e.g. `type
    /// PRG / SEQ; differs from byte 2`).
    pub fn describe(&self, kinds: [Option<&str>; 2]) -> String {
        match self {
            Status::Same => "same".into(),
            Status::Differs(d) => {
                let mut notes = Vec::new();
                if d.kind {
                    let k = |k: Option<&str>| k.unwrap_or("?").to_uppercase();
                    notes.push(format!("type {} / {}", k(kinds[0]), k(kinds[1])));
                }
                if let Some(at) = d.first {
                    notes.push(format!("differs from byte {at}"));
                }
                notes.join("; ")
            }
            Status::Unreadable => "can't be read to compare".into(),
            Status::OnlyLeft => "only on the left".into(),
            Status::OnlyRight => "only on the right".into(),
        }
    }
}

/// One file name's result: its index in each listing, if present there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub left: Option<usize>,
    pub right: Option<usize>,
    pub status: Status,
}

/// How many rows of each kind a comparison has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub same: usize,
    pub differ: usize,
    pub unreadable: usize,
    pub only_left: usize,
    pub only_right: usize,
}

impl Summary {
    /// Whether the two listings hold exactly the same files.
    pub fn identical(&self) -> bool {
        self.differ + self.unreadable + self.only_left + self.only_right == 0
    }
}

/// A summary line, e.g. `2 differ, 1 only on the left, 14 the same`.
impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.identical() {
            return write!(
                f,
                "Identical: the same {} file(s) on both sides, byte for byte.",
                self.same
            );
        }
        let parts = [
            (self.differ, "differ"),
            (self.only_left, "only on the left"),
            (self.only_right, "only on the right"),
            (self.unreadable, "can't be read"),
            (self.same, "the same"),
        ];
        let parts: Vec<_> = parts
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, what)| format!("{n} {what}"))
            .collect();
        f.write_str(&parts.join(", "))
    }
}

/// Compare two listings. The rows follow the left listing's order, with the
/// files only on the right after them in the right listing's order.
pub fn compare(left: &[Item], right: &[Item]) -> Vec<Row> {
    // Each name's right-hand indices, in order, consumed as left copies pair
    // with them.
    let mut by_name: HashMap<&str, std::collections::VecDeque<usize>> = HashMap::new();
    for (i, item) in right.iter().enumerate() {
        by_name.entry(item.name).or_default().push_back(i);
    }
    let mut paired = vec![false; right.len()];
    let mut rows = Vec::with_capacity(left.len().max(right.len()));
    for (li, l) in left.iter().enumerate() {
        let ri = by_name.get_mut(l.name).and_then(|q| q.pop_front());
        let status = match ri {
            None => Status::OnlyLeft,
            Some(ri) => {
                paired[ri] = true;
                pair_status(l, &right[ri])
            }
        };
        rows.push(Row {
            name: l.name.to_string(),
            left: Some(li),
            right: ri,
            status,
        });
    }
    for (ri, r) in right.iter().enumerate() {
        if !paired[ri] {
            rows.push(Row {
                name: r.name.to_string(),
                left: None,
                right: Some(ri),
                status: Status::OnlyRight,
            });
        }
    }
    rows
}

fn pair_status(l: &Item, r: &Item) -> Status {
    let (Some(a), Some(b)) = (l.data, r.data) else {
        return Status::Unreadable;
    };
    let first = a
        .iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .or_else(|| (a.len() != b.len()).then(|| a.len().min(b.len())));
    let typed = !l.kind.is_empty() && !r.kind.is_empty();
    let kind = typed && !l.kind.eq_ignore_ascii_case(r.kind);
    if first.is_none() && !kind {
        Status::Same
    } else {
        Status::Differs(Diff { kind, first })
    }
}

/// Count the rows of each kind.
pub fn summarize(rows: &[Row]) -> Summary {
    let mut s = Summary::default();
    for r in rows {
        match r.status {
            Status::Same => s.same += 1,
            Status::Differs(_) => s.differ += 1,
            Status::Unreadable => s.unreadable += 1,
            Status::OnlyLeft => s.only_left += 1,
            Status::OnlyRight => s.only_right += 1,
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item<'a>(name: &'a str, kind: &'a str, data: &'a [u8]) -> Item<'a> {
        Item {
            name,
            kind,
            data: Some(data),
        }
    }

    #[test]
    fn pairs_by_name_and_reports_each_kind_of_difference() {
        let left = [
            item("SAME", "prg", b"abc"),
            item("EDITED", "prg", b"abcd"),
            item("LONGER", "seq", b"ab"),
            item("RETYPED", "prg", b"xy"),
            item("GONE", "prg", b"1"),
        ];
        let right = [
            item("NEW", "usr", b"2"),
            item("RETYPED", "seq", b"xy"),
            item("LONGER", "seq", b"abc"),
            item("EDITED", "prg", b"abXd"),
            item("SAME", "PRG", b"abc"),
        ];
        let rows = compare(&left, &right);
        let got: Vec<_> = rows
            .iter()
            .map(|r| (r.name.as_str(), r.left, r.right, r.status))
            .collect();
        let differs = |kind, first| Status::Differs(Diff { kind, first });
        assert_eq!(
            got,
            [
                ("SAME", Some(0), Some(4), Status::Same),
                ("EDITED", Some(1), Some(3), differs(false, Some(2))),
                ("LONGER", Some(2), Some(2), differs(false, Some(2))),
                ("RETYPED", Some(3), Some(1), differs(true, None)),
                ("GONE", Some(4), None, Status::OnlyLeft),
                ("NEW", None, Some(0), Status::OnlyRight),
            ]
        );
        let s = summarize(&rows);
        assert_eq!(
            s,
            Summary {
                same: 1,
                differ: 3,
                unreadable: 0,
                only_left: 1,
                only_right: 1
            }
        );
        assert!(!s.identical());
    }

    #[test]
    fn repeated_names_pair_in_order_and_unreadable_files_are_flagged() {
        let bad = Item {
            name: "BAD",
            kind: "prg",
            data: None,
        };
        let left = [item("DUP", "prg", b"1"), item("DUP", "prg", b"2"), bad];
        let right = [item("DUP", "prg", b"1"), item("BAD", "prg", b"x")];
        let rows = compare(&left, &right);
        assert_eq!(rows[0].status, Status::Same);
        assert_eq!((rows[1].right, rows[1].status), (None, Status::OnlyLeft));
        assert_eq!(rows[2].status, Status::Unreadable);
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn a_format_without_types_matches_any_type() {
        let status = |l, r| compare(&[item("A", l, b"1")], &[item("A", r, b"1")])[0].status;
        assert_eq!(status("", "seq"), Status::Same);
        assert_eq!(status("prg", ""), Status::Same);
        assert_ne!(status("prg", "seq"), Status::Same);
    }

    #[test]
    fn identical_listings() {
        let a = [item("A", "prg", b"1"), item("B", "seq", b"")];
        let s = summarize(&compare(&a, &a));
        assert!(s.identical() && s.same == 2);
        assert!(summarize(&compare(&[], &[])).identical());
    }
}
