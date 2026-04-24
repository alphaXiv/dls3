//! Utilities for generating wrappers for various classes of functions

const std = @import("std");
const hook = @import("hook");
const State = hook.State;
const c = hook.c;

pub fn OpenAdapterReturn(comptime T: type) type {
    return struct {
        value: T,
        // null on error
        fd: ?c_int,
    };
}

/// Generate a wrapper for a function that opens a file
pub fn wrapOpen(
    /// Type of the real function implementation
    comptime Fn: type,
    /// Name of the function (used to look up the real implementation)
    comptime name: [:0]const u8,
    /// Function that will execute a call to the real implementation.
    /// On success, returns both the real implementation's return value and the file descriptor.
    /// On error, returns an errno value.
    comptime adapter: fn (*const Fn, std.meta.ArgsTuple(Fn)) OpenAdapterReturn(@typeInfo(Fn).@"fn".return_type.?),
    /// Function to close the opened file in case opening succeeds but then communicating with the
    /// daemon fails
    comptime close: fn (@typeInfo(Fn).@"fn".return_type.?) void,
    /// Value to return when an error occurs
    comptime error_value: @typeInfo(Fn).@"fn".return_type.?,
) Fn {
    const Ret = @typeInfo(Fn).@"fn".return_type.?;
    const arg_types = blk: {
        var types: [@typeInfo(Fn).@"fn".params.len]type = undefined;
        for (&types, @typeInfo(Fn).@"fn".params) |*t, param| {
            t.* = param.type.?;
        }
        break :blk types;
    };

    const fallibleTupleWrapper = struct {
        fn wrapper(args: @Tuple(&arg_types)) !@typeInfo(Fn).@"fn".return_type.? {
            const realFn: *const Fn = @ptrCast(@alignCast(c.dlsym(c.RTLD_NEXT, name)));
            const result = adapter(realFn, args);
            const fd = result.fd orelse return result.value;

            var set_specific_errno = false;
            errdefer {
                close(result.value);
                if (!set_specific_errno) c.__errno_location().* = @intFromEnum(std.os.linux.E.IO);
            }

            // now we know it succeeded, so try opening it with the daemon
            const state = try State.get(&hook.hardcoded_config);
            defer state.errno.clear();
            defer _ = state.arena.reset(.retain_capacity);

            const key = (hook.path.resolveOpenedFdToKey(state.io(), state.allocator(), state.config, fd) catch |err| {
                // i assume that realpath failing is more likely because you're operating on some weird
                // file that is *not* under the mountpoint. so we pass through the opened file.
                std.log.err("realpath failed: {s}", .{@errorName(err)});
                return result.value;
            }) orelse return result.value;
            try hook.protocol.writeMessage(state.writer(), &.{ .open = .{ .path = key } });
            const response = try hook.protocol.readMessage(state.reader(), state.allocator());
            switch (response) {
                .opened => |o| {
                    if (o.errno == .SUCCESS) {
                        return result.value;
                    } else {
                        c.__errno_location().* = @intFromEnum(o.errno);
                        set_specific_errno = true;
                        return error.Errno;
                    }
                },
            }
        }
    }.wrapper;

    return switch (arg_types.len) {
        1 => struct {
            fn wrapper(param1: arg_types[0]) callconv(.c) Ret {
                return fallibleTupleWrapper(.{param1}) catch error_value;
            }
        }.wrapper,
        2 => struct {
            fn wrapper(param1: arg_types[0], param2: arg_types[1]) callconv(.c) Ret {
                return fallibleTupleWrapper(.{ param1, param2 }) catch error_value;
            }
        }.wrapper,
        3 => struct {
            fn wrapper(param1: arg_types[0], param2: arg_types[1], param3: arg_types[2]) callconv(.c) Ret {
                return fallibleTupleWrapper(.{ param1, param2, param3 }) catch error_value;
            }
        }.wrapper,
        4 => struct {
            fn wrapper(param1: arg_types[0], param2: arg_types[1], param3: arg_types[2], param4: arg_types[3]) callconv(.c) Ret {
                return fallibleTupleWrapper(.{ param1, param2, param3, param4 }) catch error_value;
            }
        }.wrapper,
        else => @compileError("too many arguments"),
    };
}
