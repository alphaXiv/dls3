const std = @import("std");
const hook = @import("hook");
const fns = hook.fns;
const c = hook.c;

pub const fopen = hook.wrappers.wrapOpen(
    .fopen,
    struct {
        fn adapter(
            realFopen: fns.FunctionPtr(.fopen),
            args: fns.FunctionArgs(.fopen),
        ) hook.wrappers.OpenAdapterReturn(?*c.FILE) {
            const pathname, const mode = args;
            const fp = realFopen(pathname, mode);
            return .{
                .value = fp,
                .fd = if (fp) |nonnull| c.fileno(nonnull) else null,
                // only try to parse the mode string if fopen() succeeded
                .writable = if (fp != null) w: {
                    const slice: [:0]const u8 = @ptrCast(std.mem.span(mode));
                    break :w slice.len > 1 and (slice[0] != 'r' or std.mem.findScalar(u8, slice, '+') != null);
                } else true,
            };
        }
    }.adapter,
    struct {
        fn close(state: *hook.State, stream: ?*c.FILE) void {
            _ = state.functions.fclose(stream);
        }
    }.close,
    null,
);

pub const fclose = hook.wrappers.wrapClose(
    .fclose,
    struct {
        fn adapter(args: struct { ?*c.FILE }) c_int {
            return c.fileno(args[0]);
        }
    }.adapter,
    c.EOF,
);
