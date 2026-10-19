use std::path::PathBuf;
use std::sync::Mutex;

use ezexb_core::{Error, HeaderInfo, ParseOptions};
use pyo3::types::PyBytes;
use pyo3::{create_exception, exceptions::PyException, prelude::*};

create_exception!(ezexb._core, ExbError, PyException);
create_exception!(ezexb._core, InputLimitError, ExbError);
create_exception!(ezexb._core, InvalidInputError, ExbError);
create_exception!(ezexb._core, UnsupportedFeatureError, ExbError);
create_exception!(ezexb._core, InvalidContainerError, ExbError);
create_exception!(ezexb._core, InvalidStreamError, ExbError);
create_exception!(ezexb._core, StreamNotFoundError, ExbError);
create_exception!(ezexb._core, IncompleteReadError, ExbError);

fn to_python_error(error: Error) -> PyErr {
    match error {
        Error::Io(err) => err.into(),
        Error::IncompleteRead(ref report) => {
            let exception = IncompleteReadError::new_err(error.to_string());
            Python::attach(|py| {
                if let Ok(json) = serde_json::to_string(report) {
                    let _ = exception.value(py).setattr("report_json", json);
                }
            });
            exception
        }
        Error::InputLimit { .. } | Error::ResourceLimit { .. } => {
            InputLimitError::new_err(error.to_string())
        }
        Error::InvalidContainer(_) => InvalidContainerError::new_err(error.to_string()),
        Error::InvalidStream(_) => InvalidStreamError::new_err(error.to_string()),
        Error::StreamNotFound(_) => StreamNotFoundError::new_err(error.to_string()),
        Error::InvalidInput(_) => InvalidInputError::new_err(error.to_string()),
        Error::UnsupportedFeature(_) => UnsupportedFeatureError::new_err(error.to_string()),
        _ => ExbError::new_err(error.to_string()),
    }
}

fn header_tuple(info: HeaderInfo) -> (&'static str, u64) {
    (info.signature.as_str(), info.byte_length)
}

#[pyfunction]
fn probe_bytes(data: &[u8], max_file_bytes: u64) -> PyResult<(&'static str, u64)> {
    ezexb_core::probe_bytes(
        data,
        ParseOptions {
            max_file_bytes,
            ..ParseOptions::default()
        },
    )
    .map(header_tuple)
    .map_err(to_python_error)
}

#[pyfunction]
fn probe_file(py: Python<'_>, path: PathBuf, max_file_bytes: u64) -> PyResult<(&'static str, u64)> {
    py.detach(move || {
        ezexb_core::probe_file(
            path,
            ParseOptions {
                max_file_bytes,
                ..ParseOptions::default()
            },
        )
    })
    .map(header_tuple)
    .map_err(to_python_error)
}

#[pyfunction]
fn require_entity_reader() -> PyResult<()> {
    ezexb_core::require_entity_reader().map_err(to_python_error)
}

type Limits = (u64, u64, u64, u64, u64, u64, u64, u64, u64, u64, u64, u64);

fn options(limits: Limits) -> ParseOptions {
    ParseOptions {
        max_file_bytes: limits.0,
        max_entries: limits.1,
        max_stream_bytes: limits.2,
        max_total_stream_bytes: limits.3,
        max_storage_depth: limits.4,
        max_container_read_bytes: limits.5,
        max_decoded_stream_bytes: limits.6,
        max_string_bytes: limits.7,
        max_preview_pixels: limits.8,
        max_records: limits.9,
        max_record_depth: limits.10,
        max_total_decoded_bytes: limits.11,
    }
}

fn info_json(info: &ezexb_core::Inspection) -> Result<String, Error> {
    serde_json::to_string(info).map_err(|_| Error::InvalidInput("cannot serialize inspection"))
}

#[pyfunction]
fn inspect_file_json(py: Python<'_>, path: PathBuf, limits: Limits) -> PyResult<String> {
    py.detach(move || {
        ezexb_core::inspect_file(path, options(limits)).and_then(|info| info_json(&info))
    })
    .map_err(to_python_error)
}

#[pyfunction]
fn inspect_bytes_json(py: Python<'_>, data: &[u8], limits: Limits) -> PyResult<String> {
    py.detach(|| ezexb_core::inspect_bytes(data, options(limits)).and_then(|info| info_json(&info)))
        .map_err(to_python_error)
}

#[pyclass(name = "Container", module = "ezexb._core", frozen)]
struct RawContainer {
    inner: Mutex<Option<ezexb_core::Container>>,
}

impl RawContainer {
    fn with_container<T: Send>(
        &self,
        py: Python<'_>,
        f: impl FnOnce(&mut ezexb_core::Container) -> Result<T, Error> + Send,
    ) -> PyResult<T> {
        py.detach(|| {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| Error::InvalidInput("container lock poisoned"))?;
            let container = guard
                .as_mut()
                .ok_or(Error::InvalidInput("container is closed"))?;
            f(container)
        })
        .map_err(to_python_error)
    }
}

#[pymethods]
impl RawContainer {
    #[staticmethod]
    fn open_file(py: Python<'_>, path: PathBuf, limits: Limits) -> PyResult<Self> {
        let container = py
            .detach(move || ezexb_core::Container::open(path, options(limits)))
            .map_err(to_python_error)?;
        Ok(Self {
            inner: Mutex::new(Some(container)),
        })
    }

