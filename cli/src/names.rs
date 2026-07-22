//! Arbitrary variable names on top of the `v0, v1, ...` the grammar accepts.
//!
//! The expression grammar only knows variables of the form `v<index>`, which is
//! what the solver wants but not what a user reading disassembly has. This maps
//! any C-style identifier onto a fresh `v<index>` before parsing, and maps the
//! indices back to the original spellings when printing.
//!
//! It works on the source and result text rather than on the tree: renaming is
//! purely a matter of spelling, and keeping it here leaves the grammar and
//! `Expr` untouched.

/// The identifiers seen in an expression, in order of first appearance, so that
/// position `i` is the name that became `v<i>`.
pub struct Names(Vec<String>);

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

impl Names {
    /// Rewrites every identifier in `source` to `v<index>`, returning the
    /// grammar-ready text and the table needed to undo it.
    ///
    /// Identifiers already spelled `v0`, `v1`, ... are not special-cased: they
    /// are interned like any other name, so an input using only them round-trips
    /// to itself.
    pub fn rewrite(source: &str) -> (String, Self) {
        let mut names: Vec<String> = Vec::new();
        let mut out = String::with_capacity(source.len());
        let chars: Vec<char> = source.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let c = chars[i];

            // Numbers are copied whole. A hex literal has to be consumed as one
            // token, or its `x1f` tail would be read as an identifier.
            if c.is_ascii_digit() {
                let hex = c == '0' && matches!(chars.get(i + 1), Some('x' | 'X'));
                if hex {
                    out.push(chars[i]);
                    out.push(chars[i + 1]);
                    i += 2;
                    while i < chars.len() && chars[i].is_ascii_hexdigit() {
                        out.push(chars[i]);
                        i += 1;
                    }
                } else {
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        out.push(chars[i]);
                        i += 1;
                    }
                }
                continue;
            }

            if is_ident_start(c) {
                let start = i;
                while i < chars.len() && is_ident_continue(chars[i]) {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();

                let index = names.iter().position(|n| *n == name).unwrap_or_else(|| {
                    names.push(name);
                    names.len() - 1
                });
                out.push_str(&format!("v{index}"));
                continue;
            }

            out.push(c);
            i += 1;
        }

        (out, Names(names))
    }

    /// The original name of variable `index`.
    pub fn get(&self, index: usize) -> String {
        self.0
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("v{index}"))
    }

    /// Replaces every `v<index>` in rendered output with the original name.
    ///
    /// Safe to do on the text: the only `v`-prefixed tokens a rendered
    /// expression contains are variables, and hex constants cannot contain a
    /// `v`.
    pub fn restore(&self, repr: &str) -> String {
        let mut out = String::with_capacity(repr.len());
        let chars: Vec<char> = repr.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == 'v' && !(i > 0 && is_ident_continue(chars[i - 1])) {
                let start = i + 1;
                let mut end = start;
                while end < chars.len() && chars[end].is_ascii_digit() {
                    end += 1;
                }
                if end > start {
                    let digits: String = chars[start..end].iter().collect();
                    if let Ok(index) = digits.parse::<usize>() {
                        out.push_str(&self.get(index));
                        i = end;
                        continue;
                    }
                }
            }

            out.push(chars[i]);
            i += 1;
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interns_names_in_order_of_first_appearance() {
        let (src, names) = Names::rewrite("rax + rbx - (rax & rbx)");

        assert_eq!(src, "v0 + v1 - (v0 & v1)");
        assert_eq!(names.restore("v0 | v1"), "rax | rbx");
    }

    #[test]
    fn leaves_numbers_alone() {
        let (src, names) = Names::rewrite("x * 0x1f + 12 - 0xdeadbeef");

        assert_eq!(src, "v0 * 0x1f + 12 - 0xdeadbeef");
        assert_eq!(names.restore(&src), "x * 0x1f + 12 - 0xdeadbeef");
    }

    /// `0x1f`'s tail must not be interned as the identifier `x1f`.
    #[test]
    fn does_not_read_an_identifier_out_of_a_hex_literal() {
        let (src, _) = Names::rewrite("0xff & 0Xab");

        assert_eq!(src, "0xff & 0Xab");
    }

    #[test]
    fn plain_v_indices_round_trip() {
        let (src, names) = Names::rewrite("v0 ^ v1");

        assert_eq!(src, "v0 ^ v1");
        assert_eq!(names.restore(&src), "v0 ^ v1");
    }

    /// Names are assigned by appearance, so `v1` first becomes `v0`; the
    /// restore has to undo that renumbering rather than pass the digits through.
    #[test]
    fn renumbers_and_restores_shuffled_v_names() {
        let (src, names) = Names::rewrite("v1 | v0");

        assert_eq!(src, "v0 | v1");
        assert_eq!(names.restore(&src), "v1 | v0");
    }

    #[test]
    fn accepts_underscores_and_digits_within_a_name() {
        let (src, names) = Names::rewrite("_tmp1 ^ r8d");

        assert_eq!(src, "v0 ^ v1");
        assert_eq!(names.restore(&src), "_tmp1 ^ r8d");
    }

    /// An index with no name (which the solver should never emit) is left in
    /// its `v<index>` form rather than dropped.
    #[test]
    fn unknown_index_falls_back_to_its_v_form() {
        let (_, names) = Names::rewrite("x");

        assert_eq!(names.restore("v0 + v7"), "x + v7");
    }
}
