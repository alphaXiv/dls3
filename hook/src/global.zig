//! Global state: an IO instance and a cache of real function implementations from dlsym()
//! The functions cache may be initialized earlier in case functions we override are called by
//! another library's initializer (in that case, we always forward the call to the libc versions)

const std = @import("std");
const hook = @import("hook");

var threaded_io: ?std.Io.Threaded = null;
var function_cache: ?hook.Functions = null;

var debug_allocator: std.heap.DebugAllocator(.{}) = .init;
pub const gpa = switch (@import("builtin").mode) {
    .Debug => debug_allocator.allocator(),
    else => std.heap.smp_allocator,
};

/// Must be called before any additional threads are spawned
pub fn init() void {
    threaded_io = .init(gpa, .{
        .async_limit = .nothing,
        .concurrent_limit = .nothing,
    });
    _ = functions();
}

pub fn io() ?std.Io {
    return if (threaded_io) |*io_impl|
        io_impl.io()
    else
        null;
}

pub fn functions() *const hook.Functions {
    if (function_cache == null) {
        var wip_functions: hook.Functions = undefined;
        inline for (@typeInfo(hook.Functions).@"struct".fields) |field| {
            @field(wip_functions, field.name) = @ptrCast(@alignCast(
                hook.c.dlsym(hook.c.RTLD_NEXT, field.name) orelse std.debug.panic("could not get libc implementation of {s}", .{field.name}),
            ));
        }
        function_cache = wip_functions;
    }
    return &(function_cache.?);
}
