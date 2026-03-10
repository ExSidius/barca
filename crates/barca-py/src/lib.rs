use pyo3::prelude::*;
use pyo3::types::{PyCFunction, PyDict, PyTuple};

/// The `@asset()` decorator wrapper. Stores the original function and metadata.
#[pyclass(name = "AssetWrapper")]
struct AssetWrapper {
    original: PyObject,
    name: Option<String>,
    serializer: Option<String>,
}

#[pymethods]
impl AssetWrapper {
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__<'py>(
        &self,
        py: Python<'py>,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<PyObject> {
        self.original.call(py, args, kwargs)
    }

    #[getter]
    fn __barca_metadata__(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("kind", "asset")?;
        dict.set_item("name", self.name.as_deref())?;
        dict.set_item("serializer", self.serializer.as_deref())?;
        Ok(dict.into_any().unbind())
    }

    #[getter]
    fn __barca_kind__(&self) -> &'static str {
        "asset"
    }

    #[getter]
    fn __barca_original__(&self, py: Python<'_>) -> PyObject {
        self.original.clone_ref(py)
    }

    fn __get__<'py>(
        slf: &Bound<'py, Self>,
        _obj: &Bound<'py, PyAny>,
        _objtype: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, Self>> {
        Ok(slf.clone())
    }

    #[getter]
    fn __name__(&self, py: Python<'_>) -> PyResult<PyObject> {
        self.original.getattr(py, "__name__")
    }

    #[getter]
    fn __doc__(&self, py: Python<'_>) -> PyResult<PyObject> {
        self.original.getattr(py, "__doc__")
    }

    #[getter]
    fn __module__(&self, py: Python<'_>) -> PyResult<PyObject> {
        self.original.getattr(py, "__module__")
    }

    #[getter]
    fn __qualname__(&self, py: Python<'_>) -> PyResult<PyObject> {
        self.original.getattr(py, "__qualname__")
    }

    #[getter]
    fn __wrapped__(&self, py: Python<'_>) -> PyObject {
        self.original.clone_ref(py)
    }
}

/// The `@asset()` decorator factory.
///
/// Usage:
///   @asset()
///   def my_func(): ...
///
///   @asset(name="custom_name", serializer="json")
///   def my_func(): ...
#[pyfunction]
#[pyo3(signature = (*, name=None, serializer=None))]
fn asset(name: Option<String>, serializer: Option<String>) -> PyResult<PyObject> {
    Python::with_gil(|py| {
        let name_clone = name.clone();
        let serializer_clone = serializer.clone();

        let decorator = PyCFunction::new_closure(
            py,
            Some(c"asset_decorator"),
            None,
            move |args: &Bound<'_, PyTuple>,
                  _kwargs: Option<&Bound<'_, PyDict>>|
                  -> PyResult<PyObject> {
                let py = args.py();
                if args.len() != 1 {
                    return Err(pyo3::exceptions::PyTypeError::new_err(
                        "asset() decorator expects exactly one argument (the function)",
                    ));
                }
                let func = args.get_item(0)?;

                let wrapper = AssetWrapper {
                    original: func.unbind(),
                    name: name_clone.clone(),
                    serializer: serializer_clone.clone(),
                };

                Ok(Py::new(py, wrapper)?.into_any())
            },
        )?;

        Ok(decorator.into())
    })
}

