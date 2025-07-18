#include <stdio.h>
#include <unistd.h>
#include <limits.h>

void change_dir(char *dir_path)
{
    chdir(dir_path);
    char current_path[PATH_MAX];
    getcwd(current_path, sizeof(current_path));
    printf("Path changed, currently at: %s\n", current_path);
}

int parse_args(int argc, char* argv[])
{

}

int main(int argc, char *argv[])
{
    parse_args(argc, argv);
    change_dir("./binaries");

    return 0;
}