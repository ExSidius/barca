use pyo3::prelude::*;
use pyo3::types::{PyCFunction, PyDict, PyList, PyTuple};
use std::collections::HashMap;

/// Marker type returned by `partitions([...])`. Stores the static values.
#[pyclass(name = "Partitions")]
#[derive(Clone)]
struct Partitions {
    values: Vec<serde_json::Value>,
}

#[pymethods]
impl Partitions {
    fn __repr__(&self) -> String {
        format!("Partitions({} values)", self.values.len())
    }
}

/// Declare a static partition universe for use with ``@asset(partitions=...)``.
///
/// Example::
///
///     from barca import asset, partitions
///
///     @asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])})
///     def prices(ticker: str): ...
#[pyfunction(name = "partitions")]
fn py_partitions(py: Python<'_>, values: &Bound<'_, PyList>) -> PyResult<Partitions> {
    let json_mod = py.import("json")?;
    let mut vals = Vec::new();
    for item in values.iter() {
        let json_str: String = json_mod.call_method1("dumps", (&item,))?.extract()?;
        let val: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid partition value: {e}")))?;
        vals.push(val);
    }
    Ok(Partitions { values: vals })
}

/// Resolved partition spec for one dimension: either inline values or an asset ref.
#[derive(Clone)]
enum PartitionSpec {
    Inline(Vec<serde_json::Value>),
}

/// The `@asset()` decorator wrapper. Stores the original function and metadata.
#[pyclass(name = "AssetWrapper")]
struct AssetWrapper {
    original: PyObject,
    name: Option<String>,
    serializer: Option<String>,
    /// Resolved inputs: param_name -> "{abs_file_path}:{function_name}"
    inputs: Option<HashMap<String, String>>,
    /// Resolved partitions: dim_name -> spec
    partitions: Option<HashMap<String, PartitionSpec>>,
}

#[pymethods]
impl AssetWrapper {
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__<'py>(&self, py: Python<'py>, args: &Bound<'py, PyTuple>, kwargs: Option<&Bound<'py, PyDict>>) -> PyResult<PyObject> {
        self.original.call(py, args, kwargs)
    }

    #[getter]
    fn __barca_metadata__(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("kind", "asset")?;
        dict.set_item("name", self.name.as_deref())?;
        dict.set_item("serializer", self.serializer.as_deref())?;
        if let Some(ref inputs) = self.inputs {
            let inputs_dict = PyDict::new(py);
            for (k, v) in inputs {
                inputs_dict.set_item(k, v)?;
            }
            dict.set_item("inputs", inputs_dict)?;
        } else {
            dict.set_item("inputs", py.None())?;
        }
        if let Some(ref parts) = self.partitions {
            let parts_dict = PyDict::new(py);
            for (dim, spec) in parts {
                match spec {
                    PartitionSpec::Inline(values) => {
                        let json_str = serde_json::to_string(values).unwrap();
                        let inner = PyDict::new(py);
                        inner.set_item("kind", "inline")?;
                        inner.set_item("values_json", &json_str)?;
                        parts_dict.set_item(dim, inner)?;
                    }
                }
            }
            dict.set_item("partitions", parts_dict)?;
        } else {
            dict.set_item("partitions", py.None())?;
        }
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

    fn __get__<'py>(slf: &Bound<'py, Self>, _obj: &Bound<'py, PyAny>, _objtype: Option<&Bound<'py, PyAny>>) -> PyResult<Bound<'py, Self>> {
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

