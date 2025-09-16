use mbalib::anf::ANF;
use mbalib::anf::ANFExpr;
use pyo3::exceptions;
use pyo3::prelude::*;

#[pyclass]
#[derive(Clone, Default)]
struct PyANF {
    inner: ANF,
}

#[pymethods]
impl PyANF {
    #[new]
    fn new() -> Self {
        Self {
            inner: ANF::default(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{}", self.inner)
    }

    fn __str__(&self) -> String {
        format!("{}", self.inner)
    }

    // Example operator overloads (if ANF supports them)
    fn __and__(&self, other: &Self) -> Self {
        Self {
            inner: &self.inner & &other.inner,
        }
    }

    fn __xor__(&self, other: &Self) -> Self {
        Self {
            inner: &self.inner ^ &other.inner,
        }
    }
}

// Macro to expose ANFExpr<N> to Python
macro_rules! py_anf_expr {
    ($name:ident, $n:expr) => {
        #[pyclass]
        #[derive(Clone, Default)]
        struct $name {
            inner: ANFExpr<$n>,
        }

        #[pymethods]
        impl $name {
            #[new]
            fn new(obj: Option<Bound<'_, PyAny>>) -> PyResult<Self> {
                if let Some(py_obj) = obj {
                    if let Ok(value) = py_obj.extract::<u128>() {
                        Ok(Self {
                            inner: ANFExpr::<$n>::from(value),
                        })
                    } else {
                        Err(pyo3::exceptions::PyTypeError::new_err(
                            "Expected an int for ANFExpr constructor",
                        ))
                    }
                } else {
                    Ok(Self {
                        inner: ANFExpr::<$n>::default(),
                    })
                }
            }

            #[staticmethod]
            fn var(id: usize) -> PyResult<Self> {
                Ok(Self {
                    inner: ANFExpr::<$n>::var(id),
                })
            }

            fn to_int(&self) -> PyResult<u128> {
                self.inner
                    .to_int()
                    .ok_or(pyo3::exceptions::PyTypeError::new_err("Not an int"))
            }

            fn __repr__(&self) -> String {
                format!("{}", self.inner)
            }
            fn __str__(&self) -> String {
                format!("{}", self.inner)
            }

            // Indexing
            fn __getitem__(&self, idx: usize) -> PyResult<PyANF> {
                self.inner
                    .bits
                    .get(idx)
                    .cloned()
                    .map(|inner| PyANF { inner })
                    .ok_or_else(|| exceptions::PyIndexError::new_err("index out of bounds"))
            }

            fn __setitem__(&mut self, idx: usize, value: PyRef<PyANF>) -> PyResult<()> {
                if idx >= $n {
                    return Err(exceptions::PyIndexError::new_err("index out of bounds"));
                }
                self.inner.bits[idx] = value.inner.clone();
                Ok(())
            }

            // Arithmetic operators
            fn __add__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
                if let Ok(rhs) = other.extract::<Self>() {
                    Ok(Self {
                        inner: &self.inner + &rhs.inner,
                    })
                } else if let Ok(rhs_int) = other.extract::<u128>() {
                    Ok(Self {
                        inner: &self.inner + &ANFExpr::<$n>::from(rhs_int),
                    })
                } else {
                    Err(exceptions::PyTypeError::new_err(
                        "Operand must be ANFExpr or int",
                    ))
                }
            }

            fn __radd__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
                self.__add__(other)
            }

            fn __sub__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
                if let Ok(rhs) = other.extract::<Self>() {
                    Ok(Self {
                        inner: &self.inner - &rhs.inner,
                    })
                } else if let Ok(rhs_int) = other.extract::<u128>() {
                    Ok(Self {
                        inner: &self.inner - &ANFExpr::<$n>::from(rhs_int),
                    })
                } else {
                    Err(exceptions::PyTypeError::new_err(
                        "Operand must be ANFExpr or int",
                    ))
                }
            }

            fn __rsub__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
                if let Ok(rhs) = other.extract::<Self>() {
                    Ok(Self {
                        inner: &rhs.inner - &self.inner,
                    })
                } else if let Ok(rhs_int) = other.extract::<u128>() {
                    Ok(Self {
                        inner: &ANFExpr::<$n>::from(rhs_int) - &self.inner,
                    })
                } else {
                    Err(exceptions::PyTypeError::new_err(
                        "Operand must be ANFExpr or int",
                    ))
                }
            }

