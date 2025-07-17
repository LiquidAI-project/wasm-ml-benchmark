extern crate wasmtime;
extern crate wasmtime_wasi;
extern crate wasi_common;
extern crate anyhow;
extern crate cap_std;
extern crate local_names;
extern crate wasmtime_wasi_nn;

use anyhow::{Ok, Result};
use local_names::{get_image_index, get_model_index};
use std::{env, path::Path, time::Instant};
use wasmtime::{Config, Engine, Module, Store};
use wasi_common::{sync::Dir, sync::WasiCtxBuilder, WasiCtx};
use wasmtime::component::__internal::wasmtime_environ::__core::result::Result::Ok as WasmtimeResultOk;
use wasmtime_wasi_nn::{InMemoryRegistry, WasiNnCtx, backend::onnxruntime::OnnxBackend};


/// The host state for running wasi-nn tests.
struct Ctx {
    wasi: WasiCtx,
    wasi_nn: WasiNnCtx,
}
impl Ctx {
    fn new(directories: &Vec<&str>) -> Result<Self> {
        let preopen_dirs = directories
            .iter()
            .map(|dir| {
                Dir::open_ambient_dir(Path::new(dir), cap_std::ambient_authority())
            }.unwrap());

        let mut binding = WasiCtxBuilder::new();
        let builder = binding.inherit_stdio();
        for (preopen_dir, path) in preopen_dirs.zip(directories) {
            builder.preopened_dir(preopen_dir, path)?;
        }

        let wasi = builder.build();
        let wasi_nn = WasiNnCtx::new(
            [OnnxBackend::default().into()],
            InMemoryRegistry::new().into()
        );

        Ok(Self { wasi, wasi_nn })
    }
}


fn main() -> wasmtime::Result<()> {
    const MODEL_DIR: &str = "assets/models";
    const IMAGE_DIR: &str = "assets/imgs";
    let shared_dirs: Vec<&str> = vec![MODEL_DIR, IMAGE_DIR];

    let args: Vec<String> = env::args().collect();
    // if args.len() != 5 {
    //     println!("Usage: {} <wasm module> <model> <image> <number of repeats>", args[0]);
    //     return Ok(());
    // }

    let wasm_module_filename: &str = &args[1];
    // let model_filename: &str = &args[2];
    // let image_name: &str = &args[3];
    // let model_index = match get_model_index(model_filename) {
    //     Some(index) => index,
    //     None => {
    //         println!("Model not found: {}", model_filename);
    //         return Ok(());
    //     }
    // };
    // let image_index = match get_image_index(image_name) {
    //     Some(index) => index,
    //     None => {
    //         println!("Image not found: {}", image_name);
    //         return Ok(());
    //     }
    // };
    // let repeats: u32 = args[4].parse().unwrap();

    let start: Instant = Instant::now();

    let config = Config::default();
    let engine = Engine::new(&config)?;
    let mut linker = wasmtime::Linker::new(&engine);

    wasi_common::sync::add_to_linker(&mut linker, |host: &mut Ctx| &mut host.wasi)?;
    wasmtime_wasi_nn::witx::add_to_linker(&mut linker, |host| &mut host.wasi_nn)?;

    let mut store = Store::new(
        &engine,
        Ctx::new(&shared_dirs)?
    );
    let environment_set_time = start.elapsed();

    let wasm_module_serialized_name = wasm_module_filename.to_string() + ".SERIALIZED";
    let wasm_module =
        match unsafe { Module::deserialize_file(&engine, wasm_module_serialized_name.clone()) } {
            WasmtimeResultOk(serialized_module) => serialized_module,
            Err(_) => {
                let loaded_module = Module::from_file(&engine, wasm_module_filename)?;
                let byte_module = loaded_module.serialize()?;
                std::fs::write(wasm_module_serialized_name, byte_module).unwrap();

                loaded_module
            }
        };

    // add the module to the linker
    const MODULE_NAME: &str = "test";
    const FUNCTION_NAME: &str = "main";
    linker.module(&mut store, MODULE_NAME, &wasm_module)?;
    let module_load_time = start.elapsed() - environment_set_time;

    let inference_function = linker
        .get(&mut store, MODULE_NAME, FUNCTION_NAME).unwrap()
        .into_func().unwrap()
        .typed::<(), ()>(&mut store).unwrap();
    let function_load_time = start.elapsed() - environment_set_time - module_load_time;

    println!("Creating the Wasm environment took: {:?}", environment_set_time);
    println!("Loading the Wasm module took: {:?}", module_load_time);
    println!("Loading the Wasm function took: {:?}\n", function_load_time);

    let _result = inference_function.call(&mut store, ());

    Ok(())
}