/// Resolve an input value (function object or string ref) to a canonical
/// asset reference string: "{absolute_file_path}:{function_name}".
fn resolve_input_ref(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<String> {
    // If it's an AssetWrapper, extract the original function's file + name
    let kind_attr = value.getattr("__barca_kind__");
    if let Ok(kind) = kind_attr {
        if let Ok(k) = kind.extract::<String>() {
            if k == "asset" {
                let original = match value.getattr("__barca_original__") {
                    Ok(orig) => orig,
                    Err(_) => value.clone(),
                };
                let inspect_mod = py.import("inspect")?;
                let pathlib = py.import("pathlib")?;
                let sf = inspect_mod.call_method1("getsourcefile", (&original,))?;
                let path = pathlib.call_method1("Path", (&sf,))?;
                let resolved = path.call_method0("resolve")?;
                let file_path: String = resolved.str()?.extract()?;
                let func_name: String = original.getattr("__name__")?.extract()?;
                return Ok(format!("{file_path}:{func_name}"));
            }
        }
    }

    // If it's a string, treat as an explicit asset ref
    if let Ok(s) = value.extract::<String>() {
        return Ok(s);
    }

    Err(pyo3::exceptions::PyTypeError::new_err("inputs values must be @asset-decorated functions or asset ref strings"))
}

/// Decorator that registers a Python function as a barca asset.
///
/// Usage
/// -----
///     from barca import asset
///
///     # Both forms are supported:
///     @asset
///     def my_data(): ...
///
///     @asset()
///     def my_data(): ...
///
/// Parameters
/// ----------
/// name : str, optional
///     Override the asset's display name / continuity key.
///     Defaults to ``"<file>:<function_name>"``.
/// inputs : dict[str, asset], optional
///     Declare upstream dependencies.  Map parameter names to other
///     ``@asset``-decorated functions.  Barca will materialize upstream
///     assets first and pass their values as keyword arguments.
///
///     Example::
///
///         @asset()
///         def raw(): return [1, 2, 3]
///
///         @asset(inputs={"data": raw})
///         def processed(data): return [x * 2 for x in data]
///
/// partitions : dict[str, partitions(...)], optional
///     Declare a partitioned asset.  Each dimension maps to a
///     ``partitions([...])`` call that enumerates the static partition keys.
///     Barca will create one materialization job per partition combination.
///
///     Example::
///
///         from barca import asset, partitions
///
///         @asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])})
///         def prices(ticker: str):
///             return fetch_price(ticker)
///
/// serializer : str, optional
///     Reserved for future use.  Leave unset.
#[pyfunction]
#[pyo3(signature = (func=None, *, name=None, inputs=None, partitions=None, serializer=None))]
fn asset(func: Option<PyObject>, name: Option<String>, inputs: Option<PyObject>, partitions: Option<PyObject>, serializer: Option<String>) -> PyResult<PyObject> {
    Python::with_gil(|py| {
        // Resolve inputs eagerly at decoration time
        let resolved_inputs: Option<HashMap<String, String>> = match inputs {
            Some(ref obj) => {
                let dict: &Bound<'_, PyDict> = obj.bind(py).downcast()?;
                let mut map = HashMap::new();
                for (k, v) in dict.iter() {
                    let param_name: String = k.extract()?;
                    let ref_str = resolve_input_ref(py, &v)?;
                    map.insert(param_name, ref_str);
                }
                Some(map)
            }
            None => None,
        };

        // Resolve partitions eagerly at decoration time
        let resolved_partitions: Option<HashMap<String, PartitionSpec>> = match partitions {
            Some(ref obj) => {
                let dict: &Bound<'_, PyDict> = obj.bind(py).downcast()?;
                let mut map = HashMap::new();
                for (k, v) in dict.iter() {
                    let dim_name: String = k.extract()?;
                    // Check if it's a Partitions instance (inline values)
                    if let Ok(p) = v.extract::<Partitions>() {
                        map.insert(dim_name, PartitionSpec::Inline(p.values));
                    } else {
                        return Err(pyo3::exceptions::PyTypeError::new_err("partition values must be partitions([...]) instances"));
                    }
                }
                Some(map)
            }
            None => None,
        };

        // @asset (no parentheses): func is the decorated function directly.
        // Wrap it immediately and return the AssetWrapper.
        if let Some(f) = func {
            let wrapper = AssetWrapper {
                original: f,
                name,
                serializer,
                inputs: resolved_inputs,
                partitions: resolved_partitions,
            };
            return Ok(Py::new(py, wrapper)?.into_any());
        }

        // @asset() or @asset(name=...) etc.: return a one-argument decorator closure.
        let name_clone = name.clone();
        let serializer_clone = serializer.clone();
        let inputs_clone = resolved_inputs.clone();
        let partitions_clone = resolved_partitions.clone();

        let decorator = PyCFunction::new_closure(
            py,
            Some(c"asset_decorator"),
            None,
            move |args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| -> PyResult<PyObject> {
                let py = args.py();
                if args.len() != 1 {
                    return Err(pyo3::exceptions::PyTypeError::new_err("asset() decorator expects exactly one argument (the function)"));
                }
                let func = args.get_item(0)?;

                let wrapper = AssetWrapper {
                    original: func.unbind(),
                    name: name_clone.clone(),
                    serializer: serializer_clone.clone(),
                    inputs: inputs_clone.clone(),
                    partitions: partitions_clone.clone(),
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
        let module = match importlib.call_method1("import_module", (module_name.as_str(),)) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("barca: skipping module '{}': {}", module_name, e);
                continue;
            }
        };

        // Get normalized module source
        let module_source_raw = inspect_mod.call_method1("getsource", (&module,))?;
        let module_source_dedented = textwrap.call_method1("dedent", (&module_source_raw,))?;
        let module_source: String = module_source_dedented.call_method0("strip")?.extract::<String>().map(|s| s + "\n")?;

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
            let function_source: String = func_source_dedented.call_method0("strip")?.extract::<String>().map(|s| s + "\n")?;

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
                    .call_method1("isinstance", (&return_annotation, py.get_type::<pyo3::types::PyType>()))?
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
    serde_json::to_string(&output).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("JSON serialization error: {e}")))
}

/// Materialize a single asset function. Called by the `python -m barca.worker` CLI stub.
///
/// Imports the module, finds the function, calls the unwrapped original,
/// serializes the result to JSON, writes value.json and result.json.
///
/// If `input_kwargs_json` is provided, it is parsed as a JSON dict and
/// passed as keyword arguments to the function.
#[pyfunction]
#[pyo3(signature = (module_name, function_name, output_dir, input_kwargs_json=None))]
fn materialize_asset(py: Python<'_>, module_name: String, function_name: String, output_dir: String, input_kwargs_json: Option<String>) -> PyResult<()> {
    let importlib = py.import("importlib")?;
    let inspect_mod = py.import("inspect")?;
    let json_mod = py.import("json")?;
    let pathlib = py.import("pathlib")?;

    let output_path = pathlib.call_method1("Path", (output_dir.as_str(),))?;
    let mkdir_kwargs = PyDict::new(py);
    mkdir_kwargs.set_item("parents", true)?;
    mkdir_kwargs.set_item("exist_ok", true)?;
    output_path.call_method("mkdir", (), Some(&mkdir_kwargs))?;

    let result_path = output_path.call_method1("__truediv__", ("result.json",))?;

    let result: PyResult<()> = (|| {
        let module = importlib.call_method1("import_module", (module_name.as_str(),))?;
        let func = module.getattr(function_name.as_str())?;

        let original = match func.getattr("__barca_original__") {
            Ok(orig) => orig,
            Err(_) => func.clone(),
        };

        // Call with or without kwargs
        let result_value = if let Some(ref kwargs_json) = input_kwargs_json {
            let kwargs_obj = json_mod.call_method1("loads", (kwargs_json.as_str(),))?;
            let kwargs_dict: &Bound<'_, PyDict> = kwargs_obj.downcast()?;
            original.call((), Some(kwargs_dict))?
        } else {
            original.call0()?
        };

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

        let payload_str = serde_json::to_string(&payload).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("JSON error: {e}")))?;
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
        let payload_str = serde_json::to_string(&payload).map_err(|e2| pyo3::exceptions::PyRuntimeError::new_err(format!("JSON error: {e2}")))?;
        result_path.call_method1("write_text", (payload_str, "utf-8"))?;

        // Exit with code 1 like the original Python worker
        let sys = py.import("sys")?;
        sys.call_method1("exit", (1i32,))?;
    }

    Ok(())
}

/// Convert a Python dict (or any object) to serde_json::Value
fn python_dict_to_json(py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    let json_mod = py.import("json")?;
    let json_str: String = json_mod.call_method1("dumps", (obj,))?.extract()?;
    serde_json::from_str(&json_str).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("JSON parse error: {e}")))
}

/// The native `_barca` module.
#[pymodule]
fn _barca(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<AssetWrapper>()?;
    m.add_class::<Partitions>()?;
    m.add_function(wrap_pyfunction!(asset, m)?)?;
    m.add_function(wrap_pyfunction!(py_partitions, m)?)?;
    m.add_function(wrap_pyfunction!(inspect_modules, m)?)?;
    m.add_function(wrap_pyfunction!(materialize_asset, m)?)?;
    Ok(())
}
