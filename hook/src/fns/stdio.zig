pub fn fopen(noalias pathname: [*:0]const c_char, noalias mode: [*:0]const c_char) callconv(.c) ?*anyopaque {}

pub fn fclose(stream: ?*anyopaque) c_int {}
