const std = @import("std");
const hook = @import("hook");
const State = hook.State;
const linux = std.os.linux;
const fns = hook.fns;

fn closeFileDescriptor(_: *State, fd: c_int) void {
    _ = linux.close(fd);
}

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
                .writable = true,
            };
        }
    }.adapter,
    closeFileDescriptor,
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
                .writable = @as(linux.O, @bitCast(flags)).ACCMODE != .RDONLY,
            };
        }
    }.adapter,
    closeFileDescriptor,
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
                .writable = @as(linux.O, @bitCast(flags)).ACCMODE != .RDONLY,
            };
        }
    }.adapter,
    closeFileDescriptor,
    -1,
);
