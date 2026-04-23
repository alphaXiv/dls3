#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>

int main(void) {
    int fd = open("/neer/prefix/hello.txt", O_RDONLY);
    if (fd < 0) {
        perror("opening hello.txt");
        return 1;
    }
    char buf[4096];
    size_t num_read;
    while ((num_read = read(fd, buf, sizeof buf)) > 0) {
        fwrite(buf, 1, num_read, stdout);
    }
    close(fd);
    return 0;
}
