//! Program-wide configuration

pub const Config = struct {
    socket_path: []const u8,
    mountpoint: []const u8,
    backing_path: []const u8,
    backing_fd: i32,
};

pub const hardcoded_config: Config = .{
    .socket_path = "/root/dls3.sock",
    .mountpoint = "/neer",
    .backing_path = "/root/dls3-store",
    .backing_fd = 666,
};
