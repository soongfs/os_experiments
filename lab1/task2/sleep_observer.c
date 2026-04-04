#include <errno.h>
#include <stdio.h>
#include <time.h>

static void print_realtime_stamp(const char *label, const struct timespec *ts) {
    struct tm local_tm;
    char buffer[64];

    localtime_r(&ts->tv_sec, &local_tm);
    strftime(buffer, sizeof(buffer), "%Y-%m-%d %H:%M:%S", &local_tm);
    printf("%s: %s.%09ld\n", label, buffer, ts->tv_nsec);
}

static double timespec_diff_seconds(const struct timespec *start, const struct timespec *end) {
    time_t sec = end->tv_sec - start->tv_sec;
    long nsec = end->tv_nsec - start->tv_nsec;

    if (nsec < 0) {
        sec -= 1;
        nsec += 1000000000L;
    }

    return (double)sec + (double)nsec / 1000000000.0;
}

int main(void) {
    struct timespec start_real;
    struct timespec end_real;
    struct timespec start_mono;
    struct timespec end_mono;
    struct timespec request = {.tv_sec = 5, .tv_nsec = 0};
    struct timespec remain = {0};
    int ret;

    if (clock_gettime(CLOCK_REALTIME, &start_real) != 0) {
        perror("clock_gettime CLOCK_REALTIME");
        return 1;
    }

    if (clock_gettime(CLOCK_MONOTONIC, &start_mono) != 0) {
        perror("clock_gettime CLOCK_MONOTONIC");
        return 1;
    }

    print_realtime_stamp("Before nanosleep", &start_real);
    printf("Sleeping for %ld seconds...\n", request.tv_sec);
    fflush(stdout);

    do {
        ret = nanosleep(&request, &remain);
        if (ret != 0 && errno == EINTR) {
            request = remain;
        }
    } while (ret != 0 && errno == EINTR);

    if (ret != 0) {
        perror("nanosleep");
        return 1;
    }

    if (clock_gettime(CLOCK_REALTIME, &end_real) != 0) {
        perror("clock_gettime CLOCK_REALTIME");
        return 1;
    }

    if (clock_gettime(CLOCK_MONOTONIC, &end_mono) != 0) {
        perror("clock_gettime CLOCK_MONOTONIC");
        return 1;
    }

    print_realtime_stamp("After nanosleep ", &end_real);
    printf("Elapsed seconds (monotonic): %.6f\n", timespec_diff_seconds(&start_mono, &end_mono));

    return 0;
}
