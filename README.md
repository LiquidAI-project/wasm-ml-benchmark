# wasm-ml-benchmark
Benchmarking Project to benchmark ML inference using Wasm

## Prerequisites
- Ensure you have a C compiler (e.g., `gcc`) installed.
- Ensure Rust is installed

## Build Instructions
1. **Build Binaries and WASM Module**
   - Run the `build.c` script to compile all binaries, including the custom `wasmtime` binary and the WebAssembly (WASM) module.
   - Command:
     ```bash
     gcc build.c -o build && ./build
     ```
   - This will generate the necessary binaries and the WASM module in the binaries folder.

2. **Build Benchmark Script**
   - Compile the `benchmark.c` script to create the benchmarking executable.
   - Command:
     ```bash
     gcc benchmark.c -o benchmark
     ```

## Running the Benchmark
1. **Execute the Benchmark Script**
   - Run the `benchmark` executable with the required number of iterations and stack trace flag.
   - Command:
     ```bash
     ./benchmark <number_of_iterations> <enable_stack_trace>
     ```
   - **Parameters**:
     - `<number_of_iterations>`: Specify the number of iterations for the benchmark (e.g., `1000`).
     - `<enable_stack_trace>` : Enable this flag to enable Rust stack trace output for debugging.

## Example
To run the benchmark with 500 iterations and disabled stack trace:
```bash
./benchmark 500 0
```

## Notes
- Ensure all dependencies are correctly installed before running the build scripts.
- The `<enable_stack_trace>` flag requires Rust to be installed and configured properly.
- Check the console output for benchmarking results and any errors.
