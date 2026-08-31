#[derive(Clone, Copy)]
pub enum Alignment {
    Left,
    Right,
}

pub fn render(headers: &[&str], rows: &[Vec<String>], alignments: &[Alignment]) -> String {
    assert_eq!(headers.len(), alignments.len());
    assert!(rows.iter().all(|row| row.len() == headers.len()));

    let widths: Vec<_> = headers
        .iter()
        .enumerate()
        .map(|(column, header)| {
            rows.iter()
                .map(|row| row[column].chars().count())
                .chain([header.chars().count()])
                .max()
                .unwrap()
        })
        .collect();

    let mut output = render_header(headers, &widths, alignments);
    for row in rows {
        output.push_str(&render_row(row, &widths, alignments));
    }
    output
}

pub fn render_header(headers: &[&str], widths: &[usize], alignments: &[Alignment]) -> String {
    assert_eq!(headers.len(), widths.len());
    assert_eq!(headers.len(), alignments.len());
    assert!(
        headers
            .iter()
            .zip(widths)
            .all(|(header, width)| header.chars().count() <= *width)
    );

    let mut output = String::new();
    push_row(
        &mut output,
        &headers
            .iter()
            .map(|cell| cell.to_string())
            .collect::<Vec<_>>(),
        widths,
        alignments,
    );
    output.push('|');
    for (width, alignment) in widths.iter().zip(alignments) {
        match alignment {
            Alignment::Left => output.push_str(&"-".repeat(width + 2)),
            Alignment::Right => output.push_str(&format!("{}:", "-".repeat(width + 1))),
        }
        output.push('|');
    }
    output.push('\n');
    output
}

pub fn render_row(cells: &[String], widths: &[usize], alignments: &[Alignment]) -> String {
    assert_eq!(cells.len(), widths.len());
    assert_eq!(cells.len(), alignments.len());
    assert!(
        cells
            .iter()
            .zip(widths)
            .all(|(cell, width)| cell.chars().count() <= *width),
        "table cell exceeds its configured width"
    );
    let mut output = String::new();
    push_row(&mut output, cells, widths, alignments);
    output
}

fn push_row(output: &mut String, cells: &[String], widths: &[usize], alignments: &[Alignment]) {
    output.push('|');
    for ((cell, width), alignment) in cells.iter().zip(widths).zip(alignments) {
        let missing = width - cell.chars().count();
        output.push(' ');
        match alignment {
            Alignment::Left => {
                output.push_str(cell);
                output.push_str(&" ".repeat(missing));
            }
            Alignment::Right => {
                output.push_str(&" ".repeat(missing));
                output.push_str(cell);
            }
        }
        output.push_str(" |");
    }
    output.push('\n');
}

#[cfg(test)]
mod tests {
    use super::{Alignment, render};

    #[test]
    fn aligns_cells_with_one_space_of_horizontal_padding() {
        let table = render(
            &["Name", "Count"],
            &[
                vec!["short".into(), "7".into()],
                vec!["x".into(), "123".into()],
            ],
            &[Alignment::Left, Alignment::Right],
        );

        assert_eq!(
            table,
            "\
| Name  | Count |
|-------|------:|
| short |     7 |
| x     |   123 |
"
        );
    }
}