    #[staticmethod]
    fn open_bytes(py: Python<'_>, data: &[u8], limits: Limits) -> PyResult<Self> {
        let container = py
            .detach(|| ezexb_core::Container::from_bytes(data, options(limits)))
            .map_err(to_python_error)?;
        Ok(Self {
            inner: Mutex::new(Some(container)),
        })
    }

    fn info_json(&self, py: Python<'_>) -> PyResult<String> {
        self.with_container(py, |container| info_json(container.info()))
    }

    #[pyo3(signature = (path, offset=0, length=None))]
    fn read_stream<'py>(
        &self,
        py: Python<'py>,
        path: &str,
        offset: u64,
        length: Option<u64>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let data =
            self.with_container(py, |container| container.read_stream(path, offset, length))?;
        Ok(PyBytes::new(py, &data))
    }

    fn stream_sha256(&self, py: Python<'_>, path: &str) -> PyResult<String> {
        self.with_container(py, |container| container.stream_sha256(path))
    }

    fn file_info_json(&self, py: Python<'_>) -> PyResult<String> {
        self.with_container(py, |container| {
            serde_json::to_string(&container.file_info()?)
                .map_err(|_| Error::InvalidInput("cannot serialize FileInfo"))
        })
    }

    fn app_info_json(&self, py: Python<'_>) -> PyResult<String> {
        self.with_container(py, |container| {
            serde_json::to_string(&container.app_info()?)
                .map_err(|_| Error::InvalidInput("cannot serialize AppInfo"))
        })
    }

    fn preview_info_json(&self, py: Python<'_>) -> PyResult<String> {
        self.with_container(py, |container| {
            serde_json::to_string(&container.preview_info()?)
                .map_err(|_| Error::InvalidInput("cannot serialize preview"))
        })
    }

    fn preview_bmp<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let data = self.with_container(py, |container| container.preview_bmp())?;
        Ok(PyBytes::new(py, &data))
    }

    fn stream_payload<'py>(
        &self,
        py: Python<'py>,
        path: &str,
    ) -> PyResult<(String, Bound<'py, PyBytes>)> {
        let (info, data) = self.with_container(py, |container| {
            let decoded = container.stream_payload(path)?;
            let info = serde_json::to_string(&decoded.info)
                .map_err(|_| Error::InvalidInput("cannot serialize payload info"))?;
            Ok((info, decoded.data))
        })?;
        Ok((info, PyBytes::new(py, &data)))
    }

    fn record_headers_json(&self, py: Python<'_>, path: &str) -> PyResult<String> {
        self.with_container(py, |container| {
            serde_json::to_string(&container.record_headers(path)?)
                .map_err(|_| Error::InvalidInput("cannot serialize record headers"))
        })
    }

    fn document_json(&self, py: Python<'_>, strict: bool) -> PyResult<String> {
        self.with_container(py, |container| {
            serde_json::to_string(&container.document(strict)?)
                .map_err(|_| Error::InvalidInput("cannot serialize document"))
        })
    }

    fn world_json(&self, py: Python<'_>, space_id: i32, strict: bool) -> PyResult<String> {
        self.with_container(py, |container| {
            serde_json::to_string(&container.world_scene(space_id, strict)?)
                .map_err(|_| Error::InvalidInput("cannot serialize world scene"))
        })
    }
    fn svg_json(&self, py: Python<'_>, space_id: i32, strict: bool) -> PyResult<String> {
        self.with_container(py, |container| {
            serde_json::to_string(&container.svg_preview(space_id, strict)?)
                .map_err(|_| Error::InvalidInput("cannot serialize SVG preview"))
        })
    }

    fn lines_json(&self, py: Python<'_>, path: &str) -> PyResult<String> {
        self.with_container(py, |container| {
            serde_json::to_string(&container.lines(path)?)
                .map_err(|_| Error::InvalidInput("cannot serialize LINE inspection"))
        })
    }

    fn close(&self, py: Python<'_>) -> PyResult<()> {
        py.detach(|| {
            self.inner
                .lock()
                .map(|mut guard| {
                    guard.take();
                })
                .map_err(|_| "container lock poisoned")
        })
        .map_err(ExbError::new_err)
    }
}

#[pymodule]
fn _core(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", ezexb_core::VERSION)?;
    module.add("DEFAULT_MAX_FILE_BYTES", ezexb_core::DEFAULT_MAX_FILE_BYTES)?;
    module.add("ExbError", module.py().get_type::<ExbError>())?;
    module.add(
        "IncompleteReadError",
        module.py().get_type::<IncompleteReadError>(),
    )?;
    module.add(
        "InvalidStreamError",
        module.py().get_type::<InvalidStreamError>(),
    )?;
    module.add(
        "InvalidContainerError",
        module.py().get_type::<InvalidContainerError>(),
    )?;
    module.add(
        "StreamNotFoundError",
        module.py().get_type::<StreamNotFoundError>(),
    )?;
    module.add("InputLimitError", module.py().get_type::<InputLimitError>())?;
    module.add(
        "InvalidInputError",
        module.py().get_type::<InvalidInputError>(),
    )?;
    module.add(
        "UnsupportedFeatureError",
        module.py().get_type::<UnsupportedFeatureError>(),
    )?;
    module.add_function(wrap_pyfunction!(probe_bytes, module)?)?;
    module.add_function(wrap_pyfunction!(probe_file, module)?)?;
    module.add_function(wrap_pyfunction!(require_entity_reader, module)?)?;
    module.add_function(wrap_pyfunction!(inspect_file_json, module)?)?;
    module.add_function(wrap_pyfunction!(inspect_bytes_json, module)?)?;
    module.add_class::<RawContainer>()?;
    Ok(())
}
