const std = @import("std");

// re-exports
pub const protocol = @import("protocol.zig");
pub const path = @import("path.zig");
pub const State = @import("state.zig");
pub const c = @import("c.zig");
pub const wrappers = @import("wrappers.zig");
pub const fns = @import("fns.zig");
pub const Functions = fns.Functions;
pub const Global = @import("global.zig");
pub const Config = Global.Config;
pub const log = std.log.scoped(.dls3);

// prevent zig from allocating a large threadlocal stack for signal handling
// since we won't use it and it eats into the space allocated for the victim's threads' stacks
pub const std_options: std.Options = .{ .signal_stack_size = null };

comptime {
    if (@import("builtin").target.os.tag != .linux) @compileError("only linux is supported");
    switch (@import("builtin").target.cpu.arch) {
        .x86_64, .aarch64 => {},
        else => @compileError("only x86_64 and aarch64 are supported"),
    }
}

fn tryInit() !void {
    try Global.init();
    const global = Global.get().?;
    const file: std.Io.File = .{ .handle = global.config.backing_fd, .flags = .{ .nonblocking = false } };
    var buf: [std.posix.PATH_MAX]u8 = undefined;

    if (file.realPath(global.threaded_io.io(), &buf)) |len| {
        const real_path = buf[0..len];
        if (!std.mem.eql(u8, real_path, global.config.backing_path)) {
            log.err("fd {} is open as {s} instead of {s}, dls3 cannot operate", .{ global.config.backing_fd, real_path, global.config.backing_path });
            return error.FixedFdFailed;
        } else {
            log.debug("fd is already open", .{});
        }
    } else |err| switch (err) {
        error.FileNotFound => {
            // we need to open it ourselves
            const fd = try std.posix.openat(std.posix.AT.FDCWD, global.config.backing_path, .{
                .ACCMODE = .RDONLY,
                .DIRECTORY = true,
                .CLOEXEC = true,
            }, 0);
            defer _ = std.os.linux.close(fd);
            const new_fd = std.os.linux.fcntl(fd, std.os.linux.F.DUPFD_CLOEXEC, @intCast(global.config.backing_fd));
            switch (std.os.linux.errno(new_fd)) {
                .SUCCESS => if (new_fd != global.config.backing_fd) {
                    _ = std.os.linux.close(@intCast(new_fd));
                    log.err("fcntl got fd {} instead of {}", .{ new_fd, global.config.backing_fd });
                    return error.FixedFdFailed;
                } else {
                    log.debug("fd opened", .{});
                },
                else => return error.FailedDupFd,
            }
        },
        else => return err,
    }
}

fn init() callconv(.c) void {
    log.debug("init hook for process {}, thread {}", .{ std.os.linux.getpid(), std.os.linux.gettid() });
    tryInit() catch |e| {
        log.err("error initializing dls3: {s}", .{@errorName(e)});
    };
}

export const init_array: [1]*const fn () callconv(.c) void linksection(".init_array") = .{&init};

comptime {
    std.testing.refAllDecls(fns);
}
