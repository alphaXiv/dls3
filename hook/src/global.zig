//! Global state: an IO instance and a cache of real function implementations from dlsym()
//! The functions cache may be initialized earlier in case functions we override are called by
//! another library's initializer (in that case, we always forward the call to the libc versions)

const std = @import("std");
const hook = @import("hook");

threaded_io: std.Io.Threaded,
config: Config,

const Global = @This();
pub const Config = struct {
    socket_path: []const u8,
    backing_path: []const u8,
    backing_fd: i32,
};

var instance: ?Global = null;

var debug_allocator: std.heap.DebugAllocator(.{}) = .init;
pub const gpa = switch (@import("builtin").mode) {
    .Debug => debug_allocator.allocator(),
    else => std.heap.smp_allocator,
};

/// Must be called once before any additional threads are spawned
pub fn init() !void {
    const new_state: Global = .{
        .threaded_io = .init(gpa, .{
            .async_limit = .nothing,
            .concurrent_limit = .nothing,
        }),
        .config = .{
            .socket_path = @ptrCast(std.mem.span(hook.c.getenv(@ptrCast("DLS3_SOCKET_PATH")) orelse {
                hook.log.err("missing DLS3_SOCKET_PATH", .{});
                return error.EnvVarMissing;
            })),
            .backing_path = @ptrCast(std.mem.span(hook.c.getenv(@ptrCast("DLS3_BACKING_PATH")) orelse {
                hook.log.err("missing DLS3_BACKING_PATH", .{});
                return error.EnvVarMissing;
            })),
            .backing_fd = std.fmt.parseInt(i32, @ptrCast(std.mem.span(hook.c.getenv(@ptrCast("DLS3_BACKING_FD")) orelse {
                hook.log.err("missing DLS3_BACKING_FD", .{});
                return error.EnvVarMissing;
            })), 10) catch |err| {
                hook.log.err("invalid DLS3_BACKING_FD", .{});
                return err;
            },
        },
    };
    instance = new_state;
}

pub fn get() ?*Global {
    return if (instance) |*global|
        global
    else
        null;
}
