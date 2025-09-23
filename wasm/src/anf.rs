use mbalib::anf::ANF;
use serde::Serialize;
use serde_wasm_bindgen::to_value;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Clone, Serialize)]
pub struct ANFVar {
    pub id: usize,
    pub bit: usize,
}

#[wasm_bindgen(getter_with_clone)]
#[derive(Clone, Serialize)]
pub struct ANFWrapper {
    pub kind: String,
    children: Vec<ANFWrapper>,
    pub var: Option<ANFVar>,
}

impl From<ANF> for ANFWrapper {
    fn from(anf: ANF) -> Self {
        match anf {
            ANF::Xor(v) => Self {
                kind: "Xor".to_string(),
                children: v.into_iter().map(ANFWrapper::from).collect(),
                var: None,
            },
            ANF::And(v) => Self {
                kind: "And".to_string(),
                children: v.into_iter().map(ANFWrapper::from).collect(),
                var: None,
            },
            ANF::One => Self {
                kind: "One".to_string(),
                children: vec![],
                var: None,
            },
            ANF::Zero => Self {
                kind: "Zero".to_string(),
                children: vec![],
                var: None,
            },
            ANF::Var(id, bit) => Self {
                kind: "Var".to_string(),
                children: vec![],
                var: Some(ANFVar { id, bit }),
            },
        }
    }
}

#[wasm_bindgen]
impl ANFWrapper {
    // Returns a JS-friendly tree
    #[wasm_bindgen]
    pub fn to_tree(&self) -> JsValue {
        to_value(&self).unwrap()
    }
}
