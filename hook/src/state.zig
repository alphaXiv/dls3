//! Mutable state for the hook: an arena, an IO instance, a connection to the daemon, and some buffers.
//! Each thread has its own.

const std = @import("std");
const Io = std.Io;
const hook = @import("hook");

arena: std.heap.ArenaAllocator,
threaded_io: Io.Threaded,
stream: Io.net.Stream,
stream_reader: Io.net.Stream.Reader,
stream_writer: Io.net.Stream.Writer,
read_buffer: [128]u8,
write_buffer: [128]u8,
config: *const hook.Config,
functions: hook.Functions,

const State = @This();

var debug_allocator: std.heap.DebugAllocator(.{}) = .init;
const gpa = switch (@import("builtin").mode) {
    .Debug => debug_allocator.allocator(),
    else => std.heap.smp_allocator,
};

// TODO: instead of a threadlocal, use a freelist so that we don't leak if a program
// constantly starts and stops threads
threadlocal var state: ?anyerror!State = null;

pub fn get(config: *const hook.Config) !*State {
    if (state == null) {
        const addr = Io.net.UnixAddress.init(config.socket_path) catch |err| {
            std.log.err("failed to create unix address from {s}: {s}", .{ config.socket_path, @errorName(err) });
            state = err;
            return err;
        };

        state = .{
            .arena = .init(gpa),
            .threaded_io = .init(gpa, .{}),
            .stream = undefined,
            .stream_reader = undefined,
            .stream_writer = undefined,
            .read_buffer = undefined,
            .write_buffer = undefined,
            .config = config,
            .functions = undefined,
        };
        const state_ptr = &(state.? catch unreachable);
        state_ptr.stream = addr.connect(state_ptr.threaded_io.io()) catch |err| {
            std.log.err("failed to connect to {s}: {s}", .{ config.socket_path, @errorName(err) });
            state = err;
            return err;
        };
        state_ptr.stream_reader = state_ptr.stream.reader(state_ptr.threaded_io.io(), &state_ptr.read_buffer);
        state_ptr.stream_writer = state_ptr.stream.writer(state_ptr.threaded_io.io(), &state_ptr.write_buffer);

        inline for (@typeInfo(hook.Functions).@"struct".fields) |field| {
            @field(state_ptr.functions, field.name) = @ptrCast(@alignCast(hook.c.dlsym(hook.c.RTLD_NEXT, field.name)));
        }

        return state_ptr;
    }

    return if (state.?) |_|
        &(state.? catch unreachable)
    else |err|
        err;
}

pub fn allocator(self: *State) std.mem.Allocator {
    return self.arena.allocator();
}

pub fn io(self: *State) Io {
    return self.threaded_io.io();
}

pub fn reader(self: *State) *Io.Reader {
    return &self.stream_reader.interface;
}

pub fn writer(self: *State) *Io.Writer {
    return &self.stream_writer.interface;
}
