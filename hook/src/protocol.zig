//! Wire protocol for communicating with daemon

const std = @import("std");
const Io = std.Io;
const Allocator = std.mem.Allocator;

pub const ClientMessageTag = enum(u16) { open, close };

pub const ClientMessage = union(ClientMessageTag) {
    open: struct { path: []const u8 },
    close: struct { path: []const u8 },
};

pub const ServerMessageTag = enum(u16) { opened };

pub const ServerMessage = union(ServerMessageTag) {
    opened: struct { path: []u8, errno: ?i32 },
};

pub const ReadMessageError = error{UnknownTag} || Allocator.Error || Io.Reader.Error;

pub fn readMessage(stream: *Io.Reader, arena: Allocator) ReadMessageError!ServerMessage {
    const payload_size = try stream.takeInt(u16, .little);
    const tag = std.enums.fromInt(ServerMessageTag, try stream.takeInt(u16, .little)) orelse return error.UnknownTag;

    switch (tag) {
        .opened => {
            const is_err = (try stream.takeByte()) != 0;
            const errno = try stream.takeInt(i32, .little);
            const path_len = payload_size - 1 - 4;
            return .{ .opened = .{
                .errno = if (is_err) errno else null,
                .path = try stream.readAlloc(arena, path_len),
            } };
        },
    }
}

pub fn writeMessage(stream: *Io.Writer, message: *const ClientMessage) std.Io.Writer.Error!void {
    const payload_size = switch (message.*) {
        .open => |o| o.path.len,
        .close => |c| c.path.len,
    };
    const tag = @intFromEnum(message.*);
    try stream.writeInt(u16, @intCast(payload_size), .little);
    try stream.writeInt(u16, tag, .little);
    switch (message.*) {
        .open => |o| try stream.writeAll(o.path),
        .close => |c| try stream.writeAll(c.path),
    }
    try stream.flush();
}
