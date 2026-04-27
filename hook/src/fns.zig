//! Index of all the functions we are overriding.
//!
//! Used both as a struct containing function pointers obtained from `dlsym`
//! for the real implementations, and as an index for type information.

const std = @import("std");
const hook = @import("hook");
const c = hook.c;
const mode_t = std.os.linux.mode_t;

pub const Functions = struct {
    // -- fcntl.h --
    // TODO: close_range
    // TODO: closefrom
    creat: *const fn (filename: [*:0]const c_char, mode: mode_t) callconv(.c) c_int,
    // `open()` and `openat()` are actually variadic instead of taking a `mode_t` parameter. But Zig doesn't support
    // reading variadic arguments on aarch64. So we type-pun to a non-variadic `mode_t` parameter,
    // which is equivalent on Linux x86_64 and aarch64. Before porting to another platform you will
    // need to ensure that this works.
    open: *const fn (pathname: [*:0]const c_char, flags: c_int, mode: mode_t) callconv(.c) c_int,
    openat: *const fn (dirfd: c_int, pathname: [*:0]const c_char, flags: c_int, mode: mode_t) callconv(.c) c_int,
    // TODO: openat2 (glibc 2.43 just added a wrapper)

    // -- stdio.h --
    fclose: *const fn (stream: ?*c.FILE) callconv(.c) c_int,
    // TODO: fcloseall
    fopen: *const fn (pathname: [*:0]const c_char, mode: [*:0]const c_char) callconv(.c) ?*c.FILE,
    // TODO: freopen

    // -- unistd.h --
    close: *const fn (fd: c_int) callconv(.c) c_int,
    // TODO: syscall
};

pub const FunctionId = std.meta.FieldEnum(Functions);

pub fn FunctionPtr(id: FunctionId) type {
    return @FieldType(Functions, @tagName(id));
}

pub fn FunctionArgs(id: FunctionId) type {
    return std.meta.ArgsTuple(@typeInfo(FunctionPtr(id)).pointer.child);
}

pub fn FunctionReturn(id: FunctionId) type {
    return @typeInfo(@typeInfo(FunctionPtr(id)).pointer.child).@"fn".return_type.?;
}

// exports
export const creat = @import("fns/fcntl.zig").creat;
export const creat64 = creat;
export const open = @import("fns/fcntl.zig").open;
export const open64 = open;
export const openat = @import("fns/fcntl.zig").openat;
export const openat64 = openat;

export const fclose = @import("fns/stdio.zig").fclose;
export const fopen = @import("fns/stdio.zig").fopen;
export const fopen64 = fopen;

export const close = @import("fns/unistd.zig").close;
