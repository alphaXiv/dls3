FROM fedora:43 AS zig_downloader
RUN dnf install -y minisign
WORKDIR /tmp
RUN curl -fsSLO https://ziglang.org/download/community-mirrors.txt
COPY download-zig.sh .
RUN bash download-zig.sh community-mirrors.txt

FROM fedora:43
RUN sed -i '/tsflags=nodocs/d' /etc/dnf/dnf.conf
RUN dnf install -y cgdb lldb debuginfod man-db man-pages git gcc clang procps-ng btop clangd strace cargo rclone fuse3 vim rust-src rustfmt clippy netcat awscli2 pv moreutils xxd
ENV DEBUGINFOD_URLS="https://debuginfod.fedoraproject.org/"
COPY <<EOF /root/.gdbinit
set debuginfod enabled on
add-auto-load-safe-path /workspaces/dls3/
EOF
COPY <<EOF /root/rclone.conf
[garage]
type = s3
access_key_id = GK_ACCESS
secret_access_key = GK_SECRETSECRETSECRET
endpoint = http://garage:3900
EOF
COPY <<EOF /root/.aws/config
[default]
endpoint_url = http://garage:3900
EOF
COPY <<EOF /root/.aws/credentials
[default]
aws_secret_access_key = GK_SECRETSECRETSECRET
aws_access_key_id = GK_ACCESS
EOF
COPY --from=zig_downloader /usr/bin/zig /usr/bin/zig
COPY --from=zig_downloader /usr/lib/zig /usr/lib/zig
RUN grep -vE "alias .*='\w+ -i'" /root/.bashrc | sponge /root/.bashrc
ENTRYPOINT [ "tail", "-f", "/dev/null" ]