/// Inspect modules for barca assets. Called by the `python -m barca.inspect` CLI stub.
///
/// Imports each module, finds functions with `__barca_kind__ == "asset"`,
/// extracts source and metadata, returns JSON string to stdout.
#[pyfunction]
fn inspect_modules(py: Python<'_>, modules: Vec<String>) -> PyResult<String> {
    let importlib = py.import("importlib")?;
    let inspect_mod = py.import("inspect")?;
    let textwrap = py.import("textwrap")?;
    let sys = py.import("sys")?;
    let pathlib = py.import("pathlib")?;

    let version_info = sys.getattr("version_info")?;
    let major: i64 = version_info.getattr("major")?.extract()?;
    let minor: i64 = version_info.getattr("minor")?.extract()?;
    let micro: i64 = version_info.getattr("micro")?.extract()?;
    let python_version = format!("{major}.{minor}.{micro}");

    let mut assets = Vec::new();

    for module_name in &modules {
        let module = importlib.call_method1("import_module", (module_name.as_str(),))?;

        // Get normalized module source
        let module_source_raw = inspect_mod.call_method1("getsource", (&module,))?;
        let module_source_dedented = textwrap.call_method1("dedent", (&module_source_raw,))?;
        let module_source: String = module_source_dedented
            .call_method0("strip")?
            .extract::<String>()
            .map(|s| s + "\n")?;

        let members = inspect_mod.call_method1("getmembers", (&module,))?;

        for member in members.try_iter()? {
            let member: Bound<'_, PyAny> = member?;
            let tuple: &Bound<'_, PyTuple> = member.downcast()?;
            let func = tuple.get_item(1)?;

            // Check __barca_kind__
            let kind_attr = func.getattr("__barca_kind__");
            let kind: Option<String> = match kind_attr {
                Ok(attr) => attr.extract::<String>().ok(),
                Err(_) => None,
            };
            if kind.as_deref() != Some("asset") {
                continue;
            }

            let metadata: serde_json::Value = {
                let meta_obj = func.getattr("__barca_metadata__")?;
                python_dict_to_json(py, &meta_obj)?
            };

            let original = match func.getattr("__barca_original__") {
                Ok(orig) => orig,
                Err(_) => func.clone(),
            };

            // Get function source
            let func_source_raw = inspect_mod.call_method1("getsource", (&original,))?;
            let func_source_dedented = textwrap.call_method1("dedent", (&func_source_raw,))?;
            let function_source: String = func_source_dedented
                .call_method0("strip")?
                .extract::<String>()
                .map(|s| s + "\n")?;

            let function_name: String = original.getattr("__name__")?.extract()?;

            // Get file path
            let source_file = inspect_mod.call_method1("getsourcefile", (&original,));
            let file_path: String = match source_file {
                Ok(sf) => {
                    let path = pathlib.call_method1("Path", (&sf,))?;
                    let resolved = path.call_method0("resolve")?;
                    resolved.str()?.extract()?
                }
                Err(_) => String::new(),
            };

            // Get return type
            let signature = inspect_mod.call_method1("signature", (&original,))?;
            let return_annotation = signature.getattr("return_annotation")?;
            let empty = inspect_mod.getattr("Signature")?.getattr("empty")?;
            let return_type: Option<String> = if return_annotation.is(&empty) {
                None
            } else {
                let is_type: bool = py
                    .import("builtins")?
                    .call_method1(
                        "isinstance",
                        (&return_annotation, py.get_type::<pyo3::types::PyType>()),
                    )?
                    .extract()?;
                if is_type {
                    Some(return_annotation.getattr("__name__")?.extract()?)
                } else {
                    Some(return_annotation.str()?.extract()?)
                }
            };

            let asset_record = serde_json::json!({
                "kind": "asset",
                "module_path": module.getattr("__name__")?.extract::<String>()?,
                "file_path": file_path,
                "function_name": function_name,
                "function_source": function_source,
                "module_source": module_source,
                "decorator_metadata": metadata,
                "return_type": return_type,
                "python_version": python_version,
            });
            assets.push(asset_record);
        }
    }

    let output = serde_json::json!({ "assets": assets });
    serde_json::to_string(&output).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("JSON serialization error: {e}"))
    })
}

/// Materialize a single asset function. Called by the `python -m barca.worker` CLI stub.
///
/// Imports the module, finds the function, calls the unwrapped original,
/// serializes the result to JSON, writes value.json and result.json.
#[pyfunction]
fn materialize_asset(
    py: Python<'_>,
    module_name: String,
    function_name: String,
    output_dir: String,
) -> PyResult<()> {
    let importlib = py.import("importlib")?;
    let inspect_mod = py.import("inspect")?;
    let json_mod = py.import("json")?;
    let pathlib = py.import("pathlib")?;

    let output_path = pathlib.call_method1("Path", (output_dir.as_str(),))?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("parents", true)?;
    kwargs.set_item("exist_ok", true)?;
    output_path.call_method("mkdir", (), Some(&kwargs))?;

    let result_path = output_path.call_method1("__truediv__", ("result.json",))?;

    let result: PyResult<()> = (|| {
        let module = importlib.call_method1("import_module", (module_name.as_str(),))?;
        let func = module.getattr(function_name.as_str())?;

        let original = match func.getattr("__barca_original__") {
            Ok(orig) => orig,
            Err(_) => func.clone(),
        };

        let result_value = original.call0()?;

        let value_path = output_path.call_method1("__truediv__", ("value.json",))?;

        // json.dumps(result)
        let json_str = json_mod.call_method1("dumps", (&result_value,))?;
        value_path.call_method1("write_text", (&json_str, "utf-8"))?;

        let value_path_str: String = value_path.str()?.extract()?;

        let sig = inspect_mod.call_method1("signature", (&original,))?;
        let sig_str: String = sig.str()?.extract()?;

        let result_type: String = result_value.get_type().getattr("__name__")?.extract()?;

        let payload = serde_json::json!({
            "ok": true,
            "artifact_format": "json",
            "value_path": value_path_str,
            "result_type": result_type,
            "module_path": module_name,
            "function_name": function_name,
            "signature": sig_str,
        });

        let payload_str = serde_json::to_string(&payload)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("JSON error: {e}")))?;
        result_path.call_method1("write_text", (payload_str, "utf-8"))?;

        Ok(())
    })();

    if let Err(e) = result {
        let error_str = e.to_string();
        let error_type = e.get_type(py).name()?.to_string();

        let payload = serde_json::json!({
            "ok": false,
            "error": error_str,
            "error_type": error_type,
        });
        let payload_str = serde_json::to_string(&payload)
            .map_err(|e2| pyo3::exceptions::PyRuntimeError::new_err(format!("JSON error: {e2}")))?;
        result_path.call_method1("write_text", (payload_str, "utf-8"))?;

        // Exit with code 1 like the original Python worker
        let sys = py.import("sys")?;
        sys.call_method1("exit", (1i32,))?;
    }

    Ok(())
}

