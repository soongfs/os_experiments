#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int compare_names(const void *left, const void *right) {
    const char *const *lhs = (const char *const *)left;
    const char *const *rhs = (const char *const *)right;
    return strcmp(*lhs, *rhs);
}

int main(void) {
    DIR *dir;
    struct dirent *entry;
    char cwd[4096];
    char **names = NULL;
    size_t count = 0;
    size_t capacity = 0;
    size_t i;

    if (getcwd(cwd, sizeof(cwd)) == NULL) {
        perror("getcwd");
        return 1;
    }

    dir = opendir(".");
    if (dir == NULL) {
        perror("opendir");
        return 1;
    }

    while ((entry = readdir(dir)) != NULL) {
        char *copy;

        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }

        if (count == capacity) {
            size_t new_capacity = (capacity == 0) ? 8 : capacity * 2;
            char **new_names = realloc(names, new_capacity * sizeof(char *));
            if (new_names == NULL) {
                perror("realloc");
                closedir(dir);
                free(names);
                return 1;
            }
            names = new_names;
            capacity = new_capacity;
        }

        copy = strdup(entry->d_name);
        if (copy == NULL) {
            perror("strdup");
            closedir(dir);
            for (i = 0; i < count; ++i) {
                free(names[i]);
            }
            free(names);
            return 1;
        }

        names[count++] = copy;
    }

    if (closedir(dir) != 0) {
        perror("closedir");
        for (i = 0; i < count; ++i) {
            free(names[i]);
        }
        free(names);
        return 1;
    }

    qsort(names, count, sizeof(char *), compare_names);

    printf("Current working directory: %s\n", cwd);
    printf("Directory entries:\n");
    for (i = 0; i < count; ++i) {
        printf("%zu. %s\n", i + 1, names[i]);
        free(names[i]);
    }

    free(names);
    return 0;
}
