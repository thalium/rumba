#[derive(Clone, Copy)]
pub struct Dataset {
    pub name: &'static str,
    pub contents: &'static str,
}

pub const DATASETS: [Dataset; 7] = [
    Dataset {
        name: "loki_tiny",
        contents: include_str!("../../../third_party/dataset/loki_tiny.csv"),
    },
    Dataset {
        name: "mba_flatten",
        contents: include_str!("../../../third_party/dataset/mba_flatten.csv"),
    },
    Dataset {
        name: "mba_obf_linear",
        contents: include_str!("../../../third_party/dataset/mba_obf_linear.csv"),
    },
    Dataset {
        name: "mba_obf_nonlinear",
        contents: include_str!("../../../third_party/dataset/mba_obf_nonlinear.csv"),
    },
    Dataset {
        name: "neureduce",
        contents: include_str!("../../../third_party/dataset/neureduce.csv"),
    },
    Dataset {
        name: "qsynth_ea",
        contents: include_str!("../../../third_party/dataset/qsynth_ea.csv"),
    },
    Dataset {
        name: "syntia",
        contents: include_str!("../../../third_party/dataset/syntia.csv"),
    },
];

pub struct CorpusRow<'a> {
    pub source: String,
    pub mba: &'a str,
    pub ground_truth: &'a str,
}

pub fn rows<'a>(dataset: &str, contents: &'a str) -> Vec<CorpusRow<'a>> {
    contents
        .lines()
        .enumerate()
        .filter_map(|(line_index, line)| {
            if line.trim().is_empty() {
                return None;
            }
            let (mba, ground_truth) = line
                .split_once(',')
                .unwrap_or_else(|| panic!("{dataset}:{} is not a two-column row", line_index + 1));
            Some(CorpusRow {
                source: format!("{dataset}:{}", line_index + 1),
                mba: mba.trim(),
                ground_truth: ground_truth.trim(),
            })
        })
        .collect()
}