/// Start the barca server. Called by the `barca` CLI entry point.
///
/// Releases the GIL and runs the full axum server (reindex, worker, HTTP)
/// on the current working directory.
#[pyfunction]
fn run_server(py: Python<'_>) -> PyResult<()> {
    // Spawn a dedicated thread that waits for SIGINT and terminates the process.
    // Python's SIGINT handler swallows the signal when the GIL is released,
    // so we need to catch it ourselves at the OS level.
    std::thread::spawn(|| {
        use std::sync::atomic::{AtomicBool, Ordering};
        static RECEIVED: AtomicBool = AtomicBool::new(false);

        unsafe {
            // Install a minimal signal handler that sets a flag
            extern "C" fn handler(_: libc::c_int) {
                RECEIVED.store(true, Ordering::SeqCst);
            }
            libc::signal(libc::SIGINT, handler as *const () as libc::sighandler_t);
        }

        // Busy-wait would be wasteful; use sigwait instead
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if RECEIVED.load(std::sync::atomic::Ordering::SeqCst) {
                eprintln!(); // newline after ^C
                std::process::exit(130); // 128 + SIGINT(2)
            }
        }
    });

    py.allow_threads(|| {
        let rt = tokio::runtime::Runtime::new().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("failed to create runtime: {e}"))
        })?;

        rt.block_on(async {
            run_server_async()
                .await
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("{e}")))
        })
    })
}

async fn run_server_async() -> anyhow::Result<()> {
    use anyhow::Context;

    tracing_subscriber::fmt().with_env_filter("info").init();

    let repo_root = std::env::current_dir().context("failed to resolve current dir")?;
    let config = barca_server::config::load_config(&repo_root.join("barca.toml"))?;
    let store =
        barca_server::store::MetadataStore::open(&repo_root.join(".barca").join("metadata.db"))
            .await?;
    let python = barca_server::python_bridge::PythonBridge::new(repo_root.clone());

    let state = barca_server::AppState::new(repo_root, config, store, python);

    barca_server::reindex(&state).await?;
    {
        let store = state.store.lock().await;
        store.requeue_running_materializations().await?;
    }
    tracing::info!("refresh queue recovery complete");
    tokio::spawn(barca_server::run_refresh_queue_worker(state.clone()));

    let app = barca_server::server::router().with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    tracing::info!("barca listening on http://127.0.0.1:3000");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen for ctrl-c");
            tracing::info!("shutting down");
        })
        .await?;
    Ok(())
}

/// Convert a Python dict (or any object) to serde_json::Value
fn python_dict_to_json(py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    let json_mod = py.import("json")?;
    let json_str: String = json_mod.call_method1("dumps", (obj,))?.extract()?;
    serde_json::from_str(&json_str)
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("JSON parse error: {e}")))
}

/// Run `barca reset` — remove generated files and caches.
#[pyfunction]
#[pyo3(signature = (*, db=false, artifacts=false, tmp=false))]
fn run_reset(db: bool, artifacts: bool, tmp: bool) -> PyResult<String> {
    let repo_root = std::env::current_dir().map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("failed to resolve current dir: {e}"))
    })?;
    barca_server::reset(&repo_root, db, artifacts, tmp)
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("{e}")))
}

/// The native `_barca` module.
#[pymodule]
fn _barca(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<AssetWrapper>()?;
    m.add_function(wrap_pyfunction!(asset, m)?)?;
    m.add_function(wrap_pyfunction!(inspect_modules, m)?)?;
    m.add_function(wrap_pyfunction!(materialize_asset, m)?)?;
    m.add_function(wrap_pyfunction!(run_server, m)?)?;
    m.add_function(wrap_pyfunction!(run_reset, m)?)?;
    Ok(())
}
