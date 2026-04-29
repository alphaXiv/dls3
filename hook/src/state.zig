//! Mutable state for the hook: an arena, a connection to the daemon, and some buffers.
//! Each thread has its own.

const std = @import("std");
const Io = std.Io;
const hook = @import("hook");

arena: std.heap.ArenaAllocator,
stream: Io.net.Stream,
stream_reader: Io.net.Stream.Reader,
stream_writer: Io.net.Stream.Writer,
read_buffer: [128]u8,
write_buffer: [128]u8,
config: *const hook.Config,

const State = @This();

// TODO: instead of a threadlocal, use a freelist so that we don't leak if a program
// constantly starts and stops threads
threadlocal var state: ?anyerror!State = null;

pub fn get(config: *const hook.Config, io: Io) !*State {
    if (state == null) {
        hook.log.debug("init state on thread {}", .{std.os.linux.gettid()});
        {
            const addr = Io.net.UnixAddress.init(config.socket_path) catch |err| {
                hook.log.err("failed to create unix address from {s}: {s}", .{ config.socket_path, @errorName(err) });
                state = err;
                return err;
            };
            const stream = addr.connect(io) catch |err| {
                hook.log.err("failed to connect to {s}: {s}", .{ config.socket_path, @errorName(err) });
                state = err;
                return err;
            };

            state = .{
                .arena = .init(hook.global.gpa),
                .stream = stream,
                .stream_reader = undefined,
                .stream_writer = undefined,
                .read_buffer = undefined,
                .write_buffer = undefined,
                .config = config,
            };
        }

        const state_ptr = &(state.? catch unreachable);
        state_ptr.stream_reader = state_ptr.stream.reader(io, &state_ptr.read_buffer);
        state_ptr.stream_writer = state_ptr.stream.writer(io, &state_ptr.write_buffer);

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

pub fn reader(self: *State) *Io.Reader {
    return &self.stream_reader.interface;
}

pub fn writer(self: *State) *Io.Writer {
    return &self.stream_writer.interface;
}
