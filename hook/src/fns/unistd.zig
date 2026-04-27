const hook = @import("hook");

pub const close = hook.wrappers.wrapClose(
    .close,
    struct {
        fn adapter(args: struct { c_int }) c_int {
            return args[0];
        }
    }.adapter,
    -1,
);
