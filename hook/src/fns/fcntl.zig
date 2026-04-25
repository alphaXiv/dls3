const std = @import("std");
const hook = @import("hook");
const State = hook.State;
const linux = std.os.linux;
const fns = hook.fns;

pub const creat = hook.wrappers.wrapOpen(
    .creat,
    struct {
        fn adapter(
            realCreat: fns.FunctionPtr(.creat),
            args: fns.FunctionArgs(.creat),
        ) hook.wrappers.OpenAdapterReturn(c_int) {
            const filename, const mode = args;
            const fd = realCreat(filename, mode);
            return .{
                .value = fd,
                .fd = if (fd < 0) null else fd,
            };
        }
    }.adapter,
    struct {
        fn close(_: *State, fd: c_int) void {
            _ = linux.close(fd);
        }
    }.close,
    -1,
);

pub const open = hook.wrappers.wrapOpen(
    .open,
    struct {
        fn adapter(
            realOpen: fns.FunctionPtr(.open),
            args: fns.FunctionArgs(.open),
        ) hook.wrappers.OpenAdapterReturn(c_int) {
            const pathname, const flags, const mode = args;
            const fd = realOpen(pathname, flags, mode);
            return .{
                .value = fd,
                .fd = if (fd < 0) null else fd,
            };
        }
    }.adapter,
    struct {
        fn close(_: *State, fd: c_int) void {
            _ = linux.close(fd);
        }
    }.close,
    -1,
);

pub const openat = hook.wrappers.wrapOpen(
    .openat,
    struct {
        fn adapter(
            realOpenat: fns.FunctionPtr(.openat),
            args: fns.FunctionArgs(.openat),
        ) hook.wrappers.OpenAdapterReturn(c_int) {
            const dirfd, const pathname, const flags, const mode = args;
            const fd = realOpenat(dirfd, pathname, flags, mode);
            return .{
                .value = fd,
                .fd = if (fd < 0) null else fd,
            };
        }
    }.adapter,
    struct {
        fn close(_: *State, fd: c_int) void {
            _ = linux.close(fd);
        }
    }.close,
    -1,
);

pub const close = hook.wrappers.wrapClose(
    .close,
    struct {
        fn adapter(args: struct { c_int }) c_int {
            return args[0];
        }
    }.adapter,
    -1,
);
