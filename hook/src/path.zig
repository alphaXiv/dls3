const std = @import("std");
const Io = std.Io;
const Allocator = std.mem.Allocator;
const hook = @import("hook");

/// Determines which S3 object a new file descriptor refers to, if any.
/// Returns `null` if this path is outside the mountpoint or is not a regular file
pub fn resolveOpenedFdToKey(io: Io, arena: Allocator, config: *const hook.Config, fd: i32) !?[:0]u8 {
    var buf: [std.posix.PATH_MAX]u8 = undefined;
    const handle: Io.File = .{ .handle = fd, .flags = .{ .nonblocking = false } };
    const stat = try handle.stat(io);
    if (stat.kind != .file) {
        return null;
    }

    const len = try handle.realPath(io, &buf);
    const real_path = buf[0..len];
    if (std.mem.startsWith(u8, real_path, config.backing_path) and
        real_path.len > config.backing_path.len and
        real_path[config.backing_path.len] == '/')
    {
        return try arena.dupeZ(u8, real_path[config.backing_path.len + 1 ..]);
    } else {
        return null;
    }
}