            fn __mul__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
                if let Ok(rhs) = other.extract::<Self>() {
                    Ok(Self {
                        inner: &self.inner * &rhs.inner,
                    })
                } else if let Ok(rhs_int) = other.extract::<u128>() {
                    Ok(Self {
                        inner: &self.inner * &ANFExpr::<$n>::from(rhs_int),
                    })
                } else {
                    Err(exceptions::PyTypeError::new_err(
                        "Operand must be ANFExpr or int",
                    ))
                }
            }
            fn __rmul__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
                self.__mul__(other)
            }

            fn __xor__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
                if let Ok(rhs) = other.extract::<Self>() {
                    Ok(Self {
                        inner: &self.inner ^ &rhs.inner,
                    })
                } else if let Ok(rhs_int) = other.extract::<u128>() {
                    Ok(Self {
                        inner: &self.inner ^ &ANFExpr::<$n>::from(rhs_int),
                    })
                } else {
                    Err(exceptions::PyTypeError::new_err(
                        "Operand must be ANFExpr or int",
                    ))
                }
            }
            fn __rxor__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
                self.__xor__(other)
            }

            fn __and__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
                if let Ok(rhs) = other.extract::<Self>() {
                    Ok(Self {
                        inner: &self.inner & &rhs.inner,
                    })
                } else if let Ok(rhs_int) = other.extract::<u128>() {
                    Ok(Self {
                        inner: &self.inner & &ANFExpr::<$n>::from(rhs_int),
                    })
                } else {
                    Err(exceptions::PyTypeError::new_err(
                        "Operand must be ANFExpr or int",
                    ))
                }
            }
            fn __rand__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
                self.__and__(other)
            }

            fn __or__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
                if let Ok(rhs) = other.extract::<Self>() {
                    Ok(Self {
                        inner: &self.inner | &rhs.inner,
                    })
                } else if let Ok(rhs_int) = other.extract::<u128>() {
                    Ok(Self {
                        inner: &self.inner | &ANFExpr::<$n>::from(rhs_int),
                    })
                } else {
                    Err(exceptions::PyTypeError::new_err(
                        "Operand must be ANFExpr or int",
                    ))
                }
            }
            fn __ror__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
                self.__or__(other)
            }

            fn __invert__(&self) -> Self {
                Self {
                    inner: !&self.inner,
                }
            }

            fn __neg__(&self) -> Self {
                Self {
                    inner: -&self.inner,
                }
            }

            fn __lshift__(&self, shift: u8) -> Self {
                Self {
                    inner: &self.inner << shift,
                }
            }
            fn __rshift__(&self, shift: u8) -> Self {
                Self {
                    inner: &self.inner >> shift,
                }
            }
        }
    };
}
// Generate bindings for multiple sizes
py_anf_expr!(ANFExpr4, 4);
py_anf_expr!(ANFExpr8, 8);
py_anf_expr!(ANFExpr16, 16);
py_anf_expr!(ANFExpr32, 32);
py_anf_expr!(ANFExpr64, 64);

#[pymodule]
mod mba {
    #[pymodule_export]
    use super::ANFExpr4;

    #[pymodule_export]
    use super::ANFExpr8;

    #[pymodule_export]
    use super::ANFExpr16;

    #[pymodule_export]
    use super::ANFExpr32;

    #[pymodule_export]
    use super::ANFExpr64;
}
