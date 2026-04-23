//! Utility to use error unions for control flow while keeping precise errno values.
//!
//! Intended usage is that a C-compatible function creates an `errno` struct, passes it to a Zig
//! implementation that uses error values for control flow, and then the wrapper checks the `errno`
//! value afterward and uses it to determine its return value.

errno: ?std.os.linux.E,

const std = @import("std");
const Errno = @This();

pub const empty: Errno = .{ .errno = null };
pub const Error = error{Errno};

/// If `syscall_result` is an error, store the error code in `self` and return `error.Errno`.
/// Else, leave `self` unmodified and return `syscall_result`.
pub fn syscall(self: *Errno, syscall_result: usize) Error!usize {
    return switch (std.os.linux.errno(syscall_result)) {
        .SUCCESS => syscall_result,
        else => |err| {
            self.errno = err;
            return error.Errno;
        },
    };
}

pub fn clear(self: *Errno) void {
    self.errno = null;
}

test Errno {
    const usage_example = struct {
        fn zigImplementation(e: *Errno, nsec: i32) !void {
            _ = try e.syscall(std.os.linux.nanosleep(&std.os.linux.timespec{ .nsec = nsec, .sec = 0 }, null));
        }

        var global_errno: i32 = 0;

        fn cWrapper(nsec: i32) i32 {
            var e: Errno = .empty;
            zigImplementation(&e, nsec) catch {};
            if (e.errno) |errno| {
                global_errno = @intFromEnum(errno);
                return -1;
            } else {
                return 0;
            }
        }
    };

    // success
    try std.testing.expectEqual(0, usage_example.cWrapper(0));
    try std.testing.expectEqual(0, usage_example.global_errno);
    // error (nsec can't be negative)
    try std.testing.expect(usage_example.cWrapper(-1) < 0);
    try std.testing.expectEqual(@intFromEnum(std.os.linux.E.INVAL), usage_example.global_errno);
}
