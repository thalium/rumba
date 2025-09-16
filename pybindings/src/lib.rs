// use core::*;
use pyo3::prelude::*;
use std::convert::Into;
use std::option::Option;

#[pyfunction]
fn double(x: usize) -> usize {
    x * 2
}

#[pymodule]
mod mba {
    use super::*;

    #[pymodule_export]
    use super::double; // Exports the double function as part of the module

    #[pymodule_export]
    const PI: f64 = std::f64::consts::PI; // Exports PI constant as part of the module

    #[pyfunction] // This will be part of the module
    fn triple(x: usize) -> usize {
        x * 3
    }

    #[pyclass] // This will be part of the module
    struct Unit;

    #[pymodule]
    mod submodule {
        // This is a submodule
    }

    #[pymodule_init]
    fn init(m: &Bound<'_, PyModule>) -> PyResult<()> {
        // Arbitrary code to run at the module initialization
        m.add("double2", m.getattr("double")?)
    }
}
