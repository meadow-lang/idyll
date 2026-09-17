/* Small POSIX shims for the rustyline port.
 *
 * ioctl(2) is variadic, and calling a variadic C function through a
 * non-variadic Haskell FFI import is wrong on some ABIs (notably
 * aarch64-darwin), so the window-size query goes through this wrapper.
 * select(2) is used rather than poll(2) because poll does not work on
 * /dev/tty on macOS.
 */
#include <errno.h>
#include <sys/ioctl.h>
#include <sys/select.h>
#include <unistd.h>

int rustyline_get_winsize(int fd, int *cols, int *rows)
{
    struct winsize ws;
    if (ioctl(fd, TIOCGWINSZ, &ws) != 0) {
        return -1;
    }
    *cols = ws.ws_col;
    *rows = ws.ws_row;
    return 0;
}

/* Wait until one of up to three descriptors is readable (negative
 * descriptors are ignored). timeout_ms < 0 waits forever.
 * Returns a bitmask (bit i set when descriptor i is readable), 0 on timeout,
 * or -1 with errno set. EINTR is reported to the caller. */
int rustyline_wait_readable(int fd0, int fd1, int fd2, int timeout_ms)
{
    fd_set readfds;
    int fds[3] = { fd0, fd1, fd2 };
    int maxfd = -1;
    FD_ZERO(&readfds);
    for (int i = 0; i < 3; i++) {
        if (fds[i] >= 0) {
            FD_SET(fds[i], &readfds);
            if (fds[i] > maxfd) {
                maxfd = fds[i];
            }
        }
    }
    struct timeval tv;
    struct timeval *tvp = NULL;
    if (timeout_ms >= 0) {
        tv.tv_sec = timeout_ms / 1000;
        tv.tv_usec = (timeout_ms % 1000) * 1000;
        tvp = &tv;
    }
    int r = select(maxfd + 1, &readfds, NULL, NULL, tvp);
    if (r <= 0) {
        return r;
    }
    int mask = 0;
    for (int i = 0; i < 3; i++) {
        if (fds[i] >= 0 && FD_ISSET(fds[i], &readfds)) {
            mask |= 1 << i;
        }
    }
    return mask;
}
