#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <stdlib.h>
#include <limits.h>

/**
 * 1-) Removes the previous builds if any target files are present
 * 2-) Change directory to wasm-module using ../wasm-module
 * 3-) Run the command "cargo build --release --target=wasm32-wasip1" to compile the wasm module
 * 4-) Move the wasm module if generated to ./build, if got an error stop the program and show the error
 * 5-) Change the directory to wasmtime-custom using ../wasmtime-custom
 * 6-) Run the command "cargo build --release" to compile the custom wasmtime wrapper
 * 7-) Move the compiled binary to the ./build folder
 */

#define WASM_MODULE_NAME "wasi-nn-module"
#define WASMTIME_NAME "wasmtime-test"

void remove_old_binaries(const char *binaries[], int size)
{
    for (int i = 0; i < size; i++)
    {
        if (remove(binaries[i]) == 0)
        {
            printf("Removed: %s\n", binaries[i]);
        }
    }
}

void change_dir(char *dir_path)
{
    chdir(dir_path);
    char current_path[PATH_MAX];
    getcwd(current_path, sizeof(current_path));
    printf("Path changed, currently at: %s\n", current_path);
}

void run_command(char *command, char *output_message, char *error_message)
{
    if (system(command) == 0)
    {
        printf("%s\n", output_message);
    }
    else
    {
        printf("%s\n", error_message);
        exit(EXIT_FAILURE);
    }
}

void move_file(char *source_path, char *destination_path, char *output_message, char *error_message)
{
    char command[PATH_MAX * 2 + 10]; // giving enough size for mv source_path dest_path command buffer
    snprintf(command, sizeof(command), "mv %s %s", source_path, destination_path);
    run_command(command, output_message, error_message);
}

int main()
{
    // Remove old binaries
    const char *binary_files[] = {"./binaries/wasmtime-test", "./binaries/wasi-nn-module.wasm", "./binaries/wasi-nn-module.wasm.SERIALIZED"};
    remove_old_binaries(binary_files, 3);

    // Change dir to wasm-module, compile the module and move to binaries folder
    change_dir("../wasm-module");
    run_command("cargo build --release --target=wasm32-wasip1", "Wasm Module Compiled Successfully", "Some error occurred while compiling wasm module");
    move_file("./target/wasm32-wasip1/release/wasi-nn-module.wasm", "../scripts/binaries/wasi-nn-module.wasm", "Moved Compiled Module Successfully", "Error while moving compiled module");

    // Change dir to wasmtime-custom, compile the binary and move to binaries folder
    change_dir("../wasmtime-custom");
    run_command("cargo build --release", "Wasmtime custom Wrapper Compiled Successfully", "Some error occurred while compiling binary");
    move_file("./target/release/wasmtime-test", "../scripts/binaries/wasmtime-test", "Moved Compiled Binary Successfully", "Error while moving compiled binary");

    return 0;
}