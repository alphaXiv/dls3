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

fn openImpl(
    state: *State,
    realOpen: *const fn ([*:0]const c_char, c_int, mode_t) callconv(.c) c_int,
    pathname: [*:0]const c_char,
    flags: c_int,
    mode: mode_t,
) !c_int {
    const flags_struct: linux.O = @bitCast(flags);
    if (flags_struct.ACCMODE != .RDONLY) {
        state.errno.errno = .ROFS;
        return error.Errno;
    } else if (flags_struct.CREAT) {
        state.errno.errno = .ROFS;
        return error.Errno;
    } else if (flags_struct.TMPFILE) {
        state.errno.errno = .OPNOTSUPP;
        return error.Errno;
    }

    const fd = realOpen(pathname, flags, mode);
    if (fd < 0) return fd;
    errdefer _ = linux.close(fd);

    const key = (try path.resolveOpenedFdToKey(state.io(), state.allocator(), state.config, fd)) orelse return fd;
    try protocol.writeMessage(state.writer(), &.{ .open = .{ .path = key } });
    while (true) {
        const response = try protocol.readMessage(state.reader(), state.allocator());
        switch (response) {
            .opened => |o| {
                if (o.errno != .SUCCESS) {
                    state.errno.errno = o.errno;
                    return error.Errno;
                } else {
                    return fd;
                }
            },
        }
    }
}

pub fn open(pathname: [*:0]const c_char, flags: c_int, mode: std.c.mode_t) callconv(.c) c_int {
    // `open()` is actually variadic instead of taking a `mode_t` parameter. But Zig doesn't support
    // reading variadic arguments on aarch64. So we type-pun to a non-variadic `mode_t` parameter,
    // which is equivalent on Linux x86_64 and aarch64. Before porting to another platform you will
    // need to ensure that this works.
    const realOpen: *const fn ([*:0]const c_char, c_int, mode_t) callconv(.c) c_int = @ptrCast(@alignCast(c.dlsym(c.RTLD_NEXT, "open")));

    const state = State.get(&hardcoded_config) catch return realOpen(pathname, flags, mode);
    defer state.errno.clear();
    defer _ = state.arena.reset(.retain_capacity);

    const result = openImpl(state, realOpen, pathname, flags, mode) catch |err| bad_result: {
        if (err != error.Errno) {
            std.log.err("open({s}): {s}", .{ pathname, @errorName(err) });
            state.errno.errno = .IO;
        }
        break :bad_result -1;
    };
    if (state.errno.errno) |errno| {
        c.__errno_location().* = @intFromEnum(errno);
        return -1;
    } else {
        return result;
    }
}

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
