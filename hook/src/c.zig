pub const RTLD_NEXT: ?*anyopaque = @ptrFromInt(@as(usize, @bitCast(@as(isize, -1))));
pub extern fn dlsym(handle: ?*anyopaque, symbol: [*:0]const u8) ?*anyopaque;
pub extern fn __errno_location() *c_int;
