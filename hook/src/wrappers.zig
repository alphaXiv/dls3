//! Utilities for generating wrappers for various classes of functions

const std = @import("std");
const hook = @import("hook");
const State = hook.State;
const c = hook.c;
const E = std.os.linux.E;
const FunctionId = hook.fns.FunctionId;
const FunctionPtr = hook.fns.FunctionPtr;
const FunctionArgs = hook.fns.FunctionArgs;
const FunctionReturn = hook.fns.FunctionReturn;

pub fn OpenAdapterReturn(comptime T: type) type {
    return struct {
        value: T,
        /// Null on error
        fd: ?c_int,
        /// Whether the file was opened for writing. If true, and the file is under the mountpoint,
        /// we will return EROFS.
        writable: bool,
    };
}

fn logArgs(id: FunctionId, args: anytype) void {
    const AdaptedArgs = comptime T: {
        var field_types: [args.len]type = undefined;
        for (&field_types, @typeInfo(@TypeOf(args)).@"struct".fields) |*out_type, field_info| {
            out_type.* = if (field_info.type == [*:0]const c_char)
                [*:0]const u8
            else
                field_info.type;
        }
        break :T @Tuple(&field_types);
    };

    var adapted_args: AdaptedArgs = undefined;
    inline for (&adapted_args, args) |*adapted, original| {
        adapted.* = if (@TypeOf(original) == [*:0]const c_char)
            @ptrCast(original)
        else
            original;
    }

    hook.log.debug(comptime fmt: {
        var fmt: []const u8 = "{s}(";
        for (@typeInfo(@TypeOf(args)).@"struct".fields, 0..) |f, i| {
            fmt = fmt ++ if (@typeInfo(f.type) == .pointer and @typeInfo(f.type).pointer.child == c_char)
                "\"{s}\""
            else
                "{any}";
            if (i != args.len - 1) fmt = fmt ++ ", ";
        }
        break :fmt fmt ++ ")";
    }, .{@tagName(id)} ++ adapted_args);
}

/// Generate a wrapper for a function that opens a file
pub fn wrapOpen(
    /// Which function to wrap (used to look up the real implementation and type information)
    comptime id: FunctionId,
    /// Function that will execute a call to the real implementation.
    /// On success, returns both the real implementation's return value and the file descriptor.
    /// On error, returns an errno value.
    comptime adapter: fn (FunctionPtr(id), FunctionArgs(id)) OpenAdapterReturn(FunctionReturn(id)),
    /// Function to close the opened file in case opening succeeds but then communicating with the
    /// daemon fails
    comptime close: fn (FunctionReturn(id)) void,
    /// Value to return when an error occurs
    comptime error_value: FunctionReturn(id),
) @typeInfo(FunctionPtr(id)).pointer.child {
    const Fn = @typeInfo(FunctionPtr(id)).pointer.child;
    const Ret = FunctionReturn(id);
    const Args = FunctionArgs(id);

    const arg_types = blk: {
        var types: [@typeInfo(Fn).@"fn".params.len]type = undefined;
        for (&types, @typeInfo(Fn).@"fn".params) |*t, param| {
            t.* = param.type.?;
        }
        break :blk types;
    };

    const fallibleTupleWrapper = struct {
        fn wrapper(args: Args) !Ret {
            logArgs(id, args);

            const realFn = @field(hook.global.functions(), @tagName(id));
            const io = hook.global.io() orelse return @call(.auto, realFn, args);
            const state = State.get(&hook.hardcoded_config, io) catch return @call(.auto, realFn, args);
            defer _ = state.arena.reset(.retain_capacity);

            const result = adapter(realFn, args);
            const fd = result.fd orelse return result.value;

            var set_specific_errno = false;
            errdefer {
                close(result.value);
                if (!set_specific_errno) c.__errno_location().* = @intFromEnum(std.os.linux.E.IO);
            }

            // now we know it succeeded, so try opening it with the daemon
            const key = (hook.path.resolveOpenedFdToKey(state.allocator(), state.config, fd) catch |err| {
                // i assume that realpath failing is more likely because you're operating on some weird
                // file that is *not* under the mountpoint. so we pass through the opened file.
                hook.log.err("realpath failed: {s}", .{@errorName(err)});
                return result.value;
            }) orelse return result.value;

            // if the file is under the mountpoint, prevent writing
            if (result.writable) {
                c.__errno_location().* = @intFromEnum(E.ROFS);
                set_specific_errno = true;
                return error.Errno;
            }

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

pub fn wrapClose(
    comptime id: hook.fns.FunctionId,
    /// Function to determine the file descriptor from the arguments
    comptime adapter: fn (hook.fns.FunctionArgs(id)) c_int,
    /// Value to return when an error occurs
    comptime error_value: FunctionReturn(id),
) @typeInfo(FunctionPtr(id)).pointer.child {
    const Fn = @typeInfo(FunctionPtr(id)).pointer.child;
    const Ret = FunctionReturn(id);
    const Args = FunctionArgs(id);

    const fallibleTupleWrapper = struct {
        fn wrapper(args: Args) !Ret {
            logArgs(id, args);

            const realFn = @field(hook.global.functions(), @tagName(id));
            const io = hook.global.io() orelse return @call(.auto, realFn, args);
            const state = State.get(&hook.hardcoded_config, io) catch return @call(.auto, realFn, args);
            defer _ = state.arena.reset(.retain_capacity);

            const fd = adapter(args);

            const key = (hook.path.resolveOpenedFdToKey(state.allocator(), state.config, fd) catch |err| {
                hook.log.err("realpath failed: {s}", .{@errorName(err)});
                return @call(.auto, realFn, args);
            }) orelse return @call(.auto, realFn, args);
            hook.protocol.writeMessage(state.writer(), &.{ .close = .{ .path = key } }) catch |err| {
                hook.log.err("send message failed: {s}", .{@errorName(err)});
            };
            return @call(.auto, realFn, args);
        }
    }.wrapper;

    const arg_types = blk: {
        var types: [@typeInfo(Fn).@"fn".params.len]type = undefined;
        for (&types, @typeInfo(Fn).@"fn".params) |*t, param| {
            t.* = param.type.?;
        }
        break :blk types;
    };

    return switch (arg_types.len) {
        1 => struct {
            fn wrapper(param1: arg_types[0]) callconv(.c) Ret {
                return fallibleTupleWrapper(.{param1}) catch error_value;
            }
        }.wrapper,
        else => @compileError("too many arguments"),
    };
}
