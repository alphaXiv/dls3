const std = @import("std");

// re-exports
pub const protocol = @import("protocol.zig");
pub const Config = @import("config.zig").Config;
pub const hardcoded_config = @import("config.zig").hardcoded_config;
pub const path = @import("path.zig");
pub const State = @import("state.zig");
pub const c = @import("c.zig");
pub const wrappers = @import("wrappers.zig");
pub const fns = @import("fns.zig");
pub const Functions = fns.Functions;

comptime {
    if (@import("builtin").target.os.tag != .linux) @compileError("only linux is supported");
    switch (@import("builtin").target.cpu.arch) {
        .x86_64, .aarch64 => {},
        else => @compileError("only x86_64 and aarch64 are supported"),
    }
}

export const close = @import("fns/fcntl.zig").close;
export const creat = @import("fns/fcntl.zig").creat;
export const creat64 = creat;
export const open = @import("fns/fcntl.zig").open;
export const open64 = open;
export const openat = @import("fns/fcntl.zig").openat;
export const openat64 = openat;

export const fclose = @import("fns/stdio.zig").fclose;
export const fopen = @import("fns/stdio.zig").fopen;
export const fopen64 = fopen;

fn tryInit() !void {
    const fd = try std.posix.openat(std.posix.AT.FDCWD, hardcoded_config.backing_path, .{ .ACCMODE = .RDONLY, .DIRECTORY = true, .CLOEXEC = true }, 0);
    defer _ = std.os.linux.close(fd);
    const new_fd = std.os.linux.fcntl(fd, std.os.linux.F.DUPFD, @intCast(hardcoded_config.backing_fd));
    switch (std.os.linux.errno(new_fd)) {
        .SUCCESS => if (new_fd != hardcoded_config.backing_fd) {
            _ = std.os.linux.close(@intCast(new_fd));
            std.log.err("got fd {} instead", .{new_fd});
            return error.GotWrongFd;
        },
        else => return error.FailedDupFd,
    }
}

fn init() callconv(.c) void {
    std.log.info("init hook for process {}, thread {}", .{ std.os.linux.getpid(), std.os.linux.gettid() });
    tryInit() catch |e| {
        // TODO: get this to print only once across multiple processes
        std.log.err("error initializing dls3: {s}", .{@errorName(e)});
    };
}

export const init_array: [1]*const fn () callconv(.c) void linksection(".init_array") = .{&init};
