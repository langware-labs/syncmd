//! Python bindings for syncmd. The `sync`/`plan` functions and the report objects mirror the
//! CLI; serialization goes through the same serde impls (via `pythonize`), so the Python dict
//! is byte-for-byte the flow-sdk-shaped JSON the CLI emits.

// The pyo3 `#[pymethods]`/`#[pyfunction]` macros emit `PyErr -> PyErr` `.into()` conversions
// in generated code, which clippy flags against our signature lines. Not our code to fix.
#![allow(clippy::useless_conversion)]

use pyo3::exceptions::{PyOSError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pythonize::pythonize;

use syncmd_core::report::{GroupReport, Summary};
use syncmd_core::{Error, Strategy, SyncOpts, SyncReport};

fn to_pyerr(e: Error) -> PyErr {
    match &e {
        Error::NotARepo(_) | Error::BadPath(_) | Error::BadConfig(_) => {
            PyValueError::new_err(e.to_string())
        }
        Error::Conflict { .. } => PyRuntimeError::new_err(e.to_string()),
        Error::Io(_) | Error::Git(_) => PyOSError::new_err(e.to_string()),
    }
}

fn parse_strategy(s: &str) -> PyResult<Strategy> {
    match s {
        "newest" => Ok(Strategy::Newest),
        "error" => Ok(Strategy::Error),
        "interactive" => Ok(Strategy::Interactive),
        other => Err(PyValueError::new_err(format!("unknown strategy: {other}"))),
    }
}

/// The run-level rollup.
#[pyclass(name = "Summary")]
#[derive(Clone)]
struct PySummary {
    inner: Summary,
}

#[pymethods]
impl PySummary {
    #[getter]
    fn groups(&self) -> usize {
        self.inner.groups
    }
    #[getter]
    fn in_sync(&self) -> usize {
        self.inner.in_sync
    }
    #[getter]
    fn propagated(&self) -> usize {
        self.inner.propagated
    }
    #[getter]
    fn conflicts(&self) -> usize {
        self.inner.conflicts
    }
    #[getter]
    fn skipped(&self) -> usize {
        self.inner.skipped
    }
    #[getter]
    fn written(&self) -> usize {
        self.inner.written
    }
    fn __repr__(&self) -> String {
        format!(
            "Summary(groups={}, written={}, conflicts={})",
            self.inner.groups, self.inner.written, self.inner.conflicts
        )
    }
}

/// One equivalence group's outcome.
#[pyclass(name = "GroupReport")]
#[derive(Clone)]
struct PyGroupReport {
    inner: GroupReport,
}

#[pymethods]
impl PyGroupReport {
    #[getter]
    fn name(&self) -> String {
        self.inner.name.clone()
    }
    #[getter]
    fn id(&self) -> String {
        self.inner.id.clone()
    }
    #[getter]
    fn r#type(&self) -> String {
        self.inner.type_.as_str().to_string()
    }
    /// The group status as a string (e.g. "in_sync", "propagated", "conflict").
    #[getter]
    fn status(&self) -> String {
        serde_json::to_value(self.inner.status)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default()
    }
    /// Alias of `status` (the resolved decision).
    #[getter]
    fn decision(&self) -> String {
        self.status()
    }
    #[getter]
    fn winner_path(&self) -> Option<String> {
        self.inner.winner_path.clone()
    }
    #[getter]
    fn winner_oid(&self) -> Option<String> {
        self.inner.winner_oid.clone()
    }
    #[getter]
    fn overridden(&self) -> Vec<String> {
        self.inner.overridden.clone()
    }
    /// flow-sdk-shaped dict for this group.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        pythonize(py, &self.inner).map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }
    fn __repr__(&self) -> String {
        format!("GroupReport(name={:?}, status={:?})", self.inner.name, self.status())
    }
}

/// The top-level result of `sync`/`plan`.
#[pyclass(name = "SyncReport")]
struct PySyncReport {
    inner: SyncReport,
}

#[pymethods]
impl PySyncReport {
    #[getter]
    fn root(&self) -> String {
        self.inner.root.clone()
    }
    #[getter]
    fn groups(&self) -> Vec<PyGroupReport> {
        self.inner
            .groups
            .iter()
            .cloned()
            .map(|inner| PyGroupReport { inner })
            .collect()
    }
    #[getter]
    fn summary(&self) -> PySummary {
        PySummary {
            inner: self.inner.summary.clone(),
        }
    }
    /// The exit code this run would produce (0 ok, 1 conflict).
    fn exit_code(&self) -> i32 {
        self.inner.exit_code()
    }
    /// flow-sdk-shaped dict (`type`/`id` first, snake_case, nulls omitted).
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        pythonize(py, &self.inner).map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }
    /// flow-sdk-shaped JSON string.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner).map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }
    fn __repr__(&self) -> String {
        format!(
            "SyncReport(root={:?}, groups={}, written={})",
            self.inner.root,
            self.inner.groups.len(),
            self.inner.summary.written
        )
    }
}

/// Discover + plan only. Writes nothing.
#[pyfunction]
#[pyo3(signature = (path, *, formats = None, recursive = false))]
fn plan(path: &str, formats: Option<Vec<String>>, recursive: bool) -> PyResult<PySyncReport> {
    let opts = SyncOpts {
        formats,
        recursive,
        ..SyncOpts::default()
    };
    let inner = syncmd_core::plan(std::path::Path::new(path), &opts).map_err(to_pyerr)?;
    Ok(PySyncReport { inner })
}

/// Discover + plan + execute. Honors `dry_run`.
#[pyfunction]
#[pyo3(signature = (
    path, *, formats = None, strategy = "newest", dry_run = false, backup = true,
    create_missing = true, allow_delete = false, recursive = false
))]
#[allow(clippy::too_many_arguments)]
fn sync(
    path: &str,
    formats: Option<Vec<String>>,
    strategy: &str,
    dry_run: bool,
    backup: bool,
    create_missing: bool,
    allow_delete: bool,
    recursive: bool,
) -> PyResult<PySyncReport> {
    let opts = SyncOpts {
        formats,
        strategy: Some(parse_strategy(strategy)?),
        dry_run,
        backup,
        create_missing,
        allow_delete,
        recursive,
    };
    let inner = syncmd_core::sync(std::path::Path::new(path), &opts).map_err(to_pyerr)?;
    Ok(PySyncReport { inner })
}

#[pymodule]
fn syncmd(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySyncReport>()?;
    m.add_class::<PyGroupReport>()?;
    m.add_class::<PySummary>()?;
    m.add_function(wrap_pyfunction!(plan, m)?)?;
    m.add_function(wrap_pyfunction!(sync, m)?)?;
    Ok(())
}
