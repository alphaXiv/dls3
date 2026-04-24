const std = @import("std");
const hook = @import("hook");
const State = hook.State;
const hardcoded_config = hook.hardcoded_config;
const path = hook.path;
const protocol = hook.protocol;
const c = hook.c;
const Errno = hook.Errno;
const linux = std.os.linux;
const mode_t = linux.mode_t;

pub const open = hook.wrappers.wrapOpen(
    // `open()` is actually variadic instead of taking a `mode_t` parameter. But Zig doesn't support
    // reading variadic arguments on aarch64. So we type-pun to a non-variadic `mode_t` parameter,
    // which is equivalent on Linux x86_64 and aarch64. Before porting to another platform you will
    // need to ensure that this works.
    fn (pathname: [*:0]const c_char, flags: c_int, mode: mode_t) callconv(.c) c_int,
    "open",
    struct {
        fn adapter(
            realOpen: *const fn ([*:0]const c_char, c_int, mode_t) callconv(.c) c_int,
            args: struct { [*:0]const c_char, c_int, mode_t },
        ) hook.wrappers.OpenAdapterReturn(c_int) {
            const pathname, const flags, const mode = args;
            const fd = realOpen(pathname, flags, mode);
            return .{
                .value = fd,
                .fd = if (fd < 0) null else fd,
            };
        }
    }.adapter,
    struct {
        fn close(fd: c_int) void {
            _ = linux.close(fd);
        }
    }.close,
    -1,
);

pub const openat = hook.wrappers.wrapOpen(
    // `openat()` is actually variadic instead of taking a `mode_t` parameter. But Zig doesn't support
    // reading variadic arguments on aarch64. So we type-pun to a non-variadic `mode_t` parameter,
    // which is equivalent on Linux x86_64 and aarch64. Before porting to another platform you will
    // need to ensure that this works.
    fn (dirfd: c_int, pathname: [*:0]const c_char, flags: c_int, mode: mode_t) callconv(.c) c_int,
    "openat",
    struct {
        fn adapter(
            realOpenat: *const fn (dirfd: c_int, pathname: [*:0]const c_char, flags: c_int, mode: mode_t) callconv(.c) c_int,
            args: struct { c_int, [*:0]const c_char, c_int, mode_t },
        ) hook.wrappers.OpenAdapterReturn(c_int) {
            const dirfd, const pathname, const flags, const mode = args;
            const fd = realOpenat(dirfd, pathname, flags, mode);
            return .{
                .value = fd,
                .fd = if (fd < 0) null else fd,
            };
        }
    }.adapter,
    struct {
        fn close(fd: c_int) void {
            _ = linux.close(fd);
        }
    }.close,
    -1,
);

pub const fopen = hook.wrappers.wrapOpen(
    fn (pathname: [*:0]const c_char, mode: [*:0]const c_char) callconv(.c) ?*c.FILE,
    "fopen",
    struct {
        fn adapter(
            realFopen: *const fn (pathname: [*:0]const c_char, mode: [*:0]const c_char) callconv(.c) ?*c.FILE,
            args: struct { [*:0]const c_char, [*:0]const c_char },
        ) hook.wrappers.OpenAdapterReturn(?*c.FILE) {
            const pathname, const mode = args;
            const fp = realFopen(pathname, mode);
            return .{
                .value = fp,
                .fd = if (fp) |nonnull| c.fileno(nonnull) else null,
            };
        }
    }.adapter,
    struct {
        fn close(stream: ?*c.FILE) void {
            if (stream) |nonnull| {
                _ = c.fclose(nonnull);
            }
        }
    }.close,
    null,
);

fn closeImpl(state: *State, fd: c_int) !void {
    const key = (try path.resolveOpenedFdToKey(state.io(), state.allocator(), state.config, fd)) orelse return;
    try protocol.writeMessage(state.writer(), &.{ .close = .{ .path = key } });
}

pub fn close(fd: c_int) callconv(.c) c_int {
    const realClose: *const fn (c_int) callconv(.c) c_int = @ptrCast(@alignCast(c.dlsym(c.RTLD_NEXT, "close")));

    const state = State.get(&hardcoded_config) catch return realClose(fd);
    defer state.errno.clear();
    defer _ = state.arena.reset(.retain_capacity);

    closeImpl(state, fd) catch |err| {
        std.log.err("close({d}): {s}", .{ fd, @errorName(err) });
    };
    return realClose(fd);
}
