const std = @import("std");
const Io = std.Io;
const Allocator = std.mem.Allocator;
const hook = @import("hook");
const linux = std.os.linux;

const path_template = std.fmt.comptimePrint("/proc/self/fd/{}", .{std.math.minInt(i32)});

/// Determines which S3 object a new file descriptor refers to, if any.
/// Returns `null` if this path is outside the mountpoint or is not a regular file
pub fn resolveOpenedFdToKey(arena: Allocator, config: *const hook.Config, fd: i32) !?[:0]u8 {
    var statx_buf: linux.Statx = undefined;
    switch (linux.errno(linux.statx(fd, "", linux.AT.EMPTY_PATH, .{ .MODE = true }, &statx_buf))) {
        .SUCCESS => {},
        .FAULT, .INVAL => unreachable,
        else => |e| {
            std.log.debug("statx failed: {s}", .{@tagName(e)});
            return null;
        },
    }
    if (!statx_buf.mask.MODE or !linux.S.ISREG(statx_buf.mode)) {
        return null;
    }

    var path_buf = path_template.*;
    const path = std.fmt.bufPrintSentinel(&path_buf, "/proc/self/fd/{}", .{fd}, 0) catch unreachable;
    var out_buf: [std.posix.PATH_MAX]u8 = undefined;
    const result = linux.readlink(path.ptr, &out_buf, out_buf.len);
    switch (linux.errno(result)) {
        .SUCCESS => {},
        .FAULT, .INVAL => unreachable,
        else => |e| {
            std.log.debug("readlink failed: {s}", .{@tagName(e)});
            return null;
        },
    }
    const real_path = out_buf[0..result];

    if (std.mem.startsWith(u8, real_path, config.backing_path) and
        real_path.len > config.backing_path.len and
        real_path[config.backing_path.len] == '/')
    {
        return try arena.dupeZ(u8, real_path[config.backing_path.len + 1 ..]);
    } else {
        return null;
    }
}
