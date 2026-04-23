const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});

    const optimize = b.standardOptimizeOption(.{});

    const mod = b.createModule(.{
        .root_source_file = b.path("src/lib.zig"),
        .target = target,
        .optimize = optimize,
        .link_libc = false,
    });
    mod.addImport("hook", mod);

    const lib = b.addLibrary(.{
        .name = "hook",
        .root_module = mod,
        .linkage = .dynamic,
    });

    b.installArtifact(lib);
}
