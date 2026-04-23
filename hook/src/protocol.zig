//! Wire protocol for communicating with daemon

const std = @import("std");
const Io = std.Io;
const Allocator = std.mem.Allocator;

pub const ClientMessageTag = enum(u8) { open, close };

pub const ClientMessage = union(ClientMessageTag) {
    open: struct { path: []const u8 },
    close: struct { path: []const u8 },
};

pub const ServerMessageTag = enum(u8) { opened };

pub const ServerMessage = union(ServerMessageTag) {
    opened: struct { errno: std.os.linux.E },
};

pub const ReadMessageError = error{UnknownTag} || Allocator.Error || Io.Reader.Error;

pub fn readMessage(stream: *Io.Reader, arena: Allocator) ReadMessageError!ServerMessage {
    _ = arena;
    const tag = std.enums.fromInt(ServerMessageTag, try stream.takeByte()) orelse return error.UnknownTag;
    const payload_size = try stream.takeInt(u16, .little);
    _ = payload_size;

    switch (tag) {
        .opened => {
            const errno = try stream.takeInt(u16, .little);
            return .{ .opened = .{ .errno = @enumFromInt(errno) } };
        },
    }
}

pub fn writeMessage(stream: *Io.Writer, message: *const ClientMessage) std.Io.Writer.Error!void {
    const payload_size = switch (message.*) {
        .open => |o| o.path.len,
        .close => |c| c.path.len,
    };
    const tag = @intFromEnum(message.*);
    try stream.writeInt(u8, tag, .little);
    try stream.writeInt(u16, @intCast(payload_size), .little);
    switch (message.*) {
        .open => |o| try stream.writeAll(o.path),
        .close => |c| try stream.writeAll(c.path),
    }
    try stream.flush();
}
