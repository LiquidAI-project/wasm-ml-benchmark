#include <stdio.h>
#include <unistd.h>
#include <limits.h>
#include <stdlib.h>

/*
    1-) The command will take arguments in the following form: usage build <number of iterations> <command to run>
    2-) The builds can be generated from the different methods but this command can be used for running the binaries or python code
    3-) This C program will parse the arguments getting the value of number of iterations and command to run
    4-) Change the directory to the binaries or build folder
    5-) Run the command and check for errors
*/

void parse_args(int argc, char *argv[], int *number_iterations, char **command_to_run)
{
    if (argc < 3)
    {
        printf("Error parsing, usage: ./benchmark <number_iterations> <command_to_run>");
        exit(EXIT_FAILURE);
    }
    *number_iterations = atoi(argv[1]);
    *command_to_run = argv[2];
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

int main(int argc, char *argv[])
{
    // Parse the command
    int number_iterations = 0;
    char *command_to_run = NULL;
    parse_args(argc, argv, &number_iterations, &command_to_run);

    // Change the directory and run the command
    change_dir("./binaries");
    for (int i = 1; i <= number_iterations; i++)
    {
        run_command(command_to_run, "Command Run Successfully", "Error occurred while running command");
    }

    return 0;
}